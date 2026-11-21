// SPDX-License-Identifier: MIT OR Apache-2.0
//! The readers: a receiver on a serial port (USB, or Bluetooth paired as a serial port), a phone or
//! program sending NMEA over TCP or UDP (GPS2IP on an iPhone, Share GPS on Android, …), or gpsd.
//! Each runs on a thread of its own, keeps a [`Tracker`], and opens its source again after a
//! failure. [`Gps::status`] says what it is doing.
//!
//! Sockets are read with plain blocking reads, never with a read timeout: on Windows a read whose
//! timeout runs out can fail with os error 997 and lose data. A TCP reader is stopped by shutting
//! its socket down, a UDP reader by a datagram to itself; on Windows a shutdown may not end a read
//! blocked on another copy of the socket, so a TCP reader stopped there ends at its next line.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Shutdown, SocketAddr, TcpStream, ToSocketAddrs, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::track::{Fix, Freshness, Hearing, Tracker};
use crate::{gpsd, nmea};

/// Where a GPS's sentences come from.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Source {
    /// A receiver on a serial port (`COM5`, `/dev/ttyUSB0`, `/dev/cu.usbmodem1101`): its speed, or
    /// none to find it among [`BAUDS`].
    Serial {
        port: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        baud: Option<u32>,
    },
    /// NMEA from a phone or program that serves it on a TCP port (GPS2IP's socket mode: port 11123
    /// in its examples, `172.20.10.1` over the iPhone's own hotspot).
    Tcp { host: String, port: u16 },
    /// NMEA datagrams sent to this computer's UDP port (GPS2IP's UDP mode, unicast).
    Udp { port: u16 },
    /// gpsd, where it already holds the receiver (Linux).
    Gpsd {
        #[serde(default = "localhost")]
        host: String,
        #[serde(default = "gpsd_port")]
        port: u16,
    },
}

fn localhost() -> String {
    "localhost".into()
}

fn gpsd_port() -> u16 {
    gpsd::PORT
}

impl Source {
    /// In words, for the operator.
    #[must_use]
    pub fn describe(&self) -> String {
        match self {
            Source::Serial {
                port,
                baud: Some(b),
            } => format!("{port} at {b} baud"),
            Source::Serial { port, baud: None } => format!("{port}, speed found"),
            Source::Tcp { host, port } => format!("TCP {host}:{port}"),
            Source::Udp { port } => format!("UDP port {port}"),
            Source::Gpsd { host, port } => format!("gpsd at {host}:{port}"),
        }
    }
}

/// The speeds tried, in this order, to find a serial receiver's.
pub const BAUDS: [u32; 4] = [4800, 9600, 38_400, 115_200];
/// How long a source that failed or closed waits before it is opened again.
pub const RETRY: Duration = Duration::from_secs(5);
/// How long each speed is listened to when finding one.
const LISTEN_EACH: Duration = Duration::from_millis(2500);
/// Valid sentences heard at a speed before it is taken.
const FOUND_AFTER: usize = 2;
/// The longest line kept; longer means the wrong speed or not NMEA.
const LONGEST: usize = 4096;

/// The connection to the source.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "state", rename_all = "kebab-case")]
pub enum Link {
    /// Being opened (or a serial port's speed being found).
    Opening,
    /// Open since `since_ms`; `detail` says how (the speed found).
    Open {
        since_ms: u64,
        #[serde(skip_serializing_if = "Option::is_none")]
        detail: Option<String>,
    },
    /// It could not be opened, or it closed; opened again after [`RETRY`].
    Failed { since_ms: u64, text: String },
}

/// What a GPS is doing, at one moment.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Status {
    pub source: Source,
    pub link: Link,
    pub hearing: Hearing,
    /// The last fix accepted, of any age.
    pub fix: Option<Fix>,
    pub freshness: Option<Freshness>,
    pub age_s: Option<u64>,
    /// A fix too far from the last to be believed is held until another confirms it.
    pub held: bool,
    /// Lines refused: a bad checksum, or a sentence that cannot be read.
    pub refused: u64,
}

impl Status {
    /// The fix, when it may be the present position (current or recent).
    #[must_use]
    pub fn usable_fix(&self) -> Option<Fix> {
        self.fix
            .filter(|_| self.freshness.is_some_and(Freshness::usable))
    }
}

/// Now, in milliseconds since 1970.
#[must_use]
pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

struct Shared {
    tracker: Tracker,
    link: Link,
    /// What ends a read blocked on the source, while one is open.
    wake: Option<Wake>,
}

enum Wake {
    Tcp(TcpStream),
    Udp(SocketAddr),
}

type Held = Arc<Mutex<Shared>>;

fn lock(shared: &Mutex<Shared>) -> MutexGuard<'_, Shared> {
    shared.lock().unwrap_or_else(PoisonError::into_inner)
}

/// A GPS being read. Dropping it stops the reader.
pub struct Gps {
    source: Source,
    shared: Held,
    stop: Arc<AtomicBool>,
}

impl Gps {
    /// Starts reading `source` on a thread of its own.
    #[must_use]
    pub fn start(source: Source) -> Gps {
        let shared = Arc::new(Mutex::new(Shared {
            tracker: Tracker::new(),
            link: Link::Opening,
            wake: None,
        }));
        let stop = Arc::new(AtomicBool::new(false));
        let (s, st, src) = (Arc::clone(&shared), Arc::clone(&stop), source.clone());
        thread::spawn(move || run(&src, &s, &st));
        Gps {
            source,
            shared,
            stop,
        }
    }

    /// What it is reading.
    #[must_use]
    pub fn source(&self) -> &Source {
        &self.source
    }

    /// What it is doing now.
    #[must_use]
    pub fn status(&self) -> Status {
        self.status_at(now_ms())
    }

    /// What it is doing at `now_ms`.
    #[must_use]
    pub fn status_at(&self, now_ms: u64) -> Status {
        let g = lock(&self.shared);
        let fix = g.tracker.fix();
        Status {
            source: self.source.clone(),
            link: g.link.clone(),
            hearing: g.tracker.hearing(now_ms),
            fix,
            freshness: fix.map(|f| f.freshness(now_ms)),
            age_s: fix.map(|f| f.age_s(now_ms)),
            held: g.tracker.held().is_some(),
            refused: g.tracker.refused_count(),
        }
    }

    /// Stops the reader.
    pub fn stop(&self) {
        if self.stop.swap(true, Ordering::SeqCst) {
            return;
        }
        match lock(&self.shared).wake.take() {
            Some(Wake::Tcp(s)) => {
                let _ = s.shutdown(Shutdown::Both);
            }
            Some(Wake::Udp(at)) => {
                let to = SocketAddr::from(([127, 0, 0, 1], at.port()));
                if let Ok(s) = UdpSocket::bind(("127.0.0.1", 0)) {
                    let _ = s.send_to(b"", to);
                }
            }
            None => {}
        }
    }
}

impl Drop for Gps {
    fn drop(&mut self) {
        self.stop();
    }
}

/// The serial ports this computer has, by name, for the operator to pick from.
#[must_use]
pub fn serial_ports() -> Vec<String> {
    serialport::available_ports()
        .map(|ports| ports.into_iter().map(|p| p.port_name).collect())
        .unwrap_or_default()
}

fn run(source: &Source, shared: &Held, stop: &AtomicBool) {
    while !stop.load(Ordering::SeqCst) {
        set_link(shared, Link::Opening);
        let ended = match source {
            Source::Serial { port, baud } => serial(port, *baud, shared, stop),
            Source::Tcp { host, port } => tcp((host.as_str(), *port), None, shared, stop),
            Source::Udp { port } => udp(*port, shared, stop),
            Source::Gpsd { host, port } => {
                tcp((host.as_str(), *port), Some(gpsd::WATCH), shared, stop)
            }
        };
        lock(shared).wake = None;
        if stop.load(Ordering::SeqCst) {
            return;
        }
        set_link(
            shared,
            Link::Failed {
                since_ms: now_ms(),
                text: ended,
            },
        );
        let until = Instant::now() + RETRY;
        while Instant::now() < until && !stop.load(Ordering::SeqCst) {
            thread::sleep(Duration::from_millis(100));
        }
    }
}

fn set_link(shared: &Held, link: Link) {
    lock(shared).link = link;
}

fn opened(shared: &Held, detail: Option<String>) {
    set_link(
        shared,
        Link::Open {
            since_ms: now_ms(),
            detail,
        },
    );
}

/// Takes a piece of NMEA text: every sentence in it.
fn take_nmea(shared: &Held, text: &str) {
    let now = now_ms();
    let mut g = lock(shared);
    for sentence in nmea::sentences(text) {
        match nmea::read(sentence) {
            Ok(said) => g.tracker.take(said, now),
            Err(_) => g.tracker.refused(),
        }
    }
}

fn take_gpsd(shared: &Held, line: &str) {
    lock(shared).tracker.take(gpsd::read(line), now_ms());
}

/// Bytes into lines: what a serial port or a stream gives arrives in pieces.
#[derive(Default)]
struct Lines {
    pending: Vec<u8>,
}

impl Lines {
    fn push(&mut self, bytes: &[u8]) -> Vec<String> {
        self.pending.extend_from_slice(bytes);
        let mut lines = Vec::new();
        while let Some(end) = self.pending.iter().position(|&b| b == b'\n') {
            let line: Vec<u8> = self.pending.drain(..=end).collect();
            lines.push(String::from_utf8_lossy(&line).trim().to_owned());
        }
        if self.pending.len() > LONGEST {
            self.pending.clear();
        }
        lines
    }
}

/// How many valid sentences a piece of text holds: how a serial speed is judged.
#[must_use]
pub fn valid_sentences(text: &str) -> usize {
    nmea::sentences(text)
        .filter(|s| nmea::read(s).is_ok())
        .count()
}

fn serial(name: &str, baud: Option<u32>, shared: &Held, stop: &AtomicBool) -> String {
    let found = match baud {
        Some(b) => open_serial(name, b).map(|p| (p, b)),
        None => find_speed(name, stop),
    };
    let (mut port, baud) = match found {
        Ok(found) => found,
        Err(text) => return text,
    };
    opened(shared, Some(format!("{baud} baud")));
    let mut lines = Lines::default();
    let mut buf = [0u8; 1024];
    while !stop.load(Ordering::SeqCst) {
        match port.read(&mut buf) {
            Ok(0) => return format!("{name} closed"),
            Ok(n) => {
                for line in lines.push(&buf[..n]) {
                    take_nmea(shared, &line);
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::TimedOut => {}
            Err(e) => return format!("{name} failed: {e}"),
        }
    }
    String::new()
}

fn open_serial(name: &str, baud: u32) -> Result<Box<dyn serialport::SerialPort>, String> {
    let mut port = serialport::new(name, baud)
        .timeout(Duration::from_millis(500))
        .open()
        .map_err(|e| format!("cannot open {name}: {e}"))?;
    // some Bluetooth receivers send nothing until DTR is raised
    let _ = port.write_data_terminal_ready(true);
    Ok(port)
}

/// Tries each of [`BAUDS`] and keeps the first that gives valid sentences.
fn find_speed(
    name: &str,
    stop: &AtomicBool,
) -> Result<(Box<dyn serialport::SerialPort>, u32), String> {
    let mut last = String::new();
    for baud in BAUDS {
        let mut port = match open_serial(name, baud) {
            Ok(p) => p,
            Err(e) => {
                last = e;
                continue;
            }
        };
        let until = Instant::now() + LISTEN_EACH;
        let (mut heard, mut buf) = (String::new(), [0u8; 512]);
        while Instant::now() < until && !stop.load(Ordering::SeqCst) {
            if let Ok(n) = port.read(&mut buf) {
                heard.push_str(&String::from_utf8_lossy(&buf[..n]));
                if valid_sentences(&heard) >= FOUND_AFTER {
                    return Ok((port, baud));
                }
            }
        }
    }
    if last.is_empty() {
        Err(format!("no NMEA from {name} at any speed tried"))
    } else {
        Err(last)
    }
}

fn tcp(at: (&str, u16), say: Option<&str>, shared: &Held, stop: &AtomicBool) -> String {
    let addrs: Vec<SocketAddr> = match at.to_socket_addrs() {
        Ok(a) => a.collect(),
        Err(e) => return format!("cannot find {}: {e}", at.0),
    };
    let Some(mut stream) = addrs
        .iter()
        .find_map(|a| TcpStream::connect_timeout(a, Duration::from_secs(5)).ok())
    else {
        return format!("cannot connect to {}:{}", at.0, at.1);
    };
    if let Some(say) = say {
        if let Err(e) = stream.write_all(say.as_bytes()) {
            return format!("{}:{} failed: {e}", at.0, at.1);
        }
    }
    match stream.try_clone() {
        Ok(c) => lock(shared).wake = Some(Wake::Tcp(c)),
        Err(e) => return format!("{}:{} failed: {e}", at.0, at.1),
    }
    if stop.load(Ordering::SeqCst) {
        return String::new();
    }
    opened(shared, None);
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) => return "the connection closed: the phone or program stopped sending".into(),
            Ok(_) if stop.load(Ordering::SeqCst) => return String::new(),
            Ok(_) if say.is_some() => take_gpsd(shared, &line),
            Ok(_) => take_nmea(shared, &line),
            Err(e) => return format!("{}:{} failed: {e}", at.0, at.1),
        }
    }
}

fn udp(port: u16, shared: &Held, stop: &AtomicBool) -> String {
    let socket = match UdpSocket::bind(("0.0.0.0", port)) {
        Ok(s) => s,
        Err(e) => return format!("cannot listen on UDP port {port}: {e}"),
    };
    if let Ok(at) = socket.local_addr() {
        lock(shared).wake = Some(Wake::Udp(at));
    }
    if stop.load(Ordering::SeqCst) {
        return String::new();
    }
    opened(shared, None);
    let mut buf = [0u8; 2048];
    loop {
        match socket.recv_from(&mut buf) {
            Ok(_) if stop.load(Ordering::SeqCst) => return String::new(),
            Ok((n, _)) => take_nmea(shared, &String::from_utf8_lossy(&buf[..n])),
            Err(e) => return format!("UDP port {port} failed: {e}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pieces_become_lines() {
        let mut l = Lines::default();
        assert!(l.push(b"$GPGGA,1").is_empty());
        assert_eq!(l.push(b"23\r\n$GP"), vec!["$GPGGA,123".to_owned()]);
        assert_eq!(l.push(b"RMC\n\n"), vec!["$GPRMC".to_owned(), String::new()]);
        // a speed that gives no line ends keeps nothing past the longest line
        l.push(&[b'x'; LONGEST + 1]);
        assert!(l.pending.is_empty());
    }

    #[test]
    fn a_speed_is_judged_by_its_valid_sentences() {
        let good = format!(
            "{}\r\n{}\r\n",
            nmea::with_checksum("GPGGA,031522,3345.123,N,08423.456,W,1,08,0.9,320.5,M,,M,,"),
            nmea::with_checksum("GPGSV,1,1,00")
        );
        assert_eq!(valid_sentences(&good), 2);
        // what a receiver at 9600 baud looks like read at 4800
        assert_eq!(
            valid_sentences("\u{fffd}\u{fffd}x\u{fffd}$\u{fffd}\r\n\u{fffd}"),
            0
        );
    }

    #[test]
    fn sources_are_described_and_kept_as_json() {
        let s = Source::Tcp {
            host: "172.20.10.1".into(),
            port: 11123,
        };
        assert_eq!(s.describe(), "TCP 172.20.10.1:11123");
        let json = serde_json::to_string(&s).expect("json");
        assert_eq!(json, r#"{"kind":"tcp","host":"172.20.10.1","port":11123}"#);
        let gpsd: Source = serde_json::from_str(r#"{"kind":"gpsd"}"#).expect("defaults");
        assert_eq!(
            gpsd,
            Source::Gpsd {
                host: "localhost".into(),
                port: 2947
            }
        );
        let serial: Source =
            serde_json::from_str(r#"{"kind":"serial","port":"COM5"}"#).expect("find the speed");
        assert_eq!(serial.describe(), "COM5, speed found");
    }
}
