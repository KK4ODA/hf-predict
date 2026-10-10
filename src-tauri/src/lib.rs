pub mod calibration;
pub mod compare;
pub mod coverage;
pub mod engine;
pub mod geo;
pub mod jsonfile;
pub mod observations;
pub mod predictor;
pub mod propagation;
pub mod solar;
pub mod spacewx;
pub mod station;
pub mod timeutil;
pub mod userdata;
pub mod voacap;
pub mod wsjtx;

use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};

use serde::{Deserialize, Serialize};
use tauri::{path::BaseDirectory, Emitter, Manager};

use compare::{BandComparison, CompareQuery};
use coverage::{Coverage, CoverageRequest};
use geo::LatLon;
use predictor::{PathOverview, PathRequest};
use spacewx::fetch::FetchResult;
use spacewx::store::{Imported, Store, Transport};
use spacewx::Conditions;
use station::{Band, Choice, Mode, StationProfile};
use observations::{BandActivity, Database, HeardStation, Observation};
use userdata::UserData;
use wsjtx::logs::{self, LogCheck, LogFile};
use wsjtx::listener::{Listener, ListenerConfig, ListenerStatus};

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

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CalibrationQuery {
    rx_position: String,
    noise_db: f64,
}

#[derive(Clone, Serialize)]
struct CalibrationProgress {
    done: usize,
    total: usize,
}

/// Predicts every stored decode's path and tallies hearing against
/// prediction. Emits `calibration-progress` as engine runs complete.
#[tauri::command(async)]
fn calibration_report(app: tauri::AppHandle, query: CalibrationQuery) -> Result<calibration::Report, String> {
    let db = app.state::<AppState>().db()?.clone();
    let engine = engine(&app)?;
    let progress = |done: usize, total: usize| {
        let _ = app.emit("calibration-progress", CalibrationProgress { done, total });
    };
    calibration::calibrate(&engine, &db, &query.rx_position, query.noise_db, &progress)
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

const OBSERVATIONS_FILE: &str = "observations.db";
const LISTENER_FILE: &str = "listener.json";

/// What stays alive for the whole session.
struct AppState {
    /// The observation database, or why it could not be opened.
    db: Result<Arc<Database>, String>,
    listener_config: Mutex<ListenerConfig>,
    listener: Mutex<Option<Listener>>,
    /// What the last check of the configured logs found.
    log_checks: Mutex<Vec<LogCheck>>,
}

impl AppState {
    fn db(&self) -> Result<&Arc<Database>, String> {
        self.db.as_ref().map_err(Clone::clone)
    }

    fn status(&self) -> ListenerStatus {
        let config = self.listener_config.lock().unwrap_or_else(PoisonError::into_inner).clone();
        match self.listener.lock().unwrap_or_else(PoisonError::into_inner).as_ref() {
            Some(listener) => listener.status(),
            None => ListenerStatus::off(config),
        }
    }

    /// Stops any running listener and starts one for `config` if it is enabled.
    fn apply(&self, config: ListenerConfig) -> Result<(), String> {
        let mut listener = self.listener.lock().unwrap_or_else(PoisonError::into_inner);
        // Dropping the old one closes its socket before the new one binds.
        *listener = None;
        if config.enabled {
            *listener = Some(Listener::start(config.clone(), self.db()?.clone()));
        }
        *self.listener_config.lock().unwrap_or_else(PoisonError::into_inner) = config;
        Ok(())
    }
}

#[tauri::command]
fn listener_status(state: tauri::State<AppState>) -> ListenerStatus {
    state.status()
}

#[tauri::command]
fn set_listener_config(
    app: tauri::AppHandle,
    state: tauri::State<AppState>,
    config: ListenerConfig,
) -> Result<ListenerStatus, String> {
    jsonfile::save(&local_data_file(&app, LISTENER_FILE)?, &config)?;
    state.apply(config)?;
    Ok(state.status())
}

#[tauri::command]
fn recent_observations(state: tauri::State<AppState>, limit: u32) -> Result<Vec<Observation>, String> {
    state.db()?.recent(limit)
}

#[tauri::command]
fn band_activity(state: tauri::State<AppState>, minutes: i64) -> Result<Vec<BandActivity>, String> {
    let now = timeutil::now();
    state.db()?.band_activity(now - minutes * 60, now)
}

/// Stations heard in the last `minutes` whose position is known.
#[tauri::command]
fn heard_stations(
    state: tauri::State<AppState>,
    minutes: i64,
    band: Option<String>,
) -> Result<Vec<HeardStation>, String> {
    state.db()?.heard_stations(timeutil::now() - minutes * 60, band.as_deref())
}

/// Sets a path's per-band predictions beside what has been heard that way.
#[tauri::command]
fn compare_path(state: tauri::State<AppState>, query: CompareQuery) -> Result<Vec<BandComparison>, String> {
    compare::compare(state.db()?, &query, timeutil::now())
}

/// Logs found in the program folders on this computer.
#[tauri::command]
fn find_log_files() -> Vec<logs::FoundLog> {
    logs::find()
}

/// What the last check of each configured log found.
#[tauri::command]
fn log_checks(state: tauri::State<AppState>) -> Vec<LogCheck> {
    state.log_checks.lock().map(|checks| checks.clone()).unwrap_or_default()
}

/// Reads each log from where its last check ended, and remembers the outcome.
#[tauri::command(async)]
fn check_log_files(app: tauri::AppHandle, files: Vec<LogFile>) -> Result<Vec<LogCheck>, String> {
    let state = app.state::<AppState>();
    let db = state.db()?.clone();
    let now = timeutil::now();
    let checks: Vec<LogCheck> = files.iter().map(|log| logs::check(&db, log, now)).collect();
    if let Ok(mut remembered) = state.log_checks.lock() {
        *remembered = checks.clone();
    }
    Ok(checks)
}

/// Reads the configured logs in the background once the app is up.
fn check_logs_on_start(app: tauri::AppHandle) {
    std::thread::spawn(move || {
        let Ok(data) = user_data_file(&app).and_then(|file| userdata::load(&file)) else {
            return;
        };
        if data.log_files.is_empty() {
            return;
        }
        let state = app.state::<AppState>();
        let Ok(db) = state.db().cloned() else {
            return;
        };
        let now = timeutil::now();
        let checks: Vec<LogCheck> = data.log_files.iter().map(|log| logs::check(&db, log, now)).collect();
        if let Ok(mut remembered) = state.log_checks.lock() {
            *remembered = checks;
        };
    });
}

/// Opens the observation database and starts the listener if it was left on.
fn start_observing(app: &tauri::AppHandle) -> AppState {
    let db = local_data_file(app, OBSERVATIONS_FILE)
        .and_then(|file| Database::open(&file))
        .map(Arc::new);
    let config: ListenerConfig = local_data_file(app, LISTENER_FILE)
        .and_then(|file| jsonfile::load(&file))
        .unwrap_or_default();
    let state = AppState {
        db,
        listener_config: Mutex::new(config.clone()),
        listener: Mutex::new(None),
        log_checks: Mutex::new(Vec::new()),
    };
    // A database that will not open is reported when the screen asks for data.
    let _ = state.apply(config);
    state
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
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(|app| {
            install_cached_ssn_table(app.handle());
            app.manage(start_observing(app.handle()));
            check_logs_on_start(app.handle().clone());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            options,
            predict_overview,
            predict_coverage,
            resolve_position,
            conditions,
            refresh_conditions,
            calibration_report,
            import_conditions,
            winlink_request,
            listener_status,
            set_listener_config,
            recent_observations,
            band_activity,
            heard_stations,
            compare_path,
            find_log_files,
            log_checks,
            check_log_files,
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
