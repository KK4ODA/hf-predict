// SPDX-License-Identifier: MIT OR Apache-2.0
//! The tiles kept on disk, and heights read from them.
//!
//! Each data set's tiles live in a folder of their own under the store's directory, as
//! `<source>/<tile>.rpt` (see [`crate::tile`]); a tile the data set does not have (the sea) is
//! `<tile>.absent`. Fetching is the program's: this crate names the URL ([`Source::url`]) and
//! converts what was fetched ([`Store::put`]), so it needs no network client of its own.
//!
//! A tile's samples run from its north edge, inclusive, to one sample short of its south edge,
//! and from its west edge to one sample short of its east edge: Copernicus's own tiles share
//! their south row and east column with their neighbours, and the copies on AWS leave them out.
//! A height between samples is read bilinearly, across tile edges where it falls on one.

use std::collections::hash_map::Entry;
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use radioprop_geo::LatLon;

use crate::geotiff::Raster;
use crate::tile::{self, TileFile, NO_HEIGHT};

/// An elevation data set.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Source {
    /// Copernicus DEM GLO-30 Public: 1″ (about 30 m), the surface radar saw in 2010–2015.
    Glo30,
    /// Copernicus DEM GLO-90: 3″ (about 90 m), the same surface.
    Glo90,
}

/// Copernicus's notice for what is made from its DEM, as its licences' Article 6(b) words it,
/// and Article 6(c)'s sentence, which goes with any distribution or communication of it.
pub const NOTICE_GLO30: &str = "produced using Copernicus WorldDEM-30 © DLR e.V. 2010-2014 and © \
    Airbus Defence and Space GmbH 2014-2018 provided under COPERNICUS by the European Union and \
    ESA; all rights reserved. The organisations in charge of the Copernicus programme by law or \
    by delegation do not incur any liability for any use of the Copernicus WorldDEM-30.";
/// The same for GLO-90, whose licence names WorldDEM™-90.
pub const NOTICE_GLO90: &str = "produced using Copernicus WorldDEM™-90 © DLR e.V. 2010-2014 and © \
    Airbus Defence and Space GmbH 2014-2018 provided under COPERNICUS by the European Union and \
    ESA; all rights reserved. The organisations in charge of the Copernicus programme by law or \
    by delegation do not incur any liability for any use of the Copernicus WorldDEM™-90.";

impl Source {
    /// The folder its tiles are kept in, and the name views give it.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Source::Glo30 => "copernicus-glo30",
            Source::Glo90 => "copernicus-glo90",
        }
    }

    /// What a view says the data is.
    #[must_use]
    pub fn words(self) -> &'static str {
        match self {
            Source::Glo30 => "Copernicus GLO-30 (surface: trees and buildings as the radar saw them, 2010–2015)",
            Source::Glo90 => "Copernicus GLO-90 (surface: trees and buildings as the radar saw them, 2010–2015)",
        }
    }

    #[must_use]
    pub fn notice(self) -> &'static str {
        match self {
            Source::Glo30 => NOTICE_GLO30,
            Source::Glo90 => NOTICE_GLO90,
        }
    }

    /// Its samples' spacing in latitude, degrees (in longitude it widens towards the poles).
    #[must_use]
    pub fn dlat(self) -> f64 {
        match self {
            Source::Glo30 => 1.0 / 3600.0,
            Source::Glo90 => 3.0 / 3600.0,
        }
    }

    /// The public bucket it is published in.
    #[must_use]
    pub fn bucket(self) -> &'static str {
        match self {
            Source::Glo30 => "https://copernicus-dem-30m.s3.amazonaws.com",
            Source::Glo90 => "https://copernicus-dem-90m.s3.amazonaws.com",
        }
    }

    /// The tile's name in the bucket, as `tileList.txt` lists it: `Copernicus_DSM_COG_10_N33_00_W085_00_DEM`.
    #[must_use]
    pub fn key(self, t: TileId) -> String {
        let res = match self {
            Source::Glo30 => 10,
            Source::Glo90 => 30,
        };
        let ns = if t.lat < 0 { 'S' } else { 'N' };
        let ew = if t.lon < 0 { 'W' } else { 'E' };
        format!(
            "Copernicus_DSM_COG_{res}_{ns}{:02}_00_{ew}{:03}_00_DEM",
            t.lat.unsigned_abs(),
            t.lon.unsigned_abs()
        )
    }

    /// Where to fetch the tile's GeoTIFF (about 40 MB at GLO-30 below 50°, 5 MB at GLO-90).
    #[must_use]
    pub fn url(self, t: TileId) -> String {
        let key = self.key(t);
        format!("{}/{key}/{key}.tif", self.bucket())
    }

    /// Where the list of the tiles it has is (`tileList.txt`, one key a line): a tile not in it
    /// is sea, or for GLO-30 Public one of the few not yet released (GLO-90 has those).
    #[must_use]
    pub fn list_url(self) -> String {
        format!("{}/tileList.txt", self.bucket())
    }
}

/// A 1° tile, named by its south-west corner.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TileId {
    pub lat: i16,
    pub lon: i16,
}

impl TileId {
    /// The tile a position's height is read in: latitudes above its south edge up to its north
    /// edge, longitudes from its west edge to short of its east edge.
    #[must_use]
    pub fn of(at: LatLon) -> TileId {
        let lat = (at.lat.ceil() as i16 - 1).clamp(-90, 89);
        let lon = (at.lon.floor() as i16).clamp(-180, 179);
        TileId { lat, lon }
    }

    /// Its name, as a file: `N33W085`.
    #[must_use]
    pub fn name(self) -> String {
        let ns = if self.lat < 0 { 'S' } else { 'N' };
        let ew = if self.lon < 0 { 'W' } else { 'E' };
        format!("{ns}{:02}{ew}{:03}", self.lat.unsigned_abs(), self.lon.unsigned_abs())
    }

    /// The tile to the south, and the one to the east (across the antimeridian too).
    #[must_use]
    pub fn south(self) -> TileId {
        TileId { lat: (self.lat - 1).max(-90), lon: self.lon }
    }
    #[must_use]
    pub fn east(self) -> TileId {
        TileId { lat: self.lat, lon: if self.lon == 179 { -180 } else { self.lon + 1 } }
    }
}

/// What the store has of a tile.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Have {
    /// Converted and on disk.
    Held,
    /// The data set has no such tile: the sea, at height 0.
    Absent,
    /// Not fetched yet.
    Missing,
}

/// A height read from the store.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Height {
    /// Metres above the geoid (EGM2008 for Copernicus).
    Metres(f64),
    /// A tile not fetched yet.
    Missing(TileId),
}

/// Blocks decoded and kept, the most recently used last.
const KEEP_BLOCKS: usize = 256;

struct Inner {
    open: HashMap<TileId, Option<TileFile>>,
    blocks: HashMap<(TileId, usize), Arc<Vec<i16>>>,
    order: VecDeque<(TileId, usize)>,
}

/// The tiles of one data set kept under a directory, with the recently used blocks in memory.
pub struct Store {
    dir: PathBuf,
    source: Source,
    inner: Mutex<Inner>,
}

impl Store {
    /// The store for `source` under `dir` (an application's cache directory).
    #[must_use]
    pub fn new(dir: &Path, source: Source) -> Store {
        Store {
            dir: dir.join(source.name()),
            source,
            inner: Mutex::new(Inner {
                open: HashMap::new(),
                blocks: HashMap::new(),
                order: VecDeque::new(),
            }),
        }
    }

    #[must_use]
    pub fn source(&self) -> Source {
        self.source
    }

    fn path(&self, t: TileId, ext: &str) -> PathBuf {
        self.dir.join(format!("{}.{ext}", t.name()))
    }

    /// What the store has of `t`.
    #[must_use]
    pub fn have(&self, t: TileId) -> Have {
        if self.path(t, "rpt").exists() {
            Have::Held
        } else if self.path(t, "absent").exists() {
            Have::Absent
        } else {
            Have::Missing
        }
    }

    /// Converts a tile's GeoTIFF, as fetched from [`Source::url`], and keeps it. Returns the
    /// size of the file kept, in bytes.
    ///
    /// # Errors
    ///
    /// The GeoTIFF cannot be read, or is not the tile asked for, or the file cannot be written.
    pub fn put(&self, t: TileId, geotiff: &[u8]) -> Result<u64, String> {
        let raster = Raster::read(geotiff).map_err(|e| format!("{}: {e}", t.name()))?;
        self.put_raster(t, &raster)
    }

    /// Keeps a tile's heights as read (or built: tests and tools use it). Returns the size of the
    /// file kept, in bytes.
    ///
    /// # Errors
    ///
    /// The raster is not the tile asked for, or the file cannot be written.
    pub fn put_raster(&self, t: TileId, raster: &Raster) -> Result<u64, String> {
        // the tile's first sample is its north-west corner
        let off = (raster.north - f64::from(t.lat + 1)).abs() + (raster.west - f64::from(t.lon)).abs();
        if off > raster.dlat {
            return Err(format!(
                "{}: the file starts at {:.4}, {:.4}, not at the tile's north-west corner",
                t.name(),
                raster.north,
                raster.west
            ));
        }
        let size = tile::write(&self.path(t, "rpt"), raster, self.source.name(), &self.source.key(t))?;
        let mut g = self.inner.lock().expect("store");
        g.open.remove(&t);
        g.blocks.retain(|(id, _), _| *id != t);
        g.order.retain(|(id, _)| *id != t);
        Ok(size)
    }

    /// Records that the data set has no tile `t` (it is not in [`Source::list_url`]'s list):
    /// heights there are the sea's, 0 m.
    ///
    /// # Errors
    ///
    /// The marker cannot be written.
    pub fn put_absent(&self, t: TileId) -> Result<(), String> {
        let p = self.path(t, "absent");
        std::fs::create_dir_all(&self.dir).map_err(|e| format!("{}: {e}", self.dir.display()))?;
        std::fs::write(&p, b"").map_err(|e| format!("{}: {e}", p.display()))?;
        self.inner.lock().expect("store").open.remove(&t);
        Ok(())
    }

    /// Tile `t`, opened once and kept: `None` when it is not fetched, `Some(None)` for the sea.
    fn opened<'a>(
        &self,
        open: &'a mut HashMap<TileId, Option<TileFile>>,
        t: TileId,
    ) -> Result<Option<&'a mut Option<TileFile>>, String> {
        Ok(Some(match open.entry(t) {
            Entry::Occupied(e) => e.into_mut(),
            Entry::Vacant(v) => v.insert(match self.have(t) {
                Have::Held => Some(TileFile::open(&self.path(t, "rpt"))?),
                Have::Absent => None,
                Have::Missing => return Ok(None),
            }),
        }))
    }

    /// The height at a grid sample: tile `t`'s row `row`, column `col`, where a column one past
    /// the tile's last is the next tile's first. `None` when the tile is not fetched.
    fn sample(&self, t: TileId, row: usize, col: usize) -> Result<Option<f64>, String> {
        let mut g = self.inner.lock().expect("store");
        let Inner { open, blocks, order } = &mut *g;
        let Some(file) = self.opened(open, t)? else {
            return Ok(None);
        };
        let Some(file) = file else {
            return Ok(Some(0.0)); // the sea
        };
        let (w, h) = (file.header.width, file.header.height);
        if row >= h || col >= w {
            let (next, row, col) = if row >= h { (t.south(), row - h, col) } else { (t.east(), row, col - w) };
            drop(g);
            // the next tile's columns are its own when its spacing differs (only across a
            // latitude band's edge, where rows go south): re-read the column there
            return if row >= h || col >= w || next == t {
                Ok(None)
            } else {
                self.sample(next, row, col)
            };
        }
        let (k, i) = file.locate(row, col);
        let block = if let Some(b) = blocks.get(&(t, k)) {
            Arc::clone(b)
        } else {
            let b = Arc::new(file.block(k)?);
            blocks.insert((t, k), Arc::clone(&b));
            order.push_back((t, k));
            if order.len() > KEEP_BLOCKS {
                if let Some(old) = order.pop_front() {
                    blocks.remove(&old);
                }
            }
            b
        };
        let v = block[i];
        Ok(if v == NO_HEIGHT { None } else { Some(f64::from(v) / 2.0) })
    }

    /// The grid of tile `t`: (north, west, dlat, dlon). The sea's is the data set's own spacing.
    fn grid(&self, t: TileId) -> Result<Option<(f64, f64, f64, f64)>, String> {
        let mut g = self.inner.lock().expect("store");
        let Some(file) = self.opened(&mut g.open, t)? else {
            return Ok(None);
        };
        Ok(Some(match file {
            Some(f) => (f.header.north, f.header.west, f.header.dlat, f.header.dlon),
            None => (f64::from(t.lat + 1), f64::from(t.lon), self.source.dlat(), self.source.dlat()),
        }))
    }

    /// The height along one row of samples, at longitude `lon`: linear between the samples
    /// either side, the east one in the next tile when it is past this one's edge.
    fn along_row(&self, t: TileId, row: usize, lon: f64) -> Result<Height, String> {
        let Some((_, west, _, dlon)) = self.grid(t)? else {
            return Ok(Height::Missing(t));
        };
        let x = ((lon - west) / dlon).max(0.0);
        let c0 = x.floor() as usize;
        let fx = x - c0 as f64;
        let a = self.sample(t, row, c0)?;
        let b = if fx > 0.0 { self.sample(t, row, c0 + 1)? } else { a };
        Ok(match (a, b) {
            (Some(a), Some(b)) => Height::Metres(a + (b - a) * fx),
            _ => Height::Missing(if a.is_none() { t } else { t.east() }),
        })
    }

    /// The height at `at`, read bilinearly between the four samples around it.
    ///
    /// # Errors
    ///
    /// A tile file is damaged.
    pub fn height(&self, at: LatLon) -> Result<Height, String> {
        let t = TileId::of(at);
        let Some((north, _, dlat, _)) = self.grid(t)? else {
            return Ok(Height::Missing(t));
        };
        let y = ((north - at.lat) / dlat).max(0.0);
        let r0 = y.floor() as usize;
        let fy = y - r0 as f64;
        let upper = self.along_row(t, r0, at.lon)?;
        if fy == 0.0 {
            return Ok(upper);
        }
        // the row below may be the first of the tile to the south
        let rows = self.rows(t)?;
        let lower = if r0 + 1 < rows {
            self.along_row(t, r0 + 1, at.lon)?
        } else {
            let s = t.south();
            if s == t {
                upper
            } else {
                self.along_row(s, 0, at.lon)?
            }
        };
        Ok(match (upper, lower) {
            (Height::Metres(a), Height::Metres(b)) => Height::Metres(a + (b - a) * fy),
            (Height::Missing(m), _) | (_, Height::Missing(m)) => Height::Missing(m),
        })
    }

    /// How many rows tile `t` has (the sea's as many as the data set's tiles have).
    fn rows(&self, t: TileId) -> Result<usize, String> {
        let g = self.inner.lock().expect("store");
        Ok(match g.open.get(&t) {
            Some(Some(f)) => f.header.height,
            _ => (1.0 / self.source.dlat()).round() as usize,
        })
    }
}
