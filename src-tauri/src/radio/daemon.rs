//! Starts `rigctld` for the operator, so the radio can be shared without a
//! terminal, and keeps an eye on it. The daemon is told to listen on the
//! configured host, which is this computer only by default. Also finds the
//! `rigctld` programs installed, the radio models one knows, and the serial
//! ports present.

use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex, PoisonError};

use serde::Serialize;

use super::RadioConfig;

/// Lines of the daemon's output kept for the screen.
const OUTPUT_LINES: usize = 40;

#[cfg(windows)]
fn quiet(cmd: &mut Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    cmd.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
fn quiet(_: &mut Command) {}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DaemonStatus {
    pub command: String,
    pub pid: u32,
    pub running: bool,
    pub exit_code: Option<i32>,
    /// The last lines it printed.
    pub output: String,
}

/// A `rigctld` this app started. Dropping it stops the daemon.
pub struct Daemon {
    child: Child,
    command: String,
    output: Arc<Mutex<VecDeque<String>>>,
}

impl Daemon {
    /// Starts `rigctld` for the configured radio, listening where the app
    /// will connect.
    pub fn start(config: &RadioConfig) -> Result<Self, String> {
        let program = config.rigctld_path.trim();
        if program.is_empty() {
            return Err("no rigctld program chosen".into());
        }
        // A port name the daemon cannot open leaves it accepting connections
        // but never answering, so none is passed for radios that need none
        // (the dummy, a network rig).
        let mut args = vec!["-m".to_string(), config.rig_model.to_string()];
        let serial_port = config.serial_port.trim();
        if !serial_port.is_empty() {
            args.extend(["-r".to_string(), serial_port.to_string(), "-s".to_string(), config.baud.to_string()]);
        }
        args.extend(["-T".to_string(), config.host.clone(), "-t".to_string(), config.port.to_string()]);
        Self::spawn(program, &args)
    }

    pub fn spawn(program: &str, args: &[String]) -> Result<Self, String> {
        let mut cmd = Command::new(program);
        cmd.args(args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
        quiet(&mut cmd);
        let mut child = cmd.spawn().map_err(|e| format!("cannot start {program}: {e}"))?;
        let output = Arc::new(Mutex::new(VecDeque::new()));
        let pipes: [Option<Box<dyn Read + Send>>; 2] = [
            child.stdout.take().map(|p| Box::new(p) as Box<dyn Read + Send>),
            child.stderr.take().map(|p| Box::new(p) as Box<dyn Read + Send>),
        ];
        for pipe in pipes.into_iter().flatten() {
            let output = output.clone();
            std::thread::spawn(move || {
                for line in BufReader::new(pipe).lines().map_while(Result::ok) {
                    let mut output = output.lock().unwrap_or_else(PoisonError::into_inner);
                    if output.len() == OUTPUT_LINES {
                        output.pop_front();
                    }
                    output.push_back(line);
                }
            });
        }
        let command = std::iter::once(program.to_string()).chain(args.iter().cloned()).collect::<Vec<_>>().join(" ");
        Ok(Self { child, command, output })
    }

    pub fn status(&mut self) -> DaemonStatus {
        let exit_code = self.child.try_wait().ok().flatten().map(|s| s.code().unwrap_or(-1));
        DaemonStatus {
            command: self.command.clone(),
            pid: self.child.id(),
            running: exit_code.is_none(),
            exit_code,
            output: self.output.lock().unwrap_or_else(PoisonError::into_inner).iter().cloned().collect::<Vec<_>>().join("\n"),
        }
    }
}

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FoundProgram {
    pub path: String,
    /// What `--version` says, e.g. `Hamlib 4.7.1`.
    pub version: String,
}

/// The version `rigctld` reports, from its first output line.
pub fn parse_version(output: &str) -> String {
    output
        .lines()
        .next()
        .and_then(|line| line.find("Hamlib").map(|i| line[i..].split_whitespace().take(2).collect::<Vec<_>>().join(" ")))
        .unwrap_or_else(|| "unknown version".into())
}

fn version_of(program: &Path) -> String {
    let mut cmd = Command::new(program);
    cmd.arg("--version");
    quiet(&mut cmd);
    match cmd.output() {
        Ok(out) => parse_version(&String::from_utf8_lossy(&out.stdout)),
        Err(e) => format!("cannot run: {e}"),
    }
}

/// Where `rigctld` is usually installed on this platform.
fn candidates() -> Vec<PathBuf> {
    let mut found = Vec::new();
    #[cfg(windows)]
    {
        for root in ["ProgramFiles", "ProgramFiles(x86)", "ProgramW6432"].iter().filter_map(std::env::var_os) {
            let root = PathBuf::from(root);
            if let Ok(entries) = std::fs::read_dir(&root) {
                for entry in entries.flatten() {
                    if entry.file_name().to_string_lossy().to_lowercase().starts_with("hamlib") {
                        found.push(entry.path().join("bin").join("rigctld.exe"));
                    }
                }
            }
            found.push(root.join("wsjtx").join("bin").join("rigctld-wsjtx.exe"));
            found.push(root.join("WSJT-X").join("bin").join("rigctld-wsjtx.exe"));
        }
        found.push(PathBuf::from(r"C:\WSJT\wsjtx\bin\rigctld-wsjtx.exe"));
    }
    #[cfg(not(windows))]
    {
        for dir in ["/usr/bin", "/usr/local/bin", "/opt/homebrew/bin", "/opt/local/bin"] {
            found.push(PathBuf::from(dir).join("rigctld"));
            found.push(PathBuf::from(dir).join("rigctld-wsjtx"));
        }
        found.push(PathBuf::from("/Applications/wsjtx.app/Contents/MacOS/rigctld-wsjtx"));
    }
    if let Some(path) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path) {
            for name in ["rigctld", "rigctld-wsjtx", "rigctld.exe", "rigctld-wsjtx.exe"] {
                found.push(dir.join(name));
            }
        }
    }
    found
}

/// The `rigctld` programs installed, each with its version.
pub fn find() -> Vec<FoundProgram> {
    let mut seen = std::collections::BTreeSet::new();
    candidates()
        .into_iter()
        .filter(|p| p.is_file())
        .filter(|p| seen.insert(p.to_string_lossy().to_lowercase()))
        .map(|p| FoundProgram { version: version_of(&p), path: p.to_string_lossy().into_owned() })
        .collect()
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RigModel {
    pub number: u32,
    pub maker: String,
    pub model: String,
}

/// The table `rigctld -l` prints: a header naming the columns, then one
/// line per radio.
pub fn parse_models(listing: &str) -> Vec<RigModel> {
    let mut lines = listing.lines();
    let Some(header) = lines.find(|l| l.contains("Mfg") && l.contains("Model")) else {
        return Vec::new();
    };
    let (Some(maker_at), Some(model_at), Some(version_at)) =
        (header.find("Mfg"), header.find("Model"), header.find("Version"))
    else {
        return Vec::new();
    };
    let slice = |line: &str, from: usize, to: usize| line.get(from..to.min(line.len())).unwrap_or("").trim().to_string();
    lines
        .filter_map(|line| {
            let number: u32 = line.get(..maker_at)?.trim().parse().ok()?;
            Some(RigModel {
                number,
                maker: slice(line, maker_at, model_at),
                model: slice(line, model_at, version_at),
            })
        })
        .collect()
}

pub fn rig_models(program: &str) -> Result<Vec<RigModel>, String> {
    let mut cmd = Command::new(program);
    cmd.arg("-l");
    quiet(&mut cmd);
    let out = cmd.output().map_err(|e| format!("cannot run {program}: {e}"))?;
    let models = parse_models(&String::from_utf8_lossy(&out.stdout));
    if models.is_empty() {
        return Err(format!("{program} listed no radio models"));
    }
    Ok(models)
}

/// `COM3` before `COM10`.
pub fn sort_ports(ports: &mut [String]) {
    ports.sort_by_key(|p| {
        let digits: String = p.chars().rev().take_while(char::is_ascii_digit).collect::<Vec<_>>().into_iter().rev().collect();
        (p[..p.len() - digits.len()].to_lowercase(), digits.parse::<u32>().unwrap_or(0))
    });
}

/// The serial ports present on this computer.
pub fn serial_ports() -> Vec<String> {
    let mut ports: Vec<String> = Vec::new();
    #[cfg(windows)]
    {
        let mut cmd = Command::new("powershell");
        cmd.args(["-NoProfile", "-Command", "[System.IO.Ports.SerialPort]::GetPortNames()"]);
        quiet(&mut cmd);
        if let Ok(out) = cmd.output() {
            ports = String::from_utf8_lossy(&out.stdout).lines().map(str::trim).filter(|l| !l.is_empty()).map(String::from).collect();
        }
    }
    #[cfg(not(windows))]
    {
        if let Ok(entries) = std::fs::read_dir("/dev") {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                if ["ttyUSB", "ttyACM", "tty.usb", "cu.usb", "tty.SLAB", "cu.SLAB"].iter().any(|p| name.starts_with(p)) {
                    ports.push(format!("/dev/{name}"));
                }
            }
        }
    }
    sort_ports(&mut ports);
    ports
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    const LISTING: &str = "\
 Rig #  Mfg                    Model                   Version         Status      Macro
     1  Hamlib                 Dummy                   20240709.0      Stable      RIG_MODEL_DUMMY
  1042  Yaesu                  FTDX-10                 20241118.9      Stable      RIG_MODEL_FTDX10
  3073  Kenwood                TS-480                  20230406.0      Stable      RIG_MODEL_TS480
";

    #[test]
    fn reads_the_model_table() {
        let models = parse_models(LISTING);
        assert_eq!(models.len(), 3);
        assert_eq!(models[1], RigModel { number: 1042, maker: "Yaesu".into(), model: "FTDX-10".into() });
        assert_eq!(models[2].model, "TS-480");
        assert!(parse_models("nothing here").is_empty());
    }

    #[test]
    fn reads_the_version_line_and_sorts_ports() {
        assert_eq!(parse_version("rigctld Hamlib 4.7.1 2026-04-15T20:20:01Z SHA=d042479a9 64-bit\n"), "Hamlib 4.7.1");
        assert_eq!(parse_version(""), "unknown version");
        let mut ports = vec!["COM10".to_string(), "COM3".to_string(), "COM6".to_string()];
        sort_ports(&mut ports);
        assert_eq!(ports, ["COM3", "COM6", "COM10"]);
    }

    #[test]
    fn a_program_that_exits_is_reported_and_a_running_one_is_stopped_on_drop() {
        #[cfg(windows)]
        let (shell, flag, exit, stay) = ("cmd", "/c", "exit 3", "ping -n 30 127.0.0.1 > nul");
        #[cfg(not(windows))]
        let (shell, flag, exit, stay) = ("sh", "-c", "exit 3", "sleep 30");

        let mut exited = Daemon::spawn(shell, &[flag.to_string(), exit.to_string()]).unwrap();
        let mut status = exited.status();
        for _ in 0..100 {
            if !status.running {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
            status = exited.status();
        }
        assert_eq!((status.running, status.exit_code), (false, Some(3)));
        assert!(status.command.starts_with(shell));

        let mut running = Daemon::spawn(shell, &[flag.to_string(), stay.to_string()]).unwrap();
        std::thread::sleep(Duration::from_millis(300));
        assert!(running.status().running);
        let pid = running.status().pid;
        drop(running);
        assert!(pid > 0);
    }

    #[test]
    fn a_missing_program_is_an_error() {
        assert!(Daemon::spawn("/no/such/rigctld", &[]).map(|_| ()).unwrap_err().starts_with("cannot start"));
        let config = RadioConfig { start_rigctld: true, rigctld_path: " ".into(), ..RadioConfig::default() };
        assert_eq!(Daemon::start(&config).map(|_| ()).unwrap_err(), "no rigctld program chosen");
    }
}

/// Where WSJT-X and its relatives are usually installed: stock WSJT-X,
/// WS (WSJT-X improved), JTDX and Decodium.
fn wsjtx_candidates() -> Vec<PathBuf> {
    let mut found = Vec::new();
    let versions = |root: &Path, program: &str| -> Vec<PathBuf> {
        std::fs::read_dir(root)
            .map(|entries| entries.flatten().map(|e| e.path().join("bin").join(program)).collect())
            .unwrap_or_default()
    };
    #[cfg(windows)]
    {
        found.extend(versions(Path::new(r"C:\WS"), "ws.exe"));
        found.extend(versions(Path::new(r"C:\WSJT"), "wsjtx.exe"));
        found.extend(versions(Path::new(r"C:\JTDX"), "jtdx.exe"));
        for root in ["ProgramFiles", "ProgramFiles(x86)", "ProgramW6432"].iter().filter_map(std::env::var_os) {
            let root = PathBuf::from(root);
            found.push(root.join("wsjtx").join("bin").join("wsjtx.exe"));
            found.push(root.join("WSJT-X").join("bin").join("wsjtx.exe"));
            found.extend(versions(&root.join("WS"), "ws.exe"));
        }
        if let Some(local) = std::env::var_os("LOCALAPPDATA") {
            found.push(PathBuf::from(local).join("Programs").join("Decodium").join("decodium.exe"));
        }
    }
    #[cfg(not(windows))]
    {
        for dir in ["/usr/bin", "/usr/local/bin", "/opt/homebrew/bin"] {
            for program in ["wsjtx", "ws", "jtdx"] {
                found.push(PathBuf::from(dir).join(program));
            }
        }
        found.push(PathBuf::from("/Applications/wsjtx.app/Contents/MacOS/wsjtx"));
        found.push(PathBuf::from("/Applications/WS.app/Contents/MacOS/ws"));
        found.push(PathBuf::from("/Applications/JTDX.app/Contents/MacOS/jtdx"));
    }
    found
}

/// The WSJT-X programs installed. `version` carries the folder they sit in,
/// which is how the WS builds are versioned.
pub fn find_wsjtx() -> Vec<FoundProgram> {
    let mut seen = std::collections::BTreeSet::new();
    wsjtx_candidates()
        .into_iter()
        .filter(|p| p.is_file())
        .filter(|p| seen.insert(p.to_string_lossy().to_lowercase()))
        .map(|p| FoundProgram {
            version: {
                // `WS/3.2.1/bin/ws.exe` is versioned by folder; `Decodium/decodium.exe` is not.
                let parent = p.parent();
                let in_bin = parent.and_then(Path::file_name).is_some_and(|n| n.eq_ignore_ascii_case("bin"));
                let named = if in_bin { parent.and_then(Path::parent) } else { parent };
                named.and_then(Path::file_name).map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
            },
            path: p.to_string_lossy().into_owned(),
        })
        .collect()
}

/// Starts a program on its own, as the operator would from a shortcut: it
/// is not watched and not stopped when this app closes.
pub fn launch(program: &str) -> Result<u32, String> {
    let path = Path::new(program.trim());
    if program.trim().is_empty() {
        return Err("no program chosen".into());
    }
    let mut cmd = Command::new(path);
    if let Some(dir) = path.parent() {
        cmd.current_dir(dir);
    }
    cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    let child = cmd.spawn().map_err(|e| format!("cannot start {program}: {e}"))?;
    Ok(child.id())
}

#[cfg(test)]
mod launch_tests {
    use super::*;

    /// Prints what this computer has: `cargo test --lib launch_tests::what_is_installed -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn what_is_installed() {
        println!("rigctld: {:?}", find());
        println!("wsjtx: {:?}", find_wsjtx());
        println!("ports: {:?}", serial_ports());
    }

    #[test]
    fn a_missing_program_cannot_be_launched() {
        assert!(launch("/no/such/wsjtx").unwrap_err().starts_with("cannot start"));
        assert_eq!(launch("  ").unwrap_err(), "no program chosen");
    }
}
