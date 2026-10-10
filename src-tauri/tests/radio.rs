//! Reads a stand-in `rigctld` over real TCP: a server that answers the
//! extended protocol with canned replies and records every command it
//! receives, so the test can check that the app only ever asks.

use std::io::{BufRead, BufReader, Write};
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use hf_predict_lib::radio::rigctld::Rigctld;
use hf_predict_lib::radio::{Monitor, RadioConfig, RadioController, RadioStatus};

struct FakeRigctld {
    port: u16,
    commands: Arc<Mutex<Vec<String>>>,
    stop: Arc<AtomicBool>,
}

impl FakeRigctld {
    fn start(freq_reply: &'static str) -> Self {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        listener.set_nonblocking(true).unwrap();
        let port = listener.local_addr().unwrap().port();
        let commands = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let (recorded, stopping) = (commands.clone(), stop.clone());
        std::thread::spawn(move || {
            while !stopping.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        let (recorded, stopping) = (recorded.clone(), stopping.clone());
                        std::thread::spawn(move || serve(stream, recorded, stopping, freq_reply));
                    }
                    Err(_) => std::thread::sleep(Duration::from_millis(20)),
                }
            }
        });
        Self { port, commands, stop }
    }

    fn commands(&self) -> Vec<String> {
        self.commands.lock().unwrap().clone()
    }
}

impl Drop for FakeRigctld {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

fn serve(stream: TcpStream, recorded: Arc<Mutex<Vec<String>>>, stop: Arc<AtomicBool>, freq_reply: &str) {
    // On Windows an accepted socket inherits the listener's non-blocking mode.
    stream.set_nonblocking(false).unwrap();
    let mut writer = stream.try_clone().unwrap();
    let reader = BufReader::new(stream);
    for line in reader.lines() {
        if stop.load(Ordering::Relaxed) {
            return;
        }
        let Ok(line) = line else { return };
        recorded.lock().unwrap().push(line.clone());
        let reply = match line.as_str() {
            "+\\get_freq" => freq_reply,
            "+\\get_mode" => "get_mode:\nMode: USB\nPassband: 3000\nRPRT 0\n",
            "+\\get_ptt" => "get_ptt:\nPTT: 0\nRPRT 0\n",
            "+\\get_split_vfo" => "get_split_vfo:\nSplit: 0\nTX VFO: VFOA\nRPRT 0\n",
            "+\\get_vfo" => "get_vfo:\nVFO: VFOA\nRPRT 0\n",
            _ => "RPRT -1\n",
        };
        if writer.write_all(reply.as_bytes()).is_err() {
            return;
        }
    }
}

fn wait_for(monitor: &Monitor, condition: impl Fn(&RadioStatus) -> bool, seconds: u64) -> RadioStatus {
    let deadline = Instant::now() + Duration::from_secs(seconds);
    loop {
        let status = monitor.status();
        if condition(&status) || Instant::now() > deadline {
            return status;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

const FREQ_OK: &str = "get_freq:\nFrequency: 14074000\nRPRT 0\n";

#[test]
fn reads_the_radio_and_only_asks() {
    let server = FakeRigctld::start(FREQ_OK);
    let mut client = Rigctld::connect("127.0.0.1", server.port).unwrap();

    let state = client.read().unwrap();
    assert_eq!((state.freq_hz, state.band.as_str(), state.mode.as_str()), (14_074_000, "20 m", "USB"));
    assert_eq!((state.passband_hz, state.ptt, state.split), (Some(3000), false, Some(false)));
    assert_eq!((state.vfo.as_deref(), state.tx_vfo.as_deref()), (Some("VFOA"), Some("VFOA")));

    let commands = server.commands();
    assert_eq!(commands.len(), 5);
    assert!(commands.iter().all(|c| c.starts_with("+\\get_")), "{commands:?}");
}

#[test]
fn a_radio_error_is_reported_by_name() {
    let server = FakeRigctld::start("get_freq:\nRPRT -5\n");
    let mut client = Rigctld::connect("127.0.0.1", server.port).unwrap();
    let error = client.read().unwrap_err();
    assert_eq!(error, "get_freq: communication with the radio timed out (RPRT -5)");
}

#[test]
fn the_monitor_connects_reads_and_reports_a_lost_server() {
    let server = FakeRigctld::start(FREQ_OK);
    let config = RadioConfig { enabled: true, host: "127.0.0.1".into(), port: server.port, poll_seconds: 0.5, ..RadioConfig::default() };
    let monitor = Monitor::start(config);

    let status = wait_for(&monitor, |s| s.reads >= 2, 5);
    assert_eq!(status.state, "connected", "{}", status.detail);
    assert_eq!(status.radio.as_ref().map(|r| r.freq_hz), Some(14_074_000));
    assert!(status.read_utc.is_some());

    drop(server);
    let status = wait_for(&monitor, |s| s.state == "failed", 10);
    assert_eq!(status.state, "failed", "{}", status.detail);
    assert!(status.errors >= 1 && status.last_error.is_some());
}

#[test]
fn no_server_is_a_failure_not_a_panic() {
    let port = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap().local_addr().unwrap().port();
    let monitor = Monitor::start(RadioConfig { enabled: true, host: "127.0.0.1".into(), port, poll_seconds: 1.0, ..RadioConfig::default() });
    let status = wait_for(&monitor, |s| s.state == "failed", 10);
    assert_eq!(status.state, "failed");
    assert!(status.detail.starts_with("cannot connect to rigctld"), "{}", status.detail);
}

/// Starts the real `rigctld` named by `HFP_RIGCTLD` with Hamlib's dummy radio:
/// `HFP_RIGCTLD="C:/Program Files/hamlib-w64-4.7.1/bin/rigctld.exe" cargo test --test radio -- --ignored --nocapture`.
#[test]
#[ignore]
fn the_app_can_start_rigctld_itself() {
    let Ok(program) = std::env::var("HFP_RIGCTLD") else {
        return;
    };
    let port = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap().local_addr().unwrap().port();
    let config = RadioConfig {
        enabled: true,
        host: "127.0.0.1".into(),
        port,
        poll_seconds: 0.5,
        start_rigctld: true,
        rigctld_path: program,
        rig_model: 1,
        serial_port: String::new(),
        baud: 38_400,
        ..RadioConfig::default()
    };
    let monitor = Monitor::start(config);
    let status = wait_for(&monitor, |s| s.reads >= 2 || s.state == "failed", 20);
    println!("{} | daemon: {:?}", status.detail, status.daemon);
    assert_eq!(status.state, "connected", "{}", status.detail);
    let radio = status.radio.expect("a reading");
    println!("dummy radio reports {} Hz {} ptt {}", radio.freq_hz, radio.mode, radio.ptt);
    let daemon = status.daemon.expect("a daemon");
    assert!(daemon.running);
    assert!(daemon.command.contains(" -m 1 -T 127.0.0.1 -t "), "{}", daemon.command);
    drop(monitor);
}
