//! WSJT-X's `ALL.TXT` logs on disk: finding the ones each installed variant
//! keeps, and reading each configured one from where the last check left
//! off. A station may have several: WSJT-X, WSJT-X improved, a `--rig-name`
//! instance, or an older installation with history worth keeping.

use std::fs::Metadata;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::UNIX_EPOCH;

use serde::{Deserialize, Serialize};

use super::alltxt::{self, ImportSummary};
use crate::geo;
use crate::observations::Database;

/// A log the user asked to be checked.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogFile {
    pub path: String,
    /// Where the receiver was, since the log does not say: a locator or
    /// latitude, longitude. Without it decodes get no distance or bearing.
    #[serde(default)]
    pub rx_position: Option<String>,
}

/// A log found in a known data folder.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FoundLog {
    pub path: String,
    /// The data folder's name, which is the program's: `WSJT-X`, `WSJT-X - improved`, …
    pub program: String,
    pub size_bytes: u64,
    pub modified_utc: Option<i64>,
}

/// The outcome of reading one log.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogCheck {
    pub path: String,
    pub ok: bool,
    pub detail: String,
    pub checked_utc: i64,
    pub size_bytes: u64,
    /// Bytes read this time.
    pub new_bytes: u64,
    pub summary: Option<ImportSummary>,
}

/// Folders under which the programs keep their data, on this platform.
fn data_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        dirs.push(PathBuf::from(local));
    }
    if let Some(home) = std::env::var_os("HOME") {
        let home = PathBuf::from(home);
        dirs.push(home.join(".local/share"));
        dirs.push(home.join("Library/Application Support"));
    }
    dirs
}

fn modified_utc(meta: &Metadata) -> Option<i64> {
    meta.modified().ok()?.duration_since(UNIX_EPOCH).ok().map(|d| d.as_secs() as i64)
}

/// Logs in `dir` whose folder is named for WSJT-X or JTDX, newest first.
pub fn find_in(dir: &Path) -> Vec<FoundLog> {
    let mut found = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return found;
    };
    for entry in entries.flatten() {
        let program = entry.file_name().to_string_lossy().into_owned();
        let upper = program.to_uppercase();
        if !(upper.starts_with("WSJT-X") || upper.starts_with("JTDX")) {
            continue;
        }
        let path = entry.path().join("ALL.TXT");
        if let Ok(meta) = std::fs::metadata(&path) {
            found.push(FoundLog {
                path: path.to_string_lossy().into_owned(),
                program,
                size_bytes: meta.len(),
                modified_utc: modified_utc(&meta),
            });
        }
    }
    found.sort_by(|a, b| b.modified_utc.cmp(&a.modified_utc));
    found
}

/// Logs in this platform's data folders.
pub fn find() -> Vec<FoundLog> {
    data_dirs().iter().flat_map(|dir| find_in(dir)).collect()
}

fn describe(summary: &ImportSummary, restarted: bool, new_bytes: usize) -> String {
    if new_bytes == 0 {
        return "nothing new since the last check".into();
    }
    let mut parts = vec![format!("{} new decodes", summary.stored)];
    if summary.already_stored > 0 {
        parts.push(format!("{} already stored", summary.already_stored));
    }
    if summary.transmissions > 0 {
        parts.push(format!("{} own transmissions", summary.transmissions));
    }
    if summary.not_understood > 0 {
        parts.push(format!("{} lines not understood", summary.not_understood));
    }
    let prefix = if restarted { "read again from the start: " } else { "" };
    format!("{prefix}{}", parts.join(", "))
}

/// Imports what was added to `log` since it was last checked. A file that
/// shrank or was replaced is read from the start; the database keeps one
/// copy of each decode either way. Only whole lines are read, since the
/// program may be in the middle of writing one.
pub fn check(db: &Arc<Database>, log: &LogFile, now: i64) -> LogCheck {
    match check_inner(db, log, now) {
        Ok(check) => check,
        Err(detail) => LogCheck {
            path: log.path.clone(),
            ok: false,
            detail,
            checked_utc: now,
            size_bytes: 0,
            new_bytes: 0,
            summary: None,
        },
    }
}

fn check_inner(db: &Arc<Database>, log: &LogFile, now: i64) -> Result<LogCheck, String> {
    let rx_grid = match log.rx_position.as_deref().map(str::trim).filter(|p| !p.is_empty()) {
        Some(text) => {
            Some(geo::to_maidenhead(geo::parse_position(text).map_err(|e| format!("receiver position: {e}"))?))
        }
        None => None,
    };
    let bytes = std::fs::read(&log.path).map_err(|e| format!("cannot read the file: {e}"))?;
    let imported = db.log_progress(&log.path)?.map_or(0, |(bytes, _)| bytes as usize);
    let continues = imported <= bytes.len() && (imported == 0 || bytes[imported - 1] == b'\n');
    let start = if continues { imported } else { 0 };
    let end = bytes[start..].iter().rposition(|&b| b == b'\n').map_or(start, |i| start + i + 1);
    let text = String::from_utf8_lossy(&bytes[start..end]);
    let summary = alltxt::import(db, &text, rx_grid.as_deref())?;
    db.set_log_progress(&log.path, end as u64, now)?;
    Ok(LogCheck {
        path: log.path.clone(),
        ok: true,
        detail: describe(&summary, !continues && imported > 0, end - start),
        checked_utc: now,
        size_bytes: bytes.len() as u64,
        new_bytes: (end - start) as u64,
        summary: Some(summary),
    })
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::io::Write;

    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("hfp-logs-{name}-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    const FIRST: &str = "241004_153000    14.074 Rx FT8    -12  0.3 1234 CQ K1ABC FN42\n";
    const SECOND: &str = "241004_153015    14.074 Rx FT8     -5  0.2 1711 K1ABC W9XYZ EN37\n";

    #[test]
    fn reads_only_what_was_added_and_only_whole_lines() {
        let dir = scratch("incremental");
        let path = dir.join("ALL.TXT");
        fs::write(&path, FIRST).unwrap();
        let db = Arc::new(Database::in_memory().unwrap());
        let log = LogFile { path: path.to_string_lossy().into_owned(), rx_position: Some("33.9, -84.3".into()) };

        let first = check(&db, &log, 1);
        assert!(first.ok, "{}", first.detail);
        assert_eq!((first.new_bytes as usize, first.summary.as_ref().unwrap().stored), (FIRST.len(), 1));
        assert_eq!(first.detail, "1 new decodes");
        // The receiver position becomes a locator, so the decode has a distance.
        assert!(db.all().unwrap()[0].distance_km.is_some());

        // A line still being written is left for next time.
        let mut file = fs::OpenOptions::new().append(true).open(&path).unwrap();
        file.write_all(SECOND.trim_end().as_bytes()).unwrap();
        let partial = check(&db, &log, 2);
        assert_eq!((partial.new_bytes, partial.detail.as_str()), (0, "nothing new since the last check"));

        file.write_all(b"\n").unwrap();
        let second = check(&db, &log, 3);
        assert_eq!((second.new_bytes as usize, second.summary.unwrap().stored), (SECOND.len(), 1));
        assert_eq!(db.count().unwrap(), 2);
    }

    #[test]
    fn a_replaced_file_is_read_from_the_start() {
        let dir = scratch("replaced");
        let path = dir.join("ALL.TXT");
        fs::write(&path, [FIRST, SECOND].concat()).unwrap();
        let db = Arc::new(Database::in_memory().unwrap());
        let log = LogFile { path: path.to_string_lossy().into_owned(), rx_position: None };
        assert_eq!(check(&db, &log, 1).summary.unwrap().stored, 2);

        fs::write(&path, SECOND).unwrap();
        let again = check(&db, &log, 2);
        assert_eq!(again.detail, "read again from the start: 0 new decodes, 1 already stored");
        assert_eq!(db.count().unwrap(), 2);
    }

    #[test]
    fn a_missing_file_or_bad_position_is_reported_not_fatal() {
        let db = Arc::new(Database::in_memory().unwrap());
        let missing = check(&db, &LogFile { path: "/no/such/ALL.TXT".into(), rx_position: None }, 1);
        assert!(!missing.ok && missing.detail.starts_with("cannot read the file"));
        let dir = scratch("bad-position");
        let path = dir.join("ALL.TXT");
        fs::write(&path, FIRST).unwrap();
        let bad = check(&db, &LogFile { path: path.to_string_lossy().into_owned(), rx_position: Some("nowhere".into()) }, 1);
        assert!(!bad.ok && bad.detail.starts_with("receiver position"), "{}", bad.detail);
    }

    #[test]
    fn finds_logs_in_program_folders() {
        let dir = scratch("find");
        for name in ["WSJT-X", "WSJT-X - improved", "JTDX", "Other"] {
            fs::create_dir_all(dir.join(name)).unwrap();
            fs::write(dir.join(name).join("ALL.TXT"), FIRST).unwrap();
        }
        fs::create_dir_all(dir.join("WSJT-X - empty")).unwrap();
        let mut programs: Vec<String> = find_in(&dir).into_iter().map(|f| f.program).collect();
        programs.sort();
        assert_eq!(programs, ["JTDX", "WSJT-X", "WSJT-X - improved"]);
    }
}
