//! Saved locations and station profiles, kept as one JSON file.

use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::station::StationProfile;

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
}

/// A missing file is an empty `UserData`; an unreadable one is an error.
pub fn load(path: &Path) -> Result<UserData, String> {
    match fs::read_to_string(path) {
        Ok(text) => serde_json::from_str(&text)
            .map_err(|e| format!("{} is not valid saved data: {e}", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(UserData::default()),
        Err(e) => Err(format!("cannot read {}: {e}", path.display())),
    }
}

/// Writes to a temporary file first so a crash cannot leave a half-written file.
pub fn save(path: &Path, data: &UserData) -> Result<(), String> {
    let text = serde_json::to_string_pretty(data).map_err(|e| e.to_string())?;
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    }
    let temporary = path.with_extension("json.tmp");
    fs::write(&temporary, text).map_err(|e| format!("cannot write {}: {e}", temporary.display()))?;
    fs::rename(&temporary, path).map_err(|e| format!("cannot replace {}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
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
