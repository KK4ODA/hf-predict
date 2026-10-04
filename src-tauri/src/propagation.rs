//! Engine-independent prediction types and the engine interface.

use serde::{Deserialize, Serialize};

use crate::geo::LatLon;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PathKind {
    Short,
    Long,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Coefficients {
    Ccir,
    Ursi,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Antenna {
    /// File under the engine's `antennas` folder, e.g. `default/isotrope`.
    pub file: String,
    /// Main-beam azimuth in degrees east of north.
    pub bearing_deg: f64,
    /// Gain in dBi for constant-gain (isotrope) files; 0 for modelled antennas.
    pub gain_dbi: f64,
}

/// One point-to-point circuit for one month, all 24 UTC hours.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PredictionRequest {
    pub tx_name: String,
    pub rx_name: String,
    pub tx: LatLon,
    pub rx: LatLon,
    pub path: PathKind,
    pub year: i32,
    pub month: u32,
    /// Monthly smoothed sunspot number.
    pub ssn: f64,
    pub frequencies_mhz: Vec<f64>,
    pub tx_power_watts: f64,
    pub tx_antenna: Antenna,
    pub rx_antenna: Antenna,
    /// Man-made noise at the receiver in dB below 1 W in 1 Hz at 3 MHz
    /// (145 means -145 dBW/Hz).
    pub rx_noise_db: f64,
    pub min_angle_deg: f64,
    pub required_reliability_pct: f64,
    /// Required signal-to-noise ratio in dB in a 1 Hz bandwidth.
    pub required_snr_db_hz: f64,
    pub coefficients: Coefficients,
}

/// Predicted values for one frequency in one hour. Field names follow the
/// VOACAP output rows given in each comment.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FrequencyPrediction {
    pub freq_mhz: f64,
    /// MODE: most reliable propagation mode, e.g. `2F2`.
    pub mode: String,
    /// TANGLE: take-off angle in degrees.
    pub takeoff_angle_deg: f64,
    /// DELAY in milliseconds.
    pub delay_ms: f64,
    /// V HITE: virtual height in km.
    pub virtual_height_km: f64,
    /// MUFday: fraction of days in the month the path supports this frequency.
    pub muf_day: f64,
    /// LOSS: median system loss in dB.
    pub loss_db: f64,
    /// DBU: median field strength in dB above 1 µV/m.
    pub field_strength_dbu: f64,
    /// S DBW: median signal power at the receiver in dBW.
    pub signal_dbw: f64,
    /// N DBW: median noise power in a 1 Hz bandwidth in dBW.
    pub noise_dbw: f64,
    /// SNR: median signal-to-noise ratio in dB-Hz.
    pub snr_db: f64,
    /// RPWRG: extra power or gain in dB needed to meet the required reliability.
    pub required_power_gain_db: f64,
    /// REL: fraction of days the required SNR is met.
    pub reliability: f64,
    /// MPROB: probability of multipath.
    pub multipath_probability: f64,
    /// S PRB: service probability.
    pub service_probability: f64,
    /// SIG LW / SIG UP: signal lower and upper decile spreads in dB.
    pub signal_lower_decile_db: f64,
    pub signal_upper_decile_db: f64,
    /// SNR LW / SNR UP: SNR lower and upper decile spreads in dB.
    pub snr_lower_decile_db: f64,
    pub snr_upper_decile_db: f64,
    /// TGAIN / RGAIN: antenna gains in dBi at the take-off angle.
    pub tx_gain_dbi: f64,
    pub rx_gain_dbi: f64,
    /// SNRxx: SNR exceeded on the required percentage of days.
    pub snr_at_required_reliability_db: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HourPrediction {
    /// UTC hour as VOACAP labels it: 1 to 24, where 24 is 00 UTC.
    pub utc_hour: u32,
    /// Median maximum usable frequency for the hour.
    pub muf_mhz: f64,
    /// Values at the MUF itself.
    pub at_muf: FrequencyPrediction,
    /// One entry per requested frequency, in request order.
    pub frequencies: Vec<FrequencyPrediction>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Prediction {
    /// Model name and version as the engine reports it.
    pub engine: String,
    pub distance_km: f64,
    pub azimuth_tx_deg: f64,
    pub azimuth_rx_deg: f64,
    pub hours: Vec<HourPrediction>,
}

/// A prediction together with the engine's raw input and output text.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineRun {
    pub prediction: Prediction,
    pub input: String,
    pub output: String,
}

pub trait PropagationEngine {
    fn name(&self) -> &str;
    fn predict(&self, request: &PredictionRequest) -> Result<EngineRun, String>;
}
