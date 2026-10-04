//! Runs the bundled VOACAP engine (`voacapl`) as a subprocess.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const ENGINE_EXE: &str = if cfg!(windows) { "voacapl.exe" } else { "voacapl" };
const DECK_FILE: &str = "hfp.dat";
const OUTPUT_FILE: &str = "hfp.out";

/// A `voacapl` executable and the `itshfbc` data tree it reads.
pub struct Engine {
    exe: PathBuf,
    root: PathBuf,
}

impl Engine {
    /// `root` holds `bin/voacapl[.exe]` and `itshfbc/`, as produced by
    /// `engines/voacapl/build.sh`.
    pub fn at(root: &Path) -> Result<Self, String> {
        let root = strip_verbatim_prefix(root);
        let exe = root.join("bin").join(ENGINE_EXE);
        if !exe.is_file() {
            return Err(format!("engine executable not found at {}", exe.display()));
        }
        if !root.join("itshfbc").is_dir() {
            return Err(format!("engine data not found under {}", root.display()));
        }
        Ok(Self { exe, root })
    }

    /// Runs one input deck in `run_dir` and returns the engine's output text.
    pub fn run_deck(&self, deck: &str, run_dir: &Path) -> Result<String, String> {
        let run_dir = strip_verbatim_prefix(run_dir);
        fs::create_dir_all(&run_dir)
            .map_err(|e| format!("cannot create run directory {}: {e}", run_dir.display()))?;
        let output = run_dir.join(OUTPUT_FILE);
        fs::write(run_dir.join(DECK_FILE), deck)
            .map_err(|e| format!("cannot write input deck: {e}"))?;
        let _ = fs::remove_file(&output);

        // The engine keeps paths in 128-character buffers. Running from the
        // engine root lets the data tree be named by a short relative path.
        let mut cmd = Command::new(&self.exe);
        cmd.current_dir(&self.root)
            .arg("-s")
            .arg(format!("--run-dir={}", run_dir.display()))
            .arg("itshfbc")
            .arg(DECK_FILE)
            .arg(OUTPUT_FILE);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }

        let result = cmd
            .output()
            .map_err(|e| format!("could not start engine {}: {e}", self.exe.display()))?;
        if !result.status.success() {
            let said = [&result.stdout[..], &result.stderr[..]].concat();
            return Err(format!(
                "engine failed ({}): {}",
                result.status,
                String::from_utf8_lossy(&said).trim()
            ));
        }
        fs::read_to_string(&output).map_err(|e| format!("engine produced no output: {e}"))
    }
}

/// Windows APIs can hand back `\\?\C:\...` paths, which the engine cannot open.
fn strip_verbatim_prefix(path: &Path) -> PathBuf {
    match path.to_str().and_then(|s| s.strip_prefix(r"\\?\")) {
        Some(rest) if !rest.starts_with("UNC\\") => PathBuf::from(rest),
        _ => path.to_path_buf(),
    }
}
