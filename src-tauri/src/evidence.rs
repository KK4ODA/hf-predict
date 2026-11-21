//! The evidence API: what this station has heard lately toward a place, band by band, for another
//! program on the same computer (RadioPost's choosing a gateway, its design §11.4). A small
//! read-only HTTP server on loopback answers `GET /v1/evidence` from the observations, through the
//! same comparison the Paths screen makes: stations heard near the place or along the way to it,
//! and stations there heard reporting this station's area. Nothing is written.
//!
//! ```text
//! GET http://127.0.0.1:8625/v1/evidence?from=33.75,-84.39&toward=41.71,-72.73&since=1794687840
//!     &bands=40m,20m
//! ```
//!
//! `from` is this station, `toward` a place (repeat it for several, at most 200), both latitude
//! and longitude; `since` how far back, in seconds since 1970 (default an hour ago, at most a day);
//! `bands` the bands to say (`40m` or `40 m`; default every HF band). Each place gets each band's
//! observed tier: `notSampled` (under a minute listened to), `none`, `limited`, `moderate` or
//! `strong`, with the counts it rests on.

use std::io::{BufRead, BufReader, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::Arc;
use std::time::Duration;

use serde_json::{Value, json};

use crate::compare::{self, BandPrediction, CompareQuery, ObservedTier};
use crate::geo::{self, LatLon};
use crate::observations::Database;
use crate::timeutil;

/// The port the API listens on, on 127.0.0.1 only.
pub const PORT: u16 = 8625;
/// At most this many places in one request.
const MAX_PLACES: usize = 200;
/// How far back a request may look, seconds, and how far by default.
const MAX_SPAN_S: i64 = 86_400;
const DEFAULT_SPAN_S: i64 = 3600;
/// A request's head larger than this is refused.
const MAX_HEAD: usize = 16 * 1024;

/// Starts the server on `127.0.0.1:port` (0 for any free port) on a thread of its own, each
/// connection answered on its own. `own_call` gives the operator's callsign, for the stations
/// heard reporting it.
///
/// # Errors
///
/// The port could not be bound: another program holds it.
pub fn serve(
    port: u16,
    db: Arc<Database>,
    own_call: impl Fn() -> Option<String> + Send + Sync + 'static,
) -> Result<SocketAddr, String> {
    let listener = TcpListener::bind(("127.0.0.1", port))
        .map_err(|e| format!("evidence API: cannot listen on 127.0.0.1:{port}: {e}"))?;
    let at = listener.local_addr().map_err(|e| e.to_string())?;
    let own_call = Arc::new(own_call);
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let (db, own_call) = (Arc::clone(&db), Arc::clone(&own_call));
            std::thread::spawn(move || {
                let _ = answer_connection(stream, at.port(), &db, &*own_call);
            });
        }
    });
    Ok(at)
}

/// Reads one request and writes its answer. A request is one exchange that gives up if the other
/// end goes quiet: the read timeout ends it, never a read resumed.
fn answer_connection(
    stream: TcpStream,
    port: u16,
    db: &Database,
    own_call: &(dyn Fn() -> Option<String> + Send + Sync),
) -> std::io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut head = Vec::new();
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 || line == "\r\n" || line == "\n" {
            break;
        }
        head.push(line);
        if head.iter().map(String::len).sum::<usize>() > MAX_HEAD {
            break;
        }
    }
    let (status, body) = answer(&head, port, db, own_call().as_deref(), timeutil::now());
    let text = body.to_string();
    let mut stream = stream;
    write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\
         Cache-Control: no-store\r\nConnection: close\r\n\r\n{text}",
        text.len()
    )?;
    stream.flush()
}

/// The answer to a request's head (its request line, then its headers): a status line's code
/// and reason, and the JSON body.
pub fn answer(
    head: &[String],
    port: u16,
    db: &Database,
    own_call: Option<&str>,
    now: i64,
) -> (&'static str, Value) {
    let refuse = |status, why: &str| (status, json!({"error": why}));
    let Some(line) = head.first() else {
        return refuse("400 Bad Request", "no request");
    };
    let mut words = line.split_whitespace();
    let (method, target) = (words.next().unwrap_or(""), words.next().unwrap_or(""));
    // a page in a browser can be made to send here under another name: only ours is answered
    let host = head.iter().skip(1).find_map(|h| {
        let (name, value) = h.split_once(':')?;
        name.trim().eq_ignore_ascii_case("host").then(|| value.trim().to_ascii_lowercase())
    });
    let ours = [format!("127.0.0.1:{port}"), format!("localhost:{port}")];
    if !host.is_some_and(|h| ours.contains(&h)) {
        return refuse("403 Forbidden", "only requests to 127.0.0.1 are answered");
    }
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    if path != "/v1/evidence" {
        return refuse("404 Not Found", "the API answers GET /v1/evidence only");
    }
    if method != "GET" {
        return refuse("405 Method Not Allowed", "the API answers GET only");
    }
    match evidence(query, db, own_call, now) {
        Ok(v) => ("200 OK", v),
        Err(why) => refuse("400 Bad Request", &why),
    }
}

/// What was heard toward each place the query names.
fn evidence(query: &str, db: &Database, own_call: Option<&str>, now: i64) -> Result<Value, String> {
    let params: Vec<(String, String)> = query
        .split('&')
        .filter_map(|kv| kv.split_once('='))
        .map(|(k, v)| (k.to_owned(), unescape(v)))
        .collect();
    let one = |name: &str| params.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str());
    let place = |text: &str, name: &str| {
        geo::parse_position(text).map_err(|e| format!("{name}: {e}"))
    };
    let from = place(one("from").ok_or("from: where this station is, latitude,longitude")?, "from")?;
    let toward: Vec<LatLon> = params
        .iter()
        .filter(|(k, _)| k == "toward")
        .map(|(_, v)| place(v, "toward"))
        .collect::<Result<_, _>>()?;
    if toward.is_empty() || toward.len() > MAX_PLACES {
        return Err(format!("toward: 1 to {MAX_PLACES} places, not {}", toward.len()));
    }
    let since = match one("since") {
        None => now - DEFAULT_SPAN_S,
        Some(s) => s.parse::<i64>().map_err(|_| "since: seconds since 1970".to_owned())?,
    }
    .clamp(now - MAX_SPAN_S, now);
    let all: Vec<String> =
        radioprop_hf::station::HF_BANDS.iter().map(|b| b.name.to_owned()).collect();
    let bands: Vec<String> = match one("bands") {
        None | Some("") => all.clone(),
        Some(list) => list
            .split(',')
            .map(|b| {
                let plain = b.replace(' ', "").to_ascii_lowercase();
                all.iter()
                    .find(|a| a.replace(' ', "") == plain)
                    .cloned()
                    .ok_or_else(|| format!("bands: no HF band {b:?}"))
            })
            .collect::<Result<_, _>>()?,
    };
    let minutes = (now - since + 59) / 60;
    let mut places = Vec::new();
    for to in toward {
        let query = CompareQuery {
            minutes,
            destination: to,
            path_bearing_deg: geo::bearing_deg(from, to),
            path_distance_km: geo::distance_km(from, to),
            // the observed side only: the predictions are the asking program's own
            bands: bands
                .iter()
                .map(|name| BandPrediction {
                    name: name.clone(),
                    mode_reliability: 0.0,
                    ft8_reliability: 0.0,
                })
                .collect(),
            receiver: Some(from),
        };
        let compared = compare::compare(db, &query, own_call, now)?;
        places.push(json!({
            "toward": {"lat": to.lat, "lon": to.lon},
            "bands": compared.iter().map(|c| json!({
                "band": c.band,
                "tier": tier_name(c.observed),
                "periods_listened": c.periods,
                "stations": c.evidence_stations,
                "best_snr_db": c.evidence_best_snr_db,
                "examples": c.evidence_examples,
                "hearing_stations": c.hearing_stations,
                "hearing_best_report_db": c.hearing_best_report_db,
                "hearing_you": c.hearing_you,
            })).collect::<Vec<_>>(),
        }));
    }
    Ok(json!({
        "provider": format!("hf-predict {}", env!("CARGO_PKG_VERSION")),
        "as_of": now, "since": since, "from": {"lat": from.lat, "lon": from.lon},
        "places": places,
    }))
}

fn tier_name(t: ObservedTier) -> &'static str {
    match t {
        ObservedTier::NotSampled => "notSampled",
        ObservedTier::None => "none",
        ObservedTier::Limited => "limited",
        ObservedTier::Moderate => "moderate",
        ObservedTier::Strong => "strong",
    }
}

/// A query value with its percent escapes and `+` undone.
fn unescape(v: &str) -> String {
    let bytes = v.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let escaped = (bytes[i] == b'%')
            .then(|| bytes.get(i + 1..i + 3))
            .flatten()
            .and_then(|hex| u8::from_str_radix(std::str::from_utf8(hex).ok()?, 16).ok());
        match (bytes[i], escaped) {
            (_, Some(b)) => {
                out.push(b);
                i += 3;
            }
            (b'+', None) => {
                out.push(b' ');
                i += 1;
            }
            (b, None) => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::observations::{Observation, ORIGIN_LOCAL};
    use std::io::Read;

    const PORT_HERE: u16 = 8625;

    fn heard(db: &Database, time: i64, sender: &str, grid: &str, snr_db: i32, band: &str) {
        let rx = geo::from_maidenhead("EM73").unwrap();
        let tx = geo::from_maidenhead(grid).unwrap();
        db.insert(&Observation {
            time_utc: time,
            dial_hz: 7_074_000,
            band: band.into(),
            df_hz: 1000,
            snr_db,
            dt_s: 0.1,
            mode: "FT8".into(),
            message: format!("CQ {sender} {grid} {time}"),
            kind: "cq".into(),
            sender: Some(sender.into()),
            addressee: None,
            grid: Some(grid.into()),
            grid_source: Some("message".into()),
            distance_km: Some(geo::distance_km(rx, tx)),
            bearing_deg: Some(geo::bearing_deg(rx, tx)),
            rx_grid: Some("EM73".into()),
            origin: ORIGIN_LOCAL.into(),
            provider: "test".into(),
            low_confidence: false,
            settling: false,
        })
        .unwrap();
    }

    fn head(target: &str, host: &str) -> Vec<String> {
        vec![format!("GET {target} HTTP/1.1\r\n"), format!("Host: {host}\r\n")]
    }

    /// Ten minutes on 40 m in the last hour: three stations in New England and one in Texas.
    fn bench() -> (Database, i64) {
        let db = Database::in_memory().unwrap();
        let now = 1_794_687_840;
        let forty = db.open_interval(now - 1200, 7_074_000, "40 m", "FT8", "test").unwrap();
        db.extend_interval(forty, now - 600).unwrap();
        heard(&db, now - 1100, "W1AAA", "FN31", -12, "40 m");
        heard(&db, now - 1000, "K1BBB", "FN42", -8, "40 m");
        heard(&db, now - 900, "N1CCC", "FN41", -15, "40 m");
        heard(&db, now - 800, "W5DDD", "EM10", -3, "40 m");
        (db, now)
    }

    #[test]
    fn says_what_was_heard_toward_each_place() {
        let (db, now) = bench();
        let target = "/v1/evidence?from=33.75,-84.39&toward=41.71%2C-72.73&toward=30.27,-97.74\
                      &bands=40m,20%20m";
        let (status, v) = answer(&head(target, "127.0.0.1:8625"), PORT_HERE, &db, None, now);
        assert_eq!(status, "200 OK", "{v}");
        assert_eq!(v["since"], now - 3600);
        let places = v["places"].as_array().unwrap();
        assert_eq!(places.len(), 2);
        // Connecticut: the three New England stations, listened to for 40 periods
        let ct = &places[0]["bands"];
        assert_eq!(ct[0]["band"], "40 m");
        assert_eq!(ct[0]["tier"], "moderate", "{ct}");
        assert_eq!(ct[0]["stations"], 3);
        assert_eq!(ct[0]["best_snr_db"], -8);
        assert_eq!(ct[0]["periods_listened"], 40.0);
        // 20 m was not listened to
        assert_eq!(ct[1]["band"], "20 m");
        assert_eq!(ct[1]["tier"], "notSampled");
        // Austin: the one Texan
        assert_eq!(places[1]["bands"][0]["tier"], "limited");
        assert_eq!(places[1]["bands"][0]["examples"], json!(["W5DDD"]));
        // ten minutes back: nothing was listened to since
        let target = format!("/v1/evidence?from=33.75,-84.39&toward=41.71,-72.73&since={}&bands=40m", now - 600);
        let (_, v) = answer(&head(&target, "localhost:8625"), PORT_HERE, &db, None, now);
        assert_eq!(v["places"][0]["bands"][0]["tier"], "notSampled", "{v}");
    }

    #[test]
    fn refuses_what_it_does_not_answer() {
        let (db, now) = bench();
        let ok = "/v1/evidence?from=33.75,-84.39&toward=41.71,-72.73";
        let status = |h: Vec<String>| answer(&h, PORT_HERE, &db, None, now).0;
        // another name for this computer: a page in a browser rebinding a name of its own
        assert_eq!(status(head(ok, "evil.example:8625")), "403 Forbidden");
        assert_eq!(status(vec![format!("GET {ok} HTTP/1.1\r\n")]), "403 Forbidden");
        assert_eq!(status(head("/v1/other", "127.0.0.1:8625")), "404 Not Found");
        let mut post = head(ok, "127.0.0.1:8625");
        post[0] = format!("POST {ok} HTTP/1.1\r\n");
        assert_eq!(status(post), "405 Method Not Allowed");
        for bad in [
            "/v1/evidence?toward=41.71,-72.73",
            "/v1/evidence?from=33.75,-84.39",
            "/v1/evidence?from=33.75,-84.39&toward=north",
            "/v1/evidence?from=33.75,-84.39&toward=41.71,-72.73&bands=2m",
            "/v1/evidence?from=33.75,-84.39&toward=41.71,-72.73&since=yesterday",
        ] {
            assert_eq!(status(head(bad, "127.0.0.1:8625")), "400 Bad Request", "{bad}");
        }
        // a day back at most
        let (_, v) = answer(
            &head(&format!("{ok}&since=0"), "127.0.0.1:8625"),
            PORT_HERE,
            &db,
            None,
            now,
        );
        assert_eq!(v["since"], now - 86_400);
    }

    #[test]
    fn answers_over_loopback() {
        let (db, _) = bench();
        let at = serve(0, Arc::new(db), || Some("KK4ODA".into())).unwrap();
        let mut s = TcpStream::connect(at).unwrap();
        write!(
            s,
            "GET /v1/evidence?from=33.75,-84.39&toward=41.71,-72.73&bands=40m HTTP/1.1\r\n\
             Host: 127.0.0.1:{}\r\n\r\n",
            at.port()
        )
        .unwrap();
        let mut text = String::new();
        s.read_to_string(&mut text).unwrap();
        assert!(text.starts_with("HTTP/1.1 200 OK\r\n"), "{text}");
        let body: Value = serde_json::from_str(text.split_once("\r\n\r\n").unwrap().1).unwrap();
        assert!(body["provider"].as_str().unwrap().starts_with("hf-predict "));
        assert_eq!(body["places"][0]["bands"][0]["band"], "40 m");
    }

    #[test]
    fn query_values_are_unescaped() {
        assert_eq!(unescape("41.71%2C-72.73"), "41.71,-72.73");
        assert_eq!(unescape("20+m"), "20 m");
        assert_eq!(unescape("40%20m"), "40 m");
        assert_eq!(unescape("100%"), "100%");
        assert_eq!(unescape("%zz"), "%zz");
    }
}
