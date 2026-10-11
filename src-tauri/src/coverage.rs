//! Where a station can reach at one hour: a point-to-point prediction to the
//! centre of every cell of a world grid.
//!
//! The Map uses a fine grid, cached in the observation database by month and
//! hour, and shows a coarse one while the fine one is computed. Listening
//! plans only need a share of the world, so they use the coarse grid.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::geo::{self, LatLon};
use crate::observations::Database;
use crate::predictor::{self, PathRequest};
use crate::propagation::{HourPrediction, PredictionRequest, PropagationEngine};
use crate::solar::SsnUsed;
use crate::station::{self, Band, Mode, StationProfile, HF_BANDS};

/// Spacing of the points coverage is predicted at.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Grid {
    pub lat_step_deg: f64,
    pub lon_step_deg: f64,
}

/// 432 points: about a second.
pub const COARSE_GRID: Grid = Grid { lat_step_deg: 10.0, lon_step_deg: 15.0 };
/// 4,050 points: about seven seconds on four cores. Finer grids mostly sharpen
/// the edge of the skip zone, since the model's ionosphere is itself smooth.
pub const FINE_GRID: Grid = Grid { lat_step_deg: 4.0, lon_step_deg: 4.0 };

/// Cells this close to the transmitter are left out: the model predicts
/// sky-wave paths, not ground wave.
pub const MIN_DISTANCE_KM: f64 = 100.0;
const MAX_WORKERS: usize = 16;
/// Receivers per engine run, so large sets spread over the workers.
const RECEIVERS_PER_RUN: usize = 60;
/// One engine run serves every cell whose antenna bearings fall in the same
/// sectors, with the antennas aimed at the sector centres. At 45 degrees a
/// dipole is never more than 22.5 degrees off, under 1 dB.
const SECTOR_DEG: f64 = 45.0;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CoverageRequest {
    pub tx_position: String,
    pub year: i32,
    pub month: u32,
    pub ssn: Option<f64>,
    pub tx_station: StationProfile,
    /// The station assumed at every receiving point.
    pub rx_station: StationProfile,
    pub mode: Mode,
    pub required_reliability_pct: f64,
    /// UTC hour as VOACAP numbers them: 1 to 24, where 24 is 00 UTC.
    pub utc_hour: u32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CoverageCell {
    /// Cell centre.
    pub lat: f64,
    pub lon: f64,
    pub distance_km: f64,
    /// One value per band, in `Coverage::bands` order.
    pub reliability: Vec<f64>,
    pub snr_db: Vec<f64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Coverage {
    pub tx: LatLon,
    pub utc_hour: u32,
    pub lat_step_deg: f64,
    pub lon_step_deg: f64,
    pub ssn: SsnUsed,
    pub required_snr_db_hz: f64,
    pub bands: Vec<Band>,
    pub cells: Vec<CoverageCell>,
}

fn grid_centres(grid: Grid) -> Vec<LatLon> {
    let rows = (180.0 / grid.lat_step_deg).round() as usize;
    let columns = (360.0 / grid.lon_step_deg).round() as usize;
    (0..rows)
        .flat_map(|row| {
            (0..columns).map(move |column| LatLon {
                lat: -90.0 + grid.lat_step_deg * (row as f64 + 0.5),
                lon: -180.0 + grid.lon_step_deg * (column as f64 + 0.5),
            })
        })
        .collect()
}

/// The centres of the cells that are predicted: all but those too near.
fn predicted_centres(tx: LatLon, grid: Grid) -> Vec<LatLon> {
    grid_centres(grid).into_iter().filter(|&centre| geo::distance_km(tx, centre) >= MIN_DISTANCE_KM).collect()
}

/// The bearing used for every cell whose true bearing falls in the same
/// sector, given how far the antenna can turn before its pattern repeats.
fn sector_bearing(bearing_deg: f64, period_deg: f64) -> f64 {
    if period_deg == 0.0 {
        return 0.0;
    }
    let folded = bearing_deg.rem_euclid(period_deg);
    (folded / SECTOR_DEG).floor() * SECTOR_DEG + SECTOR_DEG / 2.0
}

/// Cells that share antenna bearings, and so one engine run.
struct Group {
    tx_bearing_deg: f64,
    rx_bearing_deg: f64,
    /// Indices into the list of cell centres.
    members: Vec<usize>,
}

fn group_by_sector(tx: LatLon, centres: &[LatLon], tx_period: f64, rx_period: f64) -> Vec<Group> {
    // Sector bearings are multiples of 22.5, so tenths of a degree key them exactly.
    let mut groups: BTreeMap<(i64, i64), Group> = BTreeMap::new();
    for (i, &centre) in centres.iter().enumerate() {
        let tx_bearing_deg = sector_bearing(geo::bearing_deg(tx, centre), tx_period);
        let rx_bearing_deg = sector_bearing(geo::bearing_deg(centre, tx), rx_period);
        groups
            .entry(((tx_bearing_deg * 10.0) as i64, (rx_bearing_deg * 10.0) as i64))
            .or_insert_with(|| Group { tx_bearing_deg, rx_bearing_deg, members: Vec::new() })
            .members
            .push(i);
    }
    groups.into_values().collect()
}

/// Planning a path to one point validates the rest of the input and gives
/// the engine request every point of an area prediction shares.
pub(crate) fn plan_area(request: &CoverageRequest, any_point: LatLon) -> Result<predictor::Plan, String> {
    predictor::plan(&PathRequest {
        tx_position: request.tx_position.clone(),
        rx_position: format!("{:.3}, {:.3}", any_point.lat, any_point.lon),
        year: request.year,
        month: request.month,
        ssn: request.ssn,
        tx_station: request.tx_station.clone(),
        rx_station: request.rx_station.clone(),
        mode: request.mode,
        required_reliability_pct: request.required_reliability_pct,
        long_path: false,
    })
}

/// Everything an area prediction depends on apart from the points, month
/// and hour, for keying cached predictions.
pub(crate) fn assumptions(engine_name: &str, tx: LatLon, ssn: f64, request: &CoverageRequest) -> String {
    let rx = &request.rx_station;
    let tx_station = &request.tx_station;
    format!(
        "{}|{:.2},{:.2}|ssn {:.1}|tx {} W {} {} dB {}°|rx {} W {} {} dB {}°|{:?}|{}%",
        engine_name,
        tx.lat,
        tx.lon,
        ssn,
        tx_station.power_watts,
        tx_station.antenna,
        tx_station.noise_db,
        tx_station.min_angle_deg,
        rx.power_watts,
        rx.antenna,
        rx.noise_db,
        rx.min_angle_deg,
        request.mode,
        request.required_reliability_pct,
    )
}

/// Predicts from `tx` to every point at the hour in `base`, with both
/// antennas aimed at the centre of each point's sector. Points in the same
/// sectors share engine runs, which several workers take in turn, and
/// `progress` hears how many runs are done of how many.
pub(crate) fn predict_points(
    engine: &(dyn PropagationEngine + Sync),
    request: &CoverageRequest,
    base: &PredictionRequest,
    tx: LatLon,
    points: &[LatLon],
    progress: &(dyn Fn(usize, usize) + Sync),
) -> Result<Vec<HourPrediction>, String> {
    let groups = group_by_sector(
        tx,
        points,
        station::azimuth_period_deg(&request.tx_station.antenna),
        station::azimuth_period_deg(&request.rx_station.antenna),
    );
    let runs: Vec<(&Group, &[usize])> =
        groups.iter().flat_map(|g| g.members.chunks(RECEIVERS_PER_RUN).map(move |chunk| (g, chunk))).collect();
    let predict_run = |(group, members): &(&Group, &[usize])| -> Result<Vec<HourPrediction>, String> {
        let mut request = base.clone();
        request.tx_antenna.bearing_deg = group.tx_bearing_deg;
        request.rx_antenna.bearing_deg = group.rx_bearing_deg;
        let receivers: Vec<LatLon> = members.iter().map(|&i| points[i]).collect();
        engine.predict_hour_to_many(&request, &receivers)
    };

    // Workers take runs from a shared counter until none are left.
    let next = AtomicUsize::new(0);
    let done = AtomicUsize::new(0);
    progress(0, runs.len());
    let predicted: Mutex<Vec<Option<HourPrediction>>> = Mutex::new(vec![None; points.len()]);
    let workers = std::thread::available_parallelism()
        .map_or(4, |n| n.get())
        .min(MAX_WORKERS)
        .min(runs.len().max(1));
    std::thread::scope(|scope| {
        let handles: Vec<_> = (0..workers)
            .map(|_| {
                scope.spawn(|| -> Result<(), String> {
                    while let Some(run) = runs.get(next.fetch_add(1, Ordering::Relaxed)) {
                        let hours = predict_run(run)?;
                        let mut predicted = predicted.lock().map_err(|_| "a coverage worker panicked")?;
                        for (&i, hour) in run.1.iter().zip(hours) {
                            predicted[i] = Some(hour);
                        }
                        drop(predicted);
                        progress(done.fetch_add(1, Ordering::Relaxed) + 1, runs.len());
                    }
                    Ok(())
                })
            })
            .collect();
        handles.into_iter().try_for_each(predictor::join)
    })?;
    predicted
        .into_inner()
        .map_err(|_| "a coverage worker panicked")?
        .into_iter()
        .map(|hour| hour.ok_or_else(|| "a point was not predicted".to_string()))
        .collect()
}

/// What every cell of a coverage map shares, worked out without the engine.
struct Area {
    tx: LatLon,
    centres: Vec<LatLon>,
    plan: predictor::Plan,
    base: PredictionRequest,
}

fn area(request: &CoverageRequest, grid: Grid) -> Result<Area, String> {
    let tx = geo::parse_position(&request.tx_position).map_err(|e| format!("Transmitter: {e}"))?;
    let centres = predicted_centres(tx, grid);
    let plan = plan_area(request, centres[0])?;
    let base = PredictionRequest { utc_hour: Some(request.utc_hour), ..plan.engine_request.clone() };
    Ok(Area { tx, centres, plan, base })
}

impl Area {
    fn coverage(self, request: &CoverageRequest, grid: Grid, cells: Vec<CoverageCell>) -> Coverage {
        Coverage {
            tx: self.tx,
            utc_hour: request.utc_hour,
            lat_step_deg: grid.lat_step_deg,
            lon_step_deg: grid.lon_step_deg,
            ssn: self.plan.ssn,
            required_snr_db_hz: self.base.required_snr_db_hz,
            bands: HF_BANDS.to_vec(),
            cells,
        }
    }

    /// The cache key: everything but the month and hour.
    fn key(&self, engine_name: &str, request: &CoverageRequest, grid: Grid) -> String {
        format!(
            "coverage {}x{}|{}",
            grid.lat_step_deg,
            grid.lon_step_deg,
            assumptions(engine_name, self.tx, self.plan.ssn.value, request)
        )
    }
}

/// A cell's place in the cache.
fn cell_name(centre: LatLon) -> String {
    format!("{:.1},{:.1}", centre.lat, centre.lon)
}

pub fn predict_coverage(
    engine: &(dyn PropagationEngine + Sync),
    request: &CoverageRequest,
    grid: Grid,
    progress: &(dyn Fn(usize, usize) + Sync),
) -> Result<Coverage, String> {
    let area = area(request, grid)?;
    let predicted = predict_points(engine, request, &area.base, area.tx, &area.centres, progress)?;
    let cells = area
        .centres
        .iter()
        .zip(predicted)
        .map(|(&centre, hour)| CoverageCell {
            lat: centre.lat,
            lon: centre.lon,
            distance_km: geo::distance_km(area.tx, centre),
            reliability: hour.frequencies.iter().map(|f| f.reliability).collect(),
            snr_db: hour.frequencies.iter().map(|f| f.snr_db).collect(),
        })
        .collect();
    Ok(area.coverage(request, grid, cells))
}

/// The map from the cache, when every cell of it is there.
pub fn cached_coverage(
    engine_name: &str,
    db: &Database,
    request: &CoverageRequest,
    grid: Grid,
) -> Result<Option<Coverage>, String> {
    let area = area(request, grid)?;
    let key = area.key(engine_name, request, grid);
    let mut stored = db.predicted_reliability_at(&key, request.year, request.month, request.utc_hour % 24)?;
    let bands = HF_BANDS.len();
    let mut cells = Vec::with_capacity(area.centres.len());
    for &centre in &area.centres {
        // Reliability for each band, then SNR for each band.
        let Some(mut values) = stored.remove(&cell_name(centre)) else { return Ok(None) };
        if values.len() != 2 * bands {
            return Ok(None);
        }
        let snr_db = values.split_off(bands);
        cells.push(CoverageCell {
            lat: centre.lat,
            lon: centre.lon,
            distance_km: geo::distance_km(area.tx, centre),
            reliability: values,
            snr_db,
        });
    }
    Ok(Some(area.coverage(request, grid, cells)))
}

/// The map from the cache, or predicted and then kept there.
pub fn predict_coverage_cached(
    engine: &(dyn PropagationEngine + Sync),
    db: &Database,
    request: &CoverageRequest,
    grid: Grid,
    progress: &(dyn Fn(usize, usize) + Sync),
) -> Result<Coverage, String> {
    if let Some(coverage) = cached_coverage(engine.name(), db, request, grid)? {
        return Ok(coverage);
    }
    let coverage = predict_coverage(engine, request, grid, progress)?;
    let key = area(request, grid)?.key(engine.name(), request, grid);
    let rows: Vec<(u32, String, Vec<f64>)> = coverage
        .cells
        .iter()
        .map(|cell| {
            let values = cell.reliability.iter().chain(&cell.snr_db).copied().collect();
            (request.utc_hour % 24, cell_name(LatLon { lat: cell.lat, lon: cell.lon }), values)
        })
        .collect();
    db.store_predicted_reliability(&key, request.year, request.month, &rows)?;
    Ok(coverage)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{engine_root, scratch_dir};
    use crate::voacap::VoacaplEngine;

    fn request() -> CoverageRequest {
        let presets = station::presets();
        CoverageRequest {
            tx_position: "EM73tr".into(),
            year: 2026,
            month: 10,
            ssn: None,
            tx_station: presets[0].clone(),
            rx_station: presets[0].clone(),
            mode: Mode::Ft8,
            required_reliability_pct: 90.0,
            utc_hour: 14,
        }
    }

    fn engine(name: &str) -> VoacaplEngine {
        VoacaplEngine::new(&engine_root(), &scratch_dir(name))
            .expect("engine not built; run engines/voacapl/build.sh")
    }

    #[test]
    fn grid_covers_the_globe_without_touching_the_poles() {
        let centres = grid_centres(COARSE_GRID);
        assert_eq!(centres.len(), 18 * 24);
        assert_eq!(centres[0], LatLon { lat: -85.0, lon: -172.5 });
        assert_eq!(centres[centres.len() - 1], LatLon { lat: 85.0, lon: 172.5 });

        let fine = grid_centres(FINE_GRID);
        assert_eq!(fine.len(), 45 * 90);
        assert_eq!(fine[0], LatLon { lat: -88.0, lon: -178.0 });
        assert_eq!(fine[fine.len() - 1], LatLon { lat: 88.0, lon: 178.0 });
    }

    #[test]
    fn sectors_follow_the_antenna_pattern() {
        // Omnidirectional: every bearing is the same.
        assert_eq!(sector_bearing(123.0, 0.0), 0.0);
        // A dipole repeats every 180 degrees, so 10 and 190 share a sector.
        assert_eq!(sector_bearing(10.0, 180.0), 22.5);
        assert_eq!(sector_bearing(190.0, 180.0), 22.5);
        assert_eq!(sector_bearing(100.0, 180.0), 112.5);
        // Unknown patterns use the full circle.
        assert_eq!(sector_bearing(190.0, 360.0), 202.5);
        assert_eq!(sector_bearing(359.9, 360.0), 337.5);
    }

    #[test]
    fn groups_hold_every_cell_once() {
        let centres = grid_centres(COARSE_GRID);
        let tx = LatLon { lat: 33.75, lon: -84.39 };

        let dipoles = group_by_sector(tx, &centres, 180.0, 180.0);
        assert!(dipoles.len() <= 16, "{} groups", dipoles.len());
        let mut seen: Vec<usize> = dipoles.iter().flat_map(|g| g.members.clone()).collect();
        seen.sort_unstable();
        assert_eq!(seen, (0..centres.len()).collect::<Vec<_>>());

        // Omnidirectional antennas at both ends need a single run.
        assert_eq!(group_by_sector(tx, &centres, 0.0, 0.0).len(), 1);
    }

    #[test]
    fn predicts_every_cell_with_the_real_engine() {
        let started = std::time::Instant::now();
        let coverage = predict_coverage(&engine("coverage"), &request(), COARSE_GRID, &|_, _| {}).unwrap();
        println!("coverage of {} cells took {:?}", coverage.cells.len(), started.elapsed());

        // Atlanta is at a cell corner, so no centre is within 100 km of it.
        assert_eq!(coverage.cells.len(), 18 * 24);
        assert_eq!(coverage.bands.len(), HF_BANDS.len());
        for cell in &coverage.cells {
            assert_eq!(cell.reliability.len(), HF_BANDS.len());
            assert!(cell.reliability.iter().all(|r| (0.0..=1.0).contains(r)));
        }
        // At 14 UTC, FT8 at 100 W reaches somewhere reliably on some band,
        // and not everywhere on every band.
        let all = || coverage.cells.iter().flat_map(|c| &c.reliability);
        assert!(all().any(|&r| r > 0.8));
        assert!(all().any(|&r| r < 0.1));
    }

    /// A cell's value must agree with a point-to-point prediction to the same
    /// place, to within the sector approximation.
    #[test]
    fn agrees_with_point_to_point_prediction() {
        let engine = engine("coverage-p2p");
        let request = request();
        let coverage = predict_coverage(&engine, &request, COARSE_GRID, &|_, _| {}).unwrap();
        let cell = coverage
            .cells
            .iter()
            .find(|c| c.lat == 55.0 && c.lon == 7.5)
            .expect("grid has a cell over northern Europe");

        let path = PathRequest {
            tx_position: request.tx_position.clone(),
            rx_position: "55.0, 7.5".into(),
            year: request.year,
            month: request.month,
            ssn: None,
            tx_station: request.tx_station.clone(),
            rx_station: request.rx_station.clone(),
            mode: request.mode,
            required_reliability_pct: request.required_reliability_pct,
            long_path: false,
        };
        let direct = predictor::predict_path(&engine, &path).unwrap();
        let hour = direct
            .run
            .prediction
            .hours
            .iter()
            .find(|h| h.utc_hour == request.utc_hour)
            .unwrap();
        for (band, f) in hour.frequencies.iter().enumerate() {
            assert!(
                (cell.snr_db[band] - f.snr_db).abs() <= 3.0,
                "band {band}: coverage SNR {} vs point-to-point {}",
                cell.snr_db[band],
                f.snr_db
            );
        }
    }

    /// The fine map is predicted once, kept, and read back the same; another
    /// hour or station is not mistaken for it.
    #[test]
    fn fine_coverage_is_cached() {
        let engine = engine("coverage-fine");
        let db = Database::in_memory().unwrap();
        let request = request();
        assert!(cached_coverage(engine.name(), &db, &request, FINE_GRID).unwrap().is_none());

        let runs = Mutex::new((0, 0));
        let started = std::time::Instant::now();
        let fine = predict_coverage_cached(&engine, &db, &request, FINE_GRID, &|done, total| {
            *runs.lock().unwrap() = (done, total);
        })
        .unwrap();
        println!("fine coverage of {} cells took {:?}", fine.cells.len(), started.elapsed());
        let (done, total) = *runs.lock().unwrap();
        assert!(total > 0 && done == total, "{done} of {total} runs");
        assert_eq!((fine.lat_step_deg, fine.lon_step_deg), (4.0, 4.0));
        // Atlanta is 150 km from the nearest centre, so every cell is predicted.
        assert_eq!(fine.cells.len(), 45 * 90);

        let started = std::time::Instant::now();
        let again = cached_coverage(engine.name(), &db, &request, FINE_GRID).unwrap().expect("kept");
        println!("read from the cache in {:?}", started.elapsed());
        assert_eq!(again.cells.len(), fine.cells.len());
        for (a, b) in again.cells.iter().zip(&fine.cells) {
            assert_eq!((a.lat, a.lon), (b.lat, b.lon));
            assert_eq!(a.reliability, b.reliability);
            assert_eq!(a.snr_db, b.snr_db);
        }
        assert_eq!(again.required_snr_db_hz, fine.required_snr_db_hz);

        let other_hour = CoverageRequest { utc_hour: 15, ..request.clone() };
        assert!(cached_coverage(engine.name(), &db, &other_hour, FINE_GRID).unwrap().is_none());
        let mut other_station = request.clone();
        other_station.tx_station.power_watts = 5.0;
        assert!(cached_coverage(engine.name(), &db, &other_station, FINE_GRID).unwrap().is_none());
        assert!(cached_coverage(engine.name(), &db, &request, COARSE_GRID).unwrap().is_none());
    }

    /// Writes real coarse and fine maps for the documentation screenshots:
    /// HFP_DUMP=file HFP_HOUR=1-24 cargo test dump_coverage -- --ignored
    #[test]
    #[ignore]
    fn dump_coverage() {
        let Ok(path) = std::env::var("HFP_DUMP") else { return };
        let engine = engine("coverage-dump");
        let utc_hour = std::env::var("HFP_HOUR").ok().and_then(|h| h.parse().ok()).unwrap_or(14);
        let request = CoverageRequest { utc_hour, ..request() };
        let coarse = predict_coverage(&engine, &request, COARSE_GRID, &|_, _| {}).unwrap();
        let fine = predict_coverage(&engine, &request, FINE_GRID, &|_, _| {}).unwrap();
        let both = serde_json::json!({ "coarse": coarse, "fine": fine });
        std::fs::write(path, both.to_string()).unwrap();
    }

    #[test]
    fn reports_bad_input() {
        let engine = engine("coverage-bad");
        let predict = |request: &CoverageRequest| predict_coverage(&engine, request, COARSE_GRID, &|_, _| {});
        let mut bad = request();
        bad.tx_position = "nowhere".into();
        assert!(predict(&bad).unwrap_err().starts_with("Transmitter:"));

        let mut bad = request();
        bad.year = 2100;
        assert!(predict(&bad).unwrap_err().contains("sunspot"));

        let mut bad = request();
        bad.utc_hour = 25;
        assert!(predict(&bad).unwrap_err().contains("hour 25"));
    }
}
