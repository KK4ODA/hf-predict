//! Turns what the operator asks for (two positions, a month, two stations)
//! into an engine request, and returns the prediction with its provenance.

use serde::{Deserialize, Serialize};

use crate::geo::{self, LatLon};
use crate::propagation::{
    Antenna, Coefficients, EngineRun, FrequencyPrediction, PathKind, Prediction,
    PredictionRequest, PropagationEngine,
};
use crate::solar::{self, SsnUsed};
use crate::station::{Band, Mode, StationProfile, HF_BANDS, ISOTROPE};

/// VOACAP's authors recommend a 3 degree floor when an isotrope is used,
/// because it has no ground losses at low angles.
const MIN_ANGLE_ISOTROPE: f64 = 3.0;
const MAX_SSN: f64 = 300.0;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PathRequest {
    /// Locator or latitude, longitude.
    pub tx_position: String,
    pub rx_position: String,
    pub year: i32,
    pub month: u32,
    /// Sunspot number to use instead of the bundled table.
    pub ssn: Option<f64>,
    pub tx_station: StationProfile,
    pub rx_station: StationProfile,
    pub mode: Mode,
    pub required_reliability_pct: f64,
    pub long_path: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PathPrediction {
    pub tx: LatLon,
    pub rx: LatLon,
    pub tx_locator: String,
    pub rx_locator: String,
    /// Great-circle distance along the predicted path (short or long way round).
    pub distance_km: f64,
    /// Direction each antenna is aimed, along the predicted path.
    pub tx_bearing_deg: f64,
    pub rx_bearing_deg: f64,
    pub ssn: SsnUsed,
    pub required_snr_db_hz: f64,
    /// Bands in the same order as each hour's `frequencies`.
    pub bands: Vec<Band>,
    /// Name of the engine implementation that produced the run.
    pub engine: String,
    pub run: EngineRun,
}

/// Transmit powers compared against the station's own.
const POWER_LEVELS_WATTS: [f64; 4] = [5.0, 10.0, 50.0, 100.0];
/// Frequencies swept to locate the lowest usable and optimum working
/// frequencies. An engine run takes at most eleven.
const SWEEP_MHZ: [f64; 22] = [
    2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0, 11.0, 12.0, 13.0, 14.0, 15.0, 16.0, 18.0, 20.0,
    22.0, 24.0, 26.0, 28.0, 30.0,
];
const SWEEP_CHUNK: usize = 11;
/// The optimum working frequency is the one the path supports on this share of days.
const FOT_MUF_DAY: f64 = 0.9;

/// The main prediction's reliability and SNR at another transmit power.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PowerCase {
    pub power_watts: f64,
    /// `[hour][band]`, in the main prediction's order.
    pub reliability: Vec<Vec<f64>>,
    pub snr_db: Vec<Vec<f64>>,
}

/// The usable frequency range for one hour.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FrequencyWindow {
    pub utc_hour: u32,
    /// Median maximum usable frequency.
    pub muf_mhz: f64,
    /// Optimum working frequency: supported on 90% of days. `None` if below the sweep.
    pub fot_mhz: Option<f64>,
    /// Lowest frequency whose median SNR meets the requirement. `None` if none does.
    pub luf_mhz: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PathDetail {
    pub prediction: PathPrediction,
    pub power: Vec<PowerCase>,
    pub window: Vec<FrequencyWindow>,
}

/// Everything the results screen shows, for both ways round the earth.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PathOverview {
    pub short: PathDetail,
    pub long: PathDetail,
}

/// A validated request, ready for the engine.
pub(crate) struct Plan {
    pub tx: LatLon,
    pub rx: LatLon,
    pub ssn: SsnUsed,
    pub engine_request: PredictionRequest,
}

impl Plan {
    fn into_prediction(self, engine: &str, run: EngineRun) -> PathPrediction {
        PathPrediction {
            tx: self.tx,
            rx: self.rx,
            tx_locator: geo::to_maidenhead(self.tx),
            rx_locator: geo::to_maidenhead(self.rx),
            distance_km: match self.engine_request.path {
                PathKind::Short => geo::distance_km(self.tx, self.rx),
                PathKind::Long => geo::EARTH_CIRCUMFERENCE_KM - geo::distance_km(self.tx, self.rx),
            },
            tx_bearing_deg: self.engine_request.tx_antenna.bearing_deg,
            rx_bearing_deg: self.engine_request.rx_antenna.bearing_deg,
            ssn: self.ssn,
            required_snr_db_hz: self.engine_request.required_snr_db_hz,
            bands: HF_BANDS.to_vec(),
            engine: engine.to_string(),
            run,
        }
    }
}

pub fn predict_path(
    engine: &dyn PropagationEngine,
    request: &PathRequest,
) -> Result<PathPrediction, String> {
    let plan = plan(request)?;
    let run = engine.predict(&plan.engine_request)?;
    Ok(plan.into_prediction(engine.name(), run))
}

/// Short and long path, each with its power comparison and frequency window.
/// The engine runs are independent, so they run side by side.
pub fn predict_overview(
    engine: &(dyn PropagationEngine + Sync),
    request: &PathRequest,
) -> Result<PathOverview, String> {
    std::thread::scope(|scope| {
        let long = scope
            .spawn(|| path_detail(engine, &PathRequest { long_path: true, ..request.clone() }));
        let short = path_detail(engine, &PathRequest { long_path: false, ..request.clone() })?;
        Ok(PathOverview { short, long: join(long)? })
    })
}

pub(crate) fn join<T>(handle: std::thread::ScopedJoinHandle<'_, Result<T, String>>) -> Result<T, String> {
    handle.join().unwrap_or_else(|_| Err("a prediction thread panicked".into()))
}

fn path_detail(
    engine: &(dyn PropagationEngine + Sync),
    request: &PathRequest,
) -> Result<PathDetail, String> {
    let plan = plan(request)?;
    let base = &plan.engine_request;
    let required_snr = base.required_snr_db_hz;
    let mut powers = POWER_LEVELS_WATTS.to_vec();
    if !powers.contains(&base.tx_power_watts) {
        powers.push(base.tx_power_watts);
    }
    powers.sort_by(f64::total_cmp);

    let (run, power, sweeps) = std::thread::scope(|scope| {
        let main = scope.spawn(|| engine.predict(base));
        let power_runs: Vec<_> = powers
            .iter()
            .map(|&watts| {
                scope.spawn(move || {
                    engine.predict(&PredictionRequest { tx_power_watts: watts, ..base.clone() })
                })
            })
            .collect();
        let sweep_runs: Vec<_> = SWEEP_MHZ
            .chunks(SWEEP_CHUNK)
            .map(|chunk| {
                scope.spawn(move || {
                    engine.predict(&PredictionRequest {
                        frequencies_mhz: chunk.to_vec(),
                        ..base.clone()
                    })
                })
            })
            .collect();

        let run = join(main)?;
        let power = power_runs
            .into_iter()
            .zip(&powers)
            .map(|(handle, &watts)| Ok(power_case(watts, &join(handle)?.prediction)))
            .collect::<Result<Vec<_>, String>>()?;
        let sweeps = sweep_runs
            .into_iter()
            .map(|handle| Ok(join(handle)?.prediction))
            .collect::<Result<Vec<_>, String>>()?;
        Ok::<_, String>((run, power, sweeps))
    })?;

    let window = frequency_windows(&sweeps, required_snr);
    Ok(PathDetail { prediction: plan.into_prediction(engine.name(), run), power, window })
}

fn power_case(power_watts: f64, prediction: &Prediction) -> PowerCase {
    let per_band = |value: fn(&FrequencyPrediction) -> f64| {
        prediction
            .hours
            .iter()
            .map(|hour| hour.frequencies.iter().map(value).collect())
            .collect()
    };
    PowerCase {
        power_watts,
        reliability: per_band(|f| f.reliability),
        snr_db: per_band(|f| f.snr_db),
    }
}

/// One swept frequency in one hour.
#[derive(Debug, Clone, Copy)]
struct SweepPoint {
    mhz: f64,
    muf_day: f64,
    snr_db: f64,
}

fn frequency_windows(sweeps: &[Prediction], required_snr: f64) -> Vec<FrequencyWindow> {
    let Some(first) = sweeps.first() else { return Vec::new() };
    first
        .hours
        .iter()
        .enumerate()
        .map(|(i, hour)| {
            // The sweep runs are in ascending frequency order.
            let points: Vec<SweepPoint> = sweeps
                .iter()
                .filter_map(|sweep| sweep.hours.get(i))
                .flat_map(|h| &h.frequencies)
                .map(|f| SweepPoint { mhz: f.freq_mhz, muf_day: f.muf_day, snr_db: f.snr_db })
                .collect();
            FrequencyWindow {
                utc_hour: hour.utc_hour,
                muf_mhz: hour.muf_mhz,
                fot_mhz: fot(&points),
                luf_mhz: luf(&points, required_snr),
            }
        })
        .collect()
}

/// Highest frequency the path supports on 90% of days, interpolated between sweep points.
fn fot(points: &[SweepPoint]) -> Option<f64> {
    let i = points.iter().rposition(|p| p.muf_day >= FOT_MUF_DAY)?;
    let below = points[i];
    match points.get(i + 1) {
        Some(above) if below.muf_day > above.muf_day => Some(
            below.mhz
                + (below.muf_day - FOT_MUF_DAY) / (below.muf_day - above.muf_day)
                    * (above.mhz - below.mhz),
        ),
        _ => Some(below.mhz),
    }
}

/// Lowest frequency whose median SNR meets the requirement, interpolated between sweep points.
fn luf(points: &[SweepPoint], required_snr: f64) -> Option<f64> {
    let i = points.iter().position(|p| p.snr_db >= required_snr)?;
    let meets = points[i];
    match i.checked_sub(1).map(|j| points[j]) {
        Some(short) if meets.snr_db > short.snr_db => Some(
            short.mhz
                + (required_snr - short.snr_db) / (meets.snr_db - short.snr_db)
                    * (meets.mhz - short.mhz),
        ),
        _ => Some(meets.mhz),
    }
}

pub(crate) fn plan(request: &PathRequest) -> Result<Plan, String> {
    let tx = geo::parse_position(&request.tx_position).map_err(|e| format!("Transmitter: {e}"))?;
    let rx = geo::parse_position(&request.rx_position).map_err(|e| format!("Receiver: {e}"))?;
    if !(1..=12).contains(&request.month) {
        return Err(format!("month {} is not 1-12", request.month));
    }

    let ssn = match request.ssn {
        Some(value) if (0.0..=MAX_SSN).contains(&value) => SsnUsed::manual(value),
        Some(value) => return Err(format!("sunspot number {value} is outside 0-{MAX_SSN}")),
        None => solar::smoothed_ssn(request.year, request.month).ok_or_else(|| {
            format!(
                "The bundled sunspot table does not cover {}-{:02}. Enter a sunspot number.",
                request.year, request.month
            )
        })?,
    };

    // Antennas are assumed to be aimed along the path being predicted.
    let towards = |from, to| {
        let short = geo::bearing_deg(from, to);
        if request.long_path { (short + 180.0) % 360.0 } else { short }
    };
    let antenna = |station: &StationProfile, bearing_deg| Antenna {
        file: station.antenna.clone(),
        bearing_deg,
        gain_dbi: 0.0,
    };
    let uses_isotrope =
        request.tx_station.antenna == ISOTROPE || request.rx_station.antenna == ISOTROPE;
    let min_angle_deg = request
        .tx_station
        .min_angle_deg
        .max(request.rx_station.min_angle_deg)
        .max(if uses_isotrope { MIN_ANGLE_ISOTROPE } else { 0.0 });

    let engine_request = PredictionRequest {
        tx_name: request.tx_position.trim().to_string(),
        rx_name: request.rx_position.trim().to_string(),
        tx,
        rx,
        path: if request.long_path { PathKind::Long } else { PathKind::Short },
        year: request.year,
        month: request.month,
        utc_hour: None,
        ssn: ssn.value,
        frequencies_mhz: HF_BANDS.iter().map(|band| band.mhz).collect(),
        tx_power_watts: request.tx_station.power_watts,
        tx_antenna: antenna(&request.tx_station, towards(tx, rx)),
        rx_antenna: antenna(&request.rx_station, towards(rx, tx)),
        rx_noise_db: request.rx_station.noise_db,
        min_angle_deg,
        required_reliability_pct: request.required_reliability_pct,
        required_snr_db_hz: request.mode.required_snr_db_hz(),
        coefficients: Coefficients::Ccir,
    };

    Ok(Plan { tx, rx, ssn, engine_request })
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;
    use crate::solar::SsnKind;
    use crate::station;
    use crate::voacap::output::parse_output;

    /// Records the requests it is given and answers with the reference output.
    struct RecordingEngine {
        seen: Mutex<Vec<PredictionRequest>>,
    }

    impl RecordingEngine {
        fn new() -> Self {
            Self { seen: Mutex::new(Vec::new()) }
        }
        fn requests(&self) -> Vec<PredictionRequest> {
            self.seen.lock().unwrap().clone()
        }
        fn request(&self) -> PredictionRequest {
            self.requests().pop().expect("engine was not called")
        }
    }

    impl PropagationEngine for RecordingEngine {
        fn name(&self) -> &str {
            "recording"
        }
        fn predict(&self, request: &PredictionRequest) -> Result<EngineRun, String> {
            self.seen.lock().unwrap().push(request.clone());
            let output = include_str!("../../tests/engine/cases/test01.out");
            Ok(EngineRun {
                prediction: parse_output(output)?,
                input: String::new(),
                output: output.to_string(),
            })
        }
    }

    fn atlanta_to_london() -> PathRequest {
        let presets = station::presets();
        PathRequest {
            tx_position: "EM73tr".into(),
            rx_position: "51.51, -0.13".into(),
            year: 2026,
            month: 10,
            ssn: None,
            tx_station: presets[0].clone(),
            rx_station: presets[1].clone(),
            mode: Mode::Ssb,
            required_reliability_pct: 90.0,
            long_path: false,
        }
    }

    #[test]
    fn builds_the_engine_request_from_both_stations() {
        let engine = RecordingEngine::new();
        let request = atlanta_to_london();
        let result = predict_path(&engine, &request).unwrap();
        let sent = engine.request();

        assert_eq!(sent.tx_power_watts, request.tx_station.power_watts);
        assert_eq!(sent.tx_antenna.file, request.tx_station.antenna);
        assert_eq!(sent.rx_antenna.file, request.rx_station.antenna);
        assert_eq!(sent.rx_noise_db, request.rx_station.noise_db);
        assert_eq!(sent.required_snr_db_hz, 38.0);
        assert_eq!(sent.frequencies_mhz.len(), HF_BANDS.len());
        assert_eq!(sent.path, PathKind::Short);
        assert_eq!(result.tx_locator, "EM73tr");
        assert_eq!(result.rx_locator, "IO91wm");
        assert_eq!(result.engine, "recording");
        assert_eq!(result.bands.len(), sent.frequencies_mhz.len());
    }

    #[test]
    fn aims_antennas_along_the_path() {
        let engine = RecordingEngine::new();
        predict_path(&engine, &atlanta_to_london()).unwrap();
        let short = engine.request();
        assert!((short.tx_antenna.bearing_deg - 45.0).abs() < 2.0, "{}", short.tx_antenna.bearing_deg);
        assert!((short.rx_antenna.bearing_deg - 288.0).abs() < 2.0, "{}", short.rx_antenna.bearing_deg);

        let mut request = atlanta_to_london();
        request.long_path = true;
        predict_path(&engine, &request).unwrap();
        let long = engine.request();
        assert_eq!(long.path, PathKind::Long);
        assert!((long.tx_antenna.bearing_deg - (short.tx_antenna.bearing_deg + 180.0)).abs() < 1e-9);
        assert!((long.rx_antenna.bearing_deg - (short.rx_antenna.bearing_deg - 180.0)).abs() < 1e-9);
    }

    #[test]
    fn takes_the_sunspot_number_from_the_table_unless_given() {
        let engine = RecordingEngine::new();
        let mut request = atlanta_to_london();
        request.year = 2000;
        request.month = 6;
        let result = predict_path(&engine, &request).unwrap();
        assert_eq!(result.ssn.kind, SsnKind::Observed);
        assert_eq!(engine.request().ssn, result.ssn.value);

        request.ssn = Some(42.0);
        let result = predict_path(&engine, &request).unwrap();
        assert_eq!(result.ssn.kind, SsnKind::Manual);
        assert_eq!(engine.request().ssn, 42.0);
    }

    #[test]
    fn asks_for_a_sunspot_number_when_the_table_ends() {
        let engine = RecordingEngine::new();
        let mut request = atlanta_to_london();
        request.year = 2100;
        assert!(predict_path(&engine, &request).unwrap_err().contains("Enter a sunspot number"));

        request.ssn = Some(60.0);
        assert!(predict_path(&engine, &request).is_ok());
        request.ssn = Some(900.0);
        assert!(predict_path(&engine, &request).is_err());
    }

    #[test]
    fn raises_the_minimum_angle_for_isotropes() {
        let engine = RecordingEngine::new();
        let mut request = atlanta_to_london();
        predict_path(&engine, &request).unwrap();
        assert_eq!(engine.request().min_angle_deg, 0.1);

        request.rx_station.antenna = ISOTROPE.into();
        predict_path(&engine, &request).unwrap();
        assert_eq!(engine.request().min_angle_deg, 3.0);

        request.tx_station.min_angle_deg = 8.0;
        predict_path(&engine, &request).unwrap();
        assert_eq!(engine.request().min_angle_deg, 8.0);
    }

    fn point(mhz: f64, muf_day: f64, snr_db: f64) -> SweepPoint {
        SweepPoint { mhz, muf_day, snr_db }
    }

    #[test]
    fn fot_is_where_the_path_is_open_on_ninety_percent_of_days() {
        let points = [point(10.0, 1.0, 0.0), point(12.0, 0.95, 0.0), point(14.0, 0.75, 0.0)];
        assert!((fot(&points).unwrap() - 12.5).abs() < 1e-9);
        // Open at the top of the sweep: the top frequency is the best we can say.
        assert_eq!(fot(&points[..2]), Some(12.0));
        // Never open that often.
        assert_eq!(fot(&[point(2.0, 0.5, 0.0)]), None);
    }

    #[test]
    fn luf_is_where_median_snr_first_meets_the_requirement() {
        let points = [point(2.0, 1.0, 10.0), point(3.0, 1.0, 30.0), point(4.0, 1.0, 50.0)];
        assert!((luf(&points, 38.0).unwrap() - 3.4).abs() < 1e-9);
        // Already met at the bottom of the sweep.
        assert_eq!(luf(&points, 5.0), Some(2.0));
        // Never met.
        assert_eq!(luf(&points, 60.0), None);
    }

    #[test]
    fn overview_covers_both_paths_powers_and_the_sweep() {
        let engine = RecordingEngine::new();
        let mut request = atlanta_to_london();
        request.tx_station.power_watts = 25.0;
        let overview = predict_overview(&engine, &request).unwrap();
        let sent = engine.requests();

        // Per path: the main run, five powers (four standard plus 25 W), two sweep runs.
        assert_eq!(sent.len(), 2 * (1 + 5 + 2));
        assert_eq!(sent.iter().filter(|r| r.path == PathKind::Long).count(), sent.len() / 2);
        let mut swept: Vec<f64> = sent
            .iter()
            .filter(|r| r.path == PathKind::Short && r.frequencies_mhz[0] != HF_BANDS[0].mhz)
            .flat_map(|r| r.frequencies_mhz.clone())
            .collect();
        swept.sort_by(f64::total_cmp);
        assert_eq!(swept, SWEEP_MHZ);

        for detail in [&overview.short, &overview.long] {
            let powers: Vec<f64> = detail.power.iter().map(|p| p.power_watts).collect();
            assert_eq!(powers, [5.0, 10.0, 25.0, 50.0, 100.0]);
            assert_eq!(detail.power[0].reliability.len(), 24);
            assert_eq!(detail.window.len(), 24);
        }
    }

    #[test]
    fn names_the_position_that_is_wrong() {
        let engine = RecordingEngine::new();
        let mut request = atlanta_to_london();
        request.rx_position = "nowhere".into();
        assert!(predict_path(&engine, &request).unwrap_err().starts_with("Receiver:"));
        request.tx_position = String::new();
        assert!(predict_path(&engine, &request).unwrap_err().starts_with("Transmitter:"));
    }
}
