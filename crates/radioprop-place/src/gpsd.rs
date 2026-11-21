// SPDX-License-Identifier: MIT OR Apache-2.0
//! gpsd's JSON reports (its protocol, `gpsd_json(5)`): a station on Linux where gpsd already holds
//! the receiver. The client asks with `?WATCH={"enable":true,"json":true}` and reads one report a
//! line; a `TPV` with `mode` 2 (2D) or 3 (3D) is a fix, `mode` 0 or 1 none.

use serde_json::Value;

use crate::nmea::{Quality, Said};

/// What a client sends gpsd to be told of every fix.
pub const WATCH: &str = "?WATCH={\"enable\":true,\"json\":true};\n";

/// gpsd's port.
pub const PORT: u16 = 2947;

/// Reads one of gpsd's lines.
#[must_use]
pub fn read(line: &str) -> Said {
    let Ok(Value::Object(m)) = serde_json::from_str::<Value>(line.trim()) else {
        return Said::Other;
    };
    if m.get("class").and_then(Value::as_str) != Some("TPV") {
        return Said::Other;
    }
    let mode = m.get("mode").and_then(Value::as_u64).unwrap_or(0);
    let lat = m.get("lat").and_then(Value::as_f64);
    let lon = m.get("lon").and_then(Value::as_f64);
    let (Some(lat), Some(lon)) = (lat, lon) else {
        return Said::NoFix;
    };
    if mode < 2 || !(-90.0..=90.0).contains(&lat) || !(-180.0..=180.0).contains(&lon) {
        return Said::NoFix;
    }
    // `altMSL` since gpsd 3.20; `alt` before it
    let alt_m = m
        .get("altMSL")
        .or_else(|| m.get("alt"))
        .and_then(Value::as_f64);
    // `status`: 2 DGPS, 3 RTK fixed, 4 RTK float, 5 dead reckoning, 6 GNSS and dead reckoning,
    // 7 time only, 8 simulated
    let quality = match m.get("status").and_then(Value::as_u64) {
        Some(2) => Quality::Differential,
        Some(3 | 4) => Quality::Rtk,
        Some(5 | 6) => Quality::Estimated,
        Some(7) => return Said::NoFix,
        Some(8) => Quality::Simulated,
        _ => Quality::Gps,
    };
    Said::Position {
        lat,
        lon,
        alt_m,
        quality,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tpv_with_a_2d_or_3d_fix_is_a_position() {
        let tpv = r#"{"class":"TPV","device":"/dev/ttyACM0","mode":3,"time":"2026-11-20T03:15:22.000Z","lat":33.7520,"lon":-84.3909,"altMSL":320.5,"alt":320.5}"#;
        assert_eq!(
            read(tpv),
            Said::Position {
                lat: 33.752,
                lon: -84.3909,
                alt_m: Some(320.5),
                quality: Quality::Gps
            }
        );
        let older = r#"{"class":"TPV","mode":2,"lat":33.75,"lon":-84.39,"alt":12.0,"status":2}"#;
        assert!(matches!(
            read(older),
            Said::Position {
                alt_m: Some(12.0),
                quality: Quality::Differential,
                ..
            }
        ));
    }

    #[test]
    fn no_fix_and_other_reports() {
        assert_eq!(read(r#"{"class":"TPV","mode":1}"#), Said::NoFix);
        assert_eq!(
            read(r#"{"class":"TPV","mode":1,"lat":33.75,"lon":-84.39}"#),
            Said::NoFix
        );
        assert_eq!(
            read(r#"{"class":"VERSION","release":"3.25","proto_major":3}"#),
            Said::Other
        );
        assert_eq!(read(r#"{"class":"SKY","satellites":[]}"#), Said::Other);
        assert_eq!(read("not json"), Said::Other);
    }
}
