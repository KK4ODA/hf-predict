//! What the station has heard, kept in a local SQLite database.
//!
//! Two tables: every decode, and every interval the receiver spent listening
//! on a frequency. The intervals are what make "nothing heard" meaningful:
//! without them it cannot be told apart from "not listening".

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::Path;
use std::sync::{Mutex, MutexGuard, PoisonError};

use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;

use crate::geo::LatLon;

const SCHEMA: &str = "
    CREATE TABLE IF NOT EXISTS observations (
        id INTEGER PRIMARY KEY,
        time_utc INTEGER NOT NULL,
        dial_hz INTEGER NOT NULL,
        band TEXT NOT NULL,
        df_hz INTEGER NOT NULL,
        snr_db INTEGER NOT NULL,
        dt_s REAL NOT NULL,
        mode TEXT NOT NULL,
        message TEXT NOT NULL,
        kind TEXT NOT NULL,
        sender TEXT,
        addressee TEXT,
        grid TEXT,
        grid_source TEXT,
        distance_km REAL,
        bearing_deg REAL,
        rx_grid TEXT,
        origin TEXT NOT NULL,
        provider TEXT NOT NULL,
        low_confidence INTEGER NOT NULL DEFAULT 0,
        settling INTEGER NOT NULL DEFAULT 0,
        UNIQUE (time_utc, dial_hz, df_hz, message)
    );
    CREATE INDEX IF NOT EXISTS observations_time ON observations (time_utc);
    CREATE INDEX IF NOT EXISTS observations_sender ON observations (sender, time_utc);
    CREATE TABLE IF NOT EXISTS listening_intervals (
        id INTEGER PRIMARY KEY,
        start_utc INTEGER NOT NULL,
        end_utc INTEGER NOT NULL,
        dial_hz INTEGER NOT NULL,
        band TEXT NOT NULL,
        mode TEXT NOT NULL,
        provider TEXT NOT NULL
    );
    CREATE INDEX IF NOT EXISTS listening_intervals_time ON listening_intervals (end_utc);
    CREATE TABLE IF NOT EXISTS log_files (
        path TEXT PRIMARY KEY,
        imported_bytes INTEGER NOT NULL,
        checked_utc INTEGER NOT NULL
    );
    CREATE TABLE IF NOT EXISTS predicted_reliability (
        key TEXT NOT NULL,
        year INTEGER NOT NULL,
        month INTEGER NOT NULL,
        hour INTEGER NOT NULL,
        grid TEXT NOT NULL,
        bands TEXT NOT NULL,
        PRIMARY KEY (key, year, month, hour, grid)
    );
";
const SCHEMA_VERSION: i64 = 1;

/// Decoded by this station's own receiver, as opposed to a reporting network.
pub const ORIGIN_LOCAL: &str = "LOCAL";

/// A stretch of listening on one band, as the listener recorded it.
#[derive(Debug, Clone, PartialEq)]
pub struct ListeningInterval {
    pub start_utc: i64,
    pub end_utc: i64,
    pub band: String,
}

/// One decoded signal.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Observation {
    /// Start of the transmit period the signal was decoded in, Unix seconds.
    pub time_utc: i64,
    pub dial_hz: u64,
    pub band: String,
    pub df_hz: u32,
    pub snr_db: i32,
    pub dt_s: f64,
    pub mode: String,
    pub message: String,
    /// `ft8text::Kind` as text.
    pub kind: String,
    pub sender: Option<String>,
    pub addressee: Option<String>,
    pub grid: Option<String>,
    /// `message` when the locator was in this message, `remembered` when it
    /// is the sender's locator from an earlier one.
    pub grid_source: Option<String>,
    pub distance_km: Option<f64>,
    pub bearing_deg: Option<f64>,
    pub rx_grid: Option<String>,
    pub origin: String,
    pub provider: String,
    pub low_confidence: bool,
    /// The receiver changed frequency during this transmit period, so the
    /// decode is kept but left out of statistics.
    pub settling: bool,
}

/// Stations beyond this are counted as long distance.
const LONG_DISTANCE_KM: f64 = 3000.0;
/// Compass sectors of 45 degrees, the first centred on north.
pub const SECTORS: usize = 8;

/// Length of one transmit period in seconds.
fn period_seconds(mode: &str) -> f64 {
    match mode {
        "FT2" => 3.75,
        "FT4" => 7.5,
        "WSPR" | "FST4W" => 120.0,
        "JT65" | "JT9" | "JT4" | "Q65" => 60.0,
        _ => 15.0,
    }
}

/// The compass sector a bearing falls in: 0 is north, 1 north-east, and so on.
fn sector(bearing_deg: f64) -> usize {
    let width = 360.0 / SECTORS as f64;
    ((bearing_deg + width / 2.0).rem_euclid(360.0) / width) as usize % SECTORS
}

/// What was heard on one band over a span of time.
///
/// Signal statistics are over decodes. Distance and direction statistics are
/// over stations, each counted once, so a talkative station does not skew them.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BandActivity {
    pub band: String,
    pub dial_hz: u64,
    pub listened_seconds: i64,
    /// Transmit periods listened to.
    pub periods: f64,
    pub decodes: usize,
    /// Decodes per period listened; `None` with no listening time on record.
    pub decodes_per_period: Option<f64>,
    pub unique_callsigns: usize,
    pub unique_grids: usize,
    pub median_snr_db: Option<f64>,
    /// The SNR that 90% of decodes are at or below.
    pub p90_snr_db: Option<f64>,
    /// Stations whose position is known.
    pub located_stations: usize,
    pub median_distance_km: Option<f64>,
    pub max_distance_km: Option<f64>,
    pub long_distance_stations: usize,
    /// Located stations per compass sector, north first, clockwise.
    pub sectors: [usize; SECTORS],
    /// The same counts for the equal span just before this one.
    pub previous_unique_callsigns: usize,
    pub previous_periods: f64,
}

/// One station heard, with where it is and how well it was received.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HeardStation {
    pub callsign: String,
    pub grid: String,
    /// Centre of the locator square.
    pub lat: f64,
    pub lon: f64,
    pub band: String,
    pub decodes: usize,
    pub best_snr_db: i32,
    pub last_heard_utc: i64,
    pub distance_km: Option<f64>,
    pub bearing_deg: Option<f64>,
}

/// A distant station heard sending signal reports to this station, or to
/// stations near it: evidence of the transmit direction, which this
/// receiver cannot measure on its own.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HearingStation {
    pub callsign: String,
    pub grid: String,
    /// Centre of the locator square.
    pub lat: f64,
    pub lon: f64,
    pub band: String,
    /// From this receiver.
    pub distance_km: f64,
    pub bearing_deg: f64,
    /// The strongest report it gave, dB in 2500 Hz.
    pub best_report_db: i32,
    /// Distinct stations it reported, near this one or this one itself.
    pub reported: Vec<ReportedStation>,
    /// It reported this station's own callsign.
    pub heard_you: bool,
    pub last_utc: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportedStation {
    pub callsign: String,
    /// From this receiver; 0 for this station itself.
    pub distance_km: f64,
    pub report_db: i32,
}

/// Recipients within this of the receiver always count as near it.
const NEAR_MIN_KM: f64 = 300.0;
/// Farther than this never does.
const NEAR_MAX_KM: f64 = 1000.0;
/// In between, the allowance grows with the sender's distance: seen from far
/// away, a station a few hundred kilometres off is in the same direction.
const NEAR_SHARE: f64 = 0.15;

/// Whether a report to a station `recipient_km` from this receiver says
/// something about how a sender `sender_km` away hears this station.
pub fn near_this_station(recipient_km: f64, sender_km: f64) -> bool {
    recipient_km <= (sender_km * NEAR_SHARE).clamp(NEAR_MIN_KM, NEAR_MAX_KM) && recipient_km < sender_km / 2.0
}

/// A callsign without the angle brackets WSJT-X puts round hashed ones.
fn bare(call: &str) -> String {
    call.trim().trim_start_matches('<').trim_end_matches('>').to_uppercase()
}

/// A station heard, where it last said it was, and when it is on the air.
#[derive(Debug, Clone, PartialEq)]
pub struct StationPlace {
    pub callsign: String,
    /// The locator it last sent.
    pub grid: String,
    /// A bit for each UTC hour it was heard in, on any day and band.
    pub hours: u32,
}

pub struct Database {
    connection: Mutex<Connection>,
}

fn text(e: rusqlite::Error) -> String {
    format!("observation database: {e}")
}

fn median(values: &mut [f64]) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    values.sort_by(f64::total_cmp);
    let middle = values.len() / 2;
    Some(if values.len() % 2 == 1 { values[middle] } else { (values[middle - 1] + values[middle]) / 2.0 })
}

/// The value that the given share of a sorted list is at or below.
fn percentile(sorted: &[f64], share: f64) -> Option<f64> {
    let rank = (share * sorted.len() as f64).ceil() as usize;
    sorted.get(rank.max(1) - 1).copied()
}

impl Database {
    pub fn open(path: &Path) -> Result<Self, String> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
        }
        Self::prepare(Connection::open(path).map_err(text)?)
    }

    pub fn in_memory() -> Result<Self, String> {
        Self::prepare(Connection::open_in_memory().map_err(text)?)
    }

    fn prepare(connection: Connection) -> Result<Self, String> {
        let version: i64 = connection.query_row("PRAGMA user_version", [], |r| r.get(0)).map_err(text)?;
        if version > SCHEMA_VERSION {
            return Err(format!(
                "the observation database was written by a newer version of the app \
                 (format {version}, this version reads up to {SCHEMA_VERSION})"
            ));
        }
        connection.pragma_update(None, "journal_mode", "WAL").map_err(text)?;
        connection.execute_batch(SCHEMA).map_err(text)?;
        connection.pragma_update(None, "user_version", SCHEMA_VERSION).map_err(text)?;
        Ok(Self { connection: Mutex::new(connection) })
    }

    fn lock(&self) -> MutexGuard<'_, Connection> {
        self.connection.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// How far into a log file the last check read, and when.
    pub fn log_progress(&self, path: &str) -> Result<Option<(u64, i64)>, String> {
        self.lock()
            .query_row(
                "SELECT imported_bytes, checked_utc FROM log_files WHERE path = ?1",
                [path],
                |r| Ok((r.get::<_, i64>(0)? as u64, r.get::<_, i64>(1)?)),
            )
            .optional()
            .map_err(text)
    }

    pub fn set_log_progress(&self, path: &str, imported_bytes: u64, checked_utc: i64) -> Result<(), String> {
        self.lock()
            .execute(
                "INSERT OR REPLACE INTO log_files (path, imported_bytes, checked_utc) VALUES (?1, ?2, ?3)",
                params![path, imported_bytes as i64, checked_utc],
            )
            .map(|_| ())
            .map_err(text)
    }

    /// When each band was last listened to: the end of its latest listening
    /// interval or its latest decode, whichever is later.
    pub fn last_listened_by_band(&self) -> Result<BTreeMap<String, i64>, String> {
        let connection = self.lock();
        let mut statement = connection
            .prepare(
                "SELECT band, MAX(t) FROM (
                     SELECT band, MAX(end_utc) AS t FROM listening_intervals GROUP BY band
                     UNION ALL
                     SELECT band, MAX(time_utc) AS t FROM observations GROUP BY band
                 ) GROUP BY band",
            )
            .map_err(text)?;
        let rows = statement
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))
            .map_err(text)?;
        rows.map(|r| r.map_err(text)).collect()
    }

    pub fn listening_intervals(&self) -> Result<Vec<ListeningInterval>, String> {
        let connection = self.lock();
        let mut statement = connection
            .prepare("SELECT start_utc, end_utc, band FROM listening_intervals ORDER BY start_utc")
            .map_err(text)?;
        let rows = statement
            .query_map([], |r| Ok(ListeningInterval { start_utc: r.get(0)?, end_utc: r.get(1)?, band: r.get(2)? }))
            .map_err(text)?;
        rows.map(|r| r.map_err(text)).collect()
    }

    /// Cached predictions for one month under `key` (the assumptions they
    /// were made with): (UTC hour 0-23, locator) to reliability per band.
    pub fn predicted_reliability(
        &self,
        key: &str,
        year: i32,
        month: u32,
    ) -> Result<BTreeMap<(u32, String), Vec<f64>>, String> {
        let connection = self.lock();
        let mut statement = connection
            .prepare("SELECT hour, grid, bands FROM predicted_reliability WHERE key = ?1 AND year = ?2 AND month = ?3")
            .map_err(text)?;
        let rows = statement
            .query_map(params![key, year, month], |r| {
                Ok((r.get::<_, i64>(0)? as u32, r.get::<_, String>(1)?, r.get::<_, String>(2)?))
            })
            .map_err(text)?;
        let mut table = BTreeMap::new();
        for row in rows {
            let (hour, grid, bands) = row.map_err(text)?;
            let bands = bands
                .split(',')
                .map(|v| v.parse::<f64>().map_err(|e| format!("cached prediction: {e}")))
                .collect::<Result<Vec<_>, _>>()?;
            table.insert((hour, grid), bands);
        }
        Ok(table)
    }

    pub fn store_predicted_reliability(
        &self,
        key: &str,
        year: i32,
        month: u32,
        rows: &[(u32, String, Vec<f64>)],
    ) -> Result<(), String> {
        let mut connection = self.lock();
        let transaction = connection.transaction().map_err(text)?;
        {
            let mut statement = transaction
                .prepare(
                    "INSERT OR REPLACE INTO predicted_reliability (key, year, month, hour, grid, bands)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                )
                .map_err(text)?;
            for (hour, grid, bands) in rows {
                let bands = bands.iter().map(|v| v.to_string()).collect::<Vec<_>>().join(",");
                statement.execute(params![key, year, month, i64::from(*hour), grid, bands]).map_err(text)?;
            }
        }
        transaction.commit().map_err(text)
    }

    /// Stores an observation. Returns false if the same decode was already there.
    pub fn insert(&self, o: &Observation) -> Result<bool, String> {
        let changed = self
            .lock()
            .execute(
                "INSERT OR IGNORE INTO observations (time_utc, dial_hz, band, df_hz, snr_db, dt_s,
                    mode, message, kind, sender, addressee, grid, grid_source, distance_km,
                    bearing_deg, rx_grid, origin, provider, low_confidence, settling)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16,
                    ?17, ?18, ?19, ?20)",
                params![
                    o.time_utc,
                    o.dial_hz as i64,
                    o.band,
                    o.df_hz,
                    o.snr_db,
                    o.dt_s,
                    o.mode,
                    o.message,
                    o.kind,
                    o.sender,
                    o.addressee,
                    o.grid,
                    o.grid_source,
                    o.distance_km,
                    o.bearing_deg,
                    o.rx_grid,
                    o.origin,
                    o.provider,
                    o.low_confidence,
                    o.settling,
                ],
            )
            .map_err(text)?;
        Ok(changed == 1)
    }

    fn select(&self, filter: &str, parameters: &[i64]) -> Result<Vec<Observation>, String> {
        let connection = self.lock();
        let mut statement = connection
            .prepare(&format!(
                "SELECT time_utc, dial_hz, band, df_hz, snr_db, dt_s, mode, message, kind, sender,
                    addressee, grid, grid_source, distance_km, bearing_deg, rx_grid, origin,
                    provider, low_confidence, settling
                 FROM observations {filter}"
            ))
            .map_err(text)?;
        let rows = statement
            .query_map(rusqlite::params_from_iter(parameters), |r| {
                Ok(Observation {
                    time_utc: r.get(0)?,
                    dial_hz: r.get::<_, i64>(1)? as u64,
                    band: r.get(2)?,
                    df_hz: r.get(3)?,
                    snr_db: r.get(4)?,
                    dt_s: r.get(5)?,
                    mode: r.get(6)?,
                    message: r.get(7)?,
                    kind: r.get(8)?,
                    sender: r.get(9)?,
                    addressee: r.get(10)?,
                    grid: r.get(11)?,
                    grid_source: r.get(12)?,
                    distance_km: r.get(13)?,
                    bearing_deg: r.get(14)?,
                    rx_grid: r.get(15)?,
                    origin: r.get(16)?,
                    provider: r.get(17)?,
                    low_confidence: r.get(18)?,
                    settling: r.get(19)?,
                })
            })
            .map_err(text)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(text)
    }

    /// For each UTC hour, how many clock hours since `since_utc` this
    /// receiver was listening in, on any band: from the recorded listening
    /// and from the decodes themselves, which logs read later also give.
    pub fn listened_hours_by_hour(&self, since_utc: i64) -> Result<[u32; 24], String> {
        let mut slots: HashSet<i64> = HashSet::new();
        for interval in self.listening_intervals()? {
            let mut time = interval.start_utc.max(since_utc);
            time -= time.rem_euclid(3600);
            while time < interval.end_utc {
                slots.insert(time / 3600);
                time += 3600;
            }
        }
        {
            let connection = self.lock();
            let mut statement = connection
                .prepare("SELECT DISTINCT time_utc / 3600 FROM observations WHERE time_utc >= ?1")
                .map_err(text)?;
            let rows = statement.query_map([since_utc], |r| r.get::<_, i64>(0)).map_err(text)?;
            for row in rows {
                slots.insert(row.map_err(text)?);
            }
        }
        let mut hours = [0u32; 24];
        for slot in slots {
            hours[slot.rem_euclid(24) as usize] += 1;
        }
        Ok(hours)
    }

    /// Every station heard since `since_utc` that sent a locator.
    pub fn station_places(&self, since_utc: i64) -> Result<Vec<StationPlace>, String> {
        let connection = self.lock();
        let mut statement = connection
            .prepare(
                "SELECT sender, grid, time_utc FROM observations
                 WHERE time_utc >= ?1 AND sender IS NOT NULL AND grid IS NOT NULL AND settling = 0
                 ORDER BY time_utc",
            )
            .map_err(text)?;
        let rows = statement
            .query_map([since_utc], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, i64>(2)?)))
            .map_err(text)?;
        let mut places: BTreeMap<String, StationPlace> = BTreeMap::new();
        for row in rows {
            let (sender, grid, time) = row.map_err(text)?;
            let callsign = bare(&sender);
            let place = places
                .entry(callsign.clone())
                .or_insert(StationPlace { callsign, grid: String::new(), hours: 0 });
            place.grid = grid;
            place.hours |= 1 << ((time.rem_euclid(86_400) / 3600) as u32);
        }
        Ok(places.into_values().collect())
    }

    /// The latest locator each of these callsigns sent, by callsign.
    fn last_grids(&self, calls: &BTreeSet<String>) -> Result<BTreeMap<String, String>, String> {
        let connection = self.lock();
        let mut statement = connection
            .prepare(
                "SELECT grid FROM observations WHERE sender = ?1 AND grid IS NOT NULL
                 ORDER BY time_utc DESC LIMIT 1",
            )
            .map_err(text)?;
        let mut grids = BTreeMap::new();
        for call in calls {
            let grid: Option<String> = statement.query_row([call], |r| r.get(0)).optional().map_err(text)?;
            if let Some(grid) = grid {
                grids.insert(call.clone(), grid);
            }
        }
        Ok(grids)
    }

    /// Distant stations heard sending signal reports to `own_call` or to
    /// stations near this receiver since `since_utc`, one per station and
    /// band. The receiver is where each decode was made, or `receiver` when
    /// the decode does not say.
    pub fn hearing_your_area(
        &self,
        since_utc: i64,
        band: Option<&str>,
        own_call: Option<&str>,
        receiver: Option<LatLon>,
    ) -> Result<Vec<HearingStation>, String> {
        let own = own_call.map(bare).filter(|c| !c.is_empty());
        let reports = self.select(
            "WHERE time_utc >= ?1 AND kind IN ('report', 'rogerReport') AND sender IS NOT NULL
               AND addressee IS NOT NULL AND grid IS NOT NULL AND settling = 0
             ORDER BY time_utc, id",
            &[since_utc],
        )?;
        let reports: Vec<Observation> =
            reports.into_iter().filter(|o| band.is_none_or(|wanted| wanted == o.band)).collect();
        let recipients: BTreeSet<String> = reports
            .iter()
            .filter_map(|o| o.addressee.as_deref().map(bare))
            .filter(|call| own.as_deref() != Some(call.as_str()))
            .collect();
        let grids = self.last_grids(&recipients)?;

        let mut stations: BTreeMap<(String, String), HearingStation> = BTreeMap::new();
        for o in reports {
            let (Some(sender), Some(addressee), Some(grid)) = (&o.sender, &o.addressee, &o.grid) else { continue };
            let Some(report_db) = crate::wsjtx::ft8text::parse(&o.message).report_db else { continue };
            let here = o.rx_grid.as_deref().and_then(|g| crate::geo::from_maidenhead(g).ok()).or(receiver);
            let (Some(here), Ok(there)) = (here, crate::geo::from_maidenhead(grid)) else { continue };
            let sender_km = crate::geo::distance_km(here, there);
            let recipient = bare(addressee);
            let heard_you = own.as_deref() == Some(recipient.as_str());
            let recipient_km = if heard_you {
                0.0
            } else {
                let Some(position) = grids.get(&recipient).and_then(|g| crate::geo::from_maidenhead(g).ok()) else {
                    continue;
                };
                crate::geo::distance_km(here, position)
            };
            if !heard_you && !near_this_station(recipient_km, sender_km) {
                continue;
            }
            let station = stations.entry((o.band.clone(), bare(sender))).or_insert(HearingStation {
                callsign: bare(sender),
                grid: grid.clone(),
                lat: there.lat,
                lon: there.lon,
                band: o.band.clone(),
                distance_km: sender_km,
                bearing_deg: crate::geo::bearing_deg(here, there),
                best_report_db: report_db,
                reported: Vec::new(),
                heard_you: false,
                last_utc: o.time_utc,
            });
            station.best_report_db = station.best_report_db.max(report_db);
            station.heard_you |= heard_you;
            station.last_utc = o.time_utc;
            match station.reported.iter_mut().find(|r| r.callsign == recipient) {
                Some(seen) => seen.report_db = seen.report_db.max(report_db),
                None => station.reported.push(ReportedStation { callsign: recipient, distance_km: recipient_km, report_db }),
            }
        }
        let mut stations: Vec<HearingStation> = stations.into_values().collect();
        stations.sort_by(|a, b| b.last_utc.cmp(&a.last_utc).then(a.callsign.cmp(&b.callsign)));
        Ok(stations)
    }

    /// The latest observations, newest first.
    pub fn recent(&self, limit: u32) -> Result<Vec<Observation>, String> {
        self.select("ORDER BY time_utc DESC, id DESC LIMIT ?1", &[i64::from(limit)])
    }

    /// Every observation in time order, for comparing whole runs.
    pub fn all(&self) -> Result<Vec<Observation>, String> {
        self.select("ORDER BY time_utc, id", &[])
    }

    pub fn count(&self) -> Result<i64, String> {
        self.lock().query_row("SELECT COUNT(*) FROM observations", [], |r| r.get(0)).map_err(text)
    }

    /// The locator a station last sent in one of its own messages.
    pub fn remembered_grid(&self, callsign: &str) -> Result<Option<String>, String> {
        self.lock()
            .query_row(
                "SELECT grid FROM observations
                 WHERE sender = ?1 AND grid_source = 'message'
                 ORDER BY time_utc DESC LIMIT 1",
                [callsign],
                |r| r.get(0),
            )
            .optional()
            .map_err(text)
    }

    /// Opens a listening interval and returns its id.
    pub fn open_interval(&self, start_utc: i64, dial_hz: u64, band: &str, mode: &str, provider: &str) -> Result<i64, String> {
        let connection = self.lock();
        connection
            .execute(
                "INSERT INTO listening_intervals (start_utc, end_utc, dial_hz, band, mode, provider)
                 VALUES (?1, ?1, ?2, ?3, ?4, ?5)",
                params![start_utc, dial_hz as i64, band, mode, provider],
            )
            .map_err(text)?;
        Ok(connection.last_insert_rowid())
    }

    /// Moves an interval's end forward. It never moves back.
    pub fn extend_interval(&self, id: i64, end_utc: i64) -> Result<(), String> {
        self.lock()
            .execute(
                "UPDATE listening_intervals SET end_utc = MAX(end_utc, ?2) WHERE id = ?1",
                params![id, end_utc],
            )
            .map(|_| ())
            .map_err(text)
    }

    /// Per-band tallies for decodes and listening time in `[from, to)`.
    fn tally(&self, from: i64, to: i64) -> Result<BTreeMap<String, Tally>, String> {
        let mut bands: BTreeMap<String, Tally> = BTreeMap::new();
        {
            let connection = self.lock();
            let mut statement = connection
                .prepare(
                    "SELECT band, dial_hz, mode, MAX(start_utc, ?1), MIN(end_utc, ?2)
                     FROM listening_intervals WHERE end_utc > ?1 AND start_utc < ?2",
                )
                .map_err(text)?;
            let rows = statement
                .query_map([from, to], |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, i64>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, i64>(3)?,
                        r.get::<_, i64>(4)?,
                    ))
                })
                .map_err(text)?;
            for row in rows {
                let (band, dial_hz, mode, start, end) = row.map_err(text)?;
                let entry = bands.entry(band).or_insert_with(|| Tally::new(dial_hz as u64));
                entry.listened_seconds += end - start;
                entry.periods += (end - start) as f64 / period_seconds(&mode);
            }
        }

        // Oldest first, so each station ends up with its latest position.
        let observations =
            self.select("WHERE time_utc >= ?1 AND time_utc < ?2 AND settling = 0 ORDER BY time_utc, id", &[from, to])?;
        for o in observations {
            let entry = bands.entry(o.band).or_insert_with(|| Tally::new(o.dial_hz));
            entry.snrs.push(f64::from(o.snr_db));
            entry.grids.extend(o.grid);
            if let Some(sender) = o.sender {
                let place = entry.stations.entry(sender).or_insert(None);
                if let (Some(distance), Some(bearing)) = (o.distance_km, o.bearing_deg) {
                    *place = Some((distance, bearing));
                }
            }
        }
        Ok(bands)
    }

    /// Activity per band in `[since_utc, now)`. A band that was listened to
    /// but produced no decodes is included, with zero counts.
    pub fn band_activity(&self, since_utc: i64, now: i64) -> Result<Vec<BandActivity>, String> {
        let mut previous = self.tally(since_utc - (now - since_utc), since_utc)?;
        let mut activity: Vec<BandActivity> = self
            .tally(since_utc, now)?
            .into_iter()
            .map(|(band, mut t)| {
                t.snrs.sort_by(f64::total_cmp);
                let mut distances: Vec<f64> = t.stations.values().flatten().map(|(km, _)| *km).collect();
                let mut sectors = [0; SECTORS];
                for (_, bearing) in t.stations.values().flatten() {
                    sectors[sector(*bearing)] += 1;
                }
                let before = previous.remove(&band);
                BandActivity {
                    dial_hz: t.dial_hz,
                    listened_seconds: t.listened_seconds,
                    periods: t.periods,
                    decodes: t.snrs.len(),
                    decodes_per_period: (t.periods > 0.0).then(|| t.snrs.len() as f64 / t.periods),
                    unique_callsigns: t.stations.len(),
                    unique_grids: t.grids.len(),
                    p90_snr_db: percentile(&t.snrs, 0.9),
                    median_snr_db: median(&mut t.snrs),
                    located_stations: distances.len(),
                    max_distance_km: distances.iter().copied().reduce(f64::max),
                    long_distance_stations: distances.iter().filter(|km| **km > LONG_DISTANCE_KM).count(),
                    median_distance_km: median(&mut distances),
                    sectors,
                    previous_unique_callsigns: before.as_ref().map_or(0, |b| b.stations.len()),
                    previous_periods: before.as_ref().map_or(0.0, |b| b.periods),
                    band,
                }
            })
            .collect();
        activity.sort_by_key(|a| a.dial_hz);
        Ok(activity)
    }

    /// Stations heard since `since_utc` whose locator is known, optionally on
    /// one band. Each appears once per band, at its most recent locator.
    pub fn heard_stations(&self, since_utc: i64, band: Option<&str>) -> Result<Vec<HeardStation>, String> {
        let mut stations: BTreeMap<(String, String), HeardStation> = BTreeMap::new();
        let observations =
            self.select("WHERE time_utc >= ?1 AND settling = 0 ORDER BY time_utc, id", &[since_utc])?;
        for o in observations {
            let (Some(callsign), Some(grid)) = (o.sender, o.grid) else { continue };
            if band.is_some_and(|wanted| wanted != o.band) {
                continue;
            }
            let Ok(position) = crate::geo::from_maidenhead(&grid) else { continue };
            let station = stations.entry((o.band.clone(), callsign.clone())).or_insert(HeardStation {
                callsign,
                grid: String::new(),
                lat: 0.0,
                lon: 0.0,
                band: o.band,
                decodes: 0,
                best_snr_db: i32::MIN,
                last_heard_utc: 0,
                distance_km: None,
                bearing_deg: None,
            });
            station.grid = grid;
            station.lat = position.lat;
            station.lon = position.lon;
            station.decodes += 1;
            station.best_snr_db = station.best_snr_db.max(o.snr_db);
            station.last_heard_utc = o.time_utc;
            station.distance_km = o.distance_km;
            station.bearing_deg = o.bearing_deg;
        }
        Ok(stations.into_values().collect())
    }
}

/// Running totals for one band.
struct Tally {
    dial_hz: u64,
    listened_seconds: i64,
    periods: f64,
    snrs: Vec<f64>,
    grids: HashSet<String>,
    /// Each station heard, with its latest distance and bearing if located.
    stations: BTreeMap<String, Option<(f64, f64)>>,
}

impl Tally {
    fn new(dial_hz: u64) -> Self {
        Self {
            dial_hz,
            listened_seconds: 0,
            periods: 0.0,
            snrs: Vec::new(),
            grids: HashSet::new(),
            stations: BTreeMap::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) fn observation(time_utc: i64, sender: &str, snr_db: i32) -> Observation {
        Observation {
            time_utc,
            dial_hz: 14_074_000,
            band: "20 m".into(),
            df_hz: 1200,
            snr_db,
            dt_s: 0.2,
            mode: "FT8".into(),
            message: format!("CQ {sender} FN42"),
            kind: "cq".into(),
            sender: Some(sender.into()),
            addressee: None,
            grid: Some("FN42".into()),
            grid_source: Some("message".into()),
            distance_km: Some(1500.0),
            bearing_deg: Some(45.0),
            rx_grid: Some("EM73".into()),
            origin: ORIGIN_LOCAL.into(),
            provider: "test".into(),
            low_confidence: false,
            settling: false,
        }
    }

    /// Times the search over a real database, opened read-only by copy:
    /// `HFP_DB=path/to/observations.db cargo test --lib hearing_on_real_data -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn hearing_on_real_data() {
        let Ok(path) = std::env::var("HFP_DB") else { return };
        let copy = std::env::temp_dir().join("hfp-hearing-check.db");
        std::fs::copy(&path, &copy).unwrap();
        let db = Database::open(&copy).unwrap();
        let started = std::time::Instant::now();
        let found = db.hearing_your_area(0, None, None, None).unwrap();
        let reports: usize = found.iter().map(|s| s.reported.len()).sum();
        println!("{} stations hearing this area ({} reports) in {:?}", found.len(), reports, started.elapsed());
        for s in found.iter().take(5) {
            println!("  {} {} {} {:.0} km best {} dB, reported {:?}", s.band, s.callsign, s.grid, s.distance_km, s.best_report_db,
                s.reported.iter().map(|r| format!("{} {:.0} km", r.callsign, r.distance_km)).collect::<Vec<_>>());
        }
        let _ = std::fs::remove_file(&copy);
    }

    #[test]
    fn places_stations_by_their_last_locator_and_the_hours_they_were_on() {
        let db = Database::in_memory().unwrap();
        let at = |time: i64, call: &str, grid: &str| Observation {
            message: format!("CQ {call} {grid} {time}"),
            grid: Some(grid.into()),
            ..observation(time, call, -5)
        };
        db.insert(&at(14 * 3600, "K1ABC", "FN42")).unwrap();
        db.insert(&at(86_400 + 15 * 3600, "K1ABC", "FN43")).unwrap();
        db.insert(&at(2 * 3600, "G4AAA", "IO91")).unwrap();
        let places = db.station_places(0).unwrap();
        assert_eq!(
            places,
            [
                StationPlace { callsign: "G4AAA".into(), grid: "IO91".into(), hours: 1 << 2 },
                StationPlace { callsign: "K1ABC".into(), grid: "FN43".into(), hours: 1 << 14 | 1 << 15 },
            ]
        );
        assert_eq!(db.station_places(86_400).unwrap().len(), 1);
    }

    #[test]
    fn near_this_station_grows_with_the_senders_distance() {
        assert!(near_this_station(300.0, 1000.0));
        assert!(!near_this_station(301.0, 1000.0));
        assert!(near_this_station(900.0, 6700.0));
        assert!(!near_this_station(1100.0, 9000.0), "never beyond 1000 km");
        assert!(!near_this_station(200.0, 350.0), "the recipient must be nearer here than there");
    }

    #[test]
    fn finds_distant_stations_reporting_this_one_or_its_neighbours() {
        let db = Database::in_memory().unwrap();
        let cq = |time: i64, call: &str, grid: &str| Observation {
            message: format!("CQ {call} {grid}"),
            grid: Some(grid.into()),
            ..observation(time, call, -5)
        };
        let report = |time: i64, sender: &str, grid: &str, to: &str, value: &str| Observation {
            message: format!("{to} {sender} {value}"),
            kind: "report".into(),
            sender: Some(sender.into()),
            addressee: Some(to.into()),
            grid: Some(grid.into()),
            ..observation(time, sender, -14)
        };
        // A neighbour about 100 km away and a station in California.
        db.insert(&cq(100, "N4NB", "EM74")).unwrap();
        db.insert(&cq(115, "W6FAR", "CM87")).unwrap();
        // London reports both, and this station.
        db.insert(&report(200, "G0XYZ", "IO91", "N4NB", "-07")).unwrap();
        db.insert(&report(215, "G0XYZ", "IO91", "W6FAR", "-02")).unwrap();
        db.insert(&report(230, "G0XYZ", "IO91", "<KK4ODA>", "-12")).unwrap();

        let found = db.hearing_your_area(0, None, Some("kk4oda"), None).unwrap();
        assert_eq!(found.len(), 1);
        let london = &found[0];
        assert_eq!((london.callsign.as_str(), london.best_report_db, london.heard_you), ("G0XYZ", -7, true));
        let reported: Vec<(&str, i32)> = london.reported.iter().map(|r| (r.callsign.as_str(), r.report_db)).collect();
        assert_eq!(reported, [("N4NB", -7), ("KK4ODA", -12)], "California is too far from here to count");
        assert!(london.distance_km > 6000.0 && (40.0..60.0).contains(&london.bearing_deg));
        assert!(db.hearing_your_area(0, Some("40 m"), None, None).unwrap().is_empty());
        assert!(db.hearing_your_area(300, None, None, None).unwrap().is_empty());
    }

    #[test]
    fn stores_and_returns_observations_newest_first() {
        let db = Database::in_memory().unwrap();
        let (first, second) = (observation(100, "K1ABC", -12), observation(115, "W9XYZ", -3));
        assert!(db.insert(&first).unwrap());
        assert!(db.insert(&second).unwrap());
        assert_eq!(db.recent(10).unwrap(), [second.clone(), first.clone()]);
        assert_eq!(db.recent(1).unwrap(), [second]);
        assert_eq!(db.count().unwrap(), 2);
    }

    #[test]
    fn the_same_decode_is_stored_once() {
        let db = Database::in_memory().unwrap();
        let o = observation(100, "K1ABC", -12);
        assert!(db.insert(&o).unwrap());
        assert!(!db.insert(&o).unwrap());
        assert_eq!(db.count().unwrap(), 1);
    }

    #[test]
    fn remembers_the_locator_a_station_last_sent() {
        let db = Database::in_memory().unwrap();
        db.insert(&observation(100, "K1ABC", -12)).unwrap();
        let mut moved = observation(200, "K1ABC", -12);
        moved.grid = Some("EN37".into());
        moved.message = "CQ K1ABC EN37".into();
        db.insert(&moved).unwrap();
        // A locator filled in from memory is not itself evidence of where the station is.
        let mut recalled = observation(300, "K1ABC", -12);
        recalled.grid = Some("AA00".into());
        recalled.grid_source = Some("remembered".into());
        recalled.message = "W9XYZ K1ABC -05".into();
        db.insert(&recalled).unwrap();

        assert_eq!(db.remembered_grid("K1ABC").unwrap().as_deref(), Some("EN37"));
        assert_eq!(db.remembered_grid("N0NE").unwrap(), None);
    }

    /// The Phase 5 exit test: every metric checked against a session small
    /// enough to count by hand.
    #[test]
    fn metrics_match_a_hand_counted_session() {
        let db = Database::in_memory().unwrap();
        let heard = |time: i64, sender: &str, snr: i32, km: f64, bearing: f64, grid: &str| {
            let mut o = observation(time, sender, snr);
            o.message = format!("CQ {sender} {grid} {time}");
            o.grid = Some(grid.into());
            o.distance_km = Some(km);
            o.bearing_deg = Some(bearing);
            db.insert(&o).unwrap();
        };

        // The span under test is 1000-1600: ten minutes on 20 m, 40 periods of 15 s.
        let interval = db.open_interval(1000, 14_074_000, "20 m", "FT8", "test").unwrap();
        db.extend_interval(interval, 1600).unwrap();
        // K1ABC: three decodes, 1,500 km to the north-east.
        heard(1010, "K1ABC", -10, 1500.0, 45.0, "FN42");
        heard(1040, "K1ABC", -12, 1500.0, 45.0, "FN42");
        heard(1070, "K1ABC", -8, 1500.0, 45.0, "FN42");
        // W9XYZ: two decodes, 900 km just west of north.
        heard(1025, "W9XYZ", -4, 900.0, 350.0, "EN37");
        heard(1055, "W9XYZ", -6, 900.0, 350.0, "EN37");
        // JA1ZZZ: one decode, 11,000 km to the north-west.
        heard(1100, "JA1ZZZ", -20, 11_000.0, 330.0, "PM95");
        // EA1AAA: two decodes, 6,800 km to the north-east.
        heard(1130, "EA1AAA", -15, 6800.0, 60.0, "IN73");
        heard(1160, "EA1AAA", -17, 6800.0, 60.0, "IN73");
        // One free-text decode with no sender.
        let mut free = observation(1190, "X", -22);
        free.message = "TNX BOB 73 GL".into();
        free.sender = None;
        free.grid = None;
        free.distance_km = None;
        free.bearing_deg = None;
        db.insert(&free).unwrap();
        // One decode while the receiver was changing frequency: not counted.
        let mut settling = observation(1200, "VK2AAA", -1);
        settling.settling = true;
        db.insert(&settling).unwrap();

        // The ten minutes before: five minutes of listening, two stations.
        let earlier = db.open_interval(700, 14_074_000, "20 m", "FT8", "test").unwrap();
        db.extend_interval(earlier, 1000).unwrap();
        heard(710, "K1ABC", -9, 1500.0, 45.0, "FN42");
        heard(725, "N5AAA", -9, 1200.0, 270.0, "EM12");

        let activity = db.band_activity(1000, 1600).unwrap();
        assert_eq!(activity.len(), 1);
        let a = &activity[0];
        assert_eq!((a.band.as_str(), a.listened_seconds, a.periods), ("20 m", 600, 40.0));
        // 3 + 2 + 1 + 2 + 1 free text = 9 decodes in 40 periods.
        assert_eq!(a.decodes, 9);
        assert_eq!(a.decodes_per_period, Some(0.225));
        assert_eq!((a.unique_callsigns, a.unique_grids), (4, 4));
        // SNRs in order: -22 -20 -17 -15 -12 -10 -8 -6 -4. Middle is -12; nine tenths of nine is the ninth.
        assert_eq!(a.median_snr_db, Some(-12.0));
        assert_eq!(a.p90_snr_db, Some(-4.0));
        // Distances, one per station: 900, 1500, 6800, 11000.
        assert_eq!(a.located_stations, 4);
        assert_eq!(a.median_distance_km, Some(4150.0));
        assert_eq!(a.max_distance_km, Some(11_000.0));
        assert_eq!(a.long_distance_stations, 2);
        // North: W9XYZ. North-east: K1ABC and EA1AAA. North-west: JA1ZZZ.
        assert_eq!(a.sectors, [1, 2, 0, 0, 0, 0, 0, 1]);
        // Before: 300 s is 20 periods, with K1ABC and N5AAA.
        assert_eq!((a.previous_unique_callsigns, a.previous_periods), (2, 20.0));
    }

    #[test]
    fn sectors_are_centred_on_the_compass_points() {
        assert_eq!(sector(0.0), 0);
        assert_eq!(sector(22.4), 0);
        assert_eq!(sector(22.5), 1);
        assert_eq!(sector(90.0), 2);
        assert_eq!(sector(337.4), 7);
        assert_eq!(sector(337.5), 0);
        assert_eq!(sector(359.9), 0);
    }

    #[test]
    fn a_band_listened_to_in_silence_is_reported_with_zero_counts() {
        let db = Database::in_memory().unwrap();
        let quiet = db.open_interval(1000, 7_074_000, "40 m", "FT4", "test").unwrap();
        db.extend_interval(quiet, 1300).unwrap();
        db.insert(&observation(1010, "K1ABC", -12)).unwrap();

        let activity = db.band_activity(1000, 1600).unwrap();
        let forty = &activity[0];
        // FT4 periods are 7.5 s, so 300 s is 40 of them.
        assert_eq!((forty.band.as_str(), forty.decodes, forty.periods), ("40 m", 0, 40.0));
        assert_eq!(forty.decodes_per_period, Some(0.0));
        assert_eq!((forty.median_snr_db, forty.max_distance_km), (None, None));
        // 20 m has a decode but no listening time on record, so no rate.
        assert_eq!(activity[1].decodes_per_period, None);

        // Only the part of an interval inside the span counts.
        assert_eq!(db.band_activity(1200, 1600).unwrap()[0].listened_seconds, 100);
    }

    #[test]
    fn heard_stations_are_listed_once_per_band_at_their_latest_locator() {
        let db = Database::in_memory().unwrap();
        db.insert(&observation(100, "K1ABC", -12)).unwrap();
        let mut louder = observation(130, "K1ABC", -3);
        louder.message = "CQ K1ABC EN37".into();
        louder.grid = Some("EN37".into());
        db.insert(&louder).unwrap();
        let mut other_band = observation(160, "K1ABC", -7);
        other_band.band = "40 m".into();
        other_band.dial_hz = 7_074_000;
        db.insert(&other_band).unwrap();
        let mut unplaced = observation(190, "W9XYZ", -7);
        unplaced.grid = None;
        unplaced.message = "K1ABC W9XYZ -07".into();
        db.insert(&unplaced).unwrap();

        let all = db.heard_stations(0, None).unwrap();
        assert_eq!(all.len(), 2, "one per band; the station without a locator is left out");
        let twenty = db.heard_stations(0, Some("20 m")).unwrap();
        assert_eq!(twenty.len(), 1);
        let k1abc = &twenty[0];
        assert_eq!((k1abc.callsign.as_str(), k1abc.grid.as_str()), ("K1ABC", "EN37"));
        assert_eq!((k1abc.decodes, k1abc.best_snr_db, k1abc.last_heard_utc), (2, -3, 130));
        // EN37 spans 47-48 N and 94-92 W.
        assert_eq!((k1abc.lat, k1abc.lon), (47.5, -93.0));
        assert!(db.heard_stations(150, Some("20 m")).unwrap().is_empty());
    }

    #[test]
    fn refuses_a_database_from_a_newer_version() {
        let dir = std::env::temp_dir().join(format!("hfp-db-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("observations.db");
        {
            let db = Database::open(&path).unwrap();
            db.insert(&observation(1, "K1ABC", 0)).unwrap();
            db.lock().pragma_update(None, "user_version", 99).unwrap();
        }
        assert!(Database::open(&path).err().unwrap().contains("newer version"));
    }

    #[test]
    fn reopening_keeps_the_data() {
        let dir = std::env::temp_dir().join(format!("hfp-db-reopen-{}", std::process::id()));
        let path = dir.join("observations.db");
        Database::open(&path).unwrap().insert(&observation(1, "K1ABC", 0)).unwrap();
        assert_eq!(Database::open(&path).unwrap().count().unwrap(), 1);
    }
}
