//! Turns the WSJT-X message stream into stored observations and listening
//! intervals. All time comes in as an argument, so a recorded session
//! replays to exactly the same database.

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::sync::Arc;

use serde::Serialize;

use super::ft8text;
use super::protocol::{Body, Decode, Message, Status};
use crate::geo;
use crate::observations::{Database, Observation, ORIGIN_LOCAL};
use crate::station;

pub const PROVIDER: &str = "wsjtx-udp";
/// WSJT-X sends a heartbeat every 15 s; three missed means it has gone.
const SESSION_TIMEOUT_S: i64 = 45;
/// How often a listening interval's end is written while it is running.
const INTERVAL_WRITE_EVERY_S: i64 = 10;
const SECONDS_PER_DAY: i64 = 86_400;
/// The clock check uses decodes from this long back.
const CLOCK_WINDOW_S: i64 = 180;
const CLOCK_MIN_SAMPLES: usize = 5;
/// FT8 needs the clock within about a second; warn at half of that.
const CLOCK_WARN_S: f64 = 0.5;
const CLOCK_ALARM_S: f64 = 1.0;

struct Interval {
    id: i64,
    written_end: i64,
}

struct Session {
    version: Option<String>,
    last_seen: i64,
    status: Option<Status>,
    /// When the dial frequency or mode last changed.
    changed_at: Option<i64>,
    interval: Option<Interval>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DecoderStatus {
    pub id: String,
    pub version: Option<String>,
    pub dial_hz: Option<u64>,
    pub band: Option<String>,
    pub mode: Option<String>,
    pub de_call: Option<String>,
    pub de_grid: Option<String>,
    pub transmitting: bool,
    pub seconds_since_heard: i64,
}

/// An estimate of this computer's clock error from the time offsets of
/// received signals. Most stations keep good time, so their median offset
/// is mostly our own error.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClockCheck {
    pub median_dt_s: Option<f64>,
    pub samples: usize,
    /// `unknown` (too few decodes), `ok`, `warn` or `alarm`.
    pub level: &'static str,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackerStatus {
    pub decoders: Vec<DecoderStatus>,
    pub clock: ClockCheck,
    /// Decodes stored since the listener started.
    pub stored: u64,
    /// Decodes not stored: replays, recordings, repeats, or ones that arrived
    /// before the decoder reported its frequency.
    pub skipped: u64,
}

/// Everything needed to store one decode, whatever supplied it.
pub struct Heard<'a> {
    pub time_utc: i64,
    pub dial_hz: u64,
    pub mode: &'a str,
    pub df_hz: u32,
    pub snr_db: i32,
    pub dt_s: f64,
    pub message: &'a str,
    pub rx_grid: Option<&'a str>,
    pub provider: &'a str,
    pub low_confidence: bool,
    pub settling: bool,
}

/// Remembers each station's locator so messages without one can be placed.
pub struct GridMemory {
    db: Arc<Database>,
    known: HashMap<String, String>,
}

impl GridMemory {
    pub fn new(db: Arc<Database>) -> Self {
        Self { db, known: HashMap::new() }
    }

    fn recall(&mut self, callsign: &str) -> Result<Option<String>, String> {
        if let Some(grid) = self.known.get(callsign) {
            return Ok(Some(grid.clone()));
        }
        let grid = self.db.remembered_grid(callsign)?;
        if let Some(grid) = &grid {
            self.known.insert(callsign.to_string(), grid.clone());
        }
        Ok(grid)
    }

    /// Builds the observation for a decode and stores it. Returns false if
    /// the same decode was already stored.
    pub fn store(&mut self, heard: &Heard) -> Result<bool, String> {
        let text = ft8text::parse(heard.message);
        let (grid, grid_source) = match (&text.grid, &text.from) {
            (Some(grid), Some(from)) => {
                self.known.insert(from.clone(), grid.clone());
                (Some(grid.clone()), Some("message"))
            }
            (None, Some(from)) => match self.recall(from)? {
                Some(grid) => (Some(grid), Some("remembered")),
                None => (None, None),
            },
            _ => (None, None),
        };
        let path = heard
            .rx_grid
            .and_then(|rx| geo::from_maidenhead(rx).ok())
            .zip(grid.as_deref().and_then(|g| geo::from_maidenhead(g).ok()));

        self.db.insert(&Observation {
            time_utc: heard.time_utc,
            dial_hz: heard.dial_hz,
            band: station::band_for_hz(heard.dial_hz),
            df_hz: heard.df_hz,
            snr_db: heard.snr_db,
            dt_s: heard.dt_s,
            mode: heard.mode.to_string(),
            message: heard.message.to_string(),
            kind: text.kind.as_str().to_string(),
            sender: text.from,
            addressee: text.to,
            grid,
            grid_source: grid_source.map(String::from),
            distance_km: path.map(|(rx, tx)| geo::distance_km(rx, tx)),
            bearing_deg: path.map(|(rx, tx)| geo::bearing_deg(rx, tx)),
            rx_grid: heard.rx_grid.map(String::from),
            origin: ORIGIN_LOCAL.to_string(),
            provider: heard.provider.to_string(),
            low_confidence: heard.low_confidence,
            settling: heard.settling,
        })
    }
}

pub struct Tracker {
    db: Arc<Database>,
    grids: GridMemory,
    sessions: BTreeMap<String, Session>,
    /// (arrival time, time offset) of recent decodes.
    clock: VecDeque<(i64, f64)>,
    stored: u64,
    skipped: u64,
}

/// The date a time of day belongs to: the one that puts it nearest to now.
fn resolve_time(time_ms: u32, now: i64) -> i64 {
    let midnight = now - now.rem_euclid(SECONDS_PER_DAY);
    let candidate = midnight + i64::from(time_ms / 1000);
    if candidate - now > SECONDS_PER_DAY / 2 {
        candidate - SECONDS_PER_DAY
    } else if now - candidate > SECONDS_PER_DAY / 2 {
        candidate + SECONDS_PER_DAY
    } else {
        candidate
    }
}

impl Tracker {
    pub fn new(db: Arc<Database>) -> Self {
        Self {
            grids: GridMemory::new(db.clone()),
            db,
            sessions: BTreeMap::new(),
            clock: VecDeque::new(),
            stored: 0,
            skipped: 0,
        }
    }

    /// Takes one message, received at `now` (Unix seconds).
    pub fn handle(&mut self, message: Message, now: i64) -> Result<(), String> {
        let session = self.sessions.entry(message.id.clone()).or_insert_with(|| Session {
            version: None,
            last_seen: now,
            status: None,
            changed_at: None,
            interval: None,
        });
        session.last_seen = now;

        match message.body {
            Body::Heartbeat { version } => session.version = version.or(session.version.take()),
            Body::Status(status) => {
                let changed = session
                    .status
                    .as_ref()
                    .is_some_and(|old| old.dial_hz != status.dial_hz || old.mode != status.mode);
                if changed {
                    session.changed_at = Some(now);
                    if let Some(interval) = session.interval.take() {
                        self.db.extend_interval(interval.id, now)?;
                    }
                }
                if session.interval.is_none() && status.dial_hz > 0 {
                    let id = self.db.open_interval(
                        now,
                        status.dial_hz,
                        &station::band_for_hz(status.dial_hz),
                        &status.mode,
                        PROVIDER,
                    )?;
                    session.interval = Some(Interval { id, written_end: now });
                }
                session.status = Some(status);
            }
            Body::Decode(decode) => self.decode(&message.id, decode, now)?,
            Body::Close => {
                if let Some(interval) = session.interval.take() {
                    self.db.extend_interval(interval.id, now)?;
                }
                self.sessions.remove(&message.id);
                return Ok(());
            }
            Body::Clear | Body::Other(_) => {}
        }

        // Any message shows the decoder is still listening.
        if let Some(session) = self.sessions.get_mut(&message.id) {
            if let Some(interval) = &mut session.interval {
                if now - interval.written_end >= INTERVAL_WRITE_EVERY_S {
                    self.db.extend_interval(interval.id, now)?;
                    interval.written_end = now;
                }
            }
        }
        Ok(())
    }

    fn decode(&mut self, id: &str, decode: Decode, now: i64) -> Result<(), String> {
        let session = &self.sessions[id];
        // Without a status the decode cannot be given a frequency.
        let Some(status) = session.status.as_ref().filter(|_| decode.new && !decode.off_air) else {
            self.skipped += 1;
            return Ok(());
        };
        let time_utc = resolve_time(decode.time_ms, now);
        // A period that began before the receiver moved is partly on the old frequency.
        let settling = session.changed_at.is_some_and(|changed| time_utc < changed);

        let stored = self.grids.store(&Heard {
            time_utc,
            dial_hz: status.dial_hz,
            mode: &status.mode,
            df_hz: decode.df_hz,
            snr_db: decode.snr_db,
            dt_s: decode.dt_s,
            message: &decode.message,
            rx_grid: status.de_grid.as_deref().filter(|g| !g.is_empty()),
            provider: PROVIDER,
            low_confidence: decode.low_confidence,
            settling,
        })?;
        if stored {
            self.stored += 1;
            if !settling {
                self.clock.push_back((now, decode.dt_s));
            }
        } else {
            self.skipped += 1;
        }
        Ok(())
    }

    /// Call regularly: closes decoders that have stopped sending.
    pub fn tick(&mut self, now: i64) -> Result<(), String> {
        let gone: Vec<String> = self
            .sessions
            .iter()
            .filter(|(_, s)| now - s.last_seen > SESSION_TIMEOUT_S)
            .map(|(id, _)| id.clone())
            .collect();
        for id in gone {
            if let Some(session) = self.sessions.remove(&id) {
                // It was last known to be listening when last heard from.
                if let Some(interval) = session.interval {
                    self.db.extend_interval(interval.id, session.last_seen)?;
                }
            }
        }
        while self.clock.front().is_some_and(|(at, _)| now - at > CLOCK_WINDOW_S) {
            self.clock.pop_front();
        }
        Ok(())
    }

    pub fn status(&self, now: i64) -> TrackerStatus {
        let decoders = self
            .sessions
            .iter()
            .map(|(id, session)| {
                let status = session.status.as_ref();
                DecoderStatus {
                    id: id.clone(),
                    version: session.version.clone(),
                    dial_hz: status.map(|s| s.dial_hz),
                    band: status.map(|s| station::band_for_hz(s.dial_hz)),
                    mode: status.map(|s| s.mode.clone()),
                    de_call: status.and_then(|s| s.de_call.clone()),
                    de_grid: status.and_then(|s| s.de_grid.clone()),
                    transmitting: status.is_some_and(|s| s.transmitting),
                    seconds_since_heard: now - session.last_seen,
                }
            })
            .collect();

        let mut offsets: Vec<f64> = self
            .clock
            .iter()
            .filter(|(at, _)| now - at <= CLOCK_WINDOW_S)
            .map(|(_, dt)| *dt)
            .collect();
        offsets.sort_by(f64::total_cmp);
        let median_dt_s = (offsets.len() >= CLOCK_MIN_SAMPLES).then(|| offsets[offsets.len() / 2]);
        let level = match median_dt_s.map(f64::abs) {
            None => "unknown",
            Some(error) if error >= CLOCK_ALARM_S => "alarm",
            Some(error) if error >= CLOCK_WARN_S => "warn",
            Some(_) => "ok",
        };

        TrackerStatus {
            decoders,
            clock: ClockCheck { median_dt_s, samples: offsets.len(), level },
            stored: self.stored,
            skipped: self.skipped,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::timeutil;
    use crate::wsjtx::protocol::{self, encode};

    const ID: &str = "WSJT-X";

    fn status(dial_hz: u64) -> Status {
        Status {
            dial_hz,
            mode: "FT8".into(),
            tx_enabled: false,
            transmitting: false,
            decoding: false,
            de_call: Some("N0CALL".into()),
            de_grid: Some("EM73".into()),
            tr_period_s: Some(15),
        }
    }

    fn decode(time_ms: u32, message: &str, snr_db: i32, dt_s: f64) -> Decode {
        Decode {
            new: true,
            time_ms,
            snr_db,
            dt_s,
            df_hz: 1000 + (message.len() as u32) * 37,
            mode: "~".into(),
            message: message.into(),
            low_confidence: false,
            off_air: false,
        }
    }

    /// A recorded session as it would arrive from the network: each datagram
    /// with the time it was received.
    fn session() -> Vec<(i64, Vec<u8>)> {
        let start = timeutil::from_utc(2026, 10, 4, 15, 30);
        // Milliseconds since midnight for 15:30:00.
        let slot = |n: u32| (15 * 3600 + 30 * 60 + n * 15) * 1000;
        vec![
            (start, encode::heartbeat(ID, "3.0.2")),
            (start, encode::status(ID, &status(14_074_000))),
            (start + 13, encode::decode(ID, &decode(slot(0), "CQ K1ABC FN42", -12, 0.3))),
            (start + 13, encode::decode(ID, &decode(slot(0), "K1ABC W9XYZ EN37", -5, 0.2))),
            (start + 15, encode::heartbeat(ID, "3.0.2")),
            (start + 28, encode::decode(ID, &decode(slot(1), "W9XYZ K1ABC -07", -11, 0.4))),
            (start + 28, encode::decode(ID, &decode(slot(1), "TNX BOB 73 GL", -18, 0.1))),
            // The operator changes band 3 s into the third period.
            (start + 33, encode::status(ID, &status(7_074_000))),
            (start + 43, encode::decode(ID, &decode(slot(2), "CQ DX JA1ZZZ PM95", -20, 0.2))),
            (start + 58, encode::decode(ID, &decode(slot(3), "CQ EA1AAA IN73", -9, 0.3))),
            (start + 60, encode::close(ID)),
        ]
    }

    fn replay(db: &Arc<Database>, session: &[(i64, Vec<u8>)]) -> Tracker {
        let mut tracker = Tracker::new(db.clone());
        for (at, datagram) in session {
            tracker.handle(protocol::parse(datagram).unwrap(), *at).unwrap();
        }
        tracker
    }

    #[test]
    fn a_session_becomes_observations() {
        let db = Arc::new(Database::in_memory().unwrap());
        replay(&db, &session());
        let all = db.all().unwrap();
        let start = timeutil::from_utc(2026, 10, 4, 15, 30);

        let summary: Vec<(i64, &str, Option<&str>, Option<&str>, Option<&str>, bool)> = all
            .iter()
            .map(|o| (o.time_utc - start, o.band.as_str(), o.sender.as_deref(), o.grid.as_deref(), o.grid_source.as_deref(), o.settling))
            .collect();
        assert_eq!(
            summary,
            [
                (0, "20 m", Some("K1ABC"), Some("FN42"), Some("message"), false),
                (0, "20 m", Some("W9XYZ"), Some("EN37"), Some("message"), false),
                // K1ABC sent no locator here; the one from its CQ is used.
                (15, "20 m", Some("K1ABC"), Some("FN42"), Some("remembered"), false),
                (15, "20 m", None, None, None, false),
                // This period began before the band change.
                (30, "40 m", Some("JA1ZZZ"), Some("PM95"), Some("message"), true),
                (45, "40 m", Some("EA1AAA"), Some("IN73"), Some("message"), false),
            ]
        );

        let first = &all[0];
        assert_eq!((first.dial_hz, first.mode.as_str(), first.kind.as_str()), (14_074_000, "FT8", "cq"));
        assert_eq!(first.rx_grid.as_deref(), Some("EM73"));
        // EM73 to FN42 is roughly 1,500 km to the north-east.
        assert!((first.distance_km.unwrap() - 1500.0).abs() < 200.0, "{:?}", first.distance_km);
        assert!((first.bearing_deg.unwrap() - 45.0).abs() < 20.0, "{:?}", first.bearing_deg);
        assert_eq!(all[3].distance_km, None, "free text has no sender to place");
    }

    /// The Phase 4 exit test: replaying a recorded session reproduces the
    /// stored observations exactly.
    #[test]
    fn replaying_a_session_reproduces_the_database() {
        let (first, second) = (Arc::new(Database::in_memory().unwrap()), Arc::new(Database::in_memory().unwrap()));
        replay(&first, &session());
        replay(&second, &session());
        assert_eq!(first.all().unwrap(), second.all().unwrap());
        assert_eq!(first.band_activity(0).unwrap(), second.band_activity(0).unwrap());

        // Replaying into the same database adds nothing.
        let again = replay(&first, &session());
        assert_eq!(first.count().unwrap(), 6);
        assert_eq!(again.status(0).stored, 0);
    }

    #[test]
    fn listening_intervals_follow_the_dial() {
        let db = Arc::new(Database::in_memory().unwrap());
        replay(&db, &session());
        let activity = db.band_activity(0).unwrap();
        let by_band: Vec<(&str, i64, usize)> =
            activity.iter().map(|a| (a.band.as_str(), a.listened_seconds, a.decodes)).collect();
        // 33 s on 20 m, then 27 s on 40 m until WSJT-X closed. The settling decode is not counted.
        assert_eq!(by_band, [("40 m", 27, 1), ("20 m", 33, 4)]);
    }

    #[test]
    fn a_decoder_that_stops_sending_is_closed_at_its_last_message() {
        let db = Arc::new(Database::in_memory().unwrap());
        let mut tracker = Tracker::new(db.clone());
        let start = 1_000_000;
        tracker.handle(protocol::parse(&encode::heartbeat(ID, "3.0.2")).unwrap(), start).unwrap();
        tracker.handle(protocol::parse(&encode::status(ID, &status(14_074_000))).unwrap(), start).unwrap();
        tracker.handle(protocol::parse(&encode::heartbeat(ID, "3.0.2")).unwrap(), start + 15).unwrap();

        tracker.tick(start + 40).unwrap();
        assert_eq!(tracker.status(start + 40).decoders.len(), 1);
        assert_eq!(tracker.status(start + 40).decoders[0].seconds_since_heard, 25);

        tracker.tick(start + 100).unwrap();
        assert!(tracker.status(start + 100).decoders.is_empty());
        assert_eq!(db.band_activity(0).unwrap()[0].listened_seconds, 15);
    }

    #[test]
    fn replays_recordings_and_early_decodes_are_not_stored() {
        let db = Arc::new(Database::in_memory().unwrap());
        let mut tracker = Tracker::new(db.clone());
        let mut handle = |datagram: Vec<u8>| tracker.handle(protocol::parse(&datagram).unwrap(), 50_000).unwrap();

        // Before any status: no frequency to attribute it to.
        handle(encode::decode(ID, &decode(1000, "CQ K1ABC FN42", -1, 0.0)));
        handle(encode::status(ID, &status(14_074_000)));
        let mut replayed = decode(2000, "CQ K1ABC FN42", -1, 0.0);
        replayed.new = false;
        handle(encode::decode(ID, &replayed));
        let mut recording = decode(3000, "CQ K1ABC FN42", -1, 0.0);
        recording.off_air = true;
        handle(encode::decode(ID, &recording));

        assert_eq!(db.count().unwrap(), 0);
        assert_eq!(tracker.status(50_000).skipped, 3);
    }

    #[test]
    fn decodes_just_before_midnight_keep_yesterdays_date() {
        let midnight = timeutil::from_utc(2026, 10, 5, 0, 0);
        // 23:59:45 decoded and received two seconds after midnight.
        assert_eq!(resolve_time(86_385_000, midnight + 2), midnight - 15);
        // 00:00:00 received at 00:00:13.
        assert_eq!(resolve_time(0, midnight + 13), midnight);
        // A sender whose clock is slightly ahead: 00:00:00 received at 23:59:59.
        assert_eq!(resolve_time(0, midnight - 1), midnight);
    }

    #[test]
    fn the_clock_check_reports_the_median_offset() {
        let db = Arc::new(Database::in_memory().unwrap());
        let mut tracker = Tracker::new(db);
        let now = timeutil::from_utc(2026, 10, 4, 12, 0);
        tracker.handle(protocol::parse(&encode::status(ID, &status(14_074_000))).unwrap(), now).unwrap();
        assert_eq!(tracker.status(now).clock.level, "unknown");

        let offsets = [1.2, 1.3, 1.1, 1.4, -0.2, 1.2, 1.3];
        for (i, dt) in offsets.iter().enumerate() {
            let message = format!("CQ K{i}ABC FN42");
            let d = decode(12 * 3_600_000, &message, -10, *dt);
            tracker.handle(protocol::parse(&encode::decode(ID, &d)).unwrap(), now + 13).unwrap();
        }
        let clock = tracker.status(now + 14).clock;
        assert_eq!((clock.median_dt_s, clock.samples, clock.level), (Some(1.2), 7, "alarm"));

        // Old samples stop counting.
        tracker.tick(now + 13 + CLOCK_WINDOW_S + 1).unwrap();
        assert_eq!(tracker.status(now + 13 + CLOCK_WINDOW_S + 1).clock.level, "unknown");
    }
}
