//! Parsers for the NOAA SWPC text products the app shows. A missing or
//! unreadable value becomes `None`; only the product header is required.

use serde::{Deserialize, Serialize};

use crate::timeutil;

/// WWV geophysical alert: yesterday's flux and A index, the latest K index.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Wwv {
    pub solar_flux: Option<f64>,
    pub a_index: Option<f64>,
    pub k_index: Option<f64>,
    /// When the K index was estimated, as printed (`1500 UTC on 04 October`).
    pub k_time: Option<String>,
    pub past_24h: String,
    pub next_24h: String,
}

/// Daily solar and geophysical activity summary.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Sgas {
    /// The day the indices describe, as printed (`03 Oct`).
    pub data_date: Option<String>,
    pub solar_flux: Option<f64>,
    pub sunspot_number: Option<f64>,
    pub a_fredericksburg: Option<f64>,
    pub a_planetary: Option<f64>,
    pub xray_background: Option<String>,
    /// Eight three-hour planetary K indices for the day.
    pub planetary_k: Vec<Option<f64>>,
    /// Flare lines; empty when the report says none.
    pub energetic_events: Vec<String>,
    pub proton_events: Option<String>,
    pub geomagnetic_summary: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KpRow {
    /// Three-hour period as printed (`09-12UT`).
    pub period: String,
    /// One value per forecast day.
    pub values: Vec<Option<f64>>,
}

/// Three-day forecast of geomagnetic activity, radiation storms and radio blackouts.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreeDayForecast {
    /// The forecast days as printed (`Oct 04`).
    pub days: Vec<String>,
    pub kp: Vec<KpRow>,
    pub observed_max_kp: Option<f64>,
    pub expected_max_kp: Option<f64>,
    /// Probability in percent, one per day.
    pub radiation_storm_pct: Vec<Option<f64>>,
    pub blackout_r1_r2_pct: Vec<Option<f64>>,
    pub blackout_r3_pct: Vec<Option<f64>>,
    pub geomagnetic_rationale: Option<String>,
    pub blackout_rationale: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OutlookDay {
    /// `YYYY-MM-DD`.
    pub date: String,
    pub solar_flux: Option<f64>,
    pub a_index: Option<f64>,
    pub max_kp: Option<f64>,
}

/// Twenty-seven-day outlook of flux, A index and largest Kp.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Outlook27 {
    pub days: Vec<OutlookDay>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Product {
    Wwv(Wwv),
    Sgas(Sgas),
    ThreeDay(ThreeDayForecast),
    Outlook27(Outlook27),
}

impl Product {
    /// Stable identifier, also the storage key.
    pub fn kind(&self) -> &'static str {
        match self {
            Product::Wwv(_) => "wwv",
            Product::Sgas(_) => "sgas",
            Product::ThreeDay(_) => "threeDay",
            Product::Outlook27(_) => "outlook27",
        }
    }
}

/// A product and the time NOAA issued it.
#[derive(Debug, Clone, PartialEq)]
pub struct Parsed {
    pub product: Product,
    /// Seconds since the Unix epoch.
    pub issued: i64,
}

/// Parses one product's text, starting at its `:Product:` line.
pub fn parse(text: &str) -> Result<Parsed, String> {
    let header = |name: &str| {
        text.lines().find_map(|l| l.trim().strip_prefix(name)).map(str::trim)
    };
    let title = header(":Product:").ok_or("text has no :Product: line")?;
    let issued = header(":Issued:")
        .and_then(parse_issued)
        .ok_or_else(|| format!("'{title}' has no readable :Issued: line"))?;

    let product = if title.contains("Geophysical Alert") {
        Product::Wwv(parse_wwv(text))
    } else if title.contains("Solar and Geophysical Activity Summary") {
        Product::Sgas(parse_sgas(text))
    } else if title.contains("3-Day Forecast") {
        Product::ThreeDay(parse_three_day(text))
    } else if title.contains("27-day Space Weather Outlook") {
        Product::Outlook27(parse_outlook(text))
    } else {
        return Err(format!("'{title}' is not a product this app reads"));
    };
    Ok(Parsed { product, issued })
}

/// `2026 Oct 04 1505 UTC`
fn parse_issued(text: &str) -> Option<i64> {
    let mut parts = text.split_whitespace();
    let year = parts.next()?.parse().ok()?;
    let month = timeutil::month_number(parts.next()?)?;
    let day = parts.next()?.parse().ok()?;
    let clock: u32 = parts.next()?.parse().ok()?;
    Some(timeutil::from_utc(year, month, day, clock / 100, clock % 100))
}

/// The number that follows `marker`, ignoring a sentence-ending full stop.
fn number_after(text: &str, marker: &str) -> Option<f64> {
    let rest = text[text.find(marker)? + marker.len()..].trim_start();
    let token: String =
        rest.chars().take_while(|c| c.is_ascii_digit() || matches!(c, '.' | '-')).collect();
    token.trim_end_matches('.').parse().ok()
}

/// The text between two markers, with line breaks folded into spaces.
fn between(text: &str, start: &str, end: &str) -> Option<String> {
    let rest = &text[text.find(start)? + start.len()..];
    let section = &rest[..rest.find(end).unwrap_or(rest.len())];
    let folded = section.split_whitespace().collect::<Vec<_>>().join(" ");
    (!folded.is_empty()).then_some(folded)
}

/// Lines from the one starting with `start` up to the next blank line, folded.
fn paragraph(text: &str, start: &str) -> String {
    text.lines()
        .skip_while(|l| !l.trim_start().starts_with(start))
        .take_while(|l| !l.trim().is_empty())
        .map(str::trim)
        .collect::<Vec<_>>()
        .join(" ")
}

fn parse_wwv(text: &str) -> Wwv {
    let k_time = between(text, "K-index at ", " was ");
    Wwv {
        solar_flux: number_after(text, "Solar flux "),
        a_index: number_after(text, "A-index "),
        k_index: k_time.as_ref().and_then(|_| number_after(text, " was ")),
        k_time,
        past_24h: paragraph(text, "Space weather for the past 24 hours"),
        next_24h: paragraph(text, "Space weather for the next 24 hours"),
    }
}

fn parse_sgas(text: &str) -> Sgas {
    let indices = text.lines().find(|l| l.contains("10 cm") && l.contains("SSN")).unwrap_or("");
    let a_indices = indices
        .split_whitespace()
        .skip_while(|t| *t != "Afr/Ap")
        .nth(1)
        .unwrap_or("");
    let (afr, ap) = a_indices.split_once('/').unwrap_or((a_indices, ""));

    let k_line = text
        .lines()
        .skip_while(|l| !l.contains("3 Hour K-indices"))
        .nth(1)
        .unwrap_or("");
    let planetary_k = k_line
        .split_whitespace()
        .skip_while(|t| *t != "Planetary")
        .skip(1)
        .map(|t| t.parse().ok())
        .collect();

    let energetic_events = text
        .lines()
        .skip_while(|l| !l.starts_with("A."))
        .skip(1)
        .take_while(|l| !l.starts_with("B."))
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with("Begin") && *l != "None")
        .map(String::from)
        .collect();

    Sgas {
        data_date: between(text, "data received at SWO on ", "\n"),
        solar_flux: number_after(indices, "10 cm "),
        sunspot_number: number_after(indices, "SSN "),
        a_fredericksburg: afr.parse().ok(),
        a_planetary: ap.parse().ok(),
        xray_background: indices
            .split_whitespace()
            .skip_while(|t| *t != "Background")
            .nth(1)
            .map(String::from),
        planetary_k,
        energetic_events,
        proton_events: between(text, "Proton Events:", "\nC."),
        geomagnetic_summary: between(text, "Geomagnetic Activity Summary:", "\nD."),
    }
}

/// Values from a table row, skipping scale tags such as `(G1)`.
fn row_values(line: &str, skip_tokens: usize) -> Vec<Option<f64>> {
    line.split_whitespace()
        .skip(skip_tokens)
        .filter(|t| !t.starts_with('('))
        .map(|t| t.trim_end_matches('%').parse().ok())
        .collect()
}

fn parse_three_day(text: &str) -> ThreeDayForecast {
    // The day header is the first line made only of month-day pairs.
    let days = text
        .lines()
        .skip_while(|l| !l.contains("Kp index breakdown"))
        .skip(1)
        .map(|l| l.split_whitespace().collect::<Vec<_>>())
        .find(|tokens| {
            !tokens.is_empty()
                && tokens.len() % 2 == 0
                && tokens.chunks(2).all(|pair| timeutil::month_number(pair[0]).is_some())
        })
        .map(|tokens| tokens.chunks(2).map(|pair| pair.join(" ")).collect())
        .unwrap_or_default();

    let kp = text
        .lines()
        .filter_map(|l| {
            let period = l.split_whitespace().next()?;
            let is_period = period.len() == 7 && period.ends_with("UT") && period.contains('-');
            is_period.then(|| KpRow { period: period.to_string(), values: row_values(l, 1) })
        })
        .collect();

    let row = |label: &str, skip_tokens| {
        text.lines()
            .find(|l| l.trim_start().starts_with(label))
            .map(|l| row_values(l, skip_tokens))
            .unwrap_or_default()
    };

    ThreeDayForecast {
        days,
        kp,
        observed_max_kp: number_after(text, "past 24 hours was "),
        expected_max_kp: text
            .find("greatest expected 3 hr Kp")
            .and_then(|i| number_after(&text[i..], " is ")),
        radiation_storm_pct: row("S1 or greater", 3),
        blackout_r1_r2_pct: row("R1-R2", 1),
        blackout_r3_pct: row("R3 or greater", 3),
        geomagnetic_rationale: between(text, "Rationale:", "\nB."),
        blackout_rationale: text
            .find("C. NOAA Radio Blackout")
            .and_then(|i| between(&text[i..], "Rationale:", "\n\n\n")),
    }
}

fn parse_outlook(text: &str) -> Outlook27 {
    let days = text
        .lines()
        .filter(|l| !l.starts_with('#') && !l.starts_with(':'))
        .filter_map(|l| {
            let tokens: Vec<&str> = l.split_whitespace().collect();
            let [year, month, day, flux, a_index, kp] = tokens[..] else { return None };
            let (year, day): (i32, u32) = (year.parse().ok()?, day.parse().ok()?);
            Some(OutlookDay {
                date: format!("{year:04}-{:02}-{day:02}", timeutil::month_number(month)?),
                solar_flux: flux.parse().ok(),
                a_index: a_index.parse().ok(),
                max_kp: kp.parse().ok(),
            })
        })
        .collect();
    Outlook27 { days }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WWV: &str = include_str!("../../../tests/fixtures/noaa/wwv.txt");
    const SGAS: &str = include_str!("../../../tests/fixtures/noaa/sgas.txt");
    const THREE_DAY: &str = include_str!("../../../tests/fixtures/noaa/3-day-forecast.txt");
    const OUTLOOK: &str = include_str!("../../../tests/fixtures/noaa/27-day-outlook.txt");

    #[test]
    fn reads_the_geophysical_alert() {
        let parsed = parse(WWV).unwrap();
        assert_eq!(parsed.issued, timeutil::from_utc(2026, 10, 4, 21, 10));
        let Product::Wwv(wwv) = parsed.product else { panic!("not WWV") };
        assert_eq!(wwv.solar_flux, Some(92.0));
        assert_eq!(wwv.a_index, Some(40.0));
        assert_eq!(wwv.k_index, Some(5.67));
        assert_eq!(wwv.k_time.as_deref(), Some("2100 UTC on 04 October"));
        assert!(wwv.past_24h.ends_with("Geomagnetic storms reaching the G2 level occurred."));
        assert!(wwv.past_24h.starts_with("Space weather for the past 24 hours"));
        assert!(wwv.next_24h.starts_with("Space weather for the next 24 hours"));
    }

    #[test]
    fn reads_the_activity_summary() {
        let parsed = parse(SGAS).unwrap();
        assert_eq!(parsed.issued, timeutil::from_utc(2026, 10, 4, 2, 45));
        let Product::Sgas(sgas) = parsed.product else { panic!("not SGAS") };
        assert_eq!(sgas.data_date.as_deref(), Some("03 Oct"));
        assert_eq!(sgas.solar_flux, Some(93.0));
        assert_eq!(sgas.sunspot_number, Some(53.0));
        assert_eq!(sgas.a_fredericksburg, Some(8.0));
        assert_eq!(sgas.a_planetary, None, "??? is a missing value");
        assert_eq!(sgas.xray_background.as_deref(), Some("B1.7"));
        assert_eq!(
            sgas.planetary_k,
            [Some(2.0), Some(3.0), Some(3.0), Some(2.0), Some(2.0), Some(1.0), None, None]
        );
        assert!(sgas.energetic_events.is_empty());
        assert_eq!(sgas.proton_events.as_deref(), Some("None"));
        assert_eq!(
            sgas.geomagnetic_summary.as_deref(),
            Some("Field activity was at quiet to unsettled levels.")
        );
    }

    #[test]
    fn reads_the_three_day_forecast() {
        let parsed = parse(THREE_DAY).unwrap();
        assert_eq!(parsed.issued, timeutil::from_utc(2026, 10, 4, 12, 30));
        let Product::ThreeDay(forecast) = parsed.product else { panic!("not the forecast") };
        assert_eq!(forecast.days, ["Oct 04", "Oct 05", "Oct 06"]);
        assert_eq!(forecast.kp.len(), 8);
        assert_eq!(forecast.kp[0].period, "00-03UT");
        assert_eq!(forecast.kp[0].values, [Some(3.0), Some(3.33), Some(3.33)]);
        // The (G1) tag after 5.00 must not shift the columns.
        assert_eq!(forecast.kp[3].values, [Some(5.0), Some(3.0), Some(3.0)]);
        assert_eq!(forecast.observed_max_kp, Some(5.0));
        assert_eq!(forecast.expected_max_kp, Some(5.0));
        assert_eq!(forecast.radiation_storm_pct, [Some(1.0), Some(1.0), Some(1.0)]);
        assert_eq!(forecast.blackout_r1_r2_pct, [Some(5.0), Some(5.0), Some(5.0)]);
        assert_eq!(forecast.blackout_r3_pct, [Some(1.0), Some(1.0), Some(1.0)]);
        assert!(forecast.geomagnetic_rationale.unwrap().starts_with("G1 (Minor) geomagnetic storms"));
        assert!(forecast.blackout_rationale.unwrap().starts_with("No R1 (Minor)"));
    }

    #[test]
    fn reads_the_outlook() {
        let parsed = parse(OUTLOOK).unwrap();
        assert_eq!(parsed.issued, timeutil::from_utc(2026, 9, 28, 2, 21));
        let Product::Outlook27(outlook) = parsed.product else { panic!("not the outlook") };
        assert_eq!(outlook.days.len(), 27);
        assert_eq!(
            outlook.days[0],
            OutlookDay { date: "2026-09-28".into(), solar_flux: Some(98.0), a_index: Some(5.0), max_kp: Some(2.0) }
        );
        assert_eq!(outlook.days[26].date, "2026-10-24");
        assert_eq!(outlook.days[26].max_kp, Some(4.0));
    }

    #[test]
    fn rejects_text_that_is_not_a_known_product() {
        assert!(parse("hello").unwrap_err().contains(":Product:"));
        assert!(parse(":Product: Something Else\n:Issued: 2026 Oct 04 1505 UTC\n").is_err());
        assert!(parse(":Product: 3-Day Forecast\n").unwrap_err().contains(":Issued:"));
    }
}
