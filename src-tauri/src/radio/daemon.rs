//! Starts `rigctld` for the operator, so the radio can be shared without a
//! terminal, and keeps an eye on it. The daemon is told to listen on the
//! configured host, which is this computer only by default. Also finds the
//! `rigctld` programs installed, the radio models one knows, and the serial
//! ports present.

use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use serde::{Deserialize, Serialize};

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

enum Process {
    Child(Child),
    /// Started by an earlier session of this app and left running.
    Adopted(u32),
}

/// What is written down about a running daemon, so a later session can
/// find it again.
#[derive(Serialize, Deserialize)]
struct Record {
    pid: u32,
    command: String,
}

/// A `rigctld` this app started. Dropping it stops the daemon, unless the
/// operator chose to leave it running.
pub struct Daemon {
    process: Process,
    command: String,
    output: Arc<Mutex<VecDeque<String>>>,
    record: Option<PathBuf>,
    keep: Arc<AtomicBool>,
}

/// The name of a running process, or None when there is no such process.
fn process_name(pid: u32) -> Option<String> {
    #[cfg(windows)]
    {
        let mut cmd = Command::new("tasklist");
        cmd.args(["/FI", &format!("PID eq {pid}"), "/FO", "CSV", "/NH"]);
        quiet(&mut cmd);
        let out = cmd.output().ok()?;
        let text = String::from_utf8_lossy(&out.stdout);
        let first = text.lines().next()?.trim();
        // "rigctld.exe","18024",... ; anything else means no such process.
        first.strip_prefix('"').and_then(|rest| rest.split('"').next()).map(String::from)
    }
    #[cfg(not(windows))]
    {
        let out = Command::new("ps").args(["-p", &pid.to_string(), "-o", "comm="]).output().ok()?;
        let name = String::from_utf8_lossy(&out.stdout).trim().to_string();
        (!name.is_empty()).then_some(name)
    }
}

fn is_rigctld(pid: u32) -> bool {
    process_name(pid).is_some_and(|name| name.to_lowercase().contains("rigctld"))
}

/// Stops a process by id, but only if it is still a rigctld: an id can be
/// reused once its process has gone.
fn stop_pid(pid: u32) {
    if !is_rigctld(pid) {
        return;
    }
    #[cfg(windows)]
    let mut cmd = {
        let mut cmd = Command::new("taskkill");
        cmd.args(["/PID", &pid.to_string(), "/F"]);
        cmd
    };
    #[cfg(not(windows))]
    let mut cmd = {
        let mut cmd = Command::new("kill");
        cmd.arg(pid.to_string());
        cmd
    };
    quiet(&mut cmd);
    let _ = cmd.stdout(Stdio::null()).stderr(Stdio::null()).status();
}

impl Daemon {
    /// Starts `rigctld` for the configured radio, listening where the app
    /// will connect.
    pub fn start(config: &RadioConfig) -> Result<Self, String> {
        Self::start_recorded(config, None, Arc::new(AtomicBool::new(false)))
    }

    /// As `start`, writing the daemon's process id to `record` so a later
    /// session can take it back, and leaving it running if `keep` is set by
    /// the time it is dropped.
    pub fn start_recorded(config: &RadioConfig, record: Option<&Path>, keep: Arc<AtomicBool>) -> Result<Self, String> {
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
        let mut daemon = Self::spawn(program, &args)?;
        daemon.keep = keep;
        if let Some(path) = record {
            let written = Record { pid: daemon.pid(), command: daemon.command.clone() };
            if let Ok(text) = serde_json::to_string(&written) {
                let _ = std::fs::write(path, text);
                daemon.record = Some(path.to_path_buf());
            }
        }
        Ok(daemon)
    }

    /// The daemon an earlier session left running, if `record` names a
    /// rigctld that is still alive. A stale record is removed.
    pub fn adopt(record: &Path, keep: Arc<AtomicBool>) -> Option<Self> {
        let text = std::fs::read_to_string(record).ok()?;
        let Ok(found) = serde_json::from_str::<Record>(&text) else {
            let _ = std::fs::remove_file(record);
            return None;
        };
        if !is_rigctld(found.pid) {
            let _ = std::fs::remove_file(record);
            return None;
        }
        Some(Self {
            process: Process::Adopted(found.pid),
            command: found.command,
            output: Arc::new(Mutex::new(VecDeque::from(["Left running by an earlier session; taken back.".to_string()]))),
            record: Some(record.to_path_buf()),
            keep,
        })
    }

    pub fn pid(&self) -> u32 {
        match &self.process {
            Process::Child(child) => child.id(),
            Process::Adopted(pid) => *pid,
        }
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
        Ok(Self {
            process: Process::Child(child),
            command,
            output,
            record: None,
            keep: Arc::new(AtomicBool::new(false)),
        })
    }

    pub fn status(&mut self) -> DaemonStatus {
        // An adopted daemon is taken to be running while it answers; the
        // monitor notices when it stops.
        let exit_code = match &mut self.process {
            Process::Child(child) => child.try_wait().ok().flatten().map(|s| s.code().unwrap_or(-1)),
            Process::Adopted(_) => None,
        };
        DaemonStatus {
            command: self.command.clone(),
            pid: self.pid(),
            running: exit_code.is_none(),
            exit_code,
            output: self.output.lock().unwrap_or_else(PoisonError::into_inner).iter().cloned().collect::<Vec<_>>().join("\n"),
        }
    }
}

impl Drop for Daemon {
    fn drop(&mut self) {
        if self.keep.load(Ordering::Relaxed) {
            return;
        }
        match &mut self.process {
            Process::Child(child) => {
                let _ = child.kill();
                let _ = child.wait();
            }
            Process::Adopted(pid) => stop_pid(*pid),
        }
        if let Some(record) = &self.record {
            let _ = std::fs::remove_file(record);
        }
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
    fn a_kept_daemon_survives_and_a_record_without_rigctld_is_dropped() {
        #[cfg(windows)]
        let (shell, flag, stay) = ("cmd", "/c", "ping -n 30 127.0.0.1 > nul");
        #[cfg(not(windows))]
        let (shell, flag, stay) = ("sh", "-c", "sleep 30");
        let mut kept = Daemon::spawn(shell, &[flag.to_string(), stay.to_string()]).unwrap();
        kept.keep.store(true, Ordering::Relaxed);
        let pid = kept.pid();
        let mut handle = match std::mem::replace(&mut kept.process, Process::Adopted(pid)) {
            Process::Child(child) => child,
            Process::Adopted(_) => unreachable!(),
        };
        drop(kept);
        assert!(handle.try_wait().unwrap().is_none(), "a kept daemon must still be running");
        let _ = handle.kill();
        let _ = handle.wait();

        // A record naming a process that is not rigctld is stale and removed.
        let dir = std::env::temp_dir().join(format!("hfp-daemon-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let record = dir.join("rigctld.json");
        std::fs::write(&record, format!("{{\"pid\":{},\"command\":\"x\"}}", std::process::id())).unwrap();
        assert!(Daemon::adopt(&record, Arc::new(AtomicBool::new(false))).is_none());
        assert!(!record.exists());
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

/// Whether a program with this file name is running, whoever started it.
/// Unknown (the process list cannot be read) counts as not running.
pub fn is_running(program: &str) -> bool {
    let Some(name) = Path::new(program.trim()).file_name() else {
        return false;
    };
    process_list().is_some_and(|list| names_include(&list, &name.to_string_lossy()))
}

/// Every running process, one per line.
fn process_list() -> Option<String> {
    #[cfg(windows)]
    let mut cmd = {
        let mut cmd = Command::new("tasklist");
        cmd.args(["/FO", "CSV", "/NH"]);
        cmd
    };
    #[cfg(not(windows))]
    let mut cmd = {
        let mut cmd = Command::new("ps");
        cmd.args(["-A", "-o", "comm="]);
        cmd
    };
    quiet(&mut cmd);
    let out = cmd.stdin(Stdio::null()).stderr(Stdio::null()).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Whether a process list names `program`. Windows lists `"ws.exe","1234",…`
/// per line; `ps` lists a name or a path, which Linux cuts to 15 characters.
pub fn names_include(list: &str, program: &str) -> bool {
    let stem = |name: &str| {
        let name = name.trim().to_lowercase();
        name.strip_suffix(".exe").map(str::to_string).unwrap_or(name)
    };
    let wanted = stem(program);
    if wanted.is_empty() {
        return false;
    }
    list.lines().any(|line| {
        let first = line.trim().trim_start_matches('"').split('"').next().unwrap_or("");
        let base = stem(first.rsplit(['/', '\\']).next().unwrap_or(first));
        !base.is_empty() && (base == wanted || (base.len() == 15 && wanted.starts_with(&base)))
    })
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

    #[test]
    fn a_running_program_is_found_by_its_file_name() {
        let tasklist = "\"System Idle Process\",\"0\",\"Services\",\"0\",\"8 K\"\n\"ws.exe\",\"4242\",\"Console\",\"1\",\"95,312 K\"\n";
        assert!(names_include(tasklist, "ws.exe"));
        assert!(names_include(tasklist, "WS.EXE"));
        assert!(!names_include(tasklist, "wsjtx.exe"));
        let ps = "/usr/lib/systemd/systemd\n/Applications/wsjtx.app/Contents/MacOS/wsjtx\njt9\n";
        assert!(names_include(ps, "wsjtx"));
        assert!(!names_include(ps, "ws"));
        // Linux cuts names to 15 characters.
        assert!(names_include("wsjtx-improved-\n", "wsjtx-improved-3.2"));
        assert!(!names_include(ps, ""));
    }

    /// `cargo test --lib launch_tests::this_test_runner_is_running -- --ignored`
    #[test]
    #[ignore]
    fn this_test_runner_is_running() {
        let me = std::env::current_exe().unwrap();
        assert!(is_running(&me.to_string_lossy()));
        assert!(!is_running("C:/nowhere/surely-not-running-hfp.exe"));
    }

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
