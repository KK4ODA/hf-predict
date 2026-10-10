//! Radio control through a `rigctld` shared with WSJT-X, which the app can
//! start itself. The app reads frequency, mode, PTT, split and VFO. The
//! scanner alone changes anything: the frequency, and the mode when a band
//! change recalled another, since some radios keep a mode per band. The
//! interface has no transmit function.

pub mod daemon;
pub mod rigctld;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::station;
use crate::timeutil;

/// What the radio reported.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RadioState {
    pub freq_hz: u64,
    pub band: String,
    pub mode: String,
    pub passband_hz: Option<u64>,
    pub ptt: bool,
    pub split: Option<bool>,
    pub vfo: Option<String>,
    pub tx_vfo: Option<String>,
}

impl RadioState {
    pub fn at(freq_hz: u64, mode: &str) -> Self {
        Self {
            freq_hz,
            band: station::band_for_hz(freq_hz),
            mode: mode.to_string(),
            passband_hz: None,
            ptt: false,
            split: None,
            vfo: None,
            tx_vfo: None,
        }
    }
}

/// A connection to a radio that can be read. There is deliberately no way
/// to key the transmitter.
pub trait RadioController: Send {
    fn name(&self) -> &str;
    /// Reads everything the app needs in one go.
    fn read(&mut self) -> Result<RadioState, String>;
    /// Sets the frequency; only the scanner does.
    fn set_freq(&mut self, hz: u64) -> Result<(), String>;
    /// Sets the mode, as Hamlib names it, and the passband when given;
    /// `None` leaves the passband alone. Only the scanner does, after a
    /// band change.
    fn set_mode(&mut self, mode: &str, passband_hz: Option<u64>) -> Result<(), String>;
}

/// Hamlib's mode names as radios show them: `PKTUSB` is DATA-U.
pub fn mode_name(mode: &str) -> &str {
    match mode {
        "PKTUSB" => "DATA-U",
        "PKTLSB" => "DATA-L",
        "PKTFM" => "DATA-FM",
        "PKTAM" => "DATA-AM",
        other => other,
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct RadioConfig {
    pub enabled: bool,
    pub host: String,
    pub port: u16,
    pub poll_seconds: f64,
    /// Start `rigctld` from this app rather than attach to one already running.
    pub start_rigctld: bool,
    pub rigctld_path: String,
    /// Hamlib's number for the radio, as `rigctld -l` lists it.
    pub rig_model: u32,
    pub serial_port: String,
    pub baud: u32,
    /// Start WSJT-X once rigctld answers, so it finds the daemon up.
    pub start_wsjtx: bool,
    pub wsjtx_path: String,
    /// The mode a scan keeps the radio in, as Hamlib names it (`PKTUSB` is
    /// DATA-U). Empty keeps the mode the radio had when the scan began.
    pub scan_mode: String,
}

impl Default for RadioConfig {
    /// Off until the operator turns it on; `rigctld`'s default port, on
    /// this computer only.
    fn default() -> Self {
        Self {
            enabled: false,
            host: "127.0.0.1".into(),
            port: 4532,
            poll_seconds: 2.0,
            start_rigctld: false,
            rigctld_path: String::new(),
            rig_model: 0,
            serial_port: String::new(),
            baud: 38_400,
            start_wsjtx: false,
            wsjtx_path: String::new(),
            scan_mode: "PKTUSB".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RadioStatus {
    pub config: RadioConfig,
    /// `off`, `connecting`, `connected` or `failed`.
    pub state: &'static str,
    pub detail: String,
    pub radio: Option<RadioState>,
    /// When `radio` was read.
    pub read_utc: Option<i64>,
    pub reads: u64,
    pub errors: u64,
    pub last_error: Option<String>,
    /// The `rigctld` this app started, if it did.
    pub daemon: Option<daemon::DaemonStatus>,
}

impl RadioStatus {
    pub fn off(config: RadioConfig) -> Self {
        Self {
            config,
            state: "off",
            detail: "Not connected to a radio.".into(),
            radio: None,
            read_utc: None,
            reads: 0,
            errors: 0,
            last_error: None,
            daemon: None,
        }
    }
}

/// How long to wait before trying to connect again.
const RETRY: Duration = Duration::from_secs(5);
const STOP_CHECK: Duration = Duration::from_millis(100);
/// Attempts to reach a `rigctld` this app just started, one second apart.
const STARTUP_ATTEMPTS: u32 = 8;
/// A request older than this is dropped rather than carried out late.
const REQUEST_TTL: Duration = Duration::from_secs(5);

/// A change to make on the radio.
enum Change {
    Freq(u64),
    Mode(String, Option<u64>),
}

/// Make a change and reply with the state read back.
struct Request {
    change: Change,
    sent: Instant,
    reply: Sender<Result<RadioState, String>>,
}

type Connector = dyn Fn(&RadioConfig) -> Result<Box<dyn RadioController>, String> + Send;

/// Reads the radio on a background thread at the configured rate, and
/// reconnects after a failure.
pub struct Monitor {
    stop: Arc<AtomicBool>,
    /// Leave a rigctld this monitor started running when it stops.
    keep: Arc<AtomicBool>,
    status: Arc<Mutex<RadioStatus>>,
    requests: Sender<Request>,
    thread: Option<JoinHandle<()>>,
}

impl Monitor {
    /// Connects to `rigctld` as configured, starting it first if asked.
    pub fn start(config: RadioConfig) -> Self {
        Self::start_recorded(config, None)
    }

    /// As `start`, keeping the started daemon's process id in `record`, so
    /// a daemon an earlier session left running is taken back, not started
    /// a second time.
    pub fn start_recorded(config: RadioConfig, record: Option<PathBuf>) -> Self {
        Self::launch(
            config,
            Box::new(|config: &RadioConfig| {
                rigctld::Rigctld::connect(&config.host, config.port)
                    .map(|client| Box::new(client) as Box<dyn RadioController>)
            }),
            record,
        )
    }

    /// Reads whatever `connect` provides; for tests.
    pub fn start_with(config: RadioConfig, connect: Box<Connector>) -> Self {
        Self::launch(config, connect, None)
    }

    fn launch(config: RadioConfig, connect: Box<Connector>, record: Option<PathBuf>) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let keep = Arc::new(AtomicBool::new(false));
        let status = Arc::new(Mutex::new(RadioStatus::off(config.clone())));
        let (requests, inbox) = mpsc::channel();
        let thread = {
            let (stop, keep, status) = (stop.clone(), keep.clone(), status.clone());
            std::thread::spawn(move || run(config, connect, inbox, stop, keep, record, status))
        };
        Self { stop, keep, status, requests, thread: Some(thread) }
    }

    /// When this monitor stops, leave the rigctld it started running, for
    /// other programs still using it.
    pub fn keep_daemon_running(&self) {
        self.keep.store(true, Ordering::Relaxed);
    }

    pub fn status(&self) -> RadioStatus {
        self.status.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }

    /// Sets the frequency on the monitor's own connection and returns what
    /// the radio reads afterwards. Fails when the radio is not connected.
    pub fn set_freq(&self, hz: u64) -> Result<RadioState, String> {
        self.request(Change::Freq(hz))
    }

    /// Sets the mode, and the passband when given, and returns what the
    /// radio reads afterwards.
    pub fn set_mode(&self, mode: &str, passband_hz: Option<u64>) -> Result<RadioState, String> {
        self.request(Change::Mode(mode.to_string(), passband_hz))
    }

    fn request(&self, change: Change) -> Result<RadioState, String> {
        let (reply, answer) = mpsc::channel();
        self.requests
            .send(Request { change, sent: Instant::now(), reply })
            .map_err(|_| "the radio monitor has stopped".to_string())?;
        match answer.recv_timeout(REQUEST_TTL + Duration::from_secs(5)) {
            Ok(result) => result,
            Err(_) => Err("the radio is not connected".into()),
        }
    }
}

impl Drop for Monitor {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Sleeps for `duration`, waking early when asked to stop. False when stopped.
fn pause(stop: &AtomicBool, duration: Duration) -> bool {
    let until = Instant::now() + duration;
    while Instant::now() < until {
        if stop.load(Ordering::Relaxed) {
            return false;
        }
        std::thread::sleep(STOP_CHECK.min(until - Instant::now()));
    }
    !stop.load(Ordering::Relaxed)
}

/// Waits out the polling interval while serving the scanner's requests on the
/// connection. False when asked to stop.
fn serve(
    radio: &mut dyn RadioController,
    inbox: &Receiver<Request>,
    duration: Duration,
    stop: &AtomicBool,
    status: &Mutex<RadioStatus>,
) -> bool {
    let until = Instant::now() + duration;
    loop {
        if stop.load(Ordering::Relaxed) {
            return false;
        }
        let now = Instant::now();
        if now >= until {
            return true;
        }
        match inbox.recv_timeout(STOP_CHECK.min(until - now)) {
            Ok(request) if request.sent.elapsed() > REQUEST_TTL => {
                let _ = request.reply.send(Err("the request to the radio waited too long and was dropped".into()));
            }
            Ok(request) => {
                let result = match &request.change {
                    Change::Freq(hz) => radio.set_freq(*hz),
                    Change::Mode(mode, passband_hz) => radio.set_mode(mode, *passband_hz),
                }
                .and_then(|()| radio.read());
                if let Ok(state) = &result {
                    let mut s = status.lock().unwrap_or_else(PoisonError::into_inner);
                    s.radio = Some(state.clone());
                    s.read_utc = Some(timeutil::now());
                }
                let _ = request.reply.send(result);
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return pause(stop, until.saturating_duration_since(Instant::now())),
        }
    }
}

fn run(
    config: RadioConfig,
    connect: Box<Connector>,
    inbox: Receiver<Request>,
    stop: Arc<AtomicBool>,
    keep: Arc<AtomicBool>,
    record: Option<PathBuf>,
    status: Arc<Mutex<RadioStatus>>,
) {
    let update = |f: &dyn Fn(&mut RadioStatus)| f(&mut status.lock().unwrap_or_else(PoisonError::into_inner));
    let fail = |detail: String| {
        update(&|s| {
            s.state = "failed";
            s.detail = detail.clone();
            s.errors += 1;
            s.last_error = Some(detail.clone());
        })
    };
    let poll = Duration::from_secs_f64(config.poll_seconds.max(0.5));
    let target = format!("{}:{}", config.host, config.port);

    while !stop.load(Ordering::Relaxed) {
        // The daemon, when this app runs it. Dropping it at the end of a
        // pass stops it, so a restart begins clean.
        let mut daemon = None;
        let mut already_running = false;
        if config.start_rigctld {
            update(&|s| {
                s.state = "connecting";
                s.detail = "Starting rigctld…".into();
                s.daemon = None;
            });
            if let Some(adopted) = record.as_deref().and_then(|r| daemon::Daemon::adopt(r, keep.clone())) {
                // Left running by an earlier session: take it back.
                daemon = Some(adopted);
            } else if connect(&config).is_ok() {
                // Another rigctld already answers on the port; starting a
                // second would fail, so use this one.
                already_running = true;
            } else {
                match daemon::Daemon::start_recorded(&config, record.as_deref(), keep.clone()) {
                    Ok(started) => daemon = Some(started),
                    Err(e) => {
                        fail(e);
                        if !pause(&stop, RETRY) {
                            break;
                        }
                        continue;
                    }
                }
            }
        }
        let daemon_status = |daemon: &mut Option<daemon::Daemon>| daemon.as_mut().map(|d| d.status());

        update(&|s| {
            s.state = "connecting";
            s.detail = format!("Connecting to rigctld at {target}…");
        });
        let attempts = if daemon.is_some() { STARTUP_ATTEMPTS } else { 1 };
        let mut radio = None;
        for attempt in 1..=attempts {
            let d = daemon_status(&mut daemon);
            update(&|s| s.daemon = d.clone());
            if let Some(d) = &d {
                if !d.running {
                    break;
                }
            }
            match connect(&config) {
                Ok(connected) => {
                    radio = Some(connected);
                    break;
                }
                Err(e) if attempt == attempts || d.as_ref().is_some_and(|d| !d.running) => {
                    fail(e);
                    break;
                }
                Err(_) => {
                    if !pause(&stop, Duration::from_secs(1)) {
                        return;
                    }
                }
            }
        }
        let Some(mut radio) = radio else {
            if let Some(d) = daemon_status(&mut daemon) {
                if !d.running {
                    fail(format!(
                        "rigctld exited with code {}: {}",
                        d.exit_code.unwrap_or(-1),
                        d.output.lines().last().unwrap_or("no output")
                    ));
                }
                update(&|s| s.daemon = Some(d.clone()));
            }
            if !pause(&stop, RETRY) {
                break;
            }
            continue;
        };

        let name = radio.name().to_string();
        update(&|s| {
            s.state = "connected";
            s.detail = if already_running {
                format!("Reading {name} every {} s. It was already running, so this app did not start it.", config.poll_seconds)
            } else {
                format!("Reading {name} every {} s.", config.poll_seconds)
            };
        });
        loop {
            if let Some(d) = daemon_status(&mut daemon) {
                update(&|s| s.daemon = Some(d.clone()));
                if !d.running {
                    fail(format!(
                        "rigctld exited with code {}: {}; restarting.",
                        d.exit_code.unwrap_or(-1),
                        d.output.lines().last().unwrap_or("no output")
                    ));
                    break;
                }
            }
            match radio.read() {
                Ok(state) => update(&|s| {
                    s.radio = Some(state.clone());
                    s.read_utc = Some(timeutil::now());
                    s.reads += 1;
                }),
                Err(e) => {
                    fail(format!("{e}; reconnecting."));
                    break;
                }
            }
            if !serve(radio.as_mut(), &inbox, poll, &stop, &status) {
                return;
            }
        }
        if !pause(&stop, RETRY) {
            break;
        }
    }
}
