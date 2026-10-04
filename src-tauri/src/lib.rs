pub mod engine;
pub mod geo;
pub mod predictor;
pub mod propagation;
pub mod solar;
pub mod station;
pub mod userdata;
pub mod voacap;

use std::path::PathBuf;

use serde::Serialize;
use tauri::{path::BaseDirectory, Manager};

use predictor::{PathOverview, PathRequest};
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

#[tauri::command(async)]
fn predict_overview(app: tauri::AppHandle, request: PathRequest) -> Result<PathOverview, String> {
    let engine_root = app
        .path()
        .resolve("engine", BaseDirectory::Resource)
        .map_err(|e| e.to_string())?;
    let run_root = app
        .path()
        .app_local_data_dir()
        .map_err(|e| e.to_string())?
        .join("run");
    let engine = voacap::VoacaplEngine::new(&engine_root, &run_root)?;
    predictor::predict_overview(&engine, &request)
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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .invoke_handler(tauri::generate_handler![
            options,
            predict_overview,
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
