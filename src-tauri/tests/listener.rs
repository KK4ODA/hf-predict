//! Sends WSJT-X datagrams over real UDP sockets to a running listener.

use std::net::{Ipv4Addr, UdpSocket};
use std::sync::Arc;
use std::time::{Duration, Instant};

use hf_predict_lib::observations::Database;
use hf_predict_lib::wsjtx::listener::{Listener, ListenerConfig, ListenerStatus};
use hf_predict_lib::wsjtx::protocol::{encode, Decode, Status};

const ID: &str = "WSJT-X";

fn free_port() -> u16 {
    UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap().local_addr().unwrap().port()
}

fn status() -> Status {
    Status {
        dial_hz: 14_074_000,
        mode: "FT8".into(),
        tx_enabled: false,
        transmitting: false,
        decoding: false,
        de_call: Some("N0CALL".into()),
        de_grid: Some("EM73".into()),
        tr_period_s: Some(15),
    }
}

/// A decode timed at the current UTC second, so it resolves to today.
fn decode(message: &str) -> Decode {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
    Decode {
        new: true,
        time_ms: ((now % 86_400) * 1000) as u32,
        snr_db: -10,
        dt_s: 0.2,
        df_hz: 1500,
        mode: "~".into(),
        message: message.into(),
        low_confidence: false,
        off_air: false,
    }
}

fn wait_for(listener: &Listener, done: impl Fn(&ListenerStatus) -> bool) -> ListenerStatus {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let status = listener.status();
        if done(&status) || Instant::now() > deadline {
            return status;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn send_session(sender: &UdpSocket, to: (Ipv4Addr, u16)) {
    for datagram in [
        encode::heartbeat(ID, "3.0.2"),
        encode::status(ID, &status()),
        encode::decode(ID, &decode("CQ K1ABC FN42")),
        b"not a wsjt-x datagram".to_vec(),
        encode::decode(ID, &decode("K1ABC W9XYZ EN37")),
    ] {
        sender.send_to(&datagram, to).unwrap();
    }
}

#[test]
fn receives_and_stores_decodes_sent_to_its_port() {
    let db = Arc::new(Database::in_memory().unwrap());
    let port = free_port();
    let config = ListenerConfig { enabled: true, address: "127.0.0.1".into(), port };
    let listener = Listener::start(config, db.clone());
    assert_eq!(listener.status().state, "listening", "{}", listener.status().detail);

    let sender = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    send_session(&sender, (Ipv4Addr::LOCALHOST, port));

    let status = wait_for(&listener, |s| s.datagrams >= 5);
    assert_eq!((status.datagrams, status.not_understood), (5, 1));
    assert_eq!(status.last_error, None);
    let tracker = status.tracker.unwrap();
    assert_eq!(tracker.stored, 2);
    assert_eq!(tracker.decoders.len(), 1);
    assert_eq!(tracker.decoders[0].version.as_deref(), Some("3.0.2"));
    assert_eq!(tracker.decoders[0].band.as_deref(), Some("20 m"));
    assert_eq!(tracker.decoders[0].de_grid.as_deref(), Some("EM73"));

    let stored = db.all().unwrap();
    let senders: Vec<Option<&str>> = stored.iter().map(|o| o.sender.as_deref()).collect();
    assert!(senders.contains(&Some("K1ABC")) && senders.contains(&Some("W9XYZ")), "{senders:?}");

    // Stopping the listener frees the port.
    drop(listener);
    assert!(UdpSocket::bind((Ipv4Addr::LOCALHOST, port)).is_ok());
}

#[test]
fn a_bad_address_is_reported_not_fatal() {
    let db = Arc::new(Database::in_memory().unwrap());
    let config = ListenerConfig { enabled: true, address: "not an address".into(), port: 2237 };
    let status = Listener::start(config, db).status();
    assert_eq!(status.state, "failed");
    assert!(status.detail.contains("not an IPv4 address"), "{}", status.detail);
}

/// Two programs joined to the same multicast group both receive everything,
/// which is how this app shares WSJT-X with GridTracker or JTAlert. Ignored
/// by default because it needs multicast on the loopback interface, which
/// not every build machine has.
#[test]
#[ignore]
fn two_listeners_share_one_multicast_group() {
    let group = Ipv4Addr::new(239, 255, 0, 77);
    let port = free_port();
    let config = ListenerConfig { enabled: true, address: group.to_string(), port };
    let (first_db, second_db) =
        (Arc::new(Database::in_memory().unwrap()), Arc::new(Database::in_memory().unwrap()));
    let first = Listener::start(config.clone(), first_db.clone());
    let second = Listener::start(config, second_db.clone());
    assert_eq!(first.status().state, "listening", "{}", first.status().detail);
    assert_eq!(second.status().state, "listening", "{}", second.status().detail);

    let sender = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    sender.set_multicast_loop_v4(true).unwrap();
    send_session(&sender, (group, port));

    assert_eq!(wait_for(&first, |s| s.datagrams >= 5).datagrams, 5);
    assert_eq!(wait_for(&second, |s| s.datagrams >= 5).datagrams, 5);
    assert_eq!(first_db.count().unwrap(), 2);
    assert_eq!(second_db.count().unwrap(), 2);
}
