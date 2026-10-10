//! A listening plan: which bands to listen on, for how long and in what
//! order, from what the model predicts, what has been heard lately and how
//! long each band has gone unsampled. The plan is a pure function of its
//! inputs, so it can be tested without a radio. In this phase the operator
//! follows it by hand or ticks its bands in WSJT-X's band hopping; a later
//! phase will drive the radio from it.

use std::cmp::Ordering;

use serde::Serialize;

use crate::compare::ObservedTier;

/// One FT8 transmit period.
pub const SLOT_S: u32 = 15;
/// Every enabled band is listened to at least once in this long.
pub const PROBE_FLOOR_S: u32 = 20 * 60;
/// A cell of the world counts as in reach at this FT8 reliability.
pub const REACH_RELIABILITY: f64 = 0.3;
/// Useful periods in a dwell of each tier; one more is lost to retuning.
const LONG_SLOTS: u32 = 8;
const STANDARD_SLOTS: u32 = 4;
const PROBE_SLOTS: u32 = 2;
const LONG_BANDS: usize = 3;
const STANDARD_BANDS: usize = 2;
/// Hours without listening after which staleness stops growing.
const STALE_AFTER_H: f64 = 1.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Tier {
    Long,
    Standard,
    Probe,
}

impl Tier {
    fn slots(self) -> u32 {
        match self {
            Tier::Long => LONG_SLOTS,
            Tier::Standard => STANDARD_SLOTS,
            Tier::Probe => PROBE_SLOTS,
        }
    }

    /// Time on the band, retuning included.
    pub fn dwell_s(self) -> u32 {
        (self.slots() + 1) * SLOT_S
    }
}

/// What is known about one band when the plan is made.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BandInput {
    pub band: String,
    pub dial_hz: u64,
    /// FT8 reliability to the destination, or the share of the world in reach.
    pub prediction: f64,
    pub observed: ObservedTier,
    pub minutes_since_listened: Option<f64>,
    pub excluded: bool,
}

/// A band's standing in the plan.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BandRank {
    pub band: String,
    pub dial_hz: u64,
    pub prediction: f64,
    pub observed: ObservedTier,
    pub minutes_since_listened: Option<f64>,
    pub priority: f64,
    /// None when the band is excluded.
    pub tier: Option<Tier>,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanItem {
    pub band: String,
    pub dial_hz: u64,
    /// Seconds after the plan starts.
    pub start_s: u32,
    pub dwell_s: u32,
    pub tier: Tier,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Plan {
    pub minutes: u32,
    /// Every band, best first, excluded ones last.
    pub bands: Vec<BandRank>,
    /// Enabled bands best first, for WSJT-X's band hopping.
    pub hop_bands: Vec<String>,
    /// One pass over every enabled band takes this long; the plan repeats it.
    pub cycle_s: u32,
    pub items: Vec<PlanItem>,
}

fn observed_bonus(tier: ObservedTier) -> f64 {
    match tier {
        ObservedTier::Strong => 0.5,
        ObservedTier::Moderate => 0.3,
        ObservedTier::Limited => 0.1,
        ObservedTier::None | ObservedTier::NotSampled => 0.0,
    }
}

/// 0 when the band was just listened to, 1 after `STALE_AFTER_H` or never.
fn staleness(minutes_since_listened: Option<f64>) -> f64 {
    minutes_since_listened.map_or(1.0, |m| (m / 60.0 / STALE_AFTER_H).clamp(0.0, 1.0))
}

/// Deliberately simple: each term is between 0 and 1, except the observed
/// bonus which tops out at 0.5, and they add.
pub fn priority(input: &BandInput) -> f64 {
    input.prediction + observed_bonus(input.observed) + staleness(input.minutes_since_listened)
}

fn reason(input: &BandInput) -> String {
    let heard = match input.observed {
        ObservedTier::Strong => "many stations heard in the last hour",
        ObservedTier::Moderate => "some stations heard in the last hour",
        ObservedTier::Limited => "little heard in the last hour",
        ObservedTier::None => "nothing heard in the last hour",
        ObservedTier::NotSampled => "not listened to in the last hour",
    };
    let since = match input.minutes_since_listened {
        None => "never listened to".to_string(),
        Some(m) if m < 1.0 => "listening now".to_string(),
        Some(m) if m < 90.0 => format!("last listened {} min ago", m.round()),
        Some(m) if m < 48.0 * 60.0 => format!("last listened {} h ago", (m / 60.0).round()),
        Some(m) => format!("last listened {} days ago", (m / 1440.0).round()),
    };
    format!("predicted {}% · {heard} · {since}", (input.prediction * 100.0).round())
}

/// Ranks the bands, gives the top ones long dwells and the rest probes, and
/// lays them out in repeating passes for `minutes`.
pub fn plan(inputs: &[BandInput], minutes: u32) -> Plan {
    let mut ranked: Vec<(&BandInput, f64)> = inputs.iter().map(|b| (b, priority(b))).collect();
    ranked.sort_by(|a, b| {
        a.0.excluded
            .cmp(&b.0.excluded)
            .then_with(|| b.1.partial_cmp(&a.1).unwrap_or(Ordering::Equal))
            .then_with(|| a.0.band.cmp(&b.0.band))
    });
    let mut bands = Vec::new();
    let mut enabled = 0;
    for (input, priority) in &ranked {
        let tier = if input.excluded {
            None
        } else {
            enabled += 1;
            Some(if enabled <= LONG_BANDS {
                Tier::Long
            } else if enabled <= LONG_BANDS + STANDARD_BANDS {
                Tier::Standard
            } else {
                Tier::Probe
            })
        };
        bands.push(BandRank {
            band: input.band.clone(),
            dial_hz: input.dial_hz,
            prediction: input.prediction,
            observed: input.observed,
            minutes_since_listened: input.minutes_since_listened,
            priority: *priority,
            tier,
            reason: reason(input),
        });
    }

    let pass: Vec<(&BandRank, Tier)> = bands.iter().filter_map(|b| b.tier.map(|t| (b, t))).collect();
    let cycle_s: u32 = pass.iter().map(|(_, tier)| tier.dwell_s()).sum();
    let total_s = minutes * 60;
    let mut items = Vec::new();
    let mut start = 0;
    while cycle_s > 0 && start + pass[0].1.dwell_s() <= total_s {
        for (band, tier) in &pass {
            if start + tier.dwell_s() > total_s {
                break;
            }
            items.push(PlanItem {
                band: band.band.clone(),
                dial_hz: band.dial_hz,
                start_s: start,
                dwell_s: tier.dwell_s(),
                tier: *tier,
            });
            start += tier.dwell_s();
        }
    }
    Plan {
        minutes,
        hop_bands: pass.iter().map(|(b, _)| b.band.clone()).collect(),
        bands,
        cycle_s,
        items,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(band: &str, prediction: f64, observed: ObservedTier, minutes: Option<f64>) -> BandInput {
        BandInput {
            band: band.into(),
            dial_hz: 14_074_000,
            prediction,
            observed,
            minutes_since_listened: minutes,
            excluded: false,
        }
    }

    fn nine() -> Vec<BandInput> {
        let predictions = [0.05, 0.1, 0.95, 0.6, 0.9, 0.5, 0.8, 0.3, 0.2];
        ["80 m", "60 m", "40 m", "30 m", "20 m", "17 m", "15 m", "12 m", "10 m"]
            .iter()
            .zip(predictions)
            .map(|(band, p)| input(band, p, ObservedTier::NotSampled, Some(10.0)))
            .collect()
    }

    #[test]
    fn top_bands_get_long_dwells_and_every_band_is_in_each_pass() {
        let plan = plan(&nine(), 30);

        assert_eq!(plan.hop_bands[..3], ["40 m", "20 m", "15 m"]);
        assert_eq!(plan.bands[0].tier, Some(Tier::Long));
        assert_eq!(plan.bands[3].tier, Some(Tier::Standard));
        assert_eq!(plan.bands[5].tier, Some(Tier::Probe));
        // One pass: three long, two standard, four probes, retuning included.
        assert_eq!(plan.cycle_s, 3 * 135 + 2 * 75 + 4 * 45);
        assert!(plan.cycle_s <= PROBE_FLOOR_S);
        let first_pass: Vec<&str> = plan.items.iter().take(9).map(|i| i.band.as_str()).collect();
        assert_eq!(first_pass.len(), 9);
        for band in plan.hop_bands.iter() {
            assert!(first_pass.contains(&band.as_str()), "{band} missing from the first pass");
        }
        assert_eq!((plan.items[0].band.as_str(), plan.items[0].start_s, plan.items[0].dwell_s), ("40 m", 0, 135));
        // Items follow each other and fit the plan.
        for pair in plan.items.windows(2) {
            assert_eq!(pair[0].start_s + pair[0].dwell_s, pair[1].start_s);
        }
        let last = plan.items.last().unwrap();
        assert!(last.start_s + last.dwell_s <= 30 * 60);
        assert!(plan.items.len() > 9, "a 30 minute plan repeats the pass");
    }

    #[test]
    fn what_was_heard_and_how_long_ago_change_the_order() {
        let mut inputs = nine();
        // 10 m predicted poor but busy and never sampled by this app.
        inputs[8] = input("10 m", 0.2, ObservedTier::Strong, None);
        let plan = plan(&inputs, 30);
        assert_eq!(plan.hop_bands[0], "10 m");
        assert_eq!(plan.bands[0].priority, 0.2 + 0.5 + 1.0);
        assert_eq!(plan.bands[0].reason, "predicted 20% · many stations heard in the last hour · never listened to");

        // A band listened to a moment ago gets no staleness.
        let fresh = input("20 m", 0.9, ObservedTier::None, Some(0.0));
        assert_eq!(priority(&fresh), 0.9);
        assert!(reason(&fresh).ends_with("listening now"));
        assert!(reason(&input("20 m", 0.9, ObservedTier::Limited, Some(300.0))).ends_with("last listened 5 h ago"));
    }

    #[test]
    fn excluded_bands_are_ranked_last_and_never_scheduled() {
        let mut inputs = nine();
        inputs[2].excluded = true; // 40 m, the best prediction
        let plan = plan(&inputs, 30);
        assert_eq!(plan.bands.last().map(|b| (b.band.as_str(), b.tier)), Some(("40 m", None)));
        assert!(!plan.hop_bands.iter().any(|b| b == "40 m"));
        assert!(!plan.items.iter().any(|i| i.band == "40 m"));
        assert_eq!(plan.hop_bands.len(), 8);
    }

    #[test]
    fn a_short_plan_holds_what_fits_and_nothing_with_no_bands() {
        let plan_short = plan(&nine(), 3);
        assert_eq!(plan_short.items.len(), 1, "only the first long dwell fits in three minutes");
        let mut all_excluded = nine();
        for input in &mut all_excluded {
            input.excluded = true;
        }
        let empty = plan(&all_excluded, 30);
        assert!(empty.items.is_empty() && empty.hop_bands.is_empty() && empty.cycle_s == 0);
    }
}
