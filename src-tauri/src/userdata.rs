//! Saved locations and station profiles, kept as one JSON file.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::jsonfile;
use crate::station::StationProfile;
use crate::wsjtx::logs::LogFile;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedLocation {
    pub name: String,
    /// Locator or latitude, longitude, as the user typed it.
    pub position: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct UserData {
    pub locations: Vec<SavedLocation>,
    pub stations: Vec<StationProfile>,
    /// WSJT-X ALL.TXT logs to read.
    pub log_files: Vec<LogFile>,
}

/// A missing file is an empty `UserData`; an unreadable one is an error.
pub fn load(path: &Path) -> Result<UserData, String> {
    jsonfile::load(path)
}

pub fn save(path: &Path, data: &UserData) -> Result<(), String> {
    jsonfile::save(path, data)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::station;

    fn scratch_file(name: &str) -> std::path::PathBuf {
        std::env::temp_dir()
            .join(format!("hfp-userdata-{name}-{}", std::process::id()))
            .join("userdata.json")
    }

    #[test]
    fn missing_file_is_empty() {
        assert_eq!(load(&scratch_file("missing")).unwrap(), UserData::default());
    }

    #[test]
    fn round_trips() {
        let path = scratch_file("round-trip");
        let data = UserData {
            locations: vec![SavedLocation { name: "Home".into(), position: "EM73tr".into() }],
            stations: station::presets(),
            log_files: vec![LogFile { path: "C:/logs/ALL.TXT".into(), rx_position: Some("EM73".into()) }],
        };
        save(&path, &data).unwrap();
        assert_eq!(load(&path).unwrap(), data);
        save(&path, &UserData::default()).unwrap();
        assert_eq!(load(&path).unwrap(), UserData::default());
    }

    #[test]
    fn corrupt_file_is_reported() {
        let path = scratch_file("corrupt");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "{ not json").unwrap();
        assert!(load(&path).unwrap_err().contains("not valid"));
    }
}
