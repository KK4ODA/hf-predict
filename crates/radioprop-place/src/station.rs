// SPDX-License-Identifier: MIT OR Apache-2.0
//! Station profiles: what a station is, so that a path model need not assume one (RadioPost's
//! design, *Choosing a gateway*, §6.5). Every field is optional. [`StationProfile::resolve`] gives
//! each value a model uses with whether it was set or assumed, so an explanation can name the
//! assumptions that matter ("assumed 2 m antenna: raising it may change this").
//!
//! The defaults: 100 W at HF and 25 W at VHF; at HF a dipole 10 m up, at VHF 0 dBi 2 m up on a
//! vehicle or 5 m on a mast; feed line 1 dB at HF and 2 dB at VHF; residential noise. A profile is
//! its own list beside the places: a place may name the profile picking it picks, so a radio moved
//! from the car to the field keeps one profile.

use serde::{Deserialize, Serialize};

/// The kinds of antenna a profile names (the same short list a gateway's record uses).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AntennaKind {
    Vertical,
    Dipole,
    Yagi,
    JPole,
    Collinear,
    Loop,
    Nvis,
    Whip,
    Other,
}

/// Man-made noise where the station is: the environments of ITU-R P.372-18's Table 1 (city,
/// residential, rural, quiet rural), the categories VOACAP offers under its own names.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Noise {
    /// P.372's city (curve A).
    Industrial,
    /// Curve B.
    Residential,
    /// Curve C.
    Rural,
    /// P.372's quiet rural site (curve D). P.372 does not hold its line over the whole range the
    /// others cover: above the ionosphere's critical frequency galactic noise (curve E) is the
    /// floor there.
    Remote,
}

impl Noise {
    /// The median man-made noise power at `mhz`, dBW in a 1 Hz bandwidth, by ITU-R P.372-18 §6.1:
    /// `Fam = c − d·log10(f)` dB above kT0b (its equation 17, with Table 1's c and d; stated for
    /// 0.3 to 250 MHz), and `Fam − 204` dBW/Hz (its equation 6).
    #[must_use]
    pub fn dbw_per_hz(self, mhz: f64) -> f64 {
        let (c, d) = match self {
            Noise::Industrial => (76.8, 27.7),
            Noise::Residential => (72.5, 27.7),
            Noise::Rural => (67.2, 27.7),
            Noise::Remote => (53.6, 28.6),
        };
        c - d * mhz.log10() - 204.0
    }
}

/// An antenna, as far as the operator knows it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Antenna {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<AntennaKind>,
    /// Height above ground, metres.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agl_m: Option<f64>,
    /// Gain, dBi.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gain_dbi: Option<f64>,
    /// Where a beam points, degrees from true north.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub azimuth_deg: Option<f64>,
}

/// The station at HF, or at VHF and above.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Side {
    /// Transmit power, watts.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub power_w: Option<f64>,
    pub antenna: Antenna,
    /// Feed-line loss, dB.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub feedline_db: Option<f64>,
}

/// A station profile.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct StationProfile {
    pub id: String,
    pub name: String,
    /// The radio, for people.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub radio: Option<String>,
    /// In a vehicle: its VHF antenna is assumed lower.
    pub vehicle: bool,
    pub hf: Side,
    pub vhf: Side,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub noise: Option<Noise>,
    /// The bands its radio and antennas cover, such as `40m`; empty, all the modem allows.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub bands: Vec<String>,
}

/// Whether a value was the operator's or a default.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Said {
    Set,
    Assumed,
}

/// A value a model uses, and whether it was set or assumed.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Used<T> {
    pub value: T,
    pub said: Said,
}

fn used<T>(set: Option<T>, default: T) -> Used<T> {
    match set {
        Some(value) => Used {
            value,
            said: Said::Set,
        },
        None => Used {
            value: default,
            said: Said::Assumed,
        },
    }
}

/// One side of a station as a model takes it.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct UsedSide {
    pub power_w: Used<f64>,
    pub antenna: Used<AntennaKind>,
    pub agl_m: Used<f64>,
    pub gain_dbi: Used<f64>,
    pub feedline_db: Used<f64>,
}

/// A station as a model takes it: every value, set or assumed.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Resolved {
    pub hf: UsedSide,
    pub vhf: UsedSide,
    pub noise: Used<Noise>,
}

/// The defaults, for a station that says nothing.
pub const HF_POWER_W: f64 = 100.0;
pub const VHF_POWER_W: f64 = 25.0;
pub const HF_AGL_M: f64 = 10.0;
pub const VHF_AGL_VEHICLE_M: f64 = 2.0;
pub const VHF_AGL_MAST_M: f64 = 5.0;
pub const HF_FEEDLINE_DB: f64 = 1.0;
pub const VHF_FEEDLINE_DB: f64 = 2.0;
/// A half-wave dipole's gain in free space; the ground changes it, which the HF model accounts for.
pub const DIPOLE_DBI: f64 = 2.15;

impl StationProfile {
    /// Every value a model uses, from the profile or by default.
    #[must_use]
    pub fn resolve(&self) -> Resolved {
        let hf = &self.hf;
        let vhf = &self.vhf;
        let vhf_agl = if self.vehicle {
            VHF_AGL_VEHICLE_M
        } else {
            VHF_AGL_MAST_M
        };
        Resolved {
            hf: UsedSide {
                power_w: used(hf.power_w, HF_POWER_W),
                antenna: used(hf.antenna.kind, AntennaKind::Dipole),
                agl_m: used(hf.antenna.agl_m, HF_AGL_M),
                gain_dbi: used(hf.antenna.gain_dbi, DIPOLE_DBI),
                feedline_db: used(hf.feedline_db, HF_FEEDLINE_DB),
            },
            vhf: UsedSide {
                power_w: used(vhf.power_w, VHF_POWER_W),
                antenna: used(
                    vhf.antenna.kind,
                    if self.vehicle {
                        AntennaKind::Whip
                    } else {
                        AntennaKind::Vertical
                    },
                ),
                agl_m: used(vhf.antenna.agl_m, vhf_agl),
                gain_dbi: used(vhf.antenna.gain_dbi, 0.0),
                feedline_db: used(vhf.feedline_db, VHF_FEEDLINE_DB),
            },
            noise: used(self.noise, Noise::Residential),
        }
    }

    /// Why the profile cannot be kept, or `None`: a name, and numbers in reason.
    #[must_use]
    pub fn problem(&self) -> Option<String> {
        if self.name.trim().is_empty() {
            return Some("a name for the profile".into());
        }
        for (what, side) in [("HF", &self.hf), ("VHF", &self.vhf)] {
            let a = &side.antenna;
            let bad = |v: Option<f64>, lo: f64, hi: f64| v.is_some_and(|v| !(lo..=hi).contains(&v));
            if bad(side.power_w, 0.0, 10_000.0) {
                return Some(format!("{what} power: 0 to 10 000 W"));
            }
            if bad(a.agl_m, 0.0, 1_000.0) {
                return Some(format!(
                    "{what} antenna height: 0 to 1000 m above the ground"
                ));
            }
            if bad(a.gain_dbi, -30.0, 40.0) {
                return Some(format!("{what} antenna gain: -30 to 40 dBi"));
            }
            if bad(a.azimuth_deg, 0.0, 360.0) {
                return Some(format!("{what} beam heading: 0 to 360 degrees"));
            }
            if bad(side.feedline_db, 0.0, 30.0) {
                return Some(format!("{what} feed-line loss: 0 to 30 dB"));
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_profile_that_says_nothing_takes_every_default_and_says_so() {
        let r = StationProfile {
            name: "New".into(),
            ..StationProfile::default()
        }
        .resolve();
        assert_eq!(
            r.hf.power_w,
            Used {
                value: 100.0,
                said: Said::Assumed
            }
        );
        assert_eq!(r.hf.antenna.value, AntennaKind::Dipole);
        assert_eq!((r.hf.agl_m.value, r.vhf.agl_m.value), (10.0, 5.0));
        assert_eq!((r.vhf.power_w.value, r.vhf.gain_dbi.value), (25.0, 0.0));
        assert_eq!(
            (r.hf.feedline_db.value, r.vhf.feedline_db.value),
            (1.0, 2.0)
        );
        assert_eq!(r.noise.value, Noise::Residential);
    }

    #[test]
    fn noise_follows_itu_r_p372() {
        // at 3 MHz, the levels VOACAP's categories are known by: P.372-18 Table 1's constants
        let at3 = |n: Noise| (n.dbw_per_hz(3.0) * 10.0).round() / 10.0;
        assert_eq!(
            [
                Noise::Industrial,
                Noise::Residential,
                Noise::Rural,
                Noise::Remote
            ]
            .map(at3),
            [-140.4, -144.7, -150.0, -164.0]
        );
        // and less as the frequency rises: 27.7 dB a decade
        let r = Noise::Residential;
        assert!((r.dbw_per_hz(3.0) - r.dbw_per_hz(30.0) - 27.7).abs() < 1e-9);
    }

    #[test]
    fn what_is_set_is_used_and_a_vehicle_lowers_the_vhf_antenna() {
        let p = StationProfile {
            name: "Mobile, TS-480".into(),
            vehicle: true,
            hf: Side {
                power_w: Some(65.0),
                antenna: Antenna {
                    kind: Some(AntennaKind::Whip),
                    agl_m: Some(2.5),
                    ..Antenna::default()
                },
                ..Side::default()
            },
            noise: Some(Noise::Rural),
            ..StationProfile::default()
        };
        let r = p.resolve();
        assert_eq!(
            r.hf.power_w,
            Used {
                value: 65.0,
                said: Said::Set
            }
        );
        assert_eq!(
            r.hf.antenna,
            Used {
                value: AntennaKind::Whip,
                said: Said::Set
            }
        );
        assert_eq!(
            r.vhf.agl_m,
            Used {
                value: 2.0,
                said: Said::Assumed
            }
        );
        assert_eq!(r.vhf.antenna.value, AntennaKind::Whip);
        assert_eq!(
            r.noise,
            Used {
                value: Noise::Rural,
                said: Said::Set
            }
        );
        // kept as JSON with only what was set
        let json = serde_json::to_value(&p).expect("json");
        assert_eq!(json["hf"]["antenna"]["kind"], "whip");
        assert!(json["vhf"]["antenna"]
            .as_object()
            .is_some_and(serde_json::Map::is_empty));
        assert_eq!(
            serde_json::from_value::<StationProfile>(json).expect("back"),
            p
        );
    }

    #[test]
    fn numbers_out_of_reason_are_refused() {
        let mut p = StationProfile {
            name: "Home".into(),
            ..StationProfile::default()
        };
        assert_eq!(p.problem(), None);
        p.hf.power_w = Some(-5.0);
        assert!(p.problem().is_some_and(|w| w.starts_with("HF power")));
        p.hf.power_w = Some(100.0);
        p.vhf.antenna.gain_dbi = Some(80.0);
        assert!(p
            .problem()
            .is_some_and(|w| w.starts_with("VHF antenna gain")));
        p.name = " ".into();
        assert_eq!(p.problem().as_deref(), Some("a name for the profile"));
    }
}
