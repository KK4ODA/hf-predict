//! Solar and geophysical conditions for the operator's awareness.
//!
//! None of this feeds the propagation model, which takes only the smoothed
//! sunspot number (see `solar`). It is shown alongside predictions, with its
//! source and age, so the operator can judge how far to trust them today.

pub mod fetch;
pub mod message;
pub mod products;
pub mod store;

use serde::Serialize;

use crate::solar;
use products::Product;
use store::{Store, StoredProduct};

const HOUR: i64 = 3600;
const DAY: i64 = 24 * HOUR;
/// A planetary K index at or above this is a geomagnetic storm (NOAA G1).
const STORM_K_INDEX: f64 = 5.0;
/// NOAA revises the sunspot prediction monthly.
const SSN_TABLE_STALE_AFTER: i64 = 45 * DAY;

/// One NOAA product the app reads.
pub struct ProductInfo {
    pub kind: &'static str,
    pub title: &'static str,
    pub url: &'static str,
    /// Its item in the Winlink PROPAGATION catalog.
    pub winlink_id: &'static str,
    /// Age beyond which a newer issue should exist.
    pub stale_after: i64,
}

pub const PRODUCTS: [ProductInfo; 4] = [
    ProductInfo {
        kind: "wwv",
        title: "Geophysical alert (WWV)",
        url: "https://services.swpc.noaa.gov/text/wwv.txt",
        winlink_id: "PROP_WWV",
        // Issued every three hours.
        stale_after: 6 * HOUR,
    },
    ProductInfo {
        kind: "sgas",
        title: "Daily solar and geophysical summary",
        url: "https://services.swpc.noaa.gov/text/sgas.txt",
        winlink_id: "PROP_SGAS",
        stale_after: 36 * HOUR,
    },
    ProductInfo {
        kind: "threeDay",
        title: "Three-day forecast",
        url: "https://services.swpc.noaa.gov/text/3-day-forecast.txt",
        winlink_id: "PROP3DNOAA",
        // Issued twice a day.
        stale_after: DAY,
    },
    ProductInfo {
        kind: "outlook27",
        title: "27-day outlook",
        url: "https://services.swpc.noaa.gov/text/27-day-outlook.txt",
        winlink_id: "PROP.27DO",
        // Issued weekly.
        stale_after: 8 * DAY,
    },
];

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProductStatus {
    pub kind: &'static str,
    pub title: &'static str,
    pub source_url: &'static str,
    pub winlink_id: &'static str,
    /// `None` until a copy has been fetched or imported.
    pub stored: Option<StoredProduct>,
    pub age_seconds: Option<i64>,
    pub stale_after_seconds: i64,
    /// True when the copy is older than `stale_after_seconds`. False when there is none.
    pub stale: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SsnTableStatus {
    #[serde(flatten)]
    pub table: solar::TableInfo,
    pub age_seconds: i64,
    pub stale: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Conditions {
    /// The time the ages were computed at.
    pub now: i64,
    pub products: Vec<ProductStatus>,
    pub ssn_table: SsnTableStatus,
    /// Set when current data shows a geomagnetic storm.
    pub storm: Option<String>,
}

pub fn conditions(store: &Store, now: i64) -> Conditions {
    let products: Vec<ProductStatus> = PRODUCTS
        .iter()
        .map(|info| {
            let stored = store.get(info.kind).cloned();
            let age_seconds = stored.as_ref().map(|s| now - s.issued);
            ProductStatus {
                kind: info.kind,
                title: info.title,
                source_url: info.url,
                winlink_id: info.winlink_id,
                stored,
                age_seconds,
                stale_after_seconds: info.stale_after,
                stale: age_seconds.is_some_and(|age| age > info.stale_after),
            }
        })
        .collect();

    // A stale alert says nothing about now, so it raises no storm warning.
    let storm = products.iter().find_map(|status| {
        let stored = status.stored.as_ref().filter(|_| !status.stale)?;
        let Product::Wwv(wwv) = &stored.product else { return None };
        let k_index = wwv.k_index.filter(|k| *k >= STORM_K_INDEX)?;
        Some(format!(
            "Geomagnetic storm: planetary K index {k_index} at {}. Predictions do not account \
             for it. Expect poorer conditions than predicted, most of all on paths through high \
             latitudes.",
            wwv.k_time.as_deref().unwrap_or("the last report")
        ))
    });

    let table = solar::table_info();
    let age_seconds = now - table.generated_unix;
    Conditions {
        now,
        products,
        ssn_table: SsnTableStatus { table, age_seconds, stale: age_seconds > SSN_TABLE_STALE_AFTER },
        storm,
    }
}

/// The Winlink message that asks for every product the app reads.
pub fn winlink_request() -> String {
    let ids: Vec<&str> = PRODUCTS.iter().map(|p| p.winlink_id).collect();
    format!("To: INQUIRY@winlink.org\nSubject: REQUEST\n\n{}\n", ids.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::timeutil;
    use store::Transport;

    const NOAA_WWV: &str = include_str!("../../../tests/fixtures/noaa/wwv.txt");
    const WINLINK_WWV: &str = include_str!("../../../tests/fixtures/winlink/wwv.mime");
    const NOAA_OUTLOOK: &str = include_str!("../../../tests/fixtures/noaa/27-day-outlook.txt");

    fn status<'a>(conditions: &'a Conditions, kind: &str) -> &'a ProductStatus {
        conditions.products.iter().find(|p| p.kind == kind).unwrap()
    }

    #[test]
    fn lists_every_product_even_with_nothing_stored() {
        let conditions = conditions(&Store::default(), 0);
        assert_eq!(conditions.products.len(), 4);
        assert!(conditions.products.iter().all(|p| p.stored.is_none() && !p.stale));
        assert!(conditions.storm.is_none());
    }

    #[test]
    fn ages_and_staleness_follow_the_clock() {
        let issued = timeutil::from_utc(2026, 10, 4, 21, 10);
        let mut store = Store::default();
        store.import(NOAA_WWV, Transport::Internet, issued + 60).unwrap();
        store.import(NOAA_OUTLOOK, Transport::Internet, issued + 60).unwrap();

        let fresh = conditions(&store, issued + 2 * HOUR);
        assert_eq!(status(&fresh, "wwv").age_seconds, Some(2 * HOUR));
        assert!(!status(&fresh, "wwv").stale);
        // The weekly outlook was issued six days earlier and is still current.
        assert!(!status(&fresh, "outlook27").stale);

        let later = conditions(&store, issued + 7 * HOUR);
        assert!(status(&later, "wwv").stale);
        let much_later = conditions(&store, issued + 9 * DAY);
        assert!(status(&much_later, "outlook27").stale);
    }

    #[test]
    fn warns_of_a_storm_only_from_current_data() {
        // The 2110 UTC alert reports K 5.67; the 1505 UTC one, K 4.67.
        let issued = timeutil::from_utc(2026, 10, 4, 21, 10);
        let mut stormy = Store::default();
        stormy.import(NOAA_WWV, Transport::Internet, issued).unwrap();
        let warning = conditions(&stormy, issued + HOUR).storm.unwrap();
        assert!(warning.contains("5.67") && warning.contains("2100 UTC on 04 October"));
        assert!(conditions(&stormy, issued + 7 * HOUR).storm.is_none(), "stale data must not warn");

        let mut calm = Store::default();
        calm.import(WINLINK_WWV, Transport::Pasted, issued).unwrap();
        assert!(conditions(&calm, timeutil::from_utc(2026, 10, 4, 16, 0)).storm.is_none());
    }

    #[test]
    fn winlink_request_names_each_catalog_item() {
        assert_eq!(
            winlink_request(),
            "To: INQUIRY@winlink.org\nSubject: REQUEST\n\nPROP_WWV\nPROP_SGAS\nPROP3DNOAA\nPROP.27DO\n"
        );
    }
}
