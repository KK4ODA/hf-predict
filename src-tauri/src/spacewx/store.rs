//! The latest copy of each product, with where it came from and when.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::message;
use super::products::{self, Product};
use crate::jsonfile;

/// How a product reached this computer.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Transport {
    Internet,
    Winlink,
    Pasted,
    File,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredProduct {
    pub product: Product,
    /// When NOAA issued it, in seconds since the Unix epoch.
    pub issued: i64,
    /// When this app received it.
    pub received: i64,
    pub transport: Transport,
    /// The normalised product text the values were read from.
    pub text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ImportOutcome {
    Stored,
    AlreadyHave,
    OlderThanStored,
    NotUnderstood,
}

/// What happened to one product found in imported text.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Imported {
    /// The product's own title line.
    pub title: String,
    pub outcome: ImportOutcome,
    /// Why, when the product was not understood.
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Store {
    products: BTreeMap<String, StoredProduct>,
}

impl Store {
    pub fn load(path: &Path) -> Result<Self, String> {
        jsonfile::load(path)
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        jsonfile::save(path, self)
    }

    pub fn get(&self, kind: &str) -> Option<&StoredProduct> {
        self.products.get(kind)
    }

    /// Stores every product found in `raw` that is newer than the copy held.
    /// A Winlink reply is recorded as arriving by Winlink however it was
    /// handed over. Text with no product in it is an error.
    pub fn import(&mut self, raw: &str, transport: Transport, now: i64) -> Result<Vec<Imported>, String> {
        let texts = message::extract_products(raw);
        if texts.is_empty() {
            return Err("No NOAA product found. The text should contain a line starting with \":Product:\".".into());
        }
        let transport = if message::is_winlink_message(raw) && transport != Transport::Internet {
            Transport::Winlink
        } else {
            transport
        };

        Ok(texts
            .into_iter()
            .map(|text| {
                let title = text
                    .lines()
                    .next()
                    .and_then(|l| l.strip_prefix(":Product:"))
                    .unwrap_or("")
                    .trim()
                    .to_string();
                let parsed = match products::parse(&text) {
                    Ok(parsed) => parsed,
                    Err(e) => {
                        return Imported { title, outcome: ImportOutcome::NotUnderstood, detail: Some(e) }
                    }
                };
                let kind = parsed.product.kind();
                let outcome = match self.products.get(kind).map(|held| held.issued) {
                    Some(held) if held == parsed.issued => ImportOutcome::AlreadyHave,
                    Some(held) if held > parsed.issued => ImportOutcome::OlderThanStored,
                    _ => {
                        self.products.insert(
                            kind.to_string(),
                            StoredProduct {
                                product: parsed.product,
                                issued: parsed.issued,
                                received: now,
                                transport,
                                text,
                            },
                        );
                        ImportOutcome::Stored
                    }
                };
                Imported { title, outcome, detail: None }
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WINLINK_WWV: &str = include_str!("../../../tests/fixtures/winlink/wwv.mime");
    const WINLINK_SGAS: &str = include_str!("../../../tests/fixtures/winlink/sgas.mime");
    const WINLINK_THREE_DAY: &str = include_str!("../../../tests/fixtures/winlink/3-day-forecast.mime");
    const WINLINK_OUTLOOK: &str = include_str!("../../../tests/fixtures/winlink/27-day-outlook.mime");
    const NOAA_WWV: &str = include_str!("../../../tests/fixtures/noaa/wwv.txt");
    const NOAA_SGAS: &str = include_str!("../../../tests/fixtures/noaa/sgas.txt");
    const NOAA_THREE_DAY: &str = include_str!("../../../tests/fixtures/noaa/3-day-forecast.txt");
    const NOAA_OUTLOOK: &str = include_str!("../../../tests/fixtures/noaa/27-day-outlook.txt");

    /// The Phase 3 exit test: the same product stored from the Internet and
    /// from a pasted Winlink reply holds identical values.
    #[test]
    fn internet_and_winlink_copies_store_identical_values() {
        for (kind, noaa, winlink) in [
            ("sgas", NOAA_SGAS, WINLINK_SGAS),
            ("threeDay", NOAA_THREE_DAY, WINLINK_THREE_DAY),
            ("outlook27", NOAA_OUTLOOK, WINLINK_OUTLOOK),
        ] {
            let mut online = Store::default();
            online.import(noaa, Transport::Internet, 1000).unwrap();
            let mut offline = Store::default();
            offline.import(winlink, Transport::Pasted, 2000).unwrap();

            let (a, b) = (online.get(kind).unwrap(), offline.get(kind).unwrap());
            assert_eq!(a.product, b.product, "{kind}");
            assert_eq!(a.issued, b.issued, "{kind}");
            assert_eq!(a.text, b.text, "{kind}");
            assert_eq!(a.transport, Transport::Internet);
            assert_eq!(b.transport, Transport::Winlink);
        }
    }

    #[test]
    fn newer_issue_replaces_older_and_not_the_reverse() {
        let mut store = Store::default();
        // The Winlink reply is the 1505 UTC issue; the NOAA file is the 2110 UTC one.
        let first = store.import(WINLINK_WWV, Transport::File, 10).unwrap();
        assert_eq!(first[0].outcome, ImportOutcome::Stored);
        assert_eq!(first[0].title, "Geophysical Alert Message wwv.txt");

        assert_eq!(store.import(NOAA_WWV, Transport::Internet, 20).unwrap()[0].outcome, ImportOutcome::Stored);
        let held = store.get("wwv").unwrap().clone();
        assert_eq!((held.received, held.transport), (20, Transport::Internet));

        assert_eq!(store.import(WINLINK_WWV, Transport::File, 30).unwrap()[0].outcome, ImportOutcome::OlderThanStored);
        assert_eq!(store.import(NOAA_WWV, Transport::Pasted, 40).unwrap()[0].outcome, ImportOutcome::AlreadyHave);
        assert_eq!(store.get("wwv").unwrap(), &held, "an older or repeated issue changes nothing");
    }

    #[test]
    fn imports_several_products_and_reports_the_ones_it_cannot_read() {
        let unknown = ":Product: Something Else\n:Issued: 2026 Oct 04 1505 UTC\nbody\n";
        let pasted = format!("{WINLINK_SGAS}\n{unknown}\n{NOAA_OUTLOOK}");
        let mut store = Store::default();
        let results = store.import(&pasted, Transport::Pasted, 5).unwrap();

        let outcomes: Vec<ImportOutcome> = results.iter().map(|r| r.outcome).collect();
        assert_eq!(outcomes, [ImportOutcome::Stored, ImportOutcome::NotUnderstood, ImportOutcome::Stored]);
        assert!(results[1].detail.as_deref().unwrap().contains("Something Else"));
        assert!(store.get("sgas").is_some() && store.get("outlook27").is_some());
    }

    #[test]
    fn text_without_a_product_is_an_error() {
        assert!(Store::default().import("hello", Transport::Pasted, 0).unwrap_err().contains(":Product:"));
    }

    #[test]
    fn survives_a_save_and_load() {
        let path = std::env::temp_dir()
            .join(format!("hfp-conditions-{}", std::process::id()))
            .join("conditions.json");
        let mut store = Store::default();
        store.import(NOAA_THREE_DAY, Transport::Internet, 7).unwrap();
        store.save(&path).unwrap();
        assert_eq!(Store::load(&path).unwrap(), store);
        assert_eq!(Store::load(&path.with_file_name("missing.json")).unwrap(), Store::default());
    }
}
