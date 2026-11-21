// SPDX-License-Identifier: MIT OR Apache-2.0
//! Saved places, the GPS chosen, and which of them gives the position in use.
//!
//! A place is a latitude and longitude, WGS 84, with a name; the grid square is worked out from it
//! and never kept as the truth. Entered as a grid square, a place is that square's centre and says
//! so: a 6-character square is about 4.6 by 7.7 km at Atlanta's latitude.
//!
//! The position in use ([`choose`]): the operator picks a place, the GPS, or **automatic**: the GPS
//! while its fix is current or recent; otherwise the place most recently saved within
//! [`NEAR_KM`] of the GPS's last fix; otherwise the default place. Whatever stands in for the GPS
//! is said ("GPS silent for 6 minutes: using Home").

use std::fs;
use std::io;
use std::path::Path;

use radioprop_geo::{distance_km, from_maidenhead, to_maidenhead, LatLon};
use serde::{Deserialize, Serialize};

use crate::read::{Link, Source, Status};
use crate::station::StationProfile;
use crate::track::{Fix, Freshness, Hearing};

/// How near the GPS's last fix a saved place must be to stand in for it, in kilometres.
pub const NEAR_KM: f64 = 2.0;

/// How a place was set.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Origin {
    /// Latitude and longitude typed in.
    Typed,
    /// Picked on a map.
    Map,
    /// A grid square's centre.
    Grid,
    /// Saved from the GPS.
    Gps,
}

/// A saved place.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Place {
    pub id: String,
    pub name: String,
    pub lat: f64,
    pub lon: f64,
    /// Ground elevation, metres above sea level, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ground_m: Option<f64>,
    #[serde(default)]
    pub default: bool,
    /// The station profile picking this place picks.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub station: Option<String>,
    pub source: Origin,
    /// The square it was entered as, for a place that is a square's centre.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grid: Option<String>,
    /// When it was saved, in seconds since 1970.
    #[serde(default)]
    pub saved_at: u64,
}

impl Place {
    /// A place at a grid square's centre.
    ///
    /// # Errors
    ///
    /// `grid` is not a 4-, 6- or 8-character Maidenhead locator.
    pub fn from_grid(name: &str, grid: &str, saved_at: u64) -> Result<Place, String> {
        let centre = from_maidenhead(grid)?;
        let grid = normalized(grid.trim());
        Ok(Place {
            id: String::new(),
            name: name.trim().to_owned(),
            lat: centre.lat,
            lon: centre.lon,
            ground_m: None,
            default: false,
            station: None,
            source: Origin::Grid,
            grid: Some(grid),
            saved_at,
        })
    }

    /// A place at a latitude and longitude.
    ///
    /// # Errors
    ///
    /// The latitude or longitude is out of range.
    pub fn at(
        name: &str,
        lat: f64,
        lon: f64,
        source: Origin,
        saved_at: u64,
    ) -> Result<Place, String> {
        let p = LatLon::new(lat, lon)?;
        Ok(Place {
            id: String::new(),
            name: name.trim().to_owned(),
            lat: p.lat,
            lon: p.lon,
            ground_m: None,
            default: false,
            station: None,
            source,
            grid: None,
            saved_at,
        })
    }

    /// Its position.
    #[must_use]
    pub fn position(&self) -> LatLon {
        LatLon {
            lat: self.lat,
            lon: self.lon,
        }
    }

    /// Its 6-character grid square.
    #[must_use]
    pub fn square(&self) -> String {
        to_maidenhead(self.position())
    }
}

/// A locator as written by convention: field in capitals, subsquare in small letters.
fn normalized(grid: &str) -> String {
    grid.chars()
        .enumerate()
        .map(|(i, c)| {
            if i < 2 {
                c.to_ascii_uppercase()
            } else {
                c.to_ascii_lowercase()
            }
        })
        .collect()
}

/// What gives the position in use.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "use", rename_all = "lowercase")]
pub enum Choice {
    /// The GPS while it is current or recent, otherwise a saved place.
    #[default]
    Automatic,
    /// Always this place.
    Place { id: String },
    /// The GPS; a saved place only while it has no usable fix, and said so.
    Gps,
}

/// The places a program keeps, its GPS and its choice: kept by each program in a file of its own.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Book {
    pub places: Vec<Place>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gps: Option<Source>,
    pub choice: Choice,
    /// The station profiles (§6.5 of RadioPost's design).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub stations: Vec<StationProfile>,
    /// The profile chosen; none, the one the place in use names, else the default place's, else
    /// the first.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub station: Option<String>,
}

impl Book {
    /// Reads a book; a file that is not there is an empty book.
    ///
    /// # Errors
    ///
    /// The file cannot be read, or is not a book.
    pub fn load(path: &Path) -> Result<Book, String> {
        match fs::read_to_string(path) {
            Ok(text) => serde_json::from_str(&text)
                .map_err(|e| format!("{} is not a list of places: {e}", path.display())),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Book::default()),
            Err(e) => Err(format!("cannot read {}: {e}", path.display())),
        }
    }

    /// Writes a book whole, by way of a temporary file, so that a crash leaves the old one.
    ///
    /// # Errors
    ///
    /// The file cannot be written.
    pub fn save(&self, path: &Path) -> Result<(), String> {
        let text = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir).map_err(|e| format!("cannot write {}: {e}", dir.display()))?;
        }
        let temporary = path.with_extension("json.new");
        fs::write(&temporary, text + "\n")
            .and_then(|()| fs::rename(&temporary, path))
            .map_err(|e| format!("cannot write {}: {e}", path.display()))
    }

    /// Adds a place, giving it an id from its name when it has none; the first place, or one
    /// marked default, becomes the default. Returns its id.
    pub fn add(&mut self, mut place: Place) -> String {
        if place.id.is_empty() || self.get(&place.id).is_some() {
            place.id = self.new_id(&place.name);
        }
        if self.places.is_empty() {
            place.default = true;
        }
        if place.default {
            for p in &mut self.places {
                p.default = false;
            }
        }
        let id = place.id.clone();
        self.places.push(place);
        id
    }

    /// Removes a place; the first left becomes the default if it was. False: no such place.
    pub fn remove(&mut self, id: &str) -> bool {
        let Some(i) = self.places.iter().position(|p| p.id == id) else {
            return false;
        };
        let was_default = self.places.remove(i).default;
        if was_default {
            if let Some(p) = self.places.first_mut() {
                p.default = true;
            }
        }
        if self.choice == (Choice::Place { id: id.to_owned() }) {
            self.choice = Choice::Automatic;
        }
        true
    }

    /// Makes a place the default. False: no such place.
    pub fn set_default(&mut self, id: &str) -> bool {
        if self.get(id).is_none() {
            return false;
        }
        for p in &mut self.places {
            p.default = p.id == id;
        }
        true
    }

    /// A place by its id.
    #[must_use]
    pub fn get(&self, id: &str) -> Option<&Place> {
        self.places.iter().find(|p| p.id == id)
    }

    /// The default place: the one marked, or the only one.
    #[must_use]
    pub fn default_place(&self) -> Option<&Place> {
        self.places
            .iter()
            .find(|p| p.default)
            .or(match self.places.as_slice() {
                [only] => Some(only),
                _ => None,
            })
    }

    /// Adds a station profile, or replaces the one with its id; one with no id gets one from its
    /// name. Returns its id.
    ///
    /// # Errors
    ///
    /// The profile has no name, or numbers out of reason.
    pub fn put_station(&mut self, mut profile: StationProfile) -> Result<String, String> {
        if let Some(why) = profile.problem() {
            return Err(why);
        }
        profile.name = profile.name.trim().to_owned();
        if let Some(i) = self.stations.iter().position(|s| s.id == profile.id) {
            self.stations[i] = profile;
            return Ok(self.stations[i].id.clone());
        }
        profile.id = slug_id("st", &profile.name, |id| {
            self.stations.iter().any(|s| s.id == id)
        });
        let id = profile.id.clone();
        self.stations.push(profile);
        Ok(id)
    }

    /// Removes a station profile, and every place's mention of it. False: no such profile.
    pub fn remove_station(&mut self, id: &str) -> bool {
        let before = self.stations.len();
        self.stations.retain(|s| s.id != id);
        if self.stations.len() == before {
            return false;
        }
        for p in &mut self.places {
            if p.station.as_deref() == Some(id) {
                p.station = None;
            }
        }
        if self.station.as_deref() == Some(id) {
            self.station = None;
        }
        true
    }

    /// A station profile by its id.
    #[must_use]
    pub fn station(&self, id: &str) -> Option<&StationProfile> {
        self.stations.iter().find(|s| s.id == id)
    }

    /// The profile in use with the position `chosen`: the one chosen, else the one the place in
    /// use names, else the default place's, else the first.
    #[must_use]
    pub fn station_in_use(&self, chosen: &Chosen) -> Option<&StationProfile> {
        let named = |id: Option<&String>| id.and_then(|id| self.station(id));
        let place = match chosen.here.as_ref().map(|h| &h.from) {
            Some(From::Place { id, .. }) => self.get(id),
            _ => None,
        };
        named(self.station.as_ref())
            .or_else(|| named(place.and_then(|p| p.station.as_ref())))
            .or_else(|| named(self.default_place().and_then(|p| p.station.as_ref())))
            .or_else(|| self.stations.first())
    }

    fn new_id(&self, name: &str) -> String {
        slug_id("pl", name, |id| self.get(id).is_some())
    }
}

/// An id from a name: `prefix-` and the name's letters and digits, made unique by `-2`, `-3`, ….
fn slug_id(prefix: &str, name: &str, taken: impl Fn(&str) -> bool) -> String {
    {
        let slug: String = name
            .to_lowercase()
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
            .collect::<String>()
            .split('-')
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join("-");
        let fallback = if prefix == "pl" { "place" } else { "station" };
        let base = format!(
            "{prefix}-{}",
            if slug.is_empty() { fallback } else { &slug }
        );
        (1..)
            .map(|n| {
                if n == 1 {
                    base.clone()
                } else {
                    format!("{base}-{n}")
                }
            })
            .find(|id| !taken(id))
            .unwrap_or(base)
    }
}

/// Where the position in use comes from.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "from", rename_all = "lowercase")]
pub enum From {
    /// The GPS's fix, `age_s` old.
    Gps { age_s: u64, freshness: Freshness },
    /// A saved place.
    Place {
        id: String,
        name: String,
        source: Origin,
    },
}

/// The position in use.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Here {
    pub lat: f64,
    pub lon: f64,
    /// Its 6-character grid square.
    pub grid: String,
    #[serde(flatten)]
    pub from: From,
}

/// What [`choose`] decided.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Chosen {
    /// None: no place is saved and the GPS has no usable fix.
    pub here: Option<Here>,
    /// Why the position is not what was asked for, or not the GPS's present fix, in plain words.
    pub note: Option<String>,
}

/// The position in use at `now_ms`, from the book and what its GPS is doing (`None`: no GPS
/// running).
#[must_use]
pub fn choose(book: &Book, gps: Option<&Status>, now_ms: u64) -> Chosen {
    let usable = gps.and_then(Status::usable_fix);
    match &book.choice {
        Choice::Place { id } => match book.get(id) {
            Some(place) => chosen(from_place(place), None),
            None => fall_back(book, None, "the place chosen is gone".into()),
        },
        Choice::Automatic | Choice::Gps => {
            if let Some(fix) = usable {
                let age_s = fix.age_s(now_ms);
                let freshness = fix.freshness(now_ms);
                let note = (freshness == Freshness::Recent)
                    .then(|| format!("GPS fix {} old", span(age_s)));
                return chosen(from_fix(&fix, age_s, freshness), note);
            }
            let why = match (gps, &book.choice) {
                (Some(status), _) => gps_words(status, now_ms),
                (None, Choice::Gps) => "no GPS is set up".into(),
                (None, _) => String::new(),
            };
            fall_back(book, gps.and_then(|s| s.fix), why)
        }
    }
}

fn chosen(here: Here, note: Option<String>) -> Chosen {
    Chosen {
        here: Some(here),
        note,
    }
}

/// A saved place standing in: the one most recently saved near the GPS's last fix, else the
/// default place.
fn fall_back(book: &Book, last: Option<Fix>, why: String) -> Chosen {
    let near = last.and_then(|fix| {
        let at = LatLon {
            lat: fix.lat,
            lon: fix.lon,
        };
        book.places
            .iter()
            .filter(|p| distance_km(p.position(), at) <= NEAR_KM)
            .max_by_key(|p| p.saved_at)
    });
    let Some(place) = near.or_else(|| book.default_place()) else {
        let note = if why.is_empty() {
            "no position: save a place or set up a GPS".to_owned()
        } else {
            format!("{why}, and no place is saved")
        };
        return Chosen {
            here: None,
            note: Some(note),
        };
    };
    let note = (!why.is_empty()).then(|| format!("{why}: using {}", place.name));
    chosen(from_place(place), note)
}

fn from_place(place: &Place) -> Here {
    Here {
        lat: place.lat,
        lon: place.lon,
        grid: place.square(),
        from: From::Place {
            id: place.id.clone(),
            name: place.name.clone(),
            source: place.source,
        },
    }
}

fn from_fix(fix: &Fix, age_s: u64, freshness: Freshness) -> Here {
    let at = LatLon {
        lat: fix.lat,
        lon: fix.lon,
    };
    Here {
        lat: fix.lat,
        lon: fix.lon,
        grid: to_maidenhead(at),
        from: From::Gps { age_s, freshness },
    }
}

/// Why the GPS gives no position, in the operator's words.
fn gps_words(status: &Status, now_ms: u64) -> String {
    let since = |ms: u64| span(now_ms.saturating_sub(ms) / 1000);
    match (&status.link, status.hearing) {
        (Link::Failed { since_ms, .. }, _) => format!("GPS not connected for {}", since(*since_ms)),
        (_, Hearing::Silent { since_ms }) => format!("GPS silent for {}", since(since_ms)),
        (_, Hearing::NoFix { since_ms }) => format!("GPS without a fix for {}", since(since_ms)),
        (Link::Opening, _) | (_, Hearing::Nothing) => "GPS not heard yet".into(),
        (_, Hearing::Positions) => match status.age_s {
            Some(age) => format!("GPS fix {} old", span(age)),
            None => "GPS not heard yet".into(),
        },
    }
}

/// A span of time in words: seconds under two minutes, minutes under two hours, then hours.
#[must_use]
pub fn span(seconds: u64) -> String {
    match seconds {
        s if s < 120 => format!("{s} s"),
        s if s < 7200 => format!("{} minutes", s / 60),
        s => format!("{} hours", s / 3600),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nmea::Quality;

    const NOW: u64 = 1_795_144_522_000;

    fn book() -> Book {
        let mut b = Book::default();
        b.add(Place::from_grid("Home", "em73TR", 100).expect("a square"));
        b.add(Place::at("Field Day", 34.10, -84.50, Origin::Typed, 200).expect("a place"));
        b
    }

    fn status(fix: Option<Fix>, hearing: Hearing, link: Link) -> Status {
        Status {
            source: Source::Udp { port: 11123 },
            link,
            hearing,
            fix,
            freshness: fix.map(|f| f.freshness(NOW)),
            age_s: fix.map(|f| f.age_s(NOW)),
            held: false,
            refused: 0,
        }
    }

    fn fix(lat: f64, lon: f64, age_s: u64) -> Fix {
        Fix {
            lat,
            lon,
            alt_m: None,
            quality: Quality::Gps,
            at_ms: NOW - age_s * 1000,
        }
    }

    fn open() -> Link {
        Link::Open {
            since_ms: NOW - 600_000,
            detail: None,
        }
    }

    #[test]
    fn a_grid_square_is_its_centre_and_says_so() {
        let b = book();
        let home = b.get("pl-home").expect("home");
        assert_eq!(home.source, Origin::Grid);
        assert_eq!(home.grid.as_deref(), Some("EM73tr"));
        assert_eq!(home.square(), "EM73tr");
        assert!(home.default, "the first place is the default");
        assert_eq!(b.get("pl-field-day").map(|p| p.default), Some(false));
    }

    #[test]
    fn automatic_uses_the_gps_while_its_fix_is_current_or_recent() {
        let b = book();
        let s = status(Some(fix(33.80, -84.40, 3)), Hearing::Positions, open());
        let c = choose(&b, Some(&s), NOW);
        assert!(matches!(
            c.here.as_ref().map(|h| &h.from),
            Some(From::Gps {
                freshness: Freshness::Current,
                ..
            })
        ));
        assert_eq!(c.note, None);
        let s = status(Some(fix(33.80, -84.40, 45)), Hearing::Positions, open());
        assert_eq!(
            choose(&b, Some(&s), NOW).note.as_deref(),
            Some("GPS fix 45 s old")
        );
    }

    #[test]
    fn a_stale_fix_gives_way_to_a_place_near_it_then_to_the_default() {
        let b = book();
        // silent six minutes, last fixed at Field Day: Field Day stands in
        let s = status(
            Some(fix(34.105, -84.505, 360)),
            Hearing::Silent {
                since_ms: NOW - 360_000,
            },
            open(),
        );
        let c = choose(&b, Some(&s), NOW);
        assert_eq!(
            c.note.as_deref(),
            Some("GPS silent for 6 minutes: using Field Day")
        );
        // last fixed far from every place: the default
        let s = status(
            Some(fix(40.0, -100.0, 400)),
            Hearing::NoFix {
                since_ms: NOW - 400_000,
            },
            open(),
        );
        assert_eq!(
            choose(&b, Some(&s), NOW).note.as_deref(),
            Some("GPS without a fix for 6 minutes: using Home")
        );
        // not connected
        let failed = Link::Failed {
            since_ms: NOW - 30_000,
            text: "the connection closed".into(),
        };
        let s = status(None, Hearing::Nothing, failed);
        assert_eq!(
            choose(&b, Some(&s), NOW).note.as_deref(),
            Some("GPS not connected for 30 s: using Home")
        );
    }

    #[test]
    fn a_place_chosen_is_used_whatever_the_gps_says() {
        let mut b = book();
        b.choice = Choice::Place {
            id: "pl-field-day".into(),
        };
        let s = status(Some(fix(33.80, -84.40, 1)), Hearing::Positions, open());
        let c = choose(&b, Some(&s), NOW);
        assert_eq!(c.here.map(|h| h.grid), Some("EM74sc".into()));
        assert_eq!(c.note, None);
        // and when it is removed, automatic again
        assert!(b.remove("pl-field-day"));
        assert_eq!(b.choice, Choice::Automatic);
    }

    #[test]
    fn nothing_saved_and_no_gps_says_what_to_do() {
        let c = choose(&Book::default(), None, NOW);
        assert_eq!(c.here, None);
        assert_eq!(
            c.note.as_deref(),
            Some("no position: save a place or set up a GPS")
        );
        let b = Book {
            choice: Choice::Gps,
            ..Book::default()
        };
        assert_eq!(
            choose(&b, None, NOW).note.as_deref(),
            Some("no GPS is set up, and no place is saved")
        );
    }

    #[test]
    fn a_book_round_trips_through_its_file_and_a_missing_file_is_empty() {
        let dir = std::env::temp_dir().join(format!("radioprop-place-{}", std::process::id()));
        let path = dir.join("places.json");
        assert_eq!(Book::load(&path), Ok(Book::default()));
        let mut b = book();
        b.gps = Some(Source::Tcp {
            host: "172.20.10.1".into(),
            port: 11123,
        });
        b.save(&path).expect("saved");
        assert_eq!(Book::load(&path), Ok(b));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn the_station_in_use_is_the_one_chosen_or_the_places_or_the_first() {
        let mut b = book();
        let home = b
            .put_station(StationProfile {
                name: "Home, FTDX10".into(),
                ..StationProfile::default()
            })
            .expect("kept");
        let mobile = b
            .put_station(StationProfile {
                name: "Mobile, TS-480".into(),
                vehicle: true,
                ..StationProfile::default()
            })
            .expect("kept");
        assert_eq!(
            (home.as_str(), mobile.as_str()),
            ("st-home-ftdx10", "st-mobile-ts-480")
        );
        let at_home = choose(&b, None, NOW);
        // nothing names one: the first
        assert_eq!(
            b.station_in_use(&at_home).map(|s| s.id.as_str()),
            Some("st-home-ftdx10")
        );
        // Field Day names the mobile profile, and is the place in use
        b.places[1].station = Some(mobile.clone());
        b.choice = Choice::Place {
            id: "pl-field-day".into(),
        };
        let there = choose(&b, None, NOW);
        assert_eq!(
            b.station_in_use(&there).map(|s| s.id.as_str()),
            Some("st-mobile-ts-480")
        );
        // a profile chosen wins
        b.station = Some(home.clone());
        assert_eq!(
            b.station_in_use(&there).map(|s| s.id.as_str()),
            Some("st-home-ftdx10")
        );
        // removed, it is forgotten everywhere
        assert!(b.remove_station(&mobile));
        assert_eq!(b.places[1].station, None);
        assert!(b
            .put_station(StationProfile {
                name: "  ".into(),
                ..StationProfile::default()
            })
            .is_err());
        // changed in place, by its id
        let mut changed = b.station(&home).cloned().expect("kept");
        changed.hf.power_w = Some(50.0);
        assert_eq!(b.put_station(changed).expect("kept"), home);
        assert_eq!(b.station(&home).and_then(|s| s.hf.power_w), Some(50.0));
    }

    #[test]
    fn ids_come_from_names_and_stay_unique() {
        let mut b = Book::default();
        let a = b.add(Place::from_grid("Home", "EM73", 1).expect("square"));
        let c = b.add(Place::from_grid("Home", "EM74", 2).expect("square"));
        let d = b.add(Place::from_grid("  ", "EM75", 3).expect("square"));
        assert_eq!(
            (a.as_str(), c.as_str(), d.as_str()),
            ("pl-home", "pl-home-2", "pl-place")
        );
        assert!(b.set_default("pl-home-2"));
        assert_eq!(b.default_place().map(|p| p.id.as_str()), Some("pl-home-2"));
        assert!(b.remove("pl-home-2"));
        assert_eq!(b.default_place().map(|p| p.id.as_str()), Some("pl-home"));
    }
}
