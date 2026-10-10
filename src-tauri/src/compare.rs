//! Sets what the model predicts for a path beside what the receiver has
//! heard in that direction, and turns the pair into a plain recommendation.
//!
//! The rule is a fixed table, not a model: every label can be traced to the
//! predicted tier and the observed tier that produced it.
//!
//! Both sides are FT8. The observations are FT8 decodes, so the prediction
//! they are compared with is the one for FT8's SNR requirement, whatever
//! mode the operator intends to use.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::geo::{self, LatLon};
use crate::observations::{Database, HearingStation, HeardStation};

/// Predicted reliability at or above this is "good"; below `MARGINAL`, "poor".
const GOOD_RELIABILITY: f64 = 0.7;
const MARGINAL_RELIABILITY: f64 = 0.3;
/// Fewer transmit periods than this on a band is too little to judge it by.
const MIN_PERIODS: f64 = 4.0;
/// A station this close to the destination is evidence for the path.
const NEAR_DESTINATION_KM: f64 = 1500.0;
/// So is one within this of the path's bearing and far enough along it.
const BEARING_TOLERANCE_DEG: f64 = 15.0;
const ALONG_PATH_SHARE: f64 = 0.6;
const ALONG_PATH_MAX_KM: f64 = 12_000.0;
/// Stations heard toward the destination at which evidence is moderate and strong.
const MODERATE_STATIONS: usize = 3;
const STRONG_STATIONS: usize = 8;
/// How many callsigns to name as examples.
const EXAMPLES: usize = 3;

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PredictedTier {
    Good,
    Marginal,
    Poor,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ObservedTier {
    /// The band has not been listened to for long enough to say anything.
    NotSampled,
    None,
    Limited,
    Moderate,
    Strong,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Verdict {
    pub label: &'static str,
    pub detail: &'static str,
    /// 1 is the band to try first.
    pub priority: u8,
}

pub fn predicted_tier(reliability: f64) -> PredictedTier {
    if reliability >= GOOD_RELIABILITY {
        PredictedTier::Good
    } else if reliability >= MARGINAL_RELIABILITY {
        PredictedTier::Marginal
    } else {
        PredictedTier::Poor
    }
}

pub fn observed_tier(periods_listened: f64, evidence_stations: usize) -> ObservedTier {
    if periods_listened < MIN_PERIODS {
        ObservedTier::NotSampled
    } else if evidence_stations >= STRONG_STATIONS {
        ObservedTier::Strong
    } else if evidence_stations >= MODERATE_STATIONS {
        ObservedTier::Moderate
    } else if evidence_stations > 0 {
        ObservedTier::Limited
    } else {
        ObservedTier::None
    }
}

/// The rule table.
pub fn verdict(predicted: PredictedTier, observed: ObservedTier) -> Verdict {
    use ObservedTier as O;
    use PredictedTier as P;
    let (label, detail, priority) = match (predicted, observed) {
        (P::Good, O::Strong | O::Moderate) => {
            ("HIGH PRIORITY", "Predicted open and confirmed by stations heard that way.", 1)
        }
        (P::Marginal, O::Strong) => {
            ("GOOD", "Better than predicted: many stations heard that way.", 2)
        }
        (P::Poor, O::Strong | O::Moderate) => {
            ("INVESTIGATE", "Predicted poor, yet stations are being heard that way. Possible unexpected opening.", 3)
        }
        (P::Good, O::Limited) => ("TRY", "Predicted open; little heard that way so far.", 4),
        (P::Good, O::None) => ("TRY", "Predicted open; nothing heard that way yet.", 4),
        (P::Marginal, O::Moderate) => {
            ("WORTH TRYING", "Marginal in prediction, and stations are being heard that way.", 4)
        }
        (P::Good, O::NotSampled) => ("PREDICTED", "Predicted open; not listened to yet.", 5),
        (P::Marginal, O::Limited) => ("MARGINAL", "Marginal in prediction; little heard that way.", 6),
        (P::Marginal, O::None) => ("MARGINAL", "Marginal in prediction; nothing heard that way.", 6),
        (P::Marginal, O::NotSampled) => ("MARGINAL", "Marginal in prediction; not listened to yet.", 6),
        (P::Poor, O::Limited) => ("LOW", "Predicted poor; little heard that way.", 7),
        (P::Poor, O::None) => ("LOW", "Predicted poor and nothing heard that way.", 7),
        (P::Poor, O::NotSampled) => ("LOW", "Predicted poor; not listened to yet.", 7),
    };
    Verdict { label, detail, priority }
}

/// The smaller angle between two bearings.
fn bearing_difference(a: f64, b: f64) -> f64 {
    ((a - b + 540.0).rem_euclid(360.0) - 180.0).abs()
}

/// Whether hearing this station says something about the path: it is near
/// the destination, or it lies in that direction and most of the way there.
pub fn is_path_evidence(
    station: &HeardStation,
    destination: LatLon,
    path_bearing_deg: f64,
    path_distance_km: f64,
) -> bool {
    lies_on_path(
        LatLon { lat: station.lat, lon: station.lon },
        station.bearing_deg.zip(station.distance_km),
        destination,
        path_bearing_deg,
        path_distance_km,
    )
}

/// The same test for a station heard reporting this one's area.
pub fn is_reverse_evidence(
    station: &HearingStation,
    destination: LatLon,
    path_bearing_deg: f64,
    path_distance_km: f64,
) -> bool {
    lies_on_path(
        LatLon { lat: station.lat, lon: station.lon },
        Some((station.bearing_deg, station.distance_km)),
        destination,
        path_bearing_deg,
        path_distance_km,
    )
}

/// Near the destination, or in its direction (`bearing, distance` from the
/// receiver) and most of the way there.
fn lies_on_path(
    position: LatLon,
    bearing_distance: Option<(f64, f64)>,
    destination: LatLon,
    path_bearing_deg: f64,
    path_distance_km: f64,
) -> bool {
    if geo::distance_km(position, destination) <= NEAR_DESTINATION_KM {
        return true;
    }
    let far_enough = (path_distance_km * ALONG_PATH_SHARE).min(ALONG_PATH_MAX_KM);
    bearing_distance.is_some_and(|(bearing, distance)| {
        bearing_difference(bearing, path_bearing_deg) <= BEARING_TOLERANCE_DEG && distance >= far_enough
    })
}

/// One band's prediction for the hour being compared.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BandPrediction {
    pub name: String,
    /// Reliability for the operator's chosen mode.
    pub mode_reliability: f64,
    /// Reliability for FT8, which is what the observations are.
    pub ft8_reliability: f64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompareQuery {
    /// How far back observations count.
    pub minutes: i64,
    pub destination: LatLon,
    /// Bearing and length of the path from the receiver, short or long way.
    pub path_bearing_deg: f64,
    pub path_distance_km: f64,
    pub bands: Vec<BandPrediction>,
    /// The receiving end, for decodes that do not say where they were made.
    #[serde(default)]
    pub receiver: Option<LatLon>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BandComparison {
    pub band: String,
    pub mode_reliability: f64,
    pub ft8_reliability: f64,
    pub predicted: PredictedTier,
    /// Transmit periods listened to on this band in the span.
    pub periods: f64,
    /// Everything heard on the band, from any direction.
    pub band_decodes: usize,
    pub band_callsigns: usize,
    /// Stations heard toward the destination.
    pub evidence_stations: usize,
    pub evidence_best_snr_db: Option<i32>,
    /// A few of them, strongest first.
    pub evidence_examples: Vec<String>,
    /// Stations toward the destination heard reporting this station or a
    /// station near it: the transmit direction.
    pub hearing_stations: usize,
    pub hearing_best_report_db: Option<i32>,
    pub hearing_examples: Vec<String>,
    /// How many of them reported this station itself.
    pub hearing_you: usize,
    pub observed: ObservedTier,
    pub verdict: Verdict,
}

pub fn compare(
    db: &Database,
    query: &CompareQuery,
    own_call: Option<&str>,
    now: i64,
) -> Result<Vec<BandComparison>, String> {
    let since = now - query.minutes * 60;
    let mut hearing: BTreeMap<String, Vec<HearingStation>> = BTreeMap::new();
    for station in db.hearing_your_area(since, None, own_call, query.receiver)? {
        if is_reverse_evidence(&station, query.destination, query.path_bearing_deg, query.path_distance_km) {
            hearing.entry(station.band.clone()).or_default().push(station);
        }
    }
    let activity: BTreeMap<String, _> = db
        .band_activity(since, now)?
        .into_iter()
        .map(|a| (a.band.clone(), a))
        .collect();

    let mut evidence: BTreeMap<String, Vec<HeardStation>> = BTreeMap::new();
    for station in db.heard_stations(since, None)? {
        if is_path_evidence(&station, query.destination, query.path_bearing_deg, query.path_distance_km) {
            evidence.entry(station.band.clone()).or_default().push(station);
        }
    }

    Ok(query
        .bands
        .iter()
        .map(|band| {
            let heard = activity.get(&band.name);
            let mut toward = evidence.remove(&band.name).unwrap_or_default();
            toward.sort_by_key(|s| std::cmp::Reverse(s.best_snr_db));
            let mut reverse = hearing.remove(&band.name).unwrap_or_default();
            reverse.sort_by_key(|s| std::cmp::Reverse((s.heard_you, s.best_report_db)));
            let periods = heard.map_or(0.0, |a| a.periods);
            let predicted = predicted_tier(band.ft8_reliability);
            let observed = observed_tier(periods, toward.len());
            BandComparison {
                band: band.name.clone(),
                mode_reliability: band.mode_reliability,
                ft8_reliability: band.ft8_reliability,
                predicted,
                periods,
                band_decodes: heard.map_or(0, |a| a.decodes),
                band_callsigns: heard.map_or(0, |a| a.unique_callsigns),
                evidence_stations: toward.len(),
                evidence_best_snr_db: toward.first().map(|s| s.best_snr_db),
                evidence_examples: toward.iter().take(EXAMPLES).map(|s| s.callsign.clone()).collect(),
                hearing_stations: reverse.len(),
                hearing_best_report_db: reverse.iter().map(|s| s.best_report_db).max(),
                hearing_examples: reverse.iter().take(EXAMPLES).map(|s| s.callsign.clone()).collect(),
                hearing_you: reverse.iter().filter(|s| s.heard_you).count(),
                observed,
                verdict: verdict(predicted, observed),
            }
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::observations::{Observation, ORIGIN_LOCAL};

    const LONDON: LatLon = LatLon { lat: 51.5, lon: -0.1 };
    /// Atlanta to London.
    const BEARING: f64 = 45.0;
    const DISTANCE: f64 = 6770.0;

    /// The Phase 6 exit test: every cell of the rule table.
    #[test]
    fn rule_table() {
        use ObservedTier as O;
        use PredictedTier as P;
        let expected = [
            (P::Good, O::Strong, "HIGH PRIORITY", 1),
            (P::Good, O::Moderate, "HIGH PRIORITY", 1),
            (P::Good, O::Limited, "TRY", 4),
            (P::Good, O::None, "TRY", 4),
            (P::Good, O::NotSampled, "PREDICTED", 5),
            (P::Marginal, O::Strong, "GOOD", 2),
            (P::Marginal, O::Moderate, "WORTH TRYING", 4),
            (P::Marginal, O::Limited, "MARGINAL", 6),
            (P::Marginal, O::None, "MARGINAL", 6),
            (P::Marginal, O::NotSampled, "MARGINAL", 6),
            (P::Poor, O::Strong, "INVESTIGATE", 3),
            (P::Poor, O::Moderate, "INVESTIGATE", 3),
            (P::Poor, O::Limited, "LOW", 7),
            (P::Poor, O::None, "LOW", 7),
            (P::Poor, O::NotSampled, "LOW", 7),
        ];
        for (predicted, observed, label, priority) in expected {
            let v = verdict(predicted, observed);
            assert_eq!((v.label, v.priority), (label, priority), "{predicted:?} + {observed:?}");
            assert!(!v.detail.is_empty());
        }
        // Silence and not having listened must never read the same.
        assert_ne!(verdict(P::Good, O::None).detail, verdict(P::Good, O::NotSampled).detail);
        assert_ne!(verdict(P::Poor, O::None).detail, verdict(P::Poor, O::NotSampled).detail);
    }

    #[test]
    fn tiers_follow_their_thresholds() {
        assert_eq!(predicted_tier(0.70), PredictedTier::Good);
        assert_eq!(predicted_tier(0.69), PredictedTier::Marginal);
        assert_eq!(predicted_tier(0.30), PredictedTier::Marginal);
        assert_eq!(predicted_tier(0.29), PredictedTier::Poor);

        // Under a minute of listening proves nothing, whatever was heard.
        assert_eq!(observed_tier(3.9, 20), ObservedTier::NotSampled);
        assert_eq!(observed_tier(4.0, 0), ObservedTier::None);
        assert_eq!(observed_tier(40.0, 2), ObservedTier::Limited);
        assert_eq!(observed_tier(40.0, 3), ObservedTier::Moderate);
        assert_eq!(observed_tier(40.0, 8), ObservedTier::Strong);
    }

    fn station(lat: f64, lon: f64, bearing: f64, distance: f64) -> HeardStation {
        HeardStation {
            callsign: "X1X".into(),
            grid: "AA00".into(),
            lat,
            lon,
            band: "20 m".into(),
            decodes: 1,
            best_snr_db: -10,
            last_heard_utc: 0,
            distance_km: Some(distance),
            bearing_deg: Some(bearing),
        }
    }

    #[test]
    fn evidence_is_near_the_destination_or_along_the_way() {
        let evidence = |s: &HeardStation| is_path_evidence(s, LONDON, BEARING, DISTANCE);
        // Paris: near London.
        assert!(evidence(&station(48.9, 2.3, 47.0, 7050.0)));
        // Near the destination counts whatever bearing was recorded.
        assert!(evidence(&station(48.9, 2.3, 200.0, 100.0)));
        // Casablanca: beyond 1,500 km from London, but recorded as the right way and far enough.
        assert!(evidence(&station(33.6, -7.6, 57.0, 6960.0)));
        // Boston: the right direction, but under 60% of the way.
        assert!(!evidence(&station(42.4, -71.1, 52.0, 1500.0)));
        // Brazil: far, in the wrong direction.
        assert!(!evidence(&station(-23.5, -46.6, 150.0, 7500.0)));
        // Bearings wrap: 355 and 5 degrees are 10 apart.
        assert!(is_path_evidence(&station(60.0, -85.0, 355.0, 5000.0), LONDON, 5.0, 6000.0));
        // A station with no recorded bearing is evidence only if near the destination.
        let mut unplaced = station(33.6, -7.6, 57.0, 6960.0);
        unplaced.bearing_deg = None;
        assert!(!evidence(&unplaced));
    }

    fn heard(db: &Database, time: i64, sender: &str, grid: &str, snr_db: i32, band: &str, dial_hz: u64) {
        let rx = geo::from_maidenhead("EM73").unwrap();
        let tx = geo::from_maidenhead(grid).unwrap();
        db.insert(&Observation {
            time_utc: time,
            dial_hz,
            band: band.into(),
            df_hz: 1000,
            snr_db,
            dt_s: 0.1,
            mode: "FT8".into(),
            message: format!("CQ {sender} {grid} {time}"),
            kind: "cq".into(),
            sender: Some(sender.into()),
            addressee: None,
            grid: Some(grid.into()),
            grid_source: Some("message".into()),
            distance_km: Some(geo::distance_km(rx, tx)),
            bearing_deg: Some(geo::bearing_deg(rx, tx)),
            rx_grid: Some("EM73".into()),
            origin: ORIGIN_LOCAL.into(),
            provider: "test".into(),
            low_confidence: false,
            settling: false,
        })
        .unwrap();
    }

    #[test]
    fn compares_each_band_with_what_was_heard_toward_the_destination() {
        let db = Database::in_memory().unwrap();
        let now = 10_000;
        // Ten minutes on 20 m, then one minute on 15 m. Nothing on 40 m or 10 m.
        let twenty = db.open_interval(now - 900, 14_074_000, "20 m", "FT8", "test").unwrap();
        db.extend_interval(twenty, now - 300).unwrap();
        let fifteen = db.open_interval(now - 300, 21_074_000, "15 m", "FT8", "test").unwrap();
        db.extend_interval(fifteen, now - 240).unwrap();

        // 20 m: three stations in western Europe and one in the US north-east.
        heard(&db, now - 800, "G4AAA", "IO91", -12, "20 m", 14_074_000);
        heard(&db, now - 785, "DL1DDD", "JO31", -5, "20 m", 14_074_000);
        heard(&db, now - 770, "EA1AAA", "IN73", -18, "20 m", 14_074_000);
        heard(&db, now - 755, "K1ABC", "FN42", 3, "20 m", 14_074_000);
        // 15 m: one station in Brazil.
        heard(&db, now - 290, "PY2CCC", "GG66", -9, "15 m", 21_074_000);

        let band = |name: &str, ft8: f64| BandPrediction {
            name: name.into(),
            mode_reliability: ft8 / 2.0,
            ft8_reliability: ft8,
        };
        let query = CompareQuery {
            minutes: 60,
            destination: LONDON,
            path_bearing_deg: BEARING,
            path_distance_km: DISTANCE,
            bands: vec![band("40 m", 0.1), band("20 m", 0.2), band("15 m", 0.9), band("10 m", 0.8)],
            receiver: None,
        };
        let rows = compare(&db, &query, None, now).unwrap();
        let summary: Vec<(&str, usize, usize, ObservedTier, &str)> = rows
            .iter()
            .map(|r| (r.band.as_str(), r.band_callsigns, r.evidence_stations, r.observed, r.verdict.label))
            .collect();
        assert_eq!(
            summary,
            [
                // Predicted poor, never listened to.
                ("40 m", 0, 0, ObservedTier::NotSampled, "LOW"),
                // Predicted poor, but three European stations heard: look into it.
                ("20 m", 4, 3, ObservedTier::Moderate, "INVESTIGATE"),
                // Predicted good; a minute of listening found only Brazil, which is not that way.
                ("15 m", 1, 0, ObservedTier::None, "TRY"),
                // Predicted good, never listened to.
                ("10 m", 0, 0, ObservedTier::NotSampled, "PREDICTED"),
            ]
        );

        let twenty = &rows[1];
        assert_eq!(twenty.periods, 40.0);
        assert_eq!(twenty.evidence_best_snr_db, Some(-5));
        assert_eq!(twenty.evidence_examples, ["DL1DDD", "G4AAA", "EA1AAA"]);
        assert_eq!((twenty.mode_reliability, twenty.ft8_reliability), (0.1, 0.2));

        // Observations older than the span do not count.
        let later = compare(&db, &CompareQuery { minutes: 3, ..query }, None, now).unwrap();
        assert_eq!(later[1].observed, ObservedTier::NotSampled);
        assert!(rows.iter().all(|r| r.hearing_stations == 0));
    }

    /// `to` reported by `sender` at `grid`, decoded here on 40 m.
    fn reported(db: &Database, time: i64, sender: &str, grid: &str, to: &str, report: &str) {
        let rx = geo::from_maidenhead("EM73").unwrap();
        let tx = geo::from_maidenhead(grid).unwrap();
        db.insert(&Observation {
            time_utc: time,
            dial_hz: 7_074_000,
            band: "40 m".into(),
            df_hz: 1500,
            snr_db: -14,
            dt_s: 0.1,
            mode: "FT8".into(),
            message: format!("{to} {sender} {report}"),
            kind: if report.starts_with('R') { "rogerReport" } else { "report" }.into(),
            sender: Some(sender.into()),
            addressee: Some(to.into()),
            grid: Some(grid.into()),
            grid_source: Some("remembered".into()),
            distance_km: Some(geo::distance_km(rx, tx)),
            bearing_deg: Some(geo::bearing_deg(rx, tx)),
            rx_grid: Some("EM73".into()),
            origin: ORIGIN_LOCAL.into(),
            provider: "test".into(),
            low_confidence: false,
            settling: false,
        })
        .unwrap();
    }

    #[test]
    fn stations_that_way_reporting_this_area_are_counted_beside_the_verdict() {
        let db = Database::in_memory().unwrap();
        let now = 10_000;
        // A neighbour about 100 km away, heard calling CQ, so its locator is known.
        heard(&db, now - 900, "N4NB", "EM74", 10, "40 m", 7_074_000);
        // London reports the neighbour and this station; Brazil reports the neighbour.
        reported(&db, now - 600, "G4AAA", "IO91", "N4NB", "-07");
        reported(&db, now - 585, "G4AAA", "IO91", "KK4ODA", "R-15");
        reported(&db, now - 570, "PY2CCC", "GG66", "N4NB", "-03");
        // A report to a station whose locator was never heard says nothing.
        reported(&db, now - 555, "DL1DDD", "JO31", "W9XYZ", "-01");

        let query = CompareQuery {
            minutes: 60,
            destination: LONDON,
            path_bearing_deg: BEARING,
            path_distance_km: DISTANCE,
            bands: vec![BandPrediction { name: "40 m".into(), mode_reliability: 0.5, ft8_reliability: 0.5 }],
            receiver: None,
        };
        let row = &compare(&db, &query, Some("kk4oda"), now).unwrap()[0];
        assert_eq!((row.hearing_stations, row.hearing_you, row.hearing_best_report_db), (1, 1, Some(-7)));
        assert_eq!(row.hearing_examples, ["G4AAA"]);
        // The verdict still rests on what this receiver heard toward London: the
        // two European senders themselves, which is limited evidence.
        assert_eq!((row.evidence_stations, row.verdict.label), (2, "MARGINAL"));
        // Without the operator's callsign, the neighbour's report alone still counts.
        let row = &compare(&db, &query, None, now).unwrap()[0];
        assert_eq!((row.hearing_stations, row.hearing_you), (1, 0));
    }
}
