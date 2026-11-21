// SPDX-License-Identifier: MIT OR Apache-2.0
//! Whole streams through the parsers and the tracker: what an iPhone running GPS2IP sends (a fixed
//! HDOP of 0.9 and eight satellites), what a USB receiver sends while it finds a fix (and a line
//! with a bad checksum, and one cut short), and gpsd's reports. `streams/README.md` says where
//! they come from.

use radioprop_place::{gpsd, nmea, Freshness, Hearing, Quality, Tracker};

const GPS2IP: &str = include_str!("streams/gps2ip.nmea");
const RECEIVER: &str = include_str!("streams/usb-receiver.nmea");
const GPSD: &str = include_str!("streams/gpsd.jsonl");

/// Feeds NMEA text, 100 ms a sentence from `start_ms`; returns the time after the last.
fn feed_nmea(t: &mut Tracker, text: &str, start_ms: u64) -> u64 {
    let mut now = start_ms;
    for s in nmea::sentences(text) {
        match nmea::read(s) {
            Ok(said) => t.take(said, now),
            Err(_) => t.refused(),
        }
        now += 100;
    }
    now
}

#[test]
fn gps2ip_gives_a_moving_fix_and_its_fixed_hdop_is_not_read_as_quality() {
    let mut t = Tracker::new();
    let end = feed_nmea(&mut t, GPS2IP, 0);
    assert_eq!(t.refused_count(), 0);
    assert_eq!(t.hearing(end), Hearing::Positions);
    let fix = t.fix().expect("a fix");
    assert!(
        (fix.lat - (33.7520 + 9.0 * 0.000_135)).abs() < 1e-6,
        "{fix:?}"
    );
    assert!((fix.lon + 84.3909).abs() < 1e-6, "{fix:?}");
    assert_eq!((fix.alt_m, fix.quality), (Some(320.5), Quality::Gps));
    assert_eq!(fix.freshness(end), Freshness::Current);
    assert_eq!(t.held(), None, "a car at 15 m/s is followed");
}

#[test]
fn a_receiver_finding_its_fix_says_so_then_gives_it() {
    let mut t = Tracker::new();
    let mut now = 0;
    let mut saw_no_fix = false;
    for s in nmea::sentences(RECEIVER) {
        match nmea::read(s) {
            Ok(said) => t.take(said, now),
            Err(_) => t.refused(),
        }
        saw_no_fix |= matches!(t.hearing(now), Hearing::NoFix { since_ms: 0 });
        now += 100;
    }
    assert!(saw_no_fix, "the three seconds without a fix are said");
    assert_eq!(
        t.refused_count(),
        2,
        "the bad checksum and the line cut short"
    );
    assert_eq!(t.hearing(now), Hearing::Positions);
    let fix = t.fix().expect("a fix");
    assert!((fix.lat - 33.749).abs() < 1e-6 && (fix.lon + 84.388).abs() < 1e-6);
    assert_eq!(fix.alt_m, Some(318.2));
}

#[test]
fn gpsd_reports_give_a_fix_after_its_greeting() {
    let mut t = Tracker::new();
    let mut now = 0;
    for line in GPSD.lines() {
        t.take(gpsd::read(line), now);
        if line.contains("\"mode\":1") {
            assert!(matches!(t.hearing(now), Hearing::NoFix { .. }));
        }
        now += 100;
    }
    let fix = t.fix().expect("a fix");
    assert_eq!(
        (fix.lat, fix.lon, fix.alt_m),
        (33.749, -84.388, Some(318.2))
    );
}
