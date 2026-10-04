//! Station positions: Maidenhead locators, latitude/longitude, great-circle geometry.

use serde::{Deserialize, Serialize};

const EARTH_RADIUS_KM: f64 = 6371.0;
pub const EARTH_CIRCUMFERENCE_KM: f64 = 2.0 * std::f64::consts::PI * EARTH_RADIUS_KM;

/// A position in degrees; north and east are positive.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct LatLon {
    pub lat: f64,
    pub lon: f64,
}

impl LatLon {
    pub fn new(lat: f64, lon: f64) -> Result<Self, String> {
        if !(-90.0..=90.0).contains(&lat) {
            return Err(format!("latitude {lat} is outside -90..90"));
        }
        if !(-180.0..=180.0).contains(&lon) {
            return Err(format!("longitude {lon} is outside -180..180"));
        }
        Ok(Self { lat, lon })
    }
}

/// Parses a Maidenhead locator (`EM73`, `EM73tr`, `EM73tr12`) or a
/// latitude/longitude pair (`33.75, -84.39` or `33.75N 84.39W`).
pub fn parse_position(text: &str) -> Result<LatLon, String> {
    let text = text.trim();
    if text.is_empty() {
        return Err("position is empty".into());
    }
    if text.chars().next().is_some_and(|c| c.is_ascii_alphabetic()) {
        return from_maidenhead(text);
    }
    let parts: Vec<&str> = text
        .split(|c: char| c == ',' || c.is_whitespace())
        .filter(|p| !p.is_empty())
        .collect();
    let [lat, lon] = parts[..] else {
        return Err(format!("'{text}' is not a locator or a latitude, longitude pair"));
    };
    LatLon::new(parse_degrees(lat, 'N', 'S')?, parse_degrees(lon, 'E', 'W')?)
}

fn parse_degrees(text: &str, positive: char, negative: char) -> Result<f64, String> {
    let (number, sign) = match text.chars().last().map(|c| c.to_ascii_uppercase()) {
        Some(c) if c == positive => (&text[..text.len() - 1], 1.0),
        Some(c) if c == negative => (&text[..text.len() - 1], -1.0),
        _ => (text, 1.0),
    };
    number
        .parse::<f64>()
        .map(|value| value * sign)
        .map_err(|_| format!("'{text}' is not a number of degrees"))
}

/// Centre of a 4-, 6- or 8-character Maidenhead locator.
pub fn from_maidenhead(locator: &str) -> Result<LatLon, String> {
    let chars: Vec<char> = locator.trim().chars().collect();
    if ![4, 6, 8].contains(&chars.len()) {
        return Err(format!("locator '{locator}' must have 4, 6 or 8 characters"));
    }
    let bad = || format!("'{locator}' is not a valid Maidenhead locator");
    let letter = |c: char, count: u32| -> Result<f64, String> {
        let index = (c.to_ascii_uppercase() as u32).wrapping_sub('A' as u32);
        if c.is_ascii_alphabetic() && index < count { Ok(index as f64) } else { Err(bad()) }
    };
    let digit = |c: char| c.to_digit(10).map(f64::from).ok_or_else(bad);

    // Each pair refines the one before: field, square, subsquare, extended square.
    let mut lon = -180.0 + letter(chars[0], 18)? * 20.0 + digit(chars[2])? * 2.0;
    let mut lat = -90.0 + letter(chars[1], 18)? * 10.0 + digit(chars[3])?;
    let (mut lon_cell, mut lat_cell) = (2.0, 1.0);
    if chars.len() >= 6 {
        lon_cell /= 24.0;
        lat_cell /= 24.0;
        lon += letter(chars[4], 24)? * lon_cell;
        lat += letter(chars[5], 24)? * lat_cell;
    }
    if chars.len() == 8 {
        lon_cell /= 10.0;
        lat_cell /= 10.0;
        lon += digit(chars[6])? * lon_cell;
        lat += digit(chars[7])? * lat_cell;
    }
    LatLon::new(lat + lat_cell / 2.0, lon + lon_cell / 2.0)
}

/// Six-character Maidenhead locator containing `position`.
pub fn to_maidenhead(position: LatLon) -> String {
    let lon = (position.lon + 180.0).clamp(0.0, 359.999_999);
    let lat = (position.lat + 90.0).clamp(0.0, 179.999_999);
    let letter = |base: u8, index: f64| (base + index as u8) as char;
    [
        letter(b'A', lon / 20.0),
        letter(b'A', lat / 10.0),
        letter(b'0', (lon % 20.0) / 2.0),
        letter(b'0', lat % 10.0),
        letter(b'a', (lon % 2.0) * 12.0),
        letter(b'a', (lat % 1.0) * 24.0),
    ]
    .iter()
    .collect()
}

/// Short-path great-circle distance.
pub fn distance_km(from: LatLon, to: LatLon) -> f64 {
    let (lat1, lat2) = (from.lat.to_radians(), to.lat.to_radians());
    let dlat = lat2 - lat1;
    let dlon = (to.lon - from.lon).to_radians();
    let a = (dlat / 2.0).sin().powi(2) + lat1.cos() * lat2.cos() * (dlon / 2.0).sin().powi(2);
    2.0 * EARTH_RADIUS_KM * a.sqrt().asin()
}

/// Short-path initial bearing from `from` towards `to`, in degrees east of north.
pub fn bearing_deg(from: LatLon, to: LatLon) -> f64 {
    let (lat1, lat2) = (from.lat.to_radians(), to.lat.to_radians());
    let dlon = (to.lon - from.lon).to_radians();
    let y = dlon.sin() * lat2.cos();
    let x = lat1.cos() * lat2.sin() - lat1.sin() * lat2.cos() * dlon.cos();
    y.atan2(x).to_degrees().rem_euclid(360.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64, tolerance: f64) -> bool {
        (a - b).abs() <= tolerance
    }

    #[test]
    fn maidenhead_square_centre() {
        let p = from_maidenhead("EM73").unwrap();
        assert!(close(p.lat, 33.5, 1e-9) && close(p.lon, -85.0, 1e-9), "{p:?}");
    }

    #[test]
    fn maidenhead_subsquare_centre() {
        // W1AW, Newington CT.
        let p = from_maidenhead("FN31pr").unwrap();
        assert!(close(p.lat, 41.7292, 1e-3) && close(p.lon, -72.7083, 1e-3), "{p:?}");
    }

    #[test]
    fn maidenhead_round_trip() {
        for locator in ["EM73tr", "FN31pr", "IO91wm", "QF56od", "AA00aa", "RR99xx"] {
            assert_eq!(to_maidenhead(from_maidenhead(locator).unwrap()), locator);
        }
    }

    #[test]
    fn rejects_bad_locators() {
        for locator in ["EM7", "SM73", "EMX3", "EM73zz", "E"] {
            assert!(from_maidenhead(locator).is_err(), "{locator} should be rejected");
        }
    }

    #[test]
    fn parses_latitude_longitude_forms() {
        let expected = LatLon { lat: 33.75, lon: -84.39 };
        assert_eq!(parse_position("33.75, -84.39").unwrap(), expected);
        assert_eq!(parse_position("33.75N 84.39W").unwrap(), expected);
        assert_eq!(parse_position(" 33.75n,84.39w ").unwrap(), expected);
        assert!(parse_position("91, 0").is_err());
        assert!(parse_position("33.75").is_err());
    }

    #[test]
    fn distance_and_bearing_match_voacap() {
        // Tangier to Belgrade, as printed in the VOACAP reference output.
        let tangier = LatLon { lat: 35.80, lon: -5.90 };
        let belgrade = LatLon { lat: 44.90, lon: 20.50 };
        assert!(close(distance_km(tangier, belgrade), 2440.5, 5.0));
        assert!(close(bearing_deg(tangier, belgrade), 57.41, 0.2));
        assert!(close(bearing_deg(belgrade, tangier), 254.73, 0.2));
    }
}
