//! Parses VOACAP Method 30 output.
//!
//! Each hour is a block of rows. A row is a 6-character hour field, twelve
//! 5-character value fields (the MUF column, then up to eleven frequencies)
//! and a row label. Values can touch (`-3.2-25.4`), so fields are cut by
//! column, not split on spaces.

use std::collections::HashMap;

use crate::propagation::{FrequencyPrediction, HourPrediction, Prediction};

const HOUR_WIDTH: usize = 6;
const FIELD_WIDTH: usize = 5;
const COLUMNS: usize = 12;
const LABEL_START: usize = HOUR_WIDTH + FIELD_WIDTH * COLUMNS;

pub fn parse_output(text: &str) -> Result<Prediction, String> {
    let lines: Vec<&str> = text.lines().filter(|l| l.is_ascii()).collect();

    let engine = lines
        .iter()
        .find_map(|l| Some(l[l.find("VOACAP")?..l.find("PAGE")?].trim().to_string()))
        .ok_or("engine output has no version banner")?;

    let geometry: Vec<f64> = lines
        .iter()
        .position(|l| l.contains("AZIMUTHS"))
        .and_then(|i| lines.get(i + 1))
        .map(|l| l.split_whitespace().rev().take(4).filter_map(|t| t.parse().ok()).collect())
        .unwrap_or_default();
    let [distance_km, _nautical_miles, azimuth_rx_deg, azimuth_tx_deg] = geometry[..] else {
        return Err("engine output has no path geometry line".into());
    };

    let mut hours = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        if row_label(lines[i]) != Some("FREQ") {
            i += 1;
            continue;
        }
        let start = i;
        let mut rows: HashMap<&str, &str> = HashMap::new();
        while let Some(label) = lines.get(i).and_then(|l| row_label(l)) {
            rows.insert(label, lines[i]);
            i += 1;
        }
        hours.push(parse_hour(lines[start], &rows)?);
    }
    if hours.is_empty() {
        return Err("engine output has no prediction tables".into());
    }

    Ok(Prediction { engine, distance_km, azimuth_tx_deg, azimuth_rx_deg, hours })
}

fn row_label(line: &str) -> Option<&str> {
    let label = line.get(LABEL_START..)?.trim();
    (!label.is_empty()).then_some(label)
}

fn field(line: &str, column: usize) -> &str {
    let start = HOUR_WIDTH + FIELD_WIDTH * column;
    line.get(start..start + FIELD_WIDTH).unwrap_or("").trim()
}

fn parse_hour(freq_row: &str, rows: &HashMap<&str, &str>) -> Result<HourPrediction, String> {
    let hour_text = freq_row[..HOUR_WIDTH].trim();
    let utc_hour = hour_text
        .parse::<f64>()
        .map_err(|_| format!("unreadable hour '{hour_text}' in engine output"))? as u32;

    let number = |label: &str, column: usize| -> Result<f64, String> {
        let row = rows
            .get(label)
            .ok_or_else(|| format!("hour {utc_hour}: engine output has no {label} row"))?;
        let text = field(row, column);
        text.parse()
            .map_err(|_| format!("hour {utc_hour}: unreadable {label} value '{text}'"))
    };
    let column = |c: usize| -> Result<FrequencyPrediction, String> {
        Ok(FrequencyPrediction {
            freq_mhz: number("FREQ", c)?,
            mode: field(rows.get("MODE").ok_or("engine output has no MODE row")?, c).to_string(),
            takeoff_angle_deg: number("TANGLE", c)?,
            delay_ms: number("DELAY", c)?,
            virtual_height_km: number("V HITE", c)?,
            muf_day: number("MUFday", c)?,
            loss_db: number("LOSS", c)?,
            field_strength_dbu: number("DBU", c)?,
            signal_dbw: number("S DBW", c)?,
            noise_dbw: number("N DBW", c)?,
            snr_db: number("SNR", c)?,
            required_power_gain_db: number("RPWRG", c)?,
            reliability: number("REL", c)?,
            multipath_probability: number("MPROB", c)?,
            service_probability: number("S PRB", c)?,
            signal_lower_decile_db: number("SIG LW", c)?,
            signal_upper_decile_db: number("SIG UP", c)?,
            snr_lower_decile_db: number("SNR LW", c)?,
            snr_upper_decile_db: number("SNR UP", c)?,
            tx_gain_dbi: number("TGAIN", c)?,
            rx_gain_dbi: number("RGAIN", c)?,
            snr_at_required_reliability_db: number("SNRxx", c)?,
        })
    };

    let at_muf = column(0)?;
    // Unused frequency slots print as 0.0 with "-" in every other row.
    let frequencies = (1..COLUMNS)
        .take_while(|&c| number("FREQ", c).is_ok_and(|f| f > 0.0))
        .map(column)
        .collect::<Result<Vec<_>, _>>()?;

    Ok(HourPrediction { utc_hour, muf_mhz: at_muf.freq_mhz, at_muf, frequencies })
}

#[cfg(test)]
mod tests {
    use super::*;

    const REFERENCE: &str = include_str!("../../../tests/engine/cases/test01.out");

    #[test]
    fn parses_windows_voacap_reference_output() {
        let prediction = parse_output(REFERENCE).unwrap();

        assert_eq!(prediction.engine, "VOACAP 16.1207W");
        assert_eq!(prediction.distance_km, 2440.5);
        assert_eq!(prediction.azimuth_tx_deg, 57.41);
        assert_eq!(prediction.azimuth_rx_deg, 254.73);
        assert_eq!(prediction.hours.len(), 24);
        assert_eq!(
            prediction.hours.iter().map(|h| h.utc_hour).collect::<Vec<_>>(),
            (1..=24).collect::<Vec<_>>()
        );

        let first = &prediction.hours[0];
        assert_eq!(first.muf_mhz, 16.2);
        assert_eq!(first.at_muf.mode, "1F2");
        assert_eq!(first.at_muf.muf_day, 0.50);
        assert_eq!(first.frequencies.len(), 9);

        let f = &first.frequencies[0];
        assert_eq!(f.freq_mhz, 6.1);
        assert_eq!(f.mode, "1F2");
        assert_eq!(f.takeoff_angle_deg, 7.8);
        assert_eq!(f.delay_ms, 8.5);
        assert_eq!(f.virtual_height_km, 295.0);
        assert_eq!(f.muf_day, 1.00);
        assert_eq!(f.loss_db, 100.0);
        assert_eq!(f.field_strength_dbu, 60.0);
        assert_eq!(f.signal_dbw, -43.0);
        assert_eq!(f.noise_dbw, -149.0);
        assert_eq!(f.snr_db, 106.0);
        assert_eq!(f.required_power_gain_db, -20.0);
        assert_eq!(f.reliability, 1.00);
        assert_eq!(f.service_probability, 0.82);
        assert_eq!(f.signal_lower_decile_db, 10.8);
        assert_eq!(f.snr_upper_decile_db, 7.5);
        assert_eq!(f.tx_gain_dbi, 10.0);
        assert_eq!(f.rx_gain_dbi, 20.0);
        assert_eq!(f.snr_at_required_reliability_db, 93.0);

        let last = &first.frequencies[8];
        assert_eq!(last.freq_mhz, 25.9);
        assert_eq!(last.signal_dbw, -173.0);
        assert_eq!(last.snr_db, -3.0);
    }

    #[test]
    fn parses_fields_that_touch() {
        let touching = REFERENCE.replace(
            "       10.0 10.0 10.0 10.0 10.0 10.0 10.0 10.0 10.0 10.0   -    -  TGAIN",
            "       -3.2-25.4-18.3 -5.1 -5.5 -3.1-22.3-11.4 -2.9-10.0   -    -  TGAIN",
        );
        let prediction = parse_output(&touching).unwrap();
        let gains: Vec<f64> = prediction.hours[0].frequencies.iter().map(|f| f.tx_gain_dbi).collect();
        assert_eq!(gains, [-25.4, -18.3, -5.1, -5.5, -3.1, -22.3, -11.4, -2.9, -10.0]);
    }

    #[test]
    fn reports_output_without_tables() {
        assert!(parse_output("VOACAP 16.1207W PAGE 1\n").is_err());
        assert!(parse_output("").unwrap_err().contains("banner"));
    }
}
