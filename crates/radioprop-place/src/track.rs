// SPDX-License-Identifier: MIT OR Apache-2.0
//! What a GPS has said, and how fresh it is: no input or output, the clock handed in, so that every
//! rule is tested with an injected clock.
//!
//! * A fix is **current** for [`CURRENT_S`] seconds after it arrived, **recent** up to
//!   [`RECENT_S`] (shown with its age), and **stale** after that. A stale fix is never the
//!   present position.
//! * A fix that implies more than [`MAX_SPEED_KMH`] from the last one is held until a second fix
//!   confirms it: one bad sentence does not move the station across a continent.
//! * A GPS that says nothing valid for [`SILENT_S`] seconds is silent; one that speaks but has no
//!   fix says so.

use serde::{Deserialize, Serialize};

use crate::nmea::{Quality, Said};

/// How long a fix is current, in seconds.
pub const CURRENT_S: u64 = 10;
/// How long a fix is recent, in seconds; after this it is stale.
pub const RECENT_S: u64 = 120;
/// How long without a valid sentence before the GPS is silent, in seconds.
pub const SILENT_S: u64 = 60;
/// The fastest a station is believed to move.
pub const MAX_SPEED_KMH: f64 = 500.0;

/// A position the GPS gave.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Fix {
    pub lat: f64,
    pub lon: f64,
    /// Above mean sea level, when the GPS said (GGA, gpsd).
    pub alt_m: Option<f64>,
    pub quality: Quality,
    /// When it arrived, in milliseconds since 1970 (UTC).
    pub at_ms: u64,
}

impl Fix {
    /// Its age at `now_ms`, in whole seconds.
    #[must_use]
    pub fn age_s(&self, now_ms: u64) -> u64 {
        now_ms.saturating_sub(self.at_ms) / 1000
    }

    /// How fresh it is at `now_ms`.
    #[must_use]
    pub fn freshness(&self, now_ms: u64) -> Freshness {
        Freshness::of(self.age_s(now_ms))
    }
}

/// How fresh a fix is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Freshness {
    /// Up to [`CURRENT_S`] seconds old.
    Current,
    /// Up to [`RECENT_S`] seconds old: still the present position, shown with its age.
    Recent,
    /// Older: never the present position.
    Stale,
}

impl Freshness {
    /// The freshness of a fix `age_s` seconds old.
    #[must_use]
    pub fn of(age_s: u64) -> Freshness {
        if age_s <= CURRENT_S {
            Freshness::Current
        } else if age_s <= RECENT_S {
            Freshness::Recent
        } else {
            Freshness::Stale
        }
    }

    /// Whether a fix this fresh may be the present position.
    #[must_use]
    pub fn usable(self) -> bool {
        self != Freshness::Stale
    }
}

/// What the GPS has been saying.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "kebab-case")]
pub enum Hearing {
    /// Nothing valid yet.
    Nothing,
    /// Positions.
    Positions,
    /// Valid sentences, but no fix since `since_ms` (indoors, or the receiver still starting).
    NoFix { since_ms: u64 },
    /// Nothing valid since `since_ms`, [`SILENT_S`] or more ago.
    Silent { since_ms: u64 },
}

/// What a GPS has said: its last accepted fix and what it says now.
#[derive(Clone, Debug, Default)]
pub struct Tracker {
    fix: Option<Fix>,
    /// A fix too far from the last to be believed until another confirms it.
    held: Option<Fix>,
    /// When the first and the last valid sentence arrived.
    first_heard_ms: Option<u64>,
    heard_ms: Option<u64>,
    /// Since when the GPS has said it has no fix, when its last word was that.
    no_fix_since: Option<u64>,
    /// Lines refused (bad checksum, malformed).
    refused: u64,
}

impl Tracker {
    /// A tracker that has heard nothing.
    #[must_use]
    pub fn new() -> Tracker {
        Tracker::default()
    }

    /// Takes what a sentence said, at `now_ms`.
    pub fn take(&mut self, said: Said, now_ms: u64) {
        self.first_heard_ms.get_or_insert(now_ms);
        self.heard_ms = Some(now_ms);
        match said {
            Said::Position {
                lat,
                lon,
                alt_m,
                quality,
            } => {
                self.no_fix_since = None;
                // an RMC after the GGA of the same second keeps the GGA's altitude
                let alt_m = alt_m.or_else(|| {
                    self.fix
                        .filter(|f| now_ms.saturating_sub(f.at_ms) <= 2000)
                        .and_then(|f| f.alt_m)
                });
                let fix = Fix {
                    lat,
                    lon,
                    alt_m,
                    quality,
                    at_ms: now_ms,
                };
                self.consider(fix);
            }
            Said::NoFix => {
                self.no_fix_since.get_or_insert(now_ms);
            }
            Said::Other => {}
        }
    }

    /// Counts a line that was refused.
    pub fn refused(&mut self) {
        self.refused += 1;
    }

    /// Lines refused so far.
    #[must_use]
    pub fn refused_count(&self) -> u64 {
        self.refused
    }

    /// A fix is accepted when it is the first, plausible from the last, or confirmed by the one
    /// held before it.
    fn consider(&mut self, fix: Fix) {
        let plausible = |from: &Fix| speed_kmh(from, &fix) <= MAX_SPEED_KMH;
        match (&self.fix, &self.held) {
            (None, _) => self.accept(fix),
            (Some(last), _) if plausible(last) => self.accept(fix),
            (_, Some(held)) if plausible(held) => self.accept(fix),
            _ => self.held = Some(fix),
        }
    }

    fn accept(&mut self, fix: Fix) {
        self.fix = Some(fix);
        self.held = None;
    }

    /// The last fix accepted, of any age.
    #[must_use]
    pub fn fix(&self) -> Option<Fix> {
        self.fix
    }

    /// A fix held until another confirms it.
    #[must_use]
    pub fn held(&self) -> Option<Fix> {
        self.held
    }

    /// What the GPS has been saying, at `now_ms`.
    #[must_use]
    pub fn hearing(&self, now_ms: u64) -> Hearing {
        match (self.heard_ms, self.no_fix_since) {
            (None, _) => Hearing::Nothing,
            (Some(heard), _) if now_ms.saturating_sub(heard) >= SILENT_S * 1000 => {
                Hearing::Silent { since_ms: heard }
            }
            (Some(_), Some(since_ms)) => Hearing::NoFix { since_ms },
            (Some(_), None) if self.fix.is_some() => Hearing::Positions,
            // valid sentences, none of them a position yet
            (Some(_), None) => Hearing::NoFix {
                since_ms: self.first_heard_ms.unwrap_or(now_ms),
            },
        }
    }
}

/// The speed from one fix to the next, counting at least a second between them (an RMC and a GGA
/// of the same second arrive together).
fn speed_kmh(from: &Fix, to: &Fix) -> f64 {
    let km = radioprop_geo::distance_km(
        radioprop_geo::LatLon {
            lat: from.lat,
            lon: from.lon,
        },
        radioprop_geo::LatLon {
            lat: to.lat,
            lon: to.lon,
        },
    );
    let hours = (to.at_ms.saturating_sub(from.at_ms).max(1000)) as f64 / 3_600_000.0;
    km / hours
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(lat: f64, lon: f64) -> Said {
        Said::Position {
            lat,
            lon,
            alt_m: None,
            quality: Quality::Gps,
        }
    }

    #[test]
    fn a_fix_is_current_then_recent_then_stale() {
        let mut t = Tracker::new();
        t.take(at(33.75, -84.39), 1_000_000);
        let f = t.fix().expect("a fix");
        assert_eq!(f.freshness(1_000_000 + 10_000), Freshness::Current);
        assert_eq!(f.freshness(1_000_000 + 11_000), Freshness::Recent);
        assert_eq!(f.freshness(1_000_000 + 120_000), Freshness::Recent);
        assert_eq!(f.freshness(1_000_000 + 121_000), Freshness::Stale);
        assert!(!Freshness::Stale.usable() && Freshness::Recent.usable());
    }

    #[test]
    fn a_jump_is_held_until_a_second_fix_confirms_it() {
        let mut t = Tracker::new();
        t.take(at(33.75, -84.39), 0);
        // Atlanta to Denver in a second: held
        t.take(at(39.74, -104.99), 1_000);
        assert_eq!(t.fix().map(|f| f.lat), Some(33.75));
        assert_eq!(t.held().map(|f| f.lat), Some(39.74));
        // a second fix beside it confirms it
        t.take(at(39.7401, -104.99), 2_000);
        assert_eq!(t.fix().map(|f| f.lat), Some(39.7401));
        assert_eq!(t.held(), None);
        // a lone wild fix, then one back near the last: the wild one is forgotten
        t.take(at(-33.87, 151.21), 3_000);
        t.take(at(39.7402, -104.99), 4_000);
        assert_eq!(t.fix().map(|f| f.lat), Some(39.7402));
        assert_eq!(t.held(), None);
    }

    #[test]
    fn a_car_at_highway_speed_is_followed() {
        let mut t = Tracker::new();
        // about 30 m a second, 108 km/h
        for i in 0..10u32 {
            t.take(
                at(33.75 + f64::from(i) * 0.00027, -84.39),
                u64::from(i) * 1000,
            );
        }
        assert_eq!(t.fix().map(|f| f.at_ms), Some(9000));
        assert_eq!(t.held(), None);
    }

    #[test]
    fn hearing_says_nothing_positions_no_fix_or_silent() {
        let mut t = Tracker::new();
        assert_eq!(t.hearing(0), Hearing::Nothing);
        t.take(Said::Other, 500);
        t.take(Said::Other, 900);
        assert_eq!(t.hearing(950), Hearing::NoFix { since_ms: 500 });
        t.take(Said::NoFix, 1_000);
        t.take(Said::NoFix, 2_000);
        assert_eq!(t.hearing(2_500), Hearing::NoFix { since_ms: 1_000 });
        t.take(at(33.75, -84.39), 3_000);
        assert_eq!(t.hearing(3_500), Hearing::Positions);
        t.take(Said::Other, 4_000);
        assert_eq!(t.hearing(63_999), Hearing::Positions);
        assert_eq!(t.hearing(64_000), Hearing::Silent { since_ms: 4_000 });
        // the fix it had stays, aging
        assert_eq!(
            t.fix().map(|f| f.freshness(64_000)),
            Some(Freshness::Recent)
        );
    }

    #[test]
    fn an_rmc_keeps_the_altitude_of_the_gga_of_its_second() {
        let mut t = Tracker::new();
        t.take(
            Said::Position {
                lat: 33.75,
                lon: -84.39,
                alt_m: Some(320.0),
                quality: Quality::Gps,
            },
            10_000,
        );
        t.take(at(33.75, -84.39), 10_100);
        assert_eq!(t.fix().and_then(|f| f.alt_m), Some(320.0));
        t.take(at(33.75, -84.39), 15_000);
        assert_eq!(t.fix().and_then(|f| f.alt_m), None);
    }
}
