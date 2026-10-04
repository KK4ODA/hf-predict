//! Runs the engine built by `engines/voacapl/build.sh` through the same code
//! path the app uses.

use std::path::PathBuf;

use hf_predict_lib::engine::Engine;

fn engine_root() -> PathBuf {
    std::env::var_os("HFP_ENGINE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.work/engine"))
}

fn scratch_dir(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("hfp-{name}-{}", std::process::id()))
}

#[test]
fn runs_reference_deck() {
    let engine = Engine::at(&engine_root()).expect("engine not built; run engines/voacapl/build.sh");
    let deck = include_str!("../../tests/engine/cases/test01.dat");

    let output = engine.run_deck(deck, &scratch_dir("ref")).expect("engine run failed");

    assert!(output.contains("VOACAP"), "no version banner in output");
    assert!(output.contains("TANGIER"), "circuit label missing from output");
    let tables = output.lines().filter(|l| l.trim_end().ends_with(" FREQ")).count();
    assert_eq!(tables, 24, "expected one table per hour");
}

#[test]
fn runs_in_directory_with_spaces() {
    let engine = Engine::at(&engine_root()).expect("engine not built; run engines/voacapl/build.sh");
    let deck = include_str!("../../tests/engine/cases/ham03-nvis.dat");

    let output = engine
        .run_deck(deck, &scratch_dir("run dir with spaces"))
        .expect("engine run failed");

    assert!(output.contains("SAVANNAH"));
}

#[test]
fn reports_engine_errors() {
    let engine = Engine::at(&engine_root()).expect("engine not built; run engines/voacapl/build.sh");
    let long = "x".repeat(140);

    let error = engine
        .run_deck("QUIT\n", &scratch_dir(&long))
        .expect_err("a run directory beyond the engine's path limit must fail");

    assert!(error.contains("engine"), "unexpected error text: {error}");
}

#[test]
fn missing_engine_is_an_error() {
    assert!(Engine::at(&scratch_dir("no-engine-here")).is_err());
}
