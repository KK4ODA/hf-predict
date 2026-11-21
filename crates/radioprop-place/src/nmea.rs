// SPDX-License-Identifier: MIT OR Apache-2.0
//! NMEA 0183 sentences, as GPS receivers and the phone apps that pass a phone's position on send
//! them: what counts as a position and what is ignored.
//!
//! * A sentence counts only with a valid checksum: the XOR of the characters between `$` and `*`.
//! * A position comes only from `RMC` with status `A` or `GGA` with a fix quality of 1 or more.
//!   Any talker (`GP`, `GN`, `GL`, `GA`, `GB`, `BD`, `GQ`, …) is taken: the sentence type decides.
//! * Satellite counts, HDOP and `GSA` are never read as quality: an iPhone app such as GPS2IP sends
//!   a fixed HDOP of 0.9 and eight satellites, because iOS does not give them.
//! * A datagram or a read may hold several sentences: [`sentences`] splits them.

use serde::{Deserialize, Serialize};

/// How the receiver says it got its fix.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Quality {
    /// A satellite fix: GGA quality 1 or 3, RMC mode `A` or none.
    Gps,
    /// Corrected: GGA quality 2 (DGPS, SBAS), RMC mode `D`.
    Differential,
    /// Real-time kinematic: GGA quality 4 or 5, RMC mode `R` or `F`.
    Rtk,
    /// Dead reckoning: GGA quality 6, RMC mode `E`.
    Estimated,
    /// Typed into the receiver: GGA quality 7, RMC mode `M`.
    Manual,
    /// A simulator: GGA quality 8, RMC mode `S`.
    Simulated,
}

/// What one sentence says of the position.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Said {
    /// A position, in degrees (north and east positive), with the altitude above mean sea level
    /// when the sentence carries one (GGA).
    Position {
        lat: f64,
        lon: f64,
        alt_m: Option<f64>,
        quality: Quality,
    },
    /// The receiver has no fix: RMC status `V` or mode `N`, GGA quality 0, or no coordinates.
    NoFix,
    /// A valid sentence that says nothing of the position (GSV, GSA, VTG, a proprietary one …).
    Other,
}

/// Why a line was not read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refused {
    /// It does not start with `$`.
    NotASentence,
    /// No `*` and two hexadecimal digits at its end.
    NoChecksum,
    /// The checksum does not match.
    BadChecksum,
    /// An RMC or GGA whose fields cannot be read.
    Malformed,
}

/// The sentences in a piece of text: split on CR and LF, blanks dropped.
pub fn sentences(text: &str) -> impl Iterator<Item = &str> {
    text.split(['\r', '\n'])
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

/// Reads one sentence.
///
/// # Errors
///
/// The line is not a sentence, its checksum is missing or wrong, or an RMC or GGA cannot be read.
pub fn read(line: &str) -> Result<Said, Refused> {
    let body = checked(line.trim())?;
    let fields: Vec<&str> = body.split(',').collect();
    let address = fields[0];
    // a talker's two letters and the type's three; a proprietary sentence starts with `P`
    if address.len() != 5 || address.starts_with('P') || !address.is_ascii() {
        return Ok(Said::Other);
    }
    match &address[2..] {
        "RMC" => rmc(&fields),
        "GGA" => gga(&fields),
        _ => Ok(Said::Other),
    }
}

/// The text between `$` and `*`, when the checksum after the `*` matches it.
fn checked(line: &str) -> Result<&str, Refused> {
    let rest = line.strip_prefix('$').ok_or(Refused::NotASentence)?;
    let (body, sum) = rest.rsplit_once('*').ok_or(Refused::NoChecksum)?;
    if sum.len() != 2 {
        return Err(Refused::NoChecksum);
    }
    let sum = u8::from_str_radix(sum, 16).map_err(|_| Refused::NoChecksum)?;
    let computed = body.bytes().fold(0u8, |x, b| x ^ b);
    if computed == sum {
        Ok(body)
    } else {
        Err(Refused::BadChecksum)
    }
}

/// `$--RMC,time,status,lat,N/S,lon,E/W,speed,course,date,variation,E/W[,mode[,navigation]]`
fn rmc(f: &[&str]) -> Result<Said, Refused> {
    if f.len() < 10 {
        return Err(Refused::Malformed);
    }
    if f[2] != "A" {
        return Ok(Said::NoFix);
    }
    let quality = match f.get(12).copied().unwrap_or("") {
        "" | "A" => Quality::Gps,
        "D" => Quality::Differential,
        "R" | "F" => Quality::Rtk,
        "E" => Quality::Estimated,
        "M" => Quality::Manual,
        "S" => Quality::Simulated,
        _ => return Ok(Said::NoFix), // `N`: not valid
    };
    position(f[3], f[4], f[5], f[6], None, quality)
}

/// `$--GGA,time,lat,N/S,lon,E/W,quality,satellites,hdop,altitude,M,separation,M,age,station`
fn gga(f: &[&str]) -> Result<Said, Refused> {
    if f.len() < 10 {
        return Err(Refused::Malformed);
    }
    let quality = match f[6] {
        "" | "0" => return Ok(Said::NoFix),
        "1" | "3" => Quality::Gps,
        "2" => Quality::Differential,
        "4" | "5" => Quality::Rtk,
        "6" => Quality::Estimated,
        "7" => Quality::Manual,
        "8" => Quality::Simulated,
        _ => return Err(Refused::Malformed),
    };
    let alt_m = match f[9] {
        "" => None,
        a => Some(a.parse::<f64>().map_err(|_| Refused::Malformed)?),
    };
    position(f[2], f[3], f[4], f[5], alt_m, quality)
}

fn position(
    lat: &str,
    ns: &str,
    lon: &str,
    ew: &str,
    alt_m: Option<f64>,
    quality: Quality,
) -> Result<Said, Refused> {
    if lat.is_empty() || lon.is_empty() {
        return Ok(Said::NoFix);
    }
    let lat = degrees(lat, 90.0)?
        * match ns {
            "N" => 1.0,
            "S" => -1.0,
            _ => return Err(Refused::Malformed),
        };
    let lon = degrees(lon, 180.0)?
        * match ew {
            "E" => 1.0,
            "W" => -1.0,
            _ => return Err(Refused::Malformed),
        };
    Ok(Said::Position {
        lat,
        lon,
        alt_m,
        quality,
    })
}

/// `ddmm.mmmm` or `dddmm.mmmm` as degrees.
fn degrees(text: &str, most: f64) -> Result<f64, Refused> {
    let value: f64 = text.parse().map_err(|_| Refused::Malformed)?;
    if !value.is_finite() || value < 0.0 {
        return Err(Refused::Malformed);
    }
    let whole = (value / 100.0).trunc();
    let minutes = value - whole * 100.0;
    let degrees = whole + minutes / 60.0;
    if minutes >= 60.0 || degrees > most {
        return Err(Refused::Malformed);
    }
    Ok(degrees)
}

/// A sentence with its checksum: for tests and for anyone making sentences.
#[must_use]
pub fn with_checksum(body: &str) -> String {
    let sum = body.bytes().fold(0u8, |x, b| x ^ b);
    format!("${body}*{sum:02X}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-6
    }

    #[test]
    fn the_checksum_is_the_xor_between_the_marks() {
        // NMEA's own example of an RMC, with its published checksum
        let line = "$GPRMC,123519,A,4807.038,N,01131.000,E,022.4,084.4,230394,003.1,W*6A";
        let Ok(Said::Position { lat, lon, .. }) = read(line) else {
            panic!("{:?}", read(line));
        };
        assert!(close(lat, 48.0 + 7.038 / 60.0) && close(lon, 11.0 + 31.0 / 60.0));
        let wrong = line.replace("*6A", "*6B");
        assert_eq!(read(&wrong), Err(Refused::BadChecksum));
        assert_eq!(read("$GPRMC,123519,A"), Err(Refused::NoChecksum));
        assert_eq!(read("GPRMC,123519,A*00"), Err(Refused::NotASentence));
    }

    #[test]
    fn a_fix_is_rmc_status_a_or_gga_quality_one_or_more() {
        let rmc = |status: &str| {
            with_checksum(&format!(
                "GNRMC,031522.00,{status},3345.123,N,08423.456,W,0.1,,201126,,,A"
            ))
        };
        assert!(matches!(read(&rmc("A")), Ok(Said::Position { .. })));
        assert_eq!(read(&rmc("V")), Ok(Said::NoFix));
        let gga = |q: &str| {
            with_checksum(&format!(
                "GPGGA,031522.00,3345.123,N,08423.456,W,{q},08,0.9,320.5,M,-30.1,M,,"
            ))
        };
        assert_eq!(read(&gga("0")), Ok(Said::NoFix));
        let Ok(Said::Position {
            lat,
            lon,
            alt_m,
            quality,
        }) = read(&gga("1"))
        else {
            panic!()
        };
        assert!(close(lat, 33.0 + 45.123 / 60.0) && close(lon, -(84.0 + 23.456 / 60.0)));
        assert_eq!((alt_m, quality), (Some(320.5), Quality::Gps));
        assert!(matches!(
            read(&gga("2")),
            Ok(Said::Position {
                quality: Quality::Differential,
                ..
            })
        ));
    }

    #[test]
    fn any_talker_is_taken_and_other_types_say_nothing() {
        for talker in ["GP", "GN", "GL", "GA", "GB", "BD", "GQ"] {
            let s = with_checksum(&format!(
                "{talker}GGA,031522,3345.123,N,08423.456,W,1,08,0.9,320.5,M,,M,,"
            ));
            assert!(matches!(read(&s), Ok(Said::Position { .. })), "{talker}");
        }
        for body in [
            "GPGSV,3,1,11,03,03,111,00,04,15,270,00,06,01,010,00,13,06,292,00",
            "GPGSA,A,3,04,05,,09,12,,,24,,,,,2.5,1.3,2.1",
            "PGRME,15.0,M,45.0,M,25.0,M",
        ] {
            assert_eq!(read(&with_checksum(body)), Ok(Said::Other), "{body}");
        }
    }

    #[test]
    fn an_rmc_mode_of_n_is_no_fix_and_others_name_the_quality() {
        let rmc = |mode: &str| {
            with_checksum(&format!(
                "GPRMC,031522,A,3345.123,N,08423.456,W,0.1,,201126,,,{mode}"
            ))
        };
        assert_eq!(read(&rmc("N")), Ok(Said::NoFix));
        assert!(matches!(
            read(&rmc("E")),
            Ok(Said::Position {
                quality: Quality::Estimated,
                ..
            })
        ));
    }

    #[test]
    fn empty_or_impossible_coordinates_are_no_fix_or_refused() {
        let empty = with_checksum("GPRMC,031522,A,,,,,,,201126,,");
        assert_eq!(read(&empty), Ok(Said::NoFix));
        let minutes = with_checksum("GPRMC,031522,A,3375.000,N,08423.456,W,0.1,,201126,,");
        assert_eq!(read(&minutes), Err(Refused::Malformed));
        let hemisphere = with_checksum("GPRMC,031522,A,3345.123,X,08423.456,W,0.1,,201126,,");
        assert_eq!(read(&hemisphere), Err(Refused::Malformed));
        assert_eq!(read(&with_checksum("GPGGA,1,2")), Err(Refused::Malformed));
    }

    #[test]
    fn a_datagram_of_several_sentences_splits_on_cr_lf() {
        let text = format!(
            "{}\r\n{}\r\n\r\n",
            with_checksum("GPGGA,031522,3345.123,N,08423.456,W,1,08,0.9,320.5,M,,M,,"),
            with_checksum("GPRMC,031522,A,3345.123,N,08423.456,W,0.1,,201126,,")
        );
        let all: Vec<&str> = sentences(&text).collect();
        assert_eq!(all.len(), 2);
        assert!(all
            .iter()
            .all(|s| matches!(read(s), Ok(Said::Position { .. }))));
    }
}
