// SPDX-License-Identifier: MIT OR Apache-2.0
//! The readers against stand-ins: a phone serving NMEA over TCP that then stops, datagrams to the
//! UDP reader, a gpsd that must be asked with `?WATCH`, and (on Linux and macOS) a serial port
//! that is a pseudo-terminal, its speed found.

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, UdpSocket};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use radioprop_place::{gpsd, Gps, Hearing, Link, Source, Status};

const GPS2IP: &str = include_str!("streams/gps2ip.nmea");
const RECEIVER: &str = include_str!("streams/usb-receiver.nmea");
const GPSD: &str = include_str!("streams/gpsd.jsonl");

/// Waits up to five seconds for the GPS's status to pass `test`.
fn until(gps: &Gps, what: &str, test: impl Fn(&Status) -> bool) -> Status {
    let start = Instant::now();
    loop {
        let s = gps.status();
        if test(&s) {
            return s;
        }
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "waited for {what}: {s:?}"
        );
        thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn a_phone_over_tcp_gives_a_fix_and_its_stopping_is_said() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("a port");
    let port = listener.local_addr().expect("bound").port();
    let (done_tx, done_rx) = mpsc::channel::<()>();
    thread::spawn(move || {
        let (mut phone, _) = listener.accept().expect("the reader connects");
        // in pieces that cut sentences in two
        for piece in GPS2IP.as_bytes().chunks(37) {
            phone.write_all(piece).expect("sent");
            thread::sleep(Duration::from_millis(2));
        }
        let _ = done_rx.recv();
        // then the phone stops (GPS2IP Lite, after four minutes)
    });
    let gps = Gps::start(Source::Tcp {
        host: "127.0.0.1".into(),
        port,
    });
    let s = until(&gps, "a fix", |s| {
        s.fix.is_some_and(|f| f.alt_m.is_some()) && s.hearing == Hearing::Positions
    });
    assert!(matches!(s.link, Link::Open { .. }));
    assert_eq!(s.refused, 0);
    assert!(s.usable_fix().is_some());
    done_tx.send(()).expect("the phone is told to stop");
    let s = until(&gps, "the closing said", |s| {
        matches!(s.link, Link::Failed { .. })
    });
    let Link::Failed { text, .. } = s.link else {
        unreachable!()
    };
    assert_eq!(
        text,
        "the connection closed: the phone or program stopped sending"
    );
    // the fix stays, aging, for the choice of position to judge
    assert!(s.fix.is_some());
}

#[test]
fn udp_datagrams_give_a_fix_and_a_stopped_reader_lets_the_port_go() {
    let free = UdpSocket::bind("127.0.0.1:0").expect("a port");
    let port = free.local_addr().expect("bound").port();
    drop(free);
    let gps = Gps::start(Source::Udp { port });
    until(&gps, "the port open", |s| {
        matches!(s.link, Link::Open { .. })
    });
    let phone = UdpSocket::bind("127.0.0.1:0").expect("a port");
    // a GGA and its RMC in each datagram, as GPS2IP sends them
    let lines: Vec<&str> = GPS2IP.lines().collect();
    for pair in lines.chunks(2) {
        let datagram = format!("{}\r\n", pair.join("\r\n"));
        phone
            .send_to(datagram.as_bytes(), ("127.0.0.1", port))
            .expect("sent");
    }
    let s = until(&gps, "a fix", |s| s.fix.is_some());
    assert_eq!(s.refused, 0);
    gps.stop();
    let start = Instant::now();
    while UdpSocket::bind(("0.0.0.0", port)).is_err() {
        assert!(
            start.elapsed() < Duration::from_secs(2),
            "the reader kept the port"
        );
        thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn gpsd_is_asked_to_watch_and_its_reports_give_a_fix() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("a port");
    let port = listener.local_addr().expect("bound").port();
    let (asked_tx, asked_rx) = mpsc::channel::<String>();
    thread::spawn(move || {
        let (mut gpsd, _) = listener.accept().expect("the reader connects");
        let mut asked = String::new();
        let mut reader = BufReader::new(gpsd.try_clone().expect("a clone"));
        reader.read_line(&mut asked).expect("the reader asks");
        asked_tx.send(asked).expect("told");
        gpsd.write_all(GPSD.as_bytes()).expect("sent");
        thread::sleep(Duration::from_secs(10));
    });
    let gps = Gps::start(Source::Gpsd {
        host: "127.0.0.1".into(),
        port,
    });
    let asked = asked_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("asked");
    assert_eq!(asked, gpsd::WATCH);
    let s = until(&gps, "a fix", |s| s.fix.is_some());
    assert_eq!(s.fix.and_then(|f| f.alt_m), Some(318.2));
}

#[cfg(unix)]
#[test]
fn a_serial_receiver_is_read_at_the_speed_found() {
    use serialport::{SerialPort, TTYPort};

    // both ends stay open for the test: the terminal goes when its last end closes
    let (mut master, slave) = TTYPort::pair().expect("a pseudo-terminal");
    let name = slave.name().expect("its path");
    let (stop_tx, stop_rx) = mpsc::channel::<()>();
    thread::spawn(move || {
        while stop_rx.try_recv().is_err() {
            let _ = master.write_all(RECEIVER.as_bytes());
            thread::sleep(Duration::from_millis(200));
        }
    });
    let gps = Gps::start(Source::Serial {
        port: name.clone(),
        baud: None,
    });
    // the reader says at once when it cannot open the port: no waiting out its retries
    let s = until(&gps, "a fix, or the port refused", |s| {
        s.fix.is_some() || matches!(s.link, Link::Failed { .. })
    });
    match &s.link {
        Link::Open { detail, .. } => assert_eq!(detail.as_deref(), Some("4800 baud")),
        Link::Failed { text, .. } => panic!("the reader could not use {name}: {text}"),
        other => panic!("{other:?}"),
    }
    drop(gps);
    let _ = stop_tx.send(());
    drop(slave);
}
