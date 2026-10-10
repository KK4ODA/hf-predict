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
//!
//! Predictions are cached in the observation database, so a second run only
//! computes locators and months not seen before.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt::Write;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use serde::Serialize;

use crate::geo::{self, LatLon};
use crate::observations::Database;
use crate::predictor::{self, PathRequest};
use crate::propagation::{PredictionRequest, PropagationEngine};
use crate::solar;
use crate::station::{self, Mode, StationProfile, HF_BANDS, ISOTROPE};
use crate::timeutil;

/// Circuits per engine run.
const RECEIVERS_PER_RUN: usize = 60;
const MAX_WORKERS: usize = 8;
/// What a heard station is assumed to run, since its equipment is unknown.
const TYPICAL_POWER_WATTS: f64 = 100.0;
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
    /// Where the receiver was taken to be.
    pub receiver: String,
    /// Paths and hours with a prediction.
    pub circuits: usize,
    /// Of those, computed in this run rather than read from the cache.
    pub circuits_computed: usize,
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

/// The receiver is where most stored decodes were made, or `rx_position`
/// when none carries a locator. Heard stations are assumed to run a typical
/// power into an isotropic antenna, with the receiver's noise level
/// `noise_db`, and only the shape of the result is read. `progress` is told
/// how many engine runs are done out of how many.
pub fn calibrate(
    engine: &(dyn PropagationEngine + Sync),
    db: &Database,
    rx_position: &str,
    noise_db: f64,
    progress: &(dyn Fn(usize, usize) + Sync),
) -> Result<Report, String> {
    let observations = db.all()?;
    let band_index: HashMap<&str, usize> =
        HF_BANDS.iter().enumerate().map(|(i, b)| (b.name, i)).collect();
    let mut report = Report::default();

    let mut receivers: HashMap<&str, usize> = HashMap::new();
    for grid in observations.iter().filter_map(|o| o.rx_grid.as_deref()) {
        *receivers.entry(grid).or_default() += 1;
    }
    let receiver = receivers
        .into_iter()
        .max_by_key(|(_, n)| *n)
        .map_or(rx_position.trim().to_string(), |(grid, _)| grid.to_string());
    if receiver.is_empty() {
        return Err("No decode carries the receiver's locator; enter your position in the From field.".into());
    }
    report.receiver = receiver.clone();

    // Which (day, hour) slots the receiver listened on each band, and which
    // of them carried a decode from each locator.
    type Slot = (u32, u32);
    let mut listening: HashMap<(i32, u32, usize), BTreeSet<Slot>> = HashMap::new();
    let mut heard: BTreeMap<(i32, u32, usize, String), BTreeSet<Slot>> = BTreeMap::new();
    let mut decodes: Vec<(i32, u32, u32, usize, String)> = Vec::new();
    for interval in db.listening_intervals()? {
        let Some(&band) = band_index.get(interval.band.as_str()) else { continue };
        let mut time = interval.start_utc - interval.start_utc.rem_euclid(3600);
        while time < interval.end_utc {
            let (year, month, day, hour) = timeutil::civil(time);
            listening.entry((year, month, band)).or_default().insert((day, hour));
            time += 3600;
        }
    }
    for o in &observations {
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

    // Predictions by month, from the cache where possible, else one engine
    // run per hour and group of locators.
    let preset = station::presets().into_iter().next().ok_or("no station presets")?;
    let typical = |power_watts: f64, noise_db: f64| StationProfile {
        name: "typical".into(),
        power_watts,
        antenna: ISOTROPE.into(),
        noise_db,
        ..preset.clone()
    };
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
    let mut table: HashMap<(i32, u32, u32, String), Vec<f64>> = HashMap::new();
    let mut requests: BTreeMap<(i32, u32), (String, PredictionRequest)> = BTreeMap::new();
    let mut jobs = Vec::new();
    for ((year, month), grids) in &months {
        if solar::smoothed_ssn(*year, *month).is_none() {
            continue;
        }
        let plan = predictor::plan(&PathRequest {
            tx_position: receiver.clone(),
            rx_position: receiver.clone(),
            year: *year,
            month: *month,
            ssn: None,
            tx_station: typical(TYPICAL_POWER_WATTS, noise_db),
            rx_station: typical(TYPICAL_POWER_WATTS, noise_db),
            mode: Mode::Ft8,
            required_reliability_pct: 90.0,
            long_path: false,
        })?;
        let key = format!(
            "{}|{receiver}|ssn {:.1}|{TYPICAL_POWER_WATTS} W isotrope|noise {noise_db} dB",
            engine.name(),
            plan.ssn.value
        );
        let mut cached_hours: HashMap<&str, usize> = HashMap::new();
        for ((hour, grid), reliability) in db.predicted_reliability(&key, *year, *month)? {
            if let Some(&known) = grids.get(grid.as_str()) {
                *cached_hours.entry(known).or_default() += 1;
                table.insert((*year, *month, hour, grid), reliability);
            }
        }
        requests.insert((*year, *month), (key, plan.engine_request));
        let missing: Vec<&str> =
            grids.iter().copied().filter(|g| cached_hours.get(g) != Some(&24)).collect();
        for hour in 1..=24 {
            for chunk in missing.chunks(RECEIVERS_PER_RUN) {
                jobs.push(Job { year: *year, month: *month, hour, grids: chunk.to_vec() });
            }
        }
    }

    // Workers take jobs from a shared counter until none are left.
    let next = AtomicUsize::new(0);
    let done = AtomicUsize::new(0);
    let computed: Mutex<Vec<(i32, u32, u32, String, Vec<f64>)>> = Mutex::new(Vec::new());
    let workers = std::thread::available_parallelism()
        .map_or(4, |n| n.get())
        .min(MAX_WORKERS)
        .min(jobs.len().max(1));
    progress(0, jobs.len());
    std::thread::scope(|scope| {
        let handles: Vec<_> = (0..workers)
            .map(|_| {
                scope.spawn(|| -> Result<(), String> {
                    while let Some(job) = jobs.get(next.fetch_add(1, Ordering::Relaxed)) {
                        let (_, base) = &requests[&(job.year, job.month)];
                        let request = PredictionRequest { utc_hour: Some(job.hour), ..base.clone() };
                        let receivers: Vec<LatLon> =
                            job.grids.iter().map(|g| geo::from_maidenhead(g)).collect::<Result<_, _>>()?;
                        let hours = engine.predict_hour_to_many(&request, &receivers)?;
                        let mut computed = computed.lock().map_err(|_| "a calibration worker panicked")?;
                        for (grid, hour) in job.grids.iter().zip(hours) {
                            let reliability = hour.frequencies.iter().map(|f| f.reliability).collect();
                            computed.push((job.year, job.month, job.hour % 24, grid.to_string(), reliability));
                        }
                        progress(done.fetch_add(1, Ordering::Relaxed) + 1, jobs.len());
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
    let computed = computed.into_inner().map_err(|_| "a calibration worker panicked")?;
    report.circuits_computed = computed.len();
    let mut to_store: BTreeMap<(i32, u32), Vec<(u32, String, Vec<f64>)>> = BTreeMap::new();
    for (year, month, hour, grid, reliability) in computed {
        to_store.entry((year, month)).or_default().push((hour, grid.clone(), reliability.clone()));
        table.insert((year, month, hour, grid), reliability);
    }
    for ((year, month), rows) in &to_store {
        let (key, _) = &requests[&(*year, *month)];
        db.store_predicted_reliability(key, *year, *month, rows)?;
    }

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
        "receiver {}; {} circuits predicted ({} computed now); {} decodes used, {} skipped",
        report.receiver, report.circuits, report.circuits_computed, report.decodes_used, report.decodes_skipped
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
    use crate::propagation::{EngineRun, FrequencyPrediction, HourPrediction};
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

    fn sample() -> Arc<Database> {
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
        // Day 3: listened for ten minutes and heard nothing.
        let id = db.open_interval(timeutil::from_utc(2026, 5, 3, 14, 0), 14_074_000, "20 m", "FT8", "test").unwrap();
        db.extend_interval(id, timeutil::from_utc(2026, 5, 3, 14, 10)).unwrap();
        db
    }

    #[test]
    fn counts_decodes_and_listening_hours_by_predicted_reliability() {
        let db = sample();

        let report = calibrate(&Stub, &db, "", 145.0, &|_, _| {}).unwrap();

        assert_eq!(report.receiver, "EM73");
        assert_eq!((report.decodes_used, report.decodes_skipped), (3, 1));
        assert_eq!((report.circuits, report.circuits_computed), (2 * 24, 2 * 24));
        let good = &report.overall[BINS - 1];
        let poor = &report.overall[0];
        assert_eq!((good.decodes, good.opportunities, good.heard), (2, 3, 2));
        assert_eq!((poor.decodes, poor.opportunities, poor.heard), (1, 3, 1));
        assert_eq!(report.by_band.keys().collect::<Vec<_>>(), ["20 m"]);
        assert_eq!(report.by_band["20 m"][0], *poor);
        let text = render(&report);
        assert!(
            text.lines().any(|l| l.starts_with(" 90–100%") && l.ends_with("66.7%")),
            "{text}"
        );
    }

    #[test]
    fn a_second_run_reads_the_cache_and_reports_progress() {
        let db = sample();
        let runs = Mutex::new(Vec::new());
        let first = calibrate(&Stub, &db, "", 145.0, &|done, total| runs.lock().unwrap().push((done, total))).unwrap();
        let second = calibrate(&Stub, &db, "", 145.0, &|_, _| {}).unwrap();

        assert_eq!(first.circuits_computed, 48);
        assert_eq!(second.circuits_computed, 0);
        assert_eq!((second.circuits, second.overall), (first.circuits, first.overall));
        let runs = runs.into_inner().unwrap();
        assert_eq!(runs.first(), Some(&(0, 24)));
        assert_eq!(runs.last(), Some(&(24, 24)));

        // A different receiver noise level is a different assumption.
        let other = calibrate(&Stub, &db, "", 155.0, &|_, _| {}).unwrap();
        assert_eq!(other.circuits_computed, 48);
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

        let started = std::time::Instant::now();
        let report = calibrate(&engine, &db, &rx_grid, 145.0, &|_, _| {}).unwrap();
        println!("{} observations in {:.0} s", db.count().unwrap(), started.elapsed().as_secs_f64());
        println!("{}", render(&report));
    }
}
