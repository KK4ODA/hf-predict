//! Small JSON files in the app's data folders.

use std::fs;
use std::path::Path;

use serde::de::DeserializeOwned;
use serde::Serialize;

/// A missing file is `T::default()`; an unreadable one is an error.
pub fn load<T: DeserializeOwned + Default>(path: &Path) -> Result<T, String> {
    match fs::read_to_string(path) {
        Ok(text) => serde_json::from_str(&text)
            .map_err(|e| format!("{} is not valid saved data: {e}", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(T::default()),
        Err(e) => Err(format!("cannot read {}: {e}", path.display())),
    }
}

/// Writes to a temporary file first so a crash cannot leave a half-written file.
pub fn save<T: Serialize>(path: &Path, data: &T) -> Result<(), String> {
    let text = serde_json::to_string_pretty(data).map_err(|e| e.to_string())?;
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    }
    let temporary = path.with_extension("json.tmp");
    fs::write(&temporary, text).map_err(|e| format!("cannot write {}: {e}", temporary.display()))?;
    fs::rename(&temporary, path).map_err(|e| format!("cannot replace {}: {e}", path.display()))
}
