//! Which band reaches the most stations, for operators who want many
//! contacts rather than one place.
//!
//! The stations are the ones this receiver has heard, placed by the locator
//! each last sent. For each band and hour the model's reliability from this
//! station to every locator square they are in is summed over the stations:
//! the number of them a signal should reach on a typical day of the month.
//! By default only stations heard at that time of day (within an hour
//! either side, on any day and band) count, so the sum follows when people
//! are on the air as well as where they are.
//!
//! Predictions are cached in the observation database by hour and square,
//! so stepping the hour or asking again costs nothing once computed.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicUsize, Ordering};

use serde::{Deserialize, Serialize};

use crate::coverage::{self, CoverageRequest};
use crate::geo::{self, LatLon};
use crate::observations::{Database, StationPlace};
use crate::propagation::{PredictionRequest, PropagationEngine};
use crate::station::HF_BANDS;

/// Distances at which reach is broken down, in km.
pub const DISTANCE_EDGES_KM: [f64; 3] = [1000.0, 3000.0, 8000.0];
/// Clock hours of listening, within an hour either side, below which "on at
/// this time of day" says more about the log than about the stations: then
/// every station counts.
pub const MIN_LISTENED_HOURS: u32 = 3;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContactsQuery {
    /// The station, mode and month; its `utc_hour` is ignored.
    pub area: CoverageRequest,
    /// UTC clock hours to work out, 0 to 23.
    pub hours: Vec<u32>,
    /// Count only stations heard at that time of day.
    pub time_of_day: bool,
    /// How far back the log counts, in days; none for all of it.
    pub history_days: Option<u32>,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BandReach {
    pub band: String,
    /// Stations a signal should reach on a typical day: the sum of the
    /// model's reliability to each one.
    pub expected: f64,
    /// The same, by distance: under 1000 km, 1000 to 3000, 3000 to 8000, over 8000.
    pub by_distance: [f64; 4],
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HourReach {
    pub clock_hour: u32,
    /// Stations counted at this hour.
    pub stations: usize,
    /// Only stations heard at this time of day were counted.
    pub time_of_day: bool,
    /// Clock hours this receiver listened in, within an hour either side.
    pub listened_hours: u32,
    pub bands: Vec<BandReach>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContactsReport {
    /// Stations in the log with a locator, in the span asked for.
    pub stations_in_log: usize,
    /// Of those, the ones within 100 km, which the sky-wave model leaves out.
    pub too_near: usize,
    pub squares: usize,
    pub ssn: f64,
    pub hours: Vec<HourReach>,
    /// Square-hours computed now, and read from the cache.
    pub computed: usize,
    pub cached: usize,
}

/// Whether a station heard in `hours` (a bit per UTC hour) is usually on at `hour`.
pub fn on_at(hours: u32, hour: u32) -> bool {
    [(hour + 23) % 24, hour % 24, (hour + 1) % 24].iter().any(|h| hours & (1 << h) != 0)
}

/// Which distance column a square falls in.
pub fn distance_column(km: f64) -> usize {
    DISTANCE_EDGES_KM.iter().take_while(|&&edge| km >= edge).count()
}

/// Sums reliability over the stations on at `clock_hour`, by band.
pub fn reach(
    places: &[(&StationPlace, usize)],
    reliability: &BTreeMap<usize, Vec<f64>>,
    distances: &[f64],
    clock_hour: u32,
    time_of_day: bool,
) -> HourReach {
    let mut bands: Vec<BandReach> =
        HF_BANDS.iter().map(|b| BandReach { band: b.name.to_string(), ..BandReach::default() }).collect();
    let mut stations = 0;
    for (place, square) in places {
        if time_of_day && !on_at(place.hours, clock_hour) {
            continue;
        }
        let Some(values) = reliability.get(square) else { continue };
        stations += 1;
        let column = distance_column(distances[*square]);
        for (band, value) in bands.iter_mut().zip(values) {
            band.expected += value;
            band.by_distance[column] += value;
        }
    }
    HourReach { clock_hour, stations, time_of_day, listened_hours: 0, bands }
}

/// Hours listened within an hour either side of `hour`.
pub fn listened_around(listened: &[u32; 24], hour: u32) -> u32 {
    [(hour + 23) % 24, hour % 24, (hour + 1) % 24].iter().map(|&h| listened[h as usize]).sum()
}

pub fn most_contacts(
    engine: &(dyn PropagationEngine + Sync),
    db: &Database,
    query: &ContactsQuery,
    own_call: Option<&str>,
    now: i64,
    progress: &(dyn Fn(usize, usize) + Sync),
) -> Result<ContactsReport, String> {
    let request = &query.area;
    let tx = geo::parse_position(&request.tx_position).map_err(|e| format!("From: {e}"))?;
    let since = query.history_days.map_or(0, |days| now - i64::from(days) * 86_400);
    let own = own_call.map(|c| c.trim().to_uppercase());

    // The stations, each in the locator square it last sent.
    let places: Vec<StationPlace> = db
        .station_places(since)?
        .into_iter()
        .filter(|p| own.as_deref() != Some(p.callsign.as_str()))
        .collect();
    let mut squares: Vec<String> = Vec::new();
    let mut index: BTreeMap<String, usize> = BTreeMap::new();
    let mut centres: Vec<LatLon> = Vec::new();
    let mut distances: Vec<f64> = Vec::new();
    let mut placed: Vec<(&StationPlace, usize)> = Vec::new();
    let mut too_near = 0;
    for place in &places {
        let square = place.grid.get(..4).unwrap_or(&place.grid).to_uppercase();
        let Ok(centre) = geo::from_maidenhead(&square) else { continue };
        let distance = geo::distance_km(tx, centre);
        if distance < coverage::MIN_DISTANCE_KM {
            too_near += 1;
            continue;
        }
        let i = *index.entry(square.clone()).or_insert_with(|| {
            squares.push(square);
            centres.push(centre);
            distances.push(distance);
            centres.len() - 1
        });
        placed.push((place, i));
    }
    let stations_in_log = placed.len() + too_near;
    if centres.is_empty() {
        return Ok(ContactsReport {
            stations_in_log,
            too_near,
            squares: 0,
            ssn: 0.0,
            hours: Vec::new(),
            computed: 0,
            cached: 0,
        });
    }

    let plan = coverage::plan_area(request, centres[0])?;
    let key = format!("contacts|{}", coverage::assumptions(engine.name(), tx, plan.ssn.value, request));
    let mut cache = db.predicted_reliability(&key, request.year, request.month)?;

    let hours: BTreeSet<u32> = query.hours.iter().map(|h| h % 24).collect();
    let missing: Vec<(u32, Vec<usize>)> = hours
        .iter()
        .map(|&hour| {
            let wanted: Vec<usize> =
                (0..centres.len()).filter(|&i| !cache.contains_key(&(hour, squares[i].clone()))).collect();
            (hour, wanted)
        })
        .filter(|(_, wanted)| !wanted.is_empty())
        .collect();
    let cached = hours.len() * centres.len() - missing.iter().map(|(_, w)| w.len()).sum::<usize>();
    let done = AtomicUsize::new(0);
    progress(0, missing.len());
    let mut computed = 0;
    for (hour, wanted) in &missing {
        // VOACAP numbers hours 1 to 24, where 24 is 00 UTC.
        let base = PredictionRequest {
            utc_hour: Some(if *hour == 0 { 24 } else { *hour }),
            ..plan.engine_request.clone()
        };
        let points: Vec<LatLon> = wanted.iter().map(|&i| centres[i]).collect();
        let predicted = coverage::predict_points(engine, request, &base, tx, &points, &|_, _| {})?;
        let rows: Vec<(u32, String, Vec<f64>)> = wanted
            .iter()
            .zip(predicted)
            .map(|(&i, p)| (*hour, squares[i].clone(), p.frequencies.iter().map(|f| f.reliability).collect()))
            .collect();
        db.store_predicted_reliability(&key, request.year, request.month, &rows)?;
        computed += rows.len();
        for (hour, square, values) in rows {
            cache.insert((hour, square), values);
        }
        progress(done.fetch_add(1, Ordering::Relaxed) + 1, missing.len());
    }

    let listened = db.listened_hours_by_hour(since)?;
    let hours = hours
        .iter()
        .map(|&hour| {
            let reliability: BTreeMap<usize, Vec<f64>> = (0..centres.len())
                .filter_map(|i| cache.get(&(hour, squares[i].clone())).map(|v| (i, v.clone())))
                .collect();
            let listened_hours = listened_around(&listened, hour);
            let time_of_day = query.time_of_day && listened_hours >= MIN_LISTENED_HOURS;
            HourReach { listened_hours, ..reach(&placed, &reliability, &distances, hour, time_of_day) }
        })
        .collect();
    Ok(ContactsReport {
        stations_in_log,
        too_near,
        squares: centres.len(),
        ssn: plan.ssn.value,
        hours,
        computed,
        cached,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::observations::Observation;
    use crate::station::{self, Mode};
    use crate::testing::{engine_root, scratch_dir};
    use crate::voacap::VoacaplEngine;

    /// Times a whole day over a copy of a real database:
    /// `HFP_DB=path/to/observations.db cargo test --lib contacts::tests::real_log -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn real_log() {
        let Ok(path) = std::env::var("HFP_DB") else { return };
        let dir = scratch_dir("contacts-real");
        std::fs::create_dir_all(&dir).unwrap();
        let copy = dir.join("observations.db");
        std::fs::copy(&path, &copy).unwrap();
        let db = Database::open(&copy).unwrap();
        let presets = station::presets();
        let query = ContactsQuery {
            area: CoverageRequest {
                tx_position: "EM73tr".into(),
                year: 2026,
                month: 10,
                ssn: None,
                tx_station: presets[0].clone(),
                rx_station: presets[0].clone(),
                mode: Mode::Ft8,
                required_reliability_pct: 90.0,
                utc_hour: 1,
            },
            hours: (0..24).collect(),
            time_of_day: true,
            history_days: None,
        };
        let engine = VoacaplEngine::new(&engine_root(), &scratch_dir("contacts-real-run")).unwrap();
        for round in ["first", "cached"] {
            let started = std::time::Instant::now();
            let report = most_contacts(&engine, &db, &query, None, crate::timeutil::now(), &|_, _| {}).unwrap();
            println!(
                "{round}: {} stations, {} too near, {} squares, {} computed, {} cached, in {:?}",
                report.stations_in_log, report.too_near, report.squares, report.computed, report.cached, started.elapsed()
            );
            if let (Ok(dump), "cached") = (std::env::var("HFP_DUMP"), round) {
                std::fs::write(dump, serde_json::to_string(&report).unwrap()).unwrap();
            }
            if round == "first" {
                for hour in report.hours.iter().filter(|h| h.clock_hour % 4 == 2 || h.clock_hour % 4 == 0) {
                    let mut bands = hour.bands.clone();
                    bands.sort_by(|a, b| b.expected.total_cmp(&a.expected));
                    let top: Vec<String> = bands.iter().take(3).map(|b| format!("{} {:.0}", b.band, b.expected)).collect();
                    println!(
                        "  {:02} UTC, {} counted ({}, {} h listened): {}",
                        hour.clock_hour,
                        hour.stations,
                        if hour.time_of_day { "on at this hour" } else { "any time" },
                        hour.listened_hours,
                        top.join(", ")
                    );
                }
            }
        }
    }

    #[test]
    fn a_station_is_on_within_an_hour_either_side() {
        let at = |hours: &[u32]| hours.iter().fold(0u32, |bits, h| bits | 1 << h);
        assert!(on_at(at(&[14]), 14));
        assert!(on_at(at(&[13]), 14));
        assert!(on_at(at(&[15]), 14));
        assert!(!on_at(at(&[16]), 14));
        assert!(on_at(at(&[23]), 0), "midnight wraps");
        assert!(on_at(at(&[0]), 23));
    }

    #[test]
    fn listening_counts_an_hour_either_side() {
        let mut listened = [0u32; 24];
        listened[13] = 1;
        listened[14] = 2;
        listened[0] = 5;
        assert_eq!(listened_around(&listened, 14), 3);
        assert_eq!(listened_around(&listened, 23), 5);
        assert_eq!(listened_around(&listened, 6), 0);
    }

    #[test]
    fn distance_columns() {
        assert_eq!(distance_column(500.0), 0);
        assert_eq!(distance_column(1000.0), 1);
        assert_eq!(distance_column(2999.0), 1);
        assert_eq!(distance_column(7000.0), 2);
        assert_eq!(distance_column(15000.0), 3);
    }

    #[test]
    fn reach_sums_reliability_over_the_stations_on_at_that_hour() {
        let place = |call: &str, hours: u32| StationPlace { callsign: call.into(), grid: "IO91".into(), hours };
        let (day, night) = (place("G4AAA", 1 << 14), place("G4BBB", 1 << 2));
        let places = [(&day, 0), (&night, 0)];
        let mut values = vec![0.0; HF_BANDS.len()];
        values[4] = 0.8;
        let reliability = BTreeMap::from([(0, values)]);
        let distances = [6770.0];
        let afternoon = reach(&places, &reliability, &distances, 14, true);
        assert_eq!(afternoon.stations, 1);
        assert!((afternoon.bands[4].expected - 0.8).abs() < 1e-9);
        assert!((afternoon.bands[4].by_distance[2] - 0.8).abs() < 1e-9);
        let anytime = reach(&places, &reliability, &distances, 14, false);
        assert_eq!(anytime.stations, 2);
        assert!((anytime.bands[4].expected - 1.6).abs() < 1e-9);
    }

    #[test]
    fn ranks_bands_from_the_log_with_the_real_engine_and_caches() {
        let db = Database::in_memory().unwrap();
        let heard = |time: i64, call: &str, grid: &str| Observation {
            time_utc: time,
            dial_hz: 14_074_000,
            band: "20 m".into(),
            df_hz: 1000,
            snr_db: -10,
            dt_s: 0.1,
            mode: "FT8".into(),
            message: format!("CQ {call} {grid}"),
            kind: "cq".into(),
            sender: Some(call.into()),
            addressee: None,
            grid: Some(grid.into()),
            grid_source: Some("message".into()),
            distance_km: None,
            bearing_deg: None,
            rx_grid: Some("EM73".into()),
            origin: crate::observations::ORIGIN_LOCAL.into(),
            provider: "test".into(),
            low_confidence: false,
            settling: false,
        };
        // 14 UTC on three days of October 2026: Europe, the US and Japan,
        // and a neighbour too near to model.
        let t = 1_790_863_200;
        for day in 0..3 {
            for (call, grid) in [("G4AAA", "IO91"), ("DL1BBB", "JO62"), ("K1CCC", "FN42"), ("JA1DDD", "PM95"), ("N4EEE", "EM73")] {
                db.insert(&heard(t + day * 86_400, call, grid)).unwrap();
            }
        }
        let presets = station::presets();
        let query = ContactsQuery {
            area: CoverageRequest {
                tx_position: "EM73tr".into(),
                year: 2026,
                month: 10,
                ssn: None,
                tx_station: presets[0].clone(),
                rx_station: presets[0].clone(),
                mode: Mode::Ft8,
                required_reliability_pct: 90.0,
                utc_hour: 1,
            },
            hours: vec![14, 2],
            time_of_day: true,
            history_days: None,
        };
        let engine = VoacaplEngine::new(&engine_root(), &scratch_dir("contacts")).expect("engine not built");
        let report = most_contacts(&engine, &db, &query, Some("kk4oda"), t + 3 * 86_400, &|_, _| {}).unwrap();
        assert_eq!((report.stations_in_log, report.too_near, report.squares), (5, 1, 4));
        assert_eq!((report.computed, report.cached), (8, 0));
        let afternoon = report.hours.iter().find(|h| h.clock_hour == 14).unwrap();
        assert_eq!(afternoon.stations, 4);
        assert!(afternoon.time_of_day);
        // Nothing was listened to near 02 UTC, so every station counts there.
        let night = report.hours.iter().find(|h| h.clock_hour == 2).unwrap();
        assert_eq!((night.stations, night.time_of_day, night.listened_hours), (4, false, 0));
        // In the afternoon some band reaches most of them, and none reaches more than there are.
        let best = afternoon.bands.iter().map(|b| b.expected).fold(0.0, f64::max);
        assert!(best > 2.0 && best <= 4.0, "{best}");
        for band in &afternoon.bands {
            assert!((band.by_distance.iter().sum::<f64>() - band.expected).abs() < 1e-9);
        }
        // A second run reads every prediction from the cache.
        let again = most_contacts(&engine, &db, &query, None, t + 3 * 86_400, &|_, _| {}).unwrap();
        assert_eq!((again.computed, again.cached), (0, 8));
    }
}
