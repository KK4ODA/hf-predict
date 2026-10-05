//! What the station has heard, kept in a local SQLite database.
//!
//! Two tables: every decode, and every interval the receiver spent listening
//! on a frequency. The intervals are what make "nothing heard" meaningful:
//! without them it cannot be told apart from "not listening".

use std::collections::{BTreeMap, HashSet};
use std::path::Path;
use std::sync::{Mutex, MutexGuard, PoisonError};

use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;

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
";
const SCHEMA_VERSION: i64 = 1;

/// Decoded by this station's own receiver, as opposed to a reporting network.
pub const ORIGIN_LOCAL: &str = "LOCAL";

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

/// What was heard on one band over a span of time.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BandActivity {
    pub band: String,
    pub dial_hz: u64,
    pub listened_seconds: i64,
    pub decodes: usize,
    pub unique_callsigns: usize,
    pub unique_grids: usize,
    pub median_snr_db: Option<f64>,
    pub max_distance_km: Option<f64>,
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

    /// Activity per band from `since_utc` on. A band that was listened to
    /// but produced no decodes is included, with zero counts.
    pub fn band_activity(&self, since_utc: i64) -> Result<Vec<BandActivity>, String> {
        struct Tally {
            dial_hz: u64,
            listened_seconds: i64,
            snrs: Vec<f64>,
            callsigns: HashSet<String>,
            grids: HashSet<String>,
            max_distance_km: Option<f64>,
        }
        let mut bands: BTreeMap<String, Tally> = BTreeMap::new();
        let tally = |bands: &mut BTreeMap<String, Tally>, band: String, dial_hz: u64| {
            bands.entry(band).or_insert_with(|| Tally {
                dial_hz,
                listened_seconds: 0,
                snrs: Vec::new(),
                callsigns: HashSet::new(),
                grids: HashSet::new(),
                max_distance_km: None,
            });
        };

        {
            let connection = self.lock();
            let mut statement = connection
                .prepare(
                    "SELECT band, dial_hz, MAX(start_utc, ?1), end_utc FROM listening_intervals
                     WHERE end_utc > ?1",
                )
                .map_err(text)?;
            let rows = statement
                .query_map([since_utc], |r| {
                    Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?, r.get::<_, i64>(2)?, r.get::<_, i64>(3)?))
                })
                .map_err(text)?;
            for row in rows {
                let (band, dial_hz, start, end) = row.map_err(text)?;
                tally(&mut bands, band.clone(), dial_hz as u64);
                bands.get_mut(&band).unwrap().listened_seconds += end - start;
            }
        }

        for o in self.select("WHERE time_utc >= ?1 AND settling = 0", &[since_utc])? {
            tally(&mut bands, o.band.clone(), o.dial_hz);
            let entry = bands.get_mut(&o.band).unwrap();
            entry.snrs.push(f64::from(o.snr_db));
            entry.callsigns.extend(o.sender);
            entry.grids.extend(o.grid);
            entry.max_distance_km = match (entry.max_distance_km, o.distance_km) {
                (Some(a), Some(b)) => Some(a.max(b)),
                (a, b) => a.or(b),
            };
        }

        let mut activity: Vec<BandActivity> = bands
            .into_iter()
            .map(|(band, mut t)| BandActivity {
                band,
                dial_hz: t.dial_hz,
                listened_seconds: t.listened_seconds,
                decodes: t.snrs.len(),
                unique_callsigns: t.callsigns.len(),
                unique_grids: t.grids.len(),
                median_snr_db: median(&mut t.snrs),
                max_distance_km: t.max_distance_km,
            })
            .collect();
        activity.sort_by_key(|a| a.dial_hz);
        Ok(activity)
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

    #[test]
    fn activity_counts_what_was_heard_and_how_long_was_listened() {
        let db = Database::in_memory().unwrap();
        let interval = db.open_interval(1000, 14_074_000, "20 m", "FT8", "test").unwrap();
        db.extend_interval(interval, 1600).unwrap();
        db.extend_interval(interval, 1500).unwrap();
        // Listened to 40 m too, and heard nothing there.
        let quiet = db.open_interval(1600, 7_074_000, "40 m", "FT8", "test").unwrap();
        db.extend_interval(quiet, 1900).unwrap();

        for (time, sender, snr) in [(1010, "K1ABC", -10), (1025, "W9XYZ", -4), (1040, "K1ABC", -16)] {
            db.insert(&observation(time, sender, snr)).unwrap();
        }
        let mut far = observation(1055, "JA1ZZZ", -20);
        far.grid = Some("PM95".into());
        far.distance_km = Some(11_000.0);
        db.insert(&far).unwrap();
        let mut settling = observation(1070, "VK2AAA", -1);
        settling.settling = true;
        db.insert(&settling).unwrap();

        let activity = db.band_activity(0).unwrap();
        assert_eq!(activity.len(), 2);
        let (forty, twenty) = (&activity[0], &activity[1]);
        assert_eq!((forty.band.as_str(), forty.decodes, forty.listened_seconds), ("40 m", 0, 300));
        assert_eq!(forty.median_snr_db, None);

        assert_eq!(twenty.listened_seconds, 600);
        assert_eq!(twenty.decodes, 4, "the settling decode is left out");
        assert_eq!(twenty.unique_callsigns, 3);
        assert_eq!(twenty.unique_grids, 2);
        assert_eq!(twenty.median_snr_db, Some(-13.0));
        assert_eq!(twenty.max_distance_km, Some(11_000.0));

        // Only the part of an interval inside the window counts.
        assert_eq!(db.band_activity(1300).unwrap()[1].listened_seconds, 300);
        assert_eq!(db.band_activity(1300).unwrap()[1].decodes, 0);
    }

    #[test]
    fn refuses_a_database_from_a_newer_version() {
        let dir = std::env::temp_dir().join(format!("hfp-db-{}", std::process::id()));
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
