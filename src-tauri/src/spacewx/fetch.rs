//! Fetches the products and the sunspot table from NOAA when there is a network.

use std::time::Duration;

use serde::Serialize;

use super::store::{ImportOutcome, Store, Transport};
use super::PRODUCTS;
use crate::{solar, timeutil};

const TIMEOUT: Duration = Duration::from_secs(15);
const USER_AGENT: &str =
    concat!("hf-predict/", env!("CARGO_PKG_VERSION"), " (+https://github.com/KK4ODA/hf-predict)");
const OBSERVED_SSN_URL: &str =
    "https://services.swpc.noaa.gov/json/solar-cycle/observed-solar-cycle-indices.json";
const PREDICTED_SSN_URL: &str =
    "https://services.swpc.noaa.gov/json/solar-cycle/predicted-solar-cycle.json";

/// What happened to one thing the refresh tried to fetch.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FetchResult {
    pub title: String,
    pub ok: bool,
    pub detail: String,
}

fn get(url: &str) -> Result<String, String> {
    ureq::get(url)
        .set("User-Agent", USER_AGENT)
        .timeout(TIMEOUT)
        .call()
        .map_err(|e| format!("{url}: {e}"))?
        .into_string()
        .map_err(|e| format!("{url}: {e}"))
}

/// Fetches every product into `store`. Each is independent: one failure does
/// not stop the others, and every outcome is reported.
pub fn refresh_products(store: &mut Store, now: i64) -> Vec<FetchResult> {
    PRODUCTS
        .iter()
        .map(|info| {
            let outcome = get(info.url)
                .and_then(|text| store.import(&text, Transport::Internet, now))
                .and_then(|imported| {
                    imported.into_iter().next().ok_or_else(|| "no product in the reply".to_string())
                });
            let (ok, detail) = match outcome {
                Ok(imported) => match imported.outcome {
                    ImportOutcome::Stored => (true, "updated".to_string()),
                    ImportOutcome::AlreadyHave => (true, "already current".to_string()),
                    ImportOutcome::OlderThanStored => (true, "stored copy is newer".to_string()),
                    ImportOutcome::NotUnderstood => {
                        (false, imported.detail.unwrap_or_else(|| "not understood".to_string()))
                    }
                },
                Err(e) => (false, e),
            };
            FetchResult { title: info.title.to_string(), ok, detail }
        })
        .collect()
}

/// Fetches NOAA's solar-cycle tables and builds a sunspot table from them.
pub fn fetch_ssn_table(now: i64) -> Result<solar::Table, String> {
    solar::table_from_swpc(
        &get(OBSERVED_SSN_URL)?,
        &get(PREDICTED_SSN_URL)?,
        &timeutil::date_string(now),
    )
}
