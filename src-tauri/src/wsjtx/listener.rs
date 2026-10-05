//! Receives WSJT-X's UDP messages on a background thread and feeds the tracker.
//!
//! The socket only ever receives. WSJT-X can be told to send to a multicast
//! group, which every listening program joins, or to a single address, which
//! only one program can receive.

use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::JoinHandle;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use socket2::{Domain, Protocol, Socket, Type};

use super::protocol;
use super::tracker::{Tracker, TrackerStatus};
use crate::observations::Database;
use crate::timeutil;

/// How often the thread wakes with nothing received, to notice a stop
/// request and decoders that have gone quiet.
const WAKE_EVERY: Duration = Duration::from_millis(500);
const MAX_DATAGRAM: usize = 4096;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ListenerConfig {
    pub enabled: bool,
    /// A multicast group to join, or a local address to receive on.
    pub address: String,
    pub port: u16,
}

impl Default for ListenerConfig {
    /// Off until the operator turns it on: receiving on a port another
    /// program already uses could take that program's messages.
    fn default() -> Self {
        Self { enabled: false, address: "224.0.0.1".into(), port: 2237 }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListenerStatus {
    pub config: ListenerConfig,
    /// `off`, `listening` or `failed`.
    pub state: &'static str,
    /// What is being listened on, or why it failed.
    pub detail: String,
    pub datagrams: u64,
    /// Datagrams that were not WSJT-X messages.
    pub not_understood: u64,
    /// The most recent problem storing data, if any.
    pub last_error: Option<String>,
    pub tracker: Option<TrackerStatus>,
}

impl ListenerStatus {
    pub fn off(config: ListenerConfig) -> Self {
        Self {
            config,
            state: "off",
            detail: "Not listening.".into(),
            datagrams: 0,
            not_understood: 0,
            last_error: None,
            tracker: None,
        }
    }
}

/// Opens the receiving socket. Address reuse lets other programs listen on
/// the same port, which multicast sharing depends on.
fn open_socket(config: &ListenerConfig) -> Result<(UdpSocket, String), String> {
    let address: Ipv4Addr = config
        .address
        .trim()
        .parse()
        .map_err(|_| format!("'{}' is not an IPv4 address", config.address))?;
    let socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP)).map_err(|e| e.to_string())?;
    socket.set_reuse_address(true).map_err(|e| e.to_string())?;
    #[cfg(all(unix, not(target_os = "solaris"), not(target_os = "illumos")))]
    socket.set_reuse_port(true).map_err(|e| e.to_string())?;

    let (bind_to, detail) = if address.is_multicast() {
        (Ipv4Addr::UNSPECIFIED, format!("multicast group {address}, port {}", config.port))
    } else {
        (address, format!("{address}, port {}", config.port))
    };
    socket
        .bind(&SocketAddr::V4(SocketAddrV4::new(bind_to, config.port)).into())
        .map_err(|e| format!("cannot listen on port {}: {e}", config.port))?;

    if address.is_multicast() {
        // WSJT-X sends multicast on the loopback interface by default.
        // Joining there and on the default interface covers both setups.
        let joined = [Ipv4Addr::LOCALHOST, Ipv4Addr::UNSPECIFIED]
            .iter()
            .filter(|interface| socket.join_multicast_v4(&address, interface).is_ok())
            .count();
        if joined == 0 {
            return Err(format!("cannot join multicast group {address}"));
        }
    }
    socket.set_read_timeout(Some(WAKE_EVERY)).map_err(|e| e.to_string())?;
    Ok((socket.into(), detail))
}

pub struct Listener {
    stop: Arc<AtomicBool>,
    status: Arc<Mutex<ListenerStatus>>,
    thread: Option<JoinHandle<()>>,
}

impl Listener {
    /// Starts listening. A socket that cannot be opened gives a listener in
    /// the `failed` state, not an error, so the reason reaches the screen.
    pub fn start(config: ListenerConfig, db: Arc<Database>) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let status = Arc::new(Mutex::new(ListenerStatus::off(config.clone())));
        let set = |state, detail: String| {
            let mut status = status.lock().unwrap_or_else(PoisonError::into_inner);
            status.state = state;
            status.detail = detail;
        };

        let thread = match open_socket(&config) {
            Err(e) => {
                set("failed", e);
                None
            }
            Ok((socket, detail)) => {
                set("listening", format!("Listening on {detail}."));
                let (stop, status) = (stop.clone(), status.clone());
                Some(std::thread::spawn(move || run(socket, db, stop, status)))
            }
        };
        Self { stop, status, thread }
    }

    pub fn status(&self) -> ListenerStatus {
        self.status.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }
}

impl Drop for Listener {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn run(socket: UdpSocket, db: Arc<Database>, stop: Arc<AtomicBool>, status: Arc<Mutex<ListenerStatus>>) {
    let mut tracker = Tracker::new(db);
    let mut buffer = [0u8; MAX_DATAGRAM];
    while !stop.load(Ordering::Relaxed) {
        let received = socket.recv(&mut buffer);
        let now = timeutil::now();
        let mut datagram = 0;
        let mut not_understood = 0;
        let mut error = None;
        match received {
            Ok(length) => {
                datagram = 1;
                match protocol::parse(&buffer[..length]) {
                    Ok(message) => error = tracker.handle(message, now).err(),
                    Err(_) => not_understood = 1,
                }
            }
            // A timeout is the regular wake-up; other errors are reported.
            Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {}
            Err(e) => error = Some(format!("receiving: {e}")),
        }
        error = error.or(tracker.tick(now).err());

        let mut status = status.lock().unwrap_or_else(PoisonError::into_inner);
        status.datagrams += datagram;
        status.not_understood += not_understood;
        status.last_error = error.or(status.last_error.take());
        status.tracker = Some(tracker.status(now));
    }
}
