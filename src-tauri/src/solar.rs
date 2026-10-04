//! The bundled monthly smoothed sunspot table (see `tools/update-ssn.mjs`).

use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

const TABLE_JSON: &str = include_str!("../data/smoothed-ssn.json");

#[derive(Deserialize)]
struct Table {
    source: String,
    generated: String,
    observed: Series,
    predicted: Series,
}

#[derive(Deserialize)]
struct Series {
    /// First month as `YYYY-MM`.
    start: String,
    values: Vec<f64>,
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
    /// Data source and the date the bundled table was generated; empty for manual values.
    pub source: String,
    pub table_generated: String,
}

impl SsnUsed {
    pub fn manual(value: f64) -> Self {
        Self { value, kind: SsnKind::Manual, source: String::new(), table_generated: String::new() }
    }
}

fn table() -> &'static Table {
    static TABLE: OnceLock<Table> = OnceLock::new();
    TABLE.get_or_init(|| serde_json::from_str(TABLE_JSON).expect("bundled sunspot table is invalid"))
}

fn month_index(year: i32, month: u32) -> i64 {
    year as i64 * 12 + month as i64 - 1
}

impl Series {
    fn get(&self, year: i32, month: u32) -> Option<f64> {
        let (start_year, start_month) = self.start.split_once('-')?;
        let start = month_index(start_year.parse().ok()?, start_month.parse().ok()?);
        let offset = usize::try_from(month_index(year, month) - start).ok()?;
        self.values.get(offset).copied()
    }
}

/// Smoothed sunspot number for a month, or `None` if the table does not cover it.
pub fn smoothed_ssn(year: i32, month: u32) -> Option<SsnUsed> {
    let table = table();
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn observed_months_come_from_the_observed_series() {
        // Solar cycle 22 maximum region; fixed history that later table updates do not change.
        let entry = smoothed_ssn(1990, 1).unwrap();
        assert_eq!(entry.kind, SsnKind::Observed);
        assert_eq!(entry.value, 201.2);
    }

    #[test]
    fn series_join_without_a_gap() {
        let table = table();
        let (y, m) = table.predicted.start.split_once('-').unwrap();
        let (y, m): (i32, u32) = (y.parse().unwrap(), m.parse().unwrap());
        let (prev_y, prev_m) = if m == 1 { (y - 1, 12) } else { (y, m - 1) };

        assert_eq!(smoothed_ssn(y, m).unwrap().kind, SsnKind::Predicted);
        assert_eq!(smoothed_ssn(prev_y, prev_m).unwrap().kind, SsnKind::Observed);
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
}
