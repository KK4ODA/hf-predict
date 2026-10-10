//! Checks predictions against what has actually been heard.
//!
//! For every received decode with a locator, the model's FT8 reliability for
//! that path at that hour and month is looked up. Hours in which the receiver
//! was listening on a band but did not hear a locator it heard at other times
//! that month count as misses for that locator. Reliability is the share of
//! days on which a path carries the mode; a station is only heard if it was
//! also transmitting, so the rate of hearing is always lower than the
//! reliability. The shape is what matters: hearing should rise with
//! predicted reliability, and a flat curve would mean the model tells the
//! operator nothing.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt::Write;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use serde::Serialize;

use crate::geo::{self, LatLon};
use crate::observations::Observation;
use crate::predictor::{self, PathRequest};
use crate::propagation::{PredictionRequest, PropagationEngine};
use crate::solar;
use crate::station::{Mode, StationProfile, HF_BANDS};
use crate::timeutil;

/// Circuits per engine run.
const RECEIVERS_PER_RUN: usize = 60;
const MAX_WORKERS: usize = 8;
/// Equal-width bins of predicted reliability from 0 to 1.
pub const BINS: usize = 10;

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Bin {
    /// Decodes whose path had a predicted reliability in this bin.
    pub decodes: usize,
    /// Listening hours in which a locator heard that month could have been heard.
    pub opportunities: usize,
    /// Those in which it was.
    pub heard: usize,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    /// Paths and hours predicted.
    pub circuits: usize,
    pub decodes_used: usize,
    /// Decodes without a locator, on a band the model does not cover, or in a
    /// month the sunspot table does not cover.
    pub decodes_skipped: usize,
    pub overall: Vec<Bin>,
    pub by_band: BTreeMap<String, Vec<Bin>>,
}

fn bin_of(reliability: f64) -> usize {
    ((reliability.clamp(0.0, 1.0) * BINS as f64) as usize).min(BINS - 1)
}

/// `rx_position` is where the observations were made. `station` stands for
/// both ends: the heard stations' equipment is unknown, so the same typical
/// station is assumed everywhere and only the shape of the result is read.
pub fn calibrate(
    engine: &(dyn PropagationEngine + Sync),
    rx_position: &str,
    station: &StationProfile,
    observations: &[Observation],
) -> Result<Report, String> {
    let band_index: HashMap<&str, usize> =
        HF_BANDS.iter().enumerate().map(|(i, b)| (b.name, i)).collect();
    let mut report = Report::default();

    // Which (day, hour) slots the receiver listened on each band, and which
    // of them carried a decode from each locator.
    type Slot = (u32, u32);
    let mut listening: HashMap<(i32, u32, usize), BTreeSet<Slot>> = HashMap::new();
    let mut heard: BTreeMap<(i32, u32, usize, String), BTreeSet<Slot>> = BTreeMap::new();
    let mut decodes: Vec<(i32, u32, u32, usize, String)> = Vec::new();
    for o in observations {
        let (year, month, day, hour) = timeutil::civil(o.time_utc);
        let Some(&band) = band_index.get(o.band.as_str()) else {
            report.decodes_skipped += 1;
            continue;
        };
        listening.entry((year, month, band)).or_default().insert((day, hour));
        let grid = o.grid.as_deref().filter(|g| geo::from_maidenhead(g).is_ok());
        let Some(grid) = grid else {
            report.decodes_skipped += 1;
            continue;
        };
        heard.entry((year, month, band, grid.to_string())).or_default().insert((day, hour));
        decodes.push((year, month, hour, band, grid.to_string()));
    }

    // One engine run per month, hour and group of locators.
    let mut months: BTreeMap<(i32, u32), BTreeSet<&str>> = BTreeMap::new();
    for (year, month, _, grid) in heard.keys() {
        months.entry((*year, *month)).or_default().insert(grid);
    }
    struct Job<'a> {
        year: i32,
        month: u32,
        hour: u32,
        grids: Vec<&'a str>,
    }
    let mut requests: BTreeMap<(i32, u32), PredictionRequest> = BTreeMap::new();
    let mut jobs = Vec::new();
    for ((year, month), grids) in &months {
        if solar::smoothed_ssn(*year, *month).is_none() {
            continue;
        }
        let plan = predictor::plan(&PathRequest {
            tx_position: rx_position.to_string(),
            rx_position: rx_position.to_string(),
            year: *year,
            month: *month,
            ssn: None,
            tx_station: station.clone(),
            rx_station: station.clone(),
            mode: Mode::Ft8,
            required_reliability_pct: 90.0,
            long_path: false,
        })?;
        requests.insert((*year, *month), plan.engine_request);
        let grids: Vec<&str> = grids.iter().copied().collect();
        for hour in 1..=24 {
            for chunk in grids.chunks(RECEIVERS_PER_RUN) {
                jobs.push(Job { year: *year, month: *month, hour, grids: chunk.to_vec() });
            }
        }
    }

    // Workers take jobs from a shared counter until none are left.
    let next = AtomicUsize::new(0);
    let table: Mutex<HashMap<(i32, u32, u32, String), Vec<f64>>> = Mutex::new(HashMap::new());
    let workers = std::thread::available_parallelism()
        .map_or(4, |n| n.get())
        .min(MAX_WORKERS)
        .min(jobs.len().max(1));
    std::thread::scope(|scope| {
        let handles: Vec<_> = (0..workers)
            .map(|_| {
                scope.spawn(|| -> Result<(), String> {
                    while let Some(job) = jobs.get(next.fetch_add(1, Ordering::Relaxed)) {
                        let base = &requests[&(job.year, job.month)];
                        let request = PredictionRequest { utc_hour: Some(job.hour), ..base.clone() };
                        let receivers: Vec<LatLon> =
                            job.grids.iter().map(|g| geo::from_maidenhead(g)).collect::<Result<_, _>>()?;
                        let hours = engine.predict_hour_to_many(&request, &receivers)?;
                        let mut table = table.lock().map_err(|_| "a calibration worker panicked")?;
                        for (grid, hour) in job.grids.iter().zip(hours) {
                            let reliability = hour.frequencies.iter().map(|f| f.reliability).collect();
                            table.insert((job.year, job.month, job.hour % 24, grid.to_string()), reliability);
                        }
                    }
                    Ok(())
                })
            })
            .collect();
        for handle in handles {
            handle.join().map_err(|_| "a calibration worker panicked".to_string())??;
        }
        Ok::<(), String>(())
    })?;
    let table = table.into_inner().map_err(|_| "a calibration worker panicked")?;

    let mut overall = vec![Bin::default(); BINS];
    let mut by_band: BTreeMap<String, Vec<Bin>> =
        HF_BANDS.iter().map(|b| (b.name.to_string(), vec![Bin::default(); BINS])).collect();
    for (year, month, hour, band, grid) in &decodes {
        let Some(reliability) = table.get(&(*year, *month, *hour, grid.clone())) else {
            report.decodes_skipped += 1;
            continue;
        };
        let bin = bin_of(reliability[*band]);
        overall[bin].decodes += 1;
        by_band.get_mut(HF_BANDS[*band].name).expect("every band has bins")[bin].decodes += 1;
        report.decodes_used += 1;
    }
    for ((year, month, band, grid), slots) in &heard {
        let Some(listened) = listening.get(&(*year, *month, *band)) else { continue };
        for (day, hour) in listened {
            let Some(reliability) = table.get(&(*year, *month, *hour, grid.clone())) else { continue };
            let bin = bin_of(reliability[*band]);
            let was_heard = slots.contains(&(*day, *hour));
            let band_bins = by_band.get_mut(HF_BANDS[*band].name).expect("every band has bins");
            for bins in [&mut overall, band_bins] {
                bins[bin].opportunities += 1;
                if was_heard {
                    bins[bin].heard += 1;
                }
            }
        }
    }
    by_band.retain(|_, bins| bins.iter().any(|b| b.opportunities > 0));
    report.circuits = table.len();
    report.overall = overall;
    report.by_band = by_band;
    Ok(report)
}

/// The report as text tables.
pub fn render(report: &Report) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "{} circuits predicted; {} decodes used, {} skipped",
        report.circuits, report.decodes_used, report.decodes_skipped
    );
    let mut table = |title: &str, bins: &[Bin]| {
        let decodes: usize = bins.iter().map(|b| b.decodes).sum();
        let _ = writeln!(out, "\n{title}");
        let _ = writeln!(out, "predicted     decodes   share   listening hours   heard    rate");
        for (i, bin) in bins.iter().enumerate() {
            let share = if decodes == 0 { 0.0 } else { 100.0 * bin.decodes as f64 / decodes as f64 };
            let rate = if bin.opportunities == 0 {
                "     —".to_string()
            } else {
                format!("{:5.1}%", 100.0 * bin.heard as f64 / bin.opportunities as f64)
            };
            let _ = writeln!(
                out,
                "{:>3}–{:<3}%  {:>9}  {:5.1}%  {:>16}  {:>7}  {rate}",
                i * 100 / BINS,
                (i + 1) * 100 / BINS,
                bin.decodes,
                share,
                bin.opportunities,
                bin.heard
            );
        }
    };
    table("All bands", &report.overall);
    for (band, bins) in &report.by_band {
        table(band, bins);
    }
    out
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::observations::Database;
    use crate::propagation::{EngineRun, FrequencyPrediction, HourPrediction};
    use crate::station::{presets, ISOTROPE};
    use crate::wsjtx::tracker::{GridMemory, Heard};

    /// Paths north of 40° are good, the rest poor, on every band.
    struct Stub;

    impl PropagationEngine for Stub {
        fn name(&self) -> &str {
            "stub"
        }

        fn predict(&self, _: &PredictionRequest) -> Result<EngineRun, String> {
            Err("not used".into())
        }

        fn predict_hour_to_many(
            &self,
            request: &PredictionRequest,
            receivers: &[LatLon],
        ) -> Result<Vec<HourPrediction>, String> {
            Ok(receivers
                .iter()
                .map(|rx| {
                    let frequency = |mhz: f64| FrequencyPrediction {
                        freq_mhz: mhz,
                        reliability: if rx.lat > 40.0 { 0.95 } else { 0.05 },
                        ..FrequencyPrediction::default()
                    };
                    HourPrediction {
                        utc_hour: request.utc_hour.unwrap_or(1),
                        muf_mhz: 0.0,
                        at_muf: frequency(0.0),
                        frequencies: request.frequencies_mhz.iter().map(|&m| frequency(m)).collect(),
                    }
                })
                .collect())
        }
    }

    fn typical() -> StationProfile {
        let mut station = presets()[0].clone();
        station.antenna = ISOTROPE.into();
        station
    }

    #[test]
    fn counts_decodes_and_listening_hours_by_predicted_reliability() {
        let db = Arc::new(Database::in_memory().unwrap());
        let mut grids = GridMemory::new(db.clone());
        let at = |day: u32, df: u32, message: &'static str| Heard {
            time_utc: timeutil::from_utc(2026, 5, day, 14, 0),
            dial_hz: 14_074_000,
            mode: "FT8",
            df_hz: df,
            snr_db: -10,
            dt_s: 0.1,
            message,
            rx_grid: Some("EM73"),
            provider: "test",
            low_confidence: false,
            settling: false,
        };
        // Day 1: a northern and a southern station; day 2: the northern one
        // only, plus a decode without a locator that still proves listening.
        for heard in [
            at(1, 1000, "CQ K1ABC FN42"),
            at(1, 1200, "CQ W4XYZ EL88"),
            at(2, 1000, "CQ K1ABC FN42"),
            at(2, 1500, "TNX 73 GL"),
        ] {
            grids.store(&heard).unwrap();
        }

        let report = calibrate(&Stub, "EM73", &typical(), &db.all().unwrap()).unwrap();

        assert_eq!((report.decodes_used, report.decodes_skipped), (3, 1));
        assert_eq!(report.circuits, 2 * 24);
        let good = &report.overall[BINS - 1];
        let poor = &report.overall[0];
        assert_eq!((good.decodes, good.opportunities, good.heard), (2, 2, 2));
        assert_eq!((poor.decodes, poor.opportunities, poor.heard), (1, 2, 1));
        assert_eq!(report.by_band.keys().collect::<Vec<_>>(), ["20 m"]);
        assert_eq!(report.by_band["20 m"][0], *poor);
        let text = render(&report);
        assert!(text.contains("90–100%          2   66.7%                 2        2  100.0%"), "{text}");
    }

    /// Runs over a real log: `HFP_ALL_TXT=path [HFP_RX_GRID=EM73] cargo test
    /// calibration::tests::real_log -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn real_log() {
        use crate::voacap::VoacaplEngine;
        use crate::wsjtx::alltxt;
        use std::path::PathBuf;

        let Ok(path) = std::env::var("HFP_ALL_TXT") else {
            return;
        };
        let rx_grid = std::env::var("HFP_RX_GRID").unwrap_or_else(|_| "EM73".into());
        let engine_root = std::env::var_os("HFP_ENGINE_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.work/engine"));
        let scratch = std::env::temp_dir().join(format!("hfp-calibration-{}", std::process::id()));
        let engine = VoacaplEngine::new(&engine_root, &scratch).expect("engine not built");

        let text = String::from_utf8_lossy(&std::fs::read(&path).unwrap()).into_owned();
        let db = Arc::new(Database::in_memory().unwrap());
        alltxt::import(&db, &text, Some(&rx_grid)).unwrap();
        let observations = db.all().unwrap();

        let started = std::time::Instant::now();
        let report = calibrate(&engine, &rx_grid, &typical(), &observations).unwrap();
        println!("{} observations in {:.0} s", observations.len(), started.elapsed().as_secs_f64());
        println!("{}", render(&report));
    }
}
