pub mod calibration;
pub mod compare;
pub mod contacts;
pub mod coverage;
pub mod engine;
pub mod geo;
pub mod jsonfile;
pub mod observations;
pub mod predictor;
pub mod propagation;
pub mod radio;
pub mod scan;
pub mod scanner;
pub mod solar;
pub mod spacewx;
pub mod station;
pub mod timeutil;
pub mod userdata;
pub mod voacap;
pub mod wsjtx;

use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use tauri::{path::BaseDirectory, Emitter, Manager};

use compare::{BandComparison, CompareQuery};
use coverage::{Coverage, CoverageRequest};
use geo::LatLon;
use predictor::{PathOverview, PathRequest};
use propagation::PropagationEngine;
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

/// Coverage for the Map: the coarse grid in about a second, or the fine one,
/// kept once computed.
#[tauri::command(async)]
fn predict_coverage(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    request: CoverageRequest,
    fine: bool,
) -> Result<Coverage, String> {
    let engine = engine(&app)?;
    if !fine {
        return coverage::predict_coverage(&engine, &request, coverage::COARSE_GRID, &|_, _| {});
    }
    let progress = |done: usize, total: usize| {
        let _ = app.emit("coverage-progress", CalibrationProgress { done, total });
    };
    coverage::predict_coverage_cached(&engine, state.db()?, &request, coverage::FINE_GRID, &progress)
}

/// The fine coverage map, if it was computed before.
#[tauri::command(async)]
fn cached_coverage(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    request: CoverageRequest,
) -> Result<Option<Coverage>, String> {
    coverage::cached_coverage(engine(&app)?.name(), state.db()?, &request, coverage::FINE_GRID)
}

/// Turns a locator or latitude, longitude into coordinates, for the map.
#[tauri::command]
fn resolve_position(text: String) -> Result<LatLon, String> {
    geo::parse_position(&text)
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PlanQuery {
    tx_position: String,
    /// Aim the plan at a destination; otherwise at the world.
    destination: Option<String>,
    year: i32,
    month: u32,
    ssn: Option<f64>,
    tx_station: StationProfile,
    rx_station: StationProfile,
    /// UTC clock hour, 0 to 23.
    clock_hour: u32,
    minutes: u32,
    excluded_bands: Vec<String>,
}

impl PlanQuery {
    /// The same plan for the UTC hour and month of a moment. A scan listens
    /// now, so its plans are for now whatever hour the views show.
    fn at(&self, unix_seconds: i64) -> PlanQuery {
        let (year, month, _, clock_hour) = timeutil::civil(unix_seconds);
        PlanQuery { year, month, clock_hour, ..self.clone() }
    }
}

/// A listening plan from the FT8 prediction for the hour, what was heard in
/// the last hour, and how long each band has gone unsampled.
#[tauri::command(async)]
fn listen_plan(app: tauri::AppHandle, query: PlanQuery) -> Result<scan::Plan, String> {
    make_plan(&app, &query)
}

fn make_plan(app: &tauri::AppHandle, query: &PlanQuery) -> Result<scan::Plan, String> {
    let engine = engine(app)?;
    let db = app.state::<AppState>().db()?.clone();
    let now = timeutil::now();
    let voacap_hour = if query.clock_hour == 0 { 24 } else { query.clock_hour };
    let destination = query.destination.as_deref().map(str::trim).filter(|d| !d.is_empty());
    let prediction: Vec<f64> = match destination {
        Some(destination) => {
            let overview = predictor::predict_overview(
                &engine,
                &PathRequest {
                    tx_position: query.tx_position.clone(),
                    rx_position: destination.to_string(),
                    year: query.year,
                    month: query.month,
                    ssn: query.ssn,
                    tx_station: query.tx_station.clone(),
                    rx_station: query.rx_station.clone(),
                    mode: station::Mode::Ft8,
                    required_reliability_pct: 90.0,
                    long_path: false,
                },
            )?;
            let hours = &overview.short.prediction.run.prediction.hours;
            let index = hours
                .iter()
                .position(|h| h.utc_hour == voacap_hour)
                .ok_or("the prediction has no such hour")?;
            overview.short.ft8_reliability[index].clone()
        }
        None => {
            let coverage = coverage::predict_coverage(
                &engine,
                &CoverageRequest {
                    tx_position: query.tx_position.clone(),
                    year: query.year,
                    month: query.month,
                    ssn: query.ssn,
                    tx_station: query.tx_station.clone(),
                    rx_station: query.rx_station.clone(),
                    mode: station::Mode::Ft8,
                    required_reliability_pct: 90.0,
                    utc_hour: voacap_hour,
                },
                coverage::COARSE_GRID,
                &|_, _| {},
            )?;
            let cells = coverage.cells.len().max(1) as f64;
            (0..coverage.bands.len())
                .map(|b| {
                    let reached = coverage.cells.iter().filter(|c| c.reliability[b] >= scan::REACH_RELIABILITY).count();
                    reached as f64 / cells
                })
                .collect()
        }
    };

    let activity: BTreeMap<String, BandActivity> =
        db.band_activity(now - 3600, now)?.into_iter().map(|a| (a.band.clone(), a)).collect();
    let last = db.last_listened_by_band()?;
    let inputs: Vec<scan::BandInput> = station::HF_BANDS
        .iter()
        .zip(prediction)
        .map(|(band, prediction)| scan::BandInput {
            band: band.name.to_string(),
            dial_hz: band.ft8_hz,
            prediction,
            observed: activity
                .get(band.name)
                .map_or(compare::ObservedTier::NotSampled, |a| compare::observed_tier(a.periods, a.unique_callsigns)),
            minutes_since_listened: last.get(band.name).map(|t| (now - t) as f64 / 60.0),
            excluded: query.excluded_bands.iter().any(|b| b == band.name),
        })
        .collect();
    Ok(scan::plan(&inputs, query.minutes))
}

fn wsjtx_state(app: &tauri::AppHandle) -> scanner::WsjtxState {
    let status = app.state::<AppState>().status();
    match status.tracker.as_ref().and_then(|t| t.decoders.first()) {
        Some(d) => scanner::WsjtxState {
            reporting: d.seconds_since_heard <= scanner::REPORTING_WITHIN_S,
            tx_enabled: d.tx_enabled,
            transmitting: d.transmitting,
        },
        None => scanner::WsjtxState::default(),
    }
}

/// Why a scan could not start right now; empty when it could.
#[tauri::command]
fn scan_preflight(app: tauri::AppHandle, confirmed: bool) -> Vec<String> {
    let radio = app.state::<AppState>().radio_status().radio;
    scanner::preflight(radio.as_ref(), wsjtx_state(&app), confirmed)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ScanRequest {
    plan: PlanQuery,
    /// The operator confirmed the antenna system is safe to retune on receive.
    confirmed: bool,
    /// Make a new plan when one runs its course.
    keep_going: bool,
}

/// Starts moving the radio through a fresh plan. Refuses unless every rule holds.
#[tauri::command(async)]
fn scan_start(app: tauri::AppHandle, request: ScanRequest) -> Result<scanner::ScanStatus, String> {
    let plan = make_plan(&app, &request.plan.at(timeutil::now()))?;
    let state = app.state::<AppState>();
    let monitor = state
        .radio
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone()
        .ok_or("the radio is not connected")?;
    let wsjtx = {
        let app = app.clone();
        Box::new(move || wsjtx_state(&app))
    };
    let replan = request.keep_going.then(|| {
        let app = app.clone();
        let query = request.plan;
        // Each new plan is for the hour it begins in.
        Box::new(move || make_plan(&app, &query.at(timeutil::now()))) as Box<dyn Fn() -> Result<scan::Plan, String> + Send>
    });
    let runner = scanner::Runner::start(plan, monitor, wsjtx, request.confirmed, replan)?;
    let status = runner.status();
    *state.scan.lock().unwrap_or_else(PoisonError::into_inner) = Some(runner);
    Ok(status)
}

/// Stops the scan; the radio is put back on the scanner's thread.
#[tauri::command]
fn scan_stop(state: tauri::State<AppState>) -> scanner::ScanStatus {
    let scan = state.scan.lock().unwrap_or_else(PoisonError::into_inner);
    match scan.as_ref() {
        Some(runner) => {
            runner.stop();
            runner.status()
        }
        None => scanner::ScanStatus::idle(),
    }
}

#[tauri::command]
fn scan_status(state: tauri::State<AppState>) -> scanner::ScanStatus {
    state.scan.lock().unwrap_or_else(PoisonError::into_inner).as_ref().map_or_else(scanner::ScanStatus::idle, |r| r.status())
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
    radio_config: Mutex<radio::RadioConfig>,
    radio: Mutex<Option<Arc<radio::Monitor>>>,
    scan: Mutex<Option<scanner::Runner>>,
    /// Where a started rigctld's process id is kept between sessions.
    rigctld_record: Option<PathBuf>,
    /// What came of starting WSJT-X from this app, for the screen.
    wsjtx_launch: Mutex<Option<String>>,
    /// The operator's callsign as WSJT-X last gave it, kept between sessions.
    own_call: Mutex<OwnCall>,
    own_call_file: Option<PathBuf>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct OwnCall {
    callsign: Option<String>,
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

    /// The operator's callsign: from WSJT-X when it is reporting, otherwise
    /// the one it last gave.
    fn own_call(&self) -> Option<String> {
        let live = self
            .status()
            .tracker
            .and_then(|t| t.decoders.iter().find_map(|d| d.de_call.clone()))
            .map(|c| c.trim().to_uppercase())
            .filter(|c| !c.is_empty());
        let mut saved = self.own_call.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(call) = live {
            if saved.callsign.as_deref() != Some(call.as_str()) {
                saved.callsign = Some(call);
                if let Some(file) = &self.own_call_file {
                    let _ = jsonfile::save(file, &*saved);
                }
            }
        }
        saved.callsign.clone()
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
    compare::compare(state.db()?, &query, state.own_call().as_deref(), timeutil::now())
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct HearingYourArea {
    /// The operator's callsign, when WSJT-X has ever given it.
    own_call: Option<String>,
    stations: Vec<observations::HearingStation>,
}

/// Which band reaches the most stations in the log, by hour.
#[tauri::command(async)]
fn most_contacts(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    query: contacts::ContactsQuery,
) -> Result<contacts::ContactsReport, String> {
    let engine = engine(&app)?;
    let own_call = state.own_call();
    let progress = |done: usize, total: usize| {
        let _ = app.emit("contacts-progress", CalibrationProgress { done, total });
    };
    contacts::most_contacts(&engine, state.db()?, &query, own_call.as_deref(), timeutil::now(), &progress)
}

/// Distant stations heard reporting this station or stations near it.
#[tauri::command]
fn hearing_your_area(
    state: tauri::State<AppState>,
    minutes: i64,
    band: Option<String>,
    receiver: Option<String>,
) -> Result<HearingYourArea, String> {
    let own_call = state.own_call();
    let receiver = receiver.as_deref().map(str::trim).filter(|r| !r.is_empty()).and_then(|r| geo::parse_position(r).ok());
    let stations =
        state.db()?.hearing_your_area(timeutil::now() - minutes * 60, band.as_deref(), own_call.as_deref(), receiver)?;
    Ok(HearingYourArea { own_call, stations })
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
    let radio_config: radio::RadioConfig = local_data_file(app, RADIO_FILE)
        .and_then(|file| jsonfile::load(&file))
        .unwrap_or_default();
    let state = AppState {
        db,
        listener_config: Mutex::new(config.clone()),
        listener: Mutex::new(None),
        log_checks: Mutex::new(Vec::new()),
        radio_config: Mutex::new(radio_config.clone()),
        radio: Mutex::new(None),
        scan: Mutex::new(None),
        rigctld_record: local_data_file(app, RIGCTLD_RECORD_FILE).ok(),
        wsjtx_launch: Mutex::new(None),
        own_call: Mutex::new(
            local_data_file(app, OWN_CALL_FILE).and_then(|file| jsonfile::load(&file)).unwrap_or_default(),
        ),
        own_call_file: local_data_file(app, OWN_CALL_FILE).ok(),
    };
    // A database that will not open is reported when the screen asks for data.
    let _ = state.apply(config);
    state.apply_radio(radio_config);
    state
}

const RADIO_FILE: &str = "radio.json";
const RIGCTLD_RECORD_FILE: &str = "rigctld.json";
const OWN_CALL_FILE: &str = "own-call.json";

/// Set once the operator has chosen how to quit, so the exit that follows
/// is not asked about again.
static QUITTING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Stops any scan, which puts the radio back, and the radio monitor, which
/// stops the rigctld it started unless `keep_rigctld`.
fn shut_down(app: &tauri::AppHandle, keep_rigctld: bool) {
    let state = app.state::<AppState>();
    let scan = state.scan.lock().unwrap_or_else(PoisonError::into_inner).take();
    drop(scan);
    let monitor = state.radio.lock().unwrap_or_else(PoisonError::into_inner).take();
    if let Some(monitor) = monitor {
        if keep_rigctld {
            monitor.keep_daemon_running();
        }
        drop(monitor);
    }
}

/// Before an update installs and restarts the app: stop any scan, which
/// puts the radio back, and leave rigctld running for the new version to
/// take back.
#[tauri::command]
fn prepare_for_restart(app: tauri::AppHandle) {
    QUITTING.store(true, std::sync::atomic::Ordering::Relaxed);
    shut_down(&app, true);
}

/// The update did not install after all: carry on as before.
#[tauri::command]
fn cancel_restart(app: tauri::AppHandle) {
    QUITTING.store(false, std::sync::atomic::Ordering::Relaxed);
    let state = app.state::<AppState>();
    let config = state.radio_config.lock().unwrap_or_else(PoisonError::into_inner).clone();
    state.apply_radio(config);
}

/// Handles a request to close the window or quit. Returns true when the
/// operator is being asked first and the request must be held back.
fn ask_before_quitting(app: &tauri::AppHandle) -> bool {
    use std::sync::atomic::Ordering;
    use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind, MessageDialogResult};

    if QUITTING.load(Ordering::Relaxed) {
        return false;
    }
    let state = app.state::<AppState>();
    let daemon = state.radio_status().daemon.filter(|d| d.running);
    let Some(daemon) = daemon else {
        QUITTING.store(true, Ordering::Relaxed);
        shut_down(app, false);
        return false;
    };

    let config = state.radio_config.lock().unwrap_or_else(PoisonError::into_inner).clone();
    let wsjtx_running = state
        .status()
        .tracker
        .as_ref()
        .and_then(|t| t.decoders.first())
        .is_some_and(|d| d.seconds_since_heard <= scanner::REPORTING_WITHIN_S);
    let scanning = state
        .scan
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .as_ref()
        .is_some_and(|r| matches!(r.status().state, "running" | "paused"));
    let port = if config.serial_port.trim().is_empty() { "the radio's port".to_string() } else { config.serial_port.trim().to_string() };
    let mut text = format!(
        "HF Predict started rigctld (process {}) to share the radio. While it runs, no other program can open {port} directly.",
        daemon.pid
    );
    if wsjtx_running {
        text.push_str("\n\nWSJT-X is still running and reaches the radio through rigctld. Stopping rigctld cuts WSJT-X off from the radio.");
    }
    if scanning {
        text.push_str("\n\nThe scan will stop and the radio go back to where it was.");
    }
    text.push_str("\n\nStop rigctld now?");

    const STOP: &str = "Stop rigctld";
    const LEAVE: &str = "Leave it running";
    let handle = app.clone();
    app.dialog()
        .message(text)
        .title("rigctld is still running")
        .kind(MessageDialogKind::Warning)
        .buttons(MessageDialogButtons::YesNoCancelCustom(STOP.into(), LEAVE.into(), "Cancel".into()))
        .show_with_result(move |result| {
            let keep = match result {
                MessageDialogResult::Yes => false,
                MessageDialogResult::No => true,
                MessageDialogResult::Custom(label) if label == STOP => false,
                MessageDialogResult::Custom(label) if label == LEAVE => true,
                _ => return,
            };
            QUITTING.store(true, Ordering::Relaxed);
            shut_down(&handle, keep);
            handle.exit(0);
        });
    true
}

impl AppState {
    fn radio_status(&self) -> radio::RadioStatus {
        let config = self.radio_config.lock().unwrap_or_else(PoisonError::into_inner).clone();
        match self.radio.lock().unwrap_or_else(PoisonError::into_inner).as_ref() {
            Some(monitor) => monitor.status(),
            None => radio::RadioStatus::off(config),
        }
    }

    /// Stops any radio monitor and starts one for `config` if it is enabled.
    fn apply_radio(&self, config: radio::RadioConfig) {
        // A running scan cannot outlive the connection it retunes through.
        *self.scan.lock().unwrap_or_else(PoisonError::into_inner) = None;
        let mut monitor = self.radio.lock().unwrap_or_else(PoisonError::into_inner);
        *monitor = None;
        if config.enabled {
            *monitor = Some(Arc::new(radio::Monitor::start_recorded(config.clone(), self.rigctld_record.clone())));
        }
        *self.radio_config.lock().unwrap_or_else(PoisonError::into_inner) = config;
    }
}

/// Seconds to wait for rigctld to answer before giving up on starting WSJT-X.
const WSJTX_START_WAIT_S: u64 = 90;

/// Starts WSJT-X once the radio monitor reads the radio, unless WSJT-X is
/// already reporting over UDP. Runs on its own thread.
fn start_wsjtx_when_ready(app: tauri::AppHandle) {
    std::thread::spawn(move || {
        let state = app.state::<AppState>();
        let config = state.radio_config.lock().unwrap_or_else(PoisonError::into_inner).clone();
        if !config.enabled || !config.start_wsjtx {
            return;
        }
        let note = |text: String| {
            *state.wsjtx_launch.lock().unwrap_or_else(PoisonError::into_inner) = Some(text);
        };
        // WSJT-X refuses a second copy of itself, with an error the operator
        // has to dismiss, so never start one while it runs.
        let already = || {
            radio::daemon::is_running(&config.wsjtx_path).then(|| {
                "WSJT-X is already running, so it was not started again. If it shows a rig \
                 error, press Retry in WSJT-X."
                    .to_string()
            })
        };
        if let Some(text) = already() {
            note(text);
            return;
        }
        note("Waiting for rigctld before starting WSJT-X…".into());
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(WSJTX_START_WAIT_S);
        loop {
            let radio = state.radio_status();
            if radio.state == "connected" && radio.radio.is_some() {
                break;
            }
            if std::time::Instant::now() > deadline {
                note(format!("WSJT-X was not started: rigctld did not answer within {WSJTX_START_WAIT_S} s ({}).", radio.detail));
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(500));
        }
        let reporting = state
            .status()
            .tracker
            .as_ref()
            .and_then(|t| t.decoders.first())
            .is_some_and(|d| d.seconds_since_heard <= scanner::REPORTING_WITHIN_S);
        if reporting {
            note("WSJT-X was already running and reporting, so it was not started again.".into());
            return;
        }
        if let Some(text) = already() {
            note(text);
            return;
        }
        match radio::daemon::launch(&config.wsjtx_path) {
            Ok(pid) => note(format!(
                "Started {} (process {pid}) once rigctld was up, at {} UTC.",
                config.wsjtx_path,
                timeutil::date_string(timeutil::now())
            )),
            Err(e) => note(format!("Could not start WSJT-X: {e}")),
        }
    });
}

#[tauri::command(async)]
fn find_wsjtx() -> Vec<radio::daemon::FoundProgram> {
    radio::daemon::find_wsjtx()
}

#[tauri::command]
fn wsjtx_launch(state: tauri::State<AppState>) -> Option<String> {
    state.wsjtx_launch.lock().unwrap_or_else(PoisonError::into_inner).clone()
}

#[tauri::command]
fn radio_status(state: tauri::State<AppState>) -> radio::RadioStatus {
    state.radio_status()
}

/// The rigctld programs installed on this computer.
#[tauri::command(async)]
fn find_rigctld() -> Vec<radio::daemon::FoundProgram> {
    radio::daemon::find()
}

/// The radios a rigctld program knows.
#[tauri::command(async)]
fn rig_models(program: String) -> Result<Vec<radio::daemon::RigModel>, String> {
    radio::daemon::rig_models(&program)
}

#[tauri::command(async)]
fn serial_ports() -> Vec<String> {
    radio::daemon::serial_ports()
}

#[tauri::command]
fn set_radio_config(
    app: tauri::AppHandle,
    state: tauri::State<AppState>,
    config: radio::RadioConfig,
) -> Result<radio::RadioStatus, String> {
    jsonfile::save(&local_data_file(&app, RADIO_FILE)?, &config)?;
    state.apply_radio(config);
    start_wsjtx_when_ready(app.clone());
    Ok(state.radio_status())
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
            start_wsjtx_when_ready(app.handle().clone());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            options,
            predict_overview,
            predict_coverage,
            cached_coverage,
            resolve_position,
            conditions,
            refresh_conditions,
            calibration_report,
            listen_plan,
            scan_preflight,
            scan_start,
            scan_stop,
            scan_status,
            radio_status,
            set_radio_config,
            prepare_for_restart,
            cancel_restart,
            find_rigctld,
            rig_models,
            serial_ports,
            find_wsjtx,
            wsjtx_launch,
            import_conditions,
            winlink_request,
            listener_status,
            set_listener_config,
            recent_observations,
            band_activity,
            heard_stations,
            hearing_your_area,
            most_contacts,
            compare_path,
            find_log_files,
            log_checks,
            check_log_files,
            load_user_data,
            save_user_data
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| match event {
            tauri::RunEvent::WindowEvent { event: tauri::WindowEvent::CloseRequested { api, .. }, .. } => {
                if ask_before_quitting(app) {
                    api.prevent_close();
                }
            }
            tauri::RunEvent::ExitRequested { api, .. } => {
                if ask_before_quitting(app) {
                    api.prevent_exit();
                }
            }
            _ => {}
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn query() -> PlanQuery {
        let station = station::presets().remove(0);
        PlanQuery {
            tx_position: "FM18".into(),
            destination: None,
            year: 2026,
            month: 10,
            ssn: Some(120.0),
            tx_station: station.clone(),
            rx_station: station,
            clock_hour: 5,
            minutes: 30,
            excluded_bands: vec!["60 m".into()],
        }
    }

    #[test]
    fn a_scan_plans_for_the_hour_it_is_in() {
        // Started at 05 UTC shown, the scan's plans follow the clock instead.
        let first = query().at(timeutil::from_utc(2026, 10, 31, 22, 59));
        assert_eq!((first.year, first.month, first.clock_hour), (2026, 10, 22));
        let renewed = query().at(timeutil::from_utc(2026, 10, 31, 23, 30));
        assert_eq!(renewed.clock_hour, 23);
        // Past midnight at the month's end, the month moves on too.
        let next_day = query().at(timeutil::from_utc(2026, 11, 1, 0, 10));
        assert_eq!((next_day.year, next_day.month, next_day.clock_hour), (2026, 11, 0));
        let new_year = query().at(timeutil::from_utc(2027, 1, 1, 0, 0));
        assert_eq!((new_year.year, new_year.month, new_year.clock_hour), (2027, 1, 0));
        // Everything else is kept as the operator set it.
        assert_eq!(renewed.ssn, Some(120.0));
        assert_eq!(renewed.minutes, 30);
        assert_eq!(renewed.excluded_bands, vec!["60 m".to_string()]);
        assert_eq!(renewed.tx_position, "FM18");
    }
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
