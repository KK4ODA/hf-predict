//! Runs the ALL.TXT importer over a real WSJT-X log and prints what it made
//! of it. Ignored unless `HFP_ALL_TXT` names the file:
//!
//! `HFP_ALL_TXT=path [HFP_RX_GRID=EM73] cargo test --test alltxt_real -- --ignored --nocapture`

use std::collections::BTreeMap;
use std::sync::Arc;

use hf_predict_lib::observations::Database;
use hf_predict_lib::wsjtx::alltxt;

#[test]
#[ignore]
fn real_log() {
    let Ok(path) = std::env::var("HFP_ALL_TXT") else {
        return;
    };
    let text = String::from_utf8_lossy(&std::fs::read(&path).unwrap()).into_owned();
    let rx_grid = std::env::var("HFP_RX_GRID").ok();
    let db = Arc::new(Database::in_memory().unwrap());
    let started = std::time::Instant::now();
    let summary = alltxt::import(&db, &text, rx_grid.as_deref()).unwrap();
    println!(
        "{} lines in {:.1} s: {summary:?}",
        text.lines().count(),
        started.elapsed().as_secs_f64()
    );

    let all = db.all().unwrap();
    let mut kinds = BTreeMap::new();
    for o in &all {
        *kinds.entry(o.kind.as_str()).or_insert(0) += 1;
    }
    println!("kinds: {kinds:?}");
    println!("with locator: {}", all.iter().filter(|o| o.grid.is_some()).count());
    let no_sender: Vec<_> = all.iter().filter(|o| o.sender.is_none()).collect();
    println!("no sender: {}", no_sender.len());
    for o in no_sender.iter().take(40) {
        println!("  {}", o.message);
    }
}
