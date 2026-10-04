pub mod engine;

use tauri::{path::BaseDirectory, Manager};

/// Deck used to prove the bundled engine runs on this machine.
const SELF_TEST_DECK: &str = include_str!("../../tests/engine/cases/ham01-short.dat");

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct EngineSelfTest {
    /// Engine name and version as printed in its output.
    banner: String,
    /// The circuit description and the first hour's prediction table.
    first_hour: String,
}

#[tauri::command(async)]
fn engine_self_test(app: tauri::AppHandle) -> Result<EngineSelfTest, String> {
    let root = app
        .path()
        .resolve("engine", BaseDirectory::Resource)
        .map_err(|e| e.to_string())?;
    let run_dir = app
        .path()
        .app_local_data_dir()
        .map_err(|e| e.to_string())?
        .join("run");

    let output = engine::Engine::at(&root)?.run_deck(SELF_TEST_DECK, &run_dir)?;
    let lines: Vec<&str> = output.lines().collect();

    let page = lines
        .iter()
        .position(|l| l.contains("PAGE   1"))
        .ok_or("engine output has no prediction page")?;
    let end = lines[page..]
        .iter()
        .position(|l| l.trim_end().ends_with("SNRxx"))
        .map(|i| page + i + 1)
        .ok_or("engine output has no prediction table")?;

    Ok(EngineSelfTest {
        banner: lines[page].trim().to_string(),
        first_hour: lines[page + 1..end].join("\n"),
    })
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .invoke_handler(tauri::generate_handler![engine_self_test])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
