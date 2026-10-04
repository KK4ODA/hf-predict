//! Station profiles, antenna choices, operating modes and amateur bands.

use serde::{Deserialize, Serialize};

/// What a station is: power, antenna and local noise. It transmits with the
/// power and antenna, and receives with the antenna against the noise.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StationProfile {
    pub name: String,
    pub power_watts: f64,
    /// Antenna file under the engine's `antennas` folder.
    pub antenna: String,
    /// Man-made noise in dB below 1 W in 1 Hz at 3 MHz (see `NOISE_LEVELS`).
    pub noise_db: f64,
    pub min_angle_deg: f64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Choice<T> {
    pub value: T,
    pub label: &'static str,
}

pub const ISOTROPE: &str = "default/isotrope";
const DIPOLE_10M: &str = "hfp/dipole10.voa";
const DIPOLE_5M: &str = "hfp/dipole05.voa";
const VERTICAL: &str = "hfp/vertical.voa";
const WHIP: &str = "hfp/whip.voa";

pub fn antennas() -> Vec<Choice<&'static str>> {
    vec![
        Choice { value: DIPOLE_10M, label: "Half-wave dipole, 10 m high" },
        Choice { value: DIPOLE_5M, label: "Half-wave dipole, 5 m high (NVIS, portable)" },
        Choice { value: VERTICAL, label: "Quarter-wave vertical" },
        Choice { value: WHIP, label: "Mobile whip, 2.5 m, unloaded" },
        Choice { value: ISOTROPE, label: "Isotropic, 0 dBi (reference)" },
    ]
}

/// The smallest rotation that leaves an antenna's azimuth pattern unchanged:
/// 0 for omnidirectional antennas, 180 for a dipole, 360 when unknown.
pub fn azimuth_period_deg(antenna_file: &str) -> f64 {
    match antenna_file {
        ISOTROPE | VERTICAL | WHIP => 0.0,
        DIPOLE_10M | DIPOLE_5M => 180.0,
        _ => 360.0,
    }
}

/// VOACAP man-made noise categories.
pub const NOISE_RESIDENTIAL: f64 = 145.0;
pub const NOISE_RURAL: f64 = 155.0;
pub const NOISE_REMOTE: f64 = 164.0;

pub fn noise_levels() -> Vec<Choice<f64>> {
    vec![
        Choice { value: NOISE_RESIDENTIAL, label: "Residential (-145 dBW/Hz)" },
        Choice { value: NOISE_RURAL, label: "Rural, quiet (-155 dBW/Hz)" },
        Choice { value: NOISE_REMOTE, label: "Remote (-164 dBW/Hz)" },
    ]
}

/// Modelled antennas already include ground effects, so the engine may use
/// any angle. Isotropes need the 3 degree floor VOACAP's authors recommend.
const MIN_ANGLE_MODELLED: f64 = 0.1;

pub fn presets() -> Vec<StationProfile> {
    let station = |name: &str, power_watts: f64, antenna: &str, noise_db: f64| StationProfile {
        name: name.into(),
        power_watts,
        antenna: antenna.into(),
        noise_db,
        min_angle_deg: MIN_ANGLE_MODELLED,
    };
    vec![
        station("100 W dipole", 100.0, DIPOLE_10M, NOISE_RESIDENTIAL),
        station("QRP portable", 5.0, DIPOLE_5M, NOISE_RURAL),
        station("Mobile HF", 100.0, WHIP, NOISE_RESIDENTIAL),
        station("EmComm portable", 100.0, DIPOLE_5M, NOISE_RURAL),
        station("Fixed gateway (RMS)", 100.0, DIPOLE_10M, NOISE_RESIDENTIAL),
    ]
}

/// Operating mode, which sets the signal-to-noise ratio a contact needs.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Mode {
    Ssb,
    Cw,
    Ft8,
}

impl Mode {
    /// Required SNR in dB in a 1 Hz bandwidth. FT8's is its -21 dB decode
    /// threshold in 2500 Hz, converted to 1 Hz.
    pub fn required_snr_db_hz(self) -> f64 {
        match self {
            Mode::Ssb => 38.0,
            Mode::Cw => 24.0,
            Mode::Ft8 => 13.0,
        }
    }
}

pub fn modes() -> Vec<Choice<Mode>> {
    vec![
        Choice { value: Mode::Ssb, label: "SSB voice" },
        Choice { value: Mode::Cw, label: "CW" },
        Choice { value: Mode::Ft8, label: "FT8" },
    ]
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Band {
    pub name: &'static str,
    /// Frequency used for predictions on this band.
    pub mhz: f64,
}

/// Amateur HF bands inside the model's 2-30 MHz range. 160 m is below it.
pub const HF_BANDS: [Band; 9] = [
    Band { name: "80 m", mhz: 3.6 },
    Band { name: "60 m", mhz: 5.36 },
    Band { name: "40 m", mhz: 7.1 },
    Band { name: "30 m", mhz: 10.13 },
    Band { name: "20 m", mhz: 14.1 },
    Band { name: "17 m", mhz: 18.1 },
    Band { name: "15 m", mhz: 21.1 },
    Band { name: "12 m", mhz: 24.9 },
    Band { name: "10 m", mhz: 28.2 },
];
