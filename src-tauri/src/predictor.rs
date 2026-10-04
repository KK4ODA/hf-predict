//! Turns what the operator asks for (two positions, a month, two stations)
//! into an engine request, and returns the prediction with its provenance.

use serde::{Deserialize, Serialize};

use crate::geo::{self, LatLon};
use crate::propagation::{
    Antenna, Coefficients, EngineRun, PathKind, PredictionRequest, PropagationEngine,
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
    pub ssn: SsnUsed,
    pub required_snr_db_hz: f64,
    /// Bands in the same order as each hour's `frequencies`.
    pub bands: Vec<Band>,
    /// Name of the engine implementation that produced the run.
    pub engine: String,
    pub run: EngineRun,
}

pub fn predict_path(
    engine: &dyn PropagationEngine,
    request: &PathRequest,
) -> Result<PathPrediction, String> {
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

    Ok(PathPrediction {
        tx,
        rx,
        tx_locator: geo::to_maidenhead(tx),
        rx_locator: geo::to_maidenhead(rx),
        ssn,
        required_snr_db_hz: engine_request.required_snr_db_hz,
        bands: HF_BANDS.to_vec(),
        engine: engine.name().to_string(),
        run: engine.predict(&engine_request)?,
    })
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use super::*;
    use crate::solar::SsnKind;
    use crate::station;
    use crate::voacap::output::parse_output;

    /// Records the request it is given and answers with the reference output.
    struct RecordingEngine {
        seen: RefCell<Option<PredictionRequest>>,
    }

    impl RecordingEngine {
        fn new() -> Self {
            Self { seen: RefCell::new(None) }
        }
        fn request(&self) -> PredictionRequest {
            self.seen.borrow().clone().expect("engine was not called")
        }
    }

    impl PropagationEngine for RecordingEngine {
        fn name(&self) -> &str {
            "recording"
        }
        fn predict(&self, request: &PredictionRequest) -> Result<EngineRun, String> {
            *self.seen.borrow_mut() = Some(request.clone());
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
