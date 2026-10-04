//! The monthly smoothed sunspot table: the one solar input the model takes.
//!
//! A table is bundled with the app (see `tools/update-ssn.mjs`) so predictions
//! work with no network. A newer one downloaded from NOAA replaces it for the
//! rest of the session and is cached for the next.

use std::sync::{OnceLock, PoisonError, RwLock};

use serde::{Deserialize, Serialize};

use crate::timeutil;

const BUNDLED_JSON: &str = include_str!("../data/smoothed-ssn.json");
const SOURCE: &str =
    "NOAA SWPC solar-cycle tables (services.swpc.noaa.gov), monthly smoothed sunspot number";
/// History before this is not needed and is left out of downloaded tables.
const FIRST_MONTH: &str = "1990-01";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Table {
    pub source: String,
    /// The date the table was built from NOAA's data, `YYYY-MM-DD`.
    pub generated: String,
    pub observed: Series,
    pub predicted: Series,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Series {
    /// First month as `YYYY-MM`.
    pub start: String,
    pub values: Vec<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SsnKind {
    Observed,
    Predicted,
    Manual,
}

/// The sunspot number used for a prediction and where it came from.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SsnUsed {
    pub value: f64,
    pub kind: SsnKind,
    /// Data source and the date the table was generated; empty for manual values.
    pub source: String,
    pub table_generated: String,
}

impl SsnUsed {
    pub fn manual(value: f64) -> Self {
        Self { value, kind: SsnKind::Manual, source: String::new(), table_generated: String::new() }
    }
}

/// What the Conditions screen says about the table in use.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TableInfo {
    pub source: String,
    pub generated: String,
    /// Midnight UTC of `generated`, in seconds since the Unix epoch.
    pub generated_unix: i64,
    /// False while the table bundled with the app is in use.
    pub downloaded: bool,
    pub last_observed_month: String,
    pub last_predicted_month: String,
}

static DOWNLOADED: RwLock<Option<Table>> = RwLock::new(None);

fn bundled() -> &'static Table {
    static TABLE: OnceLock<Table> = OnceLock::new();
    TABLE.get_or_init(|| serde_json::from_str(BUNDLED_JSON).expect("bundled sunspot table is invalid"))
}

fn with_active<R>(read: impl FnOnce(&Table, bool) -> R) -> R {
    let downloaded = DOWNLOADED.read().unwrap_or_else(PoisonError::into_inner);
    match downloaded.as_ref() {
        Some(table) => read(table, true),
        None => read(bundled(), false),
    }
}

/// Makes `table` the one predictions use if it is newer than the current
/// one. Returns whether it was taken.
pub fn install(table: Table) -> bool {
    // ISO dates order the same as text.
    let newer = with_active(|active, _| table.generated > active.generated);
    if newer {
        *DOWNLOADED.write().unwrap_or_else(PoisonError::into_inner) = Some(table);
    }
    newer
}

fn month_index(year: i32, month: u32) -> i64 {
    i64::from(year) * 12 + i64::from(month) - 1
}

fn parse_month(tag: &str) -> Option<(i32, u32)> {
    let (year, month) = tag.split_once('-')?;
    Some((year.parse().ok()?, month.parse().ok()?))
}

fn month_tag(index: i64) -> String {
    format!("{:04}-{:02}", index.div_euclid(12), index.rem_euclid(12) + 1)
}

impl Series {
    fn start_index(&self) -> Option<i64> {
        parse_month(&self.start).map(|(year, month)| month_index(year, month))
    }

    fn get(&self, year: i32, month: u32) -> Option<f64> {
        let offset = usize::try_from(month_index(year, month) - self.start_index()?).ok()?;
        self.values.get(offset).copied()
    }

    fn last_month(&self) -> String {
        self.start_index()
            .map(|start| month_tag(start + self.values.len() as i64 - 1))
            .unwrap_or_default()
    }
}

/// Smoothed sunspot number for a month, or `None` if the table does not cover it.
pub fn smoothed_ssn(year: i32, month: u32) -> Option<SsnUsed> {
    with_active(|table, _| {
        let (value, kind) = match table.observed.get(year, month) {
            Some(value) => (value, SsnKind::Observed),
            None => (table.predicted.get(year, month)?, SsnKind::Predicted),
        };
        Some(SsnUsed {
            value,
            kind,
            source: table.source.clone(),
            table_generated: table.generated.clone(),
        })
    })
}

pub fn table_info() -> TableInfo {
    with_active(|table, downloaded| {
        let generated_unix = table
            .generated
            .split('-')
            .map(|part| part.parse::<u32>().ok())
            .collect::<Option<Vec<_>>>()
            .filter(|parts| parts.len() == 3)
            .map_or(0, |p| timeutil::from_utc(p[0] as i32, p[1], p[2], 0, 0));
        TableInfo {
            source: table.source.clone(),
            generated: table.generated.clone(),
            generated_unix,
            downloaded,
            last_observed_month: table.observed.last_month(),
            last_predicted_month: table.predicted.last_month(),
        }
    })
}

#[derive(Deserialize)]
struct ObservedRow {
    #[serde(rename = "time-tag")]
    month: String,
    smoothed_ssn: f64,
}

#[derive(Deserialize)]
struct PredictedRow {
    #[serde(rename = "time-tag")]
    month: String,
    predicted_ssn: f64,
}

/// A run of consecutive months as a series, or an error naming the gap.
fn series(what: &str, rows: &[(String, f64)]) -> Result<Series, String> {
    let (first, _) = rows.first().ok_or_else(|| format!("NOAA's {what} table has no usable months"))?;
    let start = parse_month(first)
        .map(|(y, m)| month_index(y, m))
        .ok_or_else(|| format!("NOAA's {what} table has an unreadable month '{first}'"))?;
    for (offset, (month, _)) in rows.iter().enumerate() {
        if *month != month_tag(start + offset as i64) {
            return Err(format!("NOAA's {what} table has a gap before {month}"));
        }
    }
    Ok(Series { start: first.clone(), values: rows.iter().map(|(_, value)| *value).collect() })
}

/// Builds a table from NOAA's observed and predicted solar-cycle JSON, as
/// `tools/update-ssn.mjs` does for the bundled one. Observed smoothed values
/// lag about six months; predicted values continue from there.
pub fn table_from_swpc(observed_json: &str, predicted_json: &str, generated: &str) -> Result<Table, String> {
    let observed: Vec<ObservedRow> = serde_json::from_str(observed_json)
        .map_err(|e| format!("NOAA's observed table is not in the expected format: {e}"))?;
    let predicted: Vec<PredictedRow> = serde_json::from_str(predicted_json)
        .map_err(|e| format!("NOAA's predicted table is not in the expected format: {e}"))?;

    // A negative smoothed value means "not yet available".
    let observed: Vec<(String, f64)> = observed
        .into_iter()
        .filter(|row| row.month.as_str() >= FIRST_MONTH && row.smoothed_ssn >= 0.0)
        .map(|row| (row.month, row.smoothed_ssn))
        .collect();
    let observed = series("observed", &observed)?;
    let last_observed = observed.last_month();

    let predicted: Vec<(String, f64)> = predicted
        .into_iter()
        .filter(|row| row.month > last_observed)
        .map(|row| (row.month, row.predicted_ssn))
        .collect();
    let predicted = series("predicted", &predicted)?;
    let expected_start = observed.start_index().map(|s| month_tag(s + observed.values.len() as i64));
    if Some(&predicted.start) != expected_start.as_ref() {
        return Err(format!("NOAA's predicted table does not continue from {last_observed}"));
    }

    Ok(Table { source: SOURCE.to_string(), generated: generated.to_string(), observed, predicted })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn observed_months_come_from_the_observed_series() {
        // Fixed history that later table updates do not change.
        let entry = smoothed_ssn(1990, 1).unwrap();
        assert_eq!(entry.kind, SsnKind::Observed);
        assert_eq!(entry.value, 201.2);
    }

    #[test]
    fn series_join_without_a_gap() {
        let table = bundled();
        let (y, m) = parse_month(&table.predicted.start).unwrap();
        let (prev_y, prev_m) = if m == 1 { (y - 1, 12) } else { (y, m - 1) };

        assert_eq!(table.predicted.get(y, m).is_some(), true);
        assert_eq!(table.observed.get(y, m), None);
        assert!(table.observed.get(prev_y, prev_m).is_some());
        assert_eq!(table.observed.last_month(), month_tag(month_index(prev_y, prev_m)));
    }

    #[test]
    fn months_outside_the_table_are_not_covered() {
        assert!(smoothed_ssn(1989, 12).is_none());
        assert!(smoothed_ssn(2100, 1).is_none());
    }

    #[test]
    fn entries_carry_provenance() {
        let entry = smoothed_ssn(2000, 6).unwrap();
        assert!(entry.source.contains("NOAA"));
        assert_eq!(entry.table_generated.len(), 10);
    }

    #[test]
    fn builds_a_table_from_noaa_json() {
        let observed = r#"[
            {"time-tag":"1989-12","ssn":1,"smoothed_ssn":150.0},
            {"time-tag":"1990-01","ssn":1,"smoothed_ssn":201.2},
            {"time-tag":"1990-02","ssn":1,"smoothed_ssn":202.4},
            {"time-tag":"1990-03","ssn":1,"smoothed_ssn":-1}
        ]"#;
        let predicted = r#"[
            {"time-tag":"1990-02","predicted_ssn":999.0},
            {"time-tag":"1990-03","predicted_ssn":190.0},
            {"time-tag":"1990-04","predicted_ssn":180.0}
        ]"#;
        let table = table_from_swpc(observed, predicted, "2026-11-01").unwrap();
        assert_eq!(table.observed, Series { start: "1990-01".into(), values: vec![201.2, 202.4] });
        assert_eq!(table.predicted, Series { start: "1990-03".into(), values: vec![190.0, 180.0] });
        assert_eq!(table.generated, "2026-11-01");
    }

    #[test]
    fn rejects_noaa_tables_with_gaps_or_a_new_format() {
        let observed = r#"[{"time-tag":"1990-01","smoothed_ssn":201.2},{"time-tag":"1990-03","smoothed_ssn":200.0}]"#;
        let predicted = r#"[{"time-tag":"1990-04","predicted_ssn":190.0}]"#;
        assert!(table_from_swpc(observed, predicted, "x").unwrap_err().contains("gap"));

        let observed = r#"[{"time-tag":"1990-01","smoothed_ssn":201.2}]"#;
        let late = r#"[{"time-tag":"1990-05","predicted_ssn":190.0}]"#;
        assert!(table_from_swpc(observed, late, "x").unwrap_err().contains("does not continue"));

        assert!(table_from_swpc("{}", predicted, "x").unwrap_err().contains("expected format"));
        assert!(table_from_swpc("[]", predicted, "x").unwrap_err().contains("no usable months"));
    }

    /// Installing changes shared state, so this test adds one month beyond the
    /// bundled table and leaves every month other tests read unchanged.
    #[test]
    fn a_newer_table_replaces_the_bundled_one() {
        let mut newer = bundled().clone();
        newer.generated = "2999-01-01".into();
        newer.predicted.values.push(3.5);
        let extra = parse_month(&newer.predicted.last_month()).unwrap();

        let mut older = bundled().clone();
        older.generated = "2000-01-01".into();
        assert!(!install(older), "an older table must not replace the current one");

        assert!(install(newer.clone()));
        assert_eq!(smoothed_ssn(extra.0, extra.1).unwrap().value, 3.5);
        let info = table_info();
        assert!(info.downloaded);
        assert_eq!(info.generated, "2999-01-01");
        assert_eq!(info.generated_unix, timeutil::from_utc(2999, 1, 1, 0, 0));
        assert_eq!(info.last_predicted_month, newer.predicted.last_month());
        assert!(!install(newer), "the same table again is not newer");
    }
}
