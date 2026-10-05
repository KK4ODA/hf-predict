//! Imports WSJT-X's `ALL.TXT` log: one line per decode, with date, time and
//! dial frequency, so history can be loaded without the program running.
//!
//! Lines look like
//! `241004_153015    14.074 Rx FT8    -12  0.3 1234 CQ K1ABC FN42`.

use std::sync::Arc;

use serde::Serialize;

use super::tracker::{GridMemory, Heard};
use crate::observations::Database;
use crate::timeutil;

pub const PROVIDER: &str = "all.txt";

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportSummary {
    pub stored: usize,
    /// Decodes that were already in the database.
    pub already_stored: usize,
    /// The station's own transmissions, which are not observations.
    pub transmissions: usize,
    /// Lines that are not in the format above.
    pub not_understood: usize,
}

struct Line<'a> {
    time_utc: i64,
    dial_hz: u64,
    received: bool,
    mode: &'a str,
    snr_db: i32,
    dt_s: f64,
    df_hz: u32,
    message: String,
}

/// `241004_153015`
fn parse_time(stamp: &str) -> Option<i64> {
    let (date, time) = stamp.split_once('_')?;
    if date.len() != 6 || time.len() != 6 || !stamp.is_ascii() {
        return None;
    }
    let part = |text: &str, at: usize| text[at..at + 2].parse::<u32>().ok();
    let (year, month, day) = (part(date, 0)?, part(date, 2)?, part(date, 4)?);
    let (hour, minute, second) = (part(time, 0)?, part(time, 2)?, part(time, 4)?);
    let valid = (1..=12).contains(&month) && (1..=31).contains(&day) && hour < 24 && minute < 60 && second < 60;
    valid.then(|| timeutil::from_utc(2000 + year as i32, month, day, hour, minute) + i64::from(second))
}

fn parse_line(line: &str) -> Option<Line<'_>> {
    let mut tokens = line.split_whitespace();
    let time_utc = parse_time(tokens.next()?)?;
    let mhz: f64 = tokens.next()?.parse().ok()?;
    let received = match tokens.next()? {
        "Rx" => true,
        "Tx" => false,
        _ => return None,
    };
    let mode = tokens.next()?;
    let snr_db = tokens.next()?.parse().ok()?;
    let dt_s = tokens.next()?.parse().ok()?;
    let df_hz = tokens.next()?.parse().ok()?;
    let message = tokens.collect::<Vec<_>>().join(" ");
    Some(Line { time_utc, dial_hz: (mhz * 1e6).round() as u64, received, mode, snr_db, dt_s, df_hz, message })
}

/// Stores every received decode in `text`. `rx_grid` is where the receiver
/// was, which the log does not record.
pub fn import(db: &Arc<Database>, text: &str, rx_grid: Option<&str>) -> Result<ImportSummary, String> {
    let mut grids = GridMemory::new(db.clone());
    let mut summary = ImportSummary::default();
    for line in text.lines().filter(|l| !l.trim().is_empty()) {
        let Some(line) = parse_line(line) else {
            summary.not_understood += 1;
            continue;
        };
        if !line.received {
            summary.transmissions += 1;
            continue;
        }
        let stored = grids.store(&Heard {
            time_utc: line.time_utc,
            dial_hz: line.dial_hz,
            mode: line.mode,
            df_hz: line.df_hz,
            snr_db: line.snr_db,
            dt_s: line.dt_s,
            message: &line.message,
            rx_grid,
            provider: PROVIDER,
            low_confidence: false,
            settling: false,
        })?;
        if stored {
            summary.stored += 1;
        } else {
            summary.already_stored += 1;
        }
    }
    Ok(summary)
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOG: &str = "\
241004_153000    14.074 Rx FT8    -12  0.3 1234 CQ K1ABC FN42
241004_153000    14.074 Rx FT8     -5  0.2 1711 K1ABC W9XYZ EN37
241004_153015    14.074 Tx FT8      0  0.0 1500 K1ABC N0CALL EM73
241004_153030    14.074 Rx FT8    -11  0.4  902 W9XYZ K1ABC -07
241004_235945     7.074 Rx FT4     -3 -0.1 2210 CQ DX JA1ZZZ PM95

this line is something else
241004_1530 14.074 Rx FT8 -12 0.3 1234 CQ K1ABC FN42
";

    #[test]
    fn imports_received_lines_only() {
        let db = Arc::new(Database::in_memory().unwrap());
        let summary = import(&db, LOG, Some("EM73")).unwrap();
        assert_eq!(
            summary,
            ImportSummary { stored: 4, already_stored: 0, transmissions: 1, not_understood: 2 }
        );

        let all = db.all().unwrap();
        let first = &all[0];
        assert_eq!(first.time_utc, timeutil::from_utc(2024, 10, 4, 15, 30));
        assert_eq!((first.dial_hz, first.band.as_str(), first.mode.as_str()), (14_074_000, "20 m", "FT8"));
        assert_eq!((first.snr_db, first.dt_s, first.df_hz), (-12, 0.3, 1234));
        assert_eq!(first.sender.as_deref(), Some("K1ABC"));
        assert_eq!(first.provider, PROVIDER);
        assert!(first.distance_km.is_some());

        // The report from K1ABC carries no locator; the one from its CQ is used.
        assert_eq!(all[2].grid_source.as_deref(), Some("remembered"));
        let last = &all[3];
        assert_eq!(last.time_utc, timeutil::from_utc(2024, 10, 4, 23, 59) + 45);
        assert_eq!((last.band.as_str(), last.mode.as_str(), last.dt_s), ("40 m", "FT4", -0.1));
    }

    #[test]
    fn importing_the_same_log_twice_adds_nothing() {
        let db = Arc::new(Database::in_memory().unwrap());
        import(&db, LOG, None).unwrap();
        let again = import(&db, LOG, None).unwrap();
        assert_eq!((again.stored, again.already_stored), (0, 4));
        assert_eq!(db.count().unwrap(), 4);
        // Without a receiver position there is no distance.
        assert_eq!(db.all().unwrap()[0].distance_km, None);
    }
}
