pub mod coverage;
pub mod engine;
pub mod geo;
pub mod jsonfile;
pub mod predictor;
pub mod propagation;
pub mod solar;
pub mod spacewx;
pub mod station;
pub mod timeutil;
pub mod userdata;
pub mod voacap;

use std::path::PathBuf;

use serde::Serialize;
use tauri::{path::BaseDirectory, Manager};

use coverage::{Coverage, CoverageRequest};
use geo::LatLon;
use predictor::{PathOverview, PathRequest};
use spacewx::fetch::FetchResult;
use spacewx::store::{Imported, Store, Transport};
use spacewx::Conditions;
use station::{Band, Choice, Mode, StationProfile};
use userdata::UserData;

/// Everything the form offers, so the lists live in one place.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Options {
    presets: Vec<StationProfile>,
    antennas: Vec<Choice<&'static str>>,
    noise_levels: Vec<Choice<f64>>,
    modes: Vec<Choice<Mode>>,
    bands: Vec<Band>,
}

#[tauri::command]
fn options() -> Options {
    Options {
        presets: station::presets(),
        antennas: station::antennas(),
        noise_levels: station::noise_levels(),
        modes: station::modes(),
        bands: station::HF_BANDS.to_vec(),
    }
}

/// The bundled engine, with run folders under the app's local data.
fn engine(app: &tauri::AppHandle) -> Result<voacap::VoacaplEngine, String> {
    let engine_root = app
        .path()
        .resolve("engine", BaseDirectory::Resource)
        .map_err(|e| e.to_string())?;
    let run_root = app
        .path()
        .app_local_data_dir()
        .map_err(|e| e.to_string())?
        .join("run");
    voacap::VoacaplEngine::new(&engine_root, &run_root)
}

#[tauri::command(async)]
fn predict_overview(app: tauri::AppHandle, request: PathRequest) -> Result<PathOverview, String> {
    predictor::predict_overview(&engine(&app)?, &request)
}

#[tauri::command(async)]
fn predict_coverage(app: tauri::AppHandle, request: CoverageRequest) -> Result<Coverage, String> {
    coverage::predict_coverage(&engine(&app)?, &request)
}

/// Turns a locator or latitude, longitude into coordinates, for the map.
#[tauri::command]
fn resolve_position(text: String) -> Result<LatLon, String> {
    geo::parse_position(&text)
}

fn user_data_file(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    Ok(app
        .path()
        .app_config_dir()
        .map_err(|e| e.to_string())?
        .join("userdata.json"))
}

#[tauri::command]
fn load_user_data(app: tauri::AppHandle) -> Result<UserData, String> {
    userdata::load(&user_data_file(&app)?)
}

#[tauri::command]
fn save_user_data(app: tauri::AppHandle, data: UserData) -> Result<(), String> {
    userdata::save(&user_data_file(&app)?, &data)
}

fn local_data_file(app: &tauri::AppHandle, name: &str) -> Result<PathBuf, String> {
    Ok(app.path().app_local_data_dir().map_err(|e| e.to_string())?.join(name))
}

const CONDITIONS_FILE: &str = "conditions.json";
const SSN_TABLE_FILE: &str = "smoothed-ssn.json";

#[tauri::command]
fn conditions(app: tauri::AppHandle) -> Result<Conditions, String> {
    let store = Store::load(&local_data_file(&app, CONDITIONS_FILE)?)?;
    Ok(spacewx::conditions(&store, timeutil::now()))
}

/// What a refresh or import did, with the conditions as they now stand.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ConditionsUpdate<T> {
    results: Vec<T>,
    conditions: Conditions,
}

/// Fetches the products and the sunspot table from NOAA. Each item succeeds
/// or fails on its own; the results say which.
#[tauri::command(async)]
fn refresh_conditions(app: tauri::AppHandle) -> Result<ConditionsUpdate<FetchResult>, String> {
    let now = timeutil::now();
    let store_file = local_data_file(&app, CONDITIONS_FILE)?;
    let mut store = Store::load(&store_file)?;
    let mut results = spacewx::fetch::refresh_products(&mut store, now);
    store.save(&store_file)?;

    let table_file = local_data_file(&app, SSN_TABLE_FILE)?;
    let table = spacewx::fetch::fetch_ssn_table(now).and_then(|table| {
        jsonfile::save(&table_file, &table)?;
        Ok(if solar::install(table) { "updated" } else { "already current" })
    });
    results.push(FetchResult {
        title: "Smoothed sunspot table".to_string(),
        ok: table.is_ok(),
        detail: table.map_or_else(|e| e, String::from),
    });

    Ok(ConditionsUpdate { results, conditions: spacewx::conditions(&store, now) })
}

/// Imports products from pasted text or a file's contents.
#[tauri::command]
fn import_conditions(
    app: tauri::AppHandle,
    text: String,
    from_file: bool,
) -> Result<ConditionsUpdate<Imported>, String> {
    let now = timeutil::now();
    let store_file = local_data_file(&app, CONDITIONS_FILE)?;
    let mut store = Store::load(&store_file)?;
    let transport = if from_file { Transport::File } else { Transport::Pasted };
    let results = store.import(&text, transport, now)?;
    store.save(&store_file)?;
    Ok(ConditionsUpdate { results, conditions: spacewx::conditions(&store, now) })
}

#[tauri::command]
fn winlink_request() -> String {
    spacewx::winlink_request()
}

/// Uses a sunspot table downloaded in an earlier session, if it is newer
/// than the bundled one. A damaged cache file is ignored: the bundled table
/// still works.
fn install_cached_ssn_table(app: &tauri::AppHandle) {
    if let Ok(file) = local_data_file(app, SSN_TABLE_FILE) {
        if let Ok(Some(table)) = jsonfile::load::<Option<solar::Table>>(&file) {
            solar::install(table);
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(|app| {
            install_cached_ssn_table(app.handle());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            options,
            predict_overview,
            predict_coverage,
            resolve_position,
            conditions,
            refresh_conditions,
            import_conditions,
            winlink_request,
            load_user_data,
            save_user_data
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

/// Helpers for tests that run the real engine built by `engines/voacapl/build.sh`.
#[cfg(test)]
pub(crate) mod testing {
    use std::path::PathBuf;

    pub fn engine_root() -> PathBuf {
        std::env::var_os("HFP_ENGINE_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.work/engine"))
    }

    pub fn scratch_dir(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("hfp-{name}-{}", std::process::id()))
    }
}
