// SPDX-License-Identifier: MIT OR Apache-2.0
//! One elevation tile as it is kept on disk: heights in half-metres, 16 bits each, in square
//! blocks compressed one by one, so a path reads only the blocks it crosses.
//!
//! The layout, all integers little-endian:
//!
//! ```text
//! "RPTILE1\n"                        8 bytes
//! header length, then the header     u32, JSON: where the grid is and what it came from
//! block count, then for each block   u32; u64 offset from the file's start, u32 length
//! the blocks                         zlib streams of i16 half-metres, row by row; each value
//!                                    the difference from the one before it in its row, the
//!                                    first in a row from the first of the row above
//! ```
//!
//! Half-metres hold −16 383.5 to +16 383.5 m, every height on Earth with room, at a step well
//! under Copernicus's stated accuracy (under 4 m, LE90). Decimetres would not: 16 bits of them
//! stop at 3 276.7 m. A sample with no height is `i16::MIN`.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::geotiff::Raster;

const MAGIC: &[u8; 8] = b"RPTILE1\n";
/// Samples on a block's side.
pub const BLOCK: usize = 256;
/// What a sample with no height holds.
pub const NO_HEIGHT: i16 = i16::MIN;

/// Where a tile's grid is and what it came from.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Header {
    /// The data set, e.g. `copernicus-glo30`.
    pub source: String,
    /// The source's own file the tile was made from.
    pub from: String,
    pub width: usize,
    pub height: usize,
    /// The first sample's position: the north-west one.
    pub west: f64,
    pub north: f64,
    pub dlon: f64,
    pub dlat: f64,
    /// Samples on a block's side.
    pub block: usize,
}

fn half_metres(h: f32) -> i16 {
    if h.is_finite() {
        (f64::from(h) * 2.0).round().clamp(-32767.0, 32767.0) as i16
    } else {
        NO_HEIGHT
    }
}

/// Writes `raster` as a tile file at `path`, through a file beside it renamed into place, so a
/// tile is either whole or absent. Returns the file's size in bytes.
///
/// # Errors
///
/// The file cannot be written.
pub fn write(path: &Path, raster: &Raster, source: &str, from: &str) -> Result<u64, String> {
    let header = Header {
        source: source.into(),
        from: from.into(),
        width: raster.width,
        height: raster.height,
        west: raster.west,
        north: raster.north,
        dlon: raster.dlon,
        dlat: raster.dlat,
        block: BLOCK,
    };
    let json = serde_json::to_vec(&header).map_err(|e| e.to_string())?;
    let (across, down) = (raster.width.div_ceil(BLOCK), raster.height.div_ceil(BLOCK));
    let mut blocks = Vec::with_capacity(across * down);
    for by in 0..down {
        for bx in 0..across {
            let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::new(6));
            let (x0, y0) = (bx * BLOCK, by * BLOCK);
            let (w, h) = (BLOCK.min(raster.width - x0), BLOCK.min(raster.height - y0));
            let mut above = 0i16;
            for y in y0..y0 + h {
                let row = &raster.heights[y * raster.width + x0..y * raster.width + x0 + w];
                let mut before = above;
                for (i, &v) in row.iter().enumerate() {
                    let v = half_metres(v);
                    let delta = v.wrapping_sub(before);
                    z.write_all(&delta.to_le_bytes()).map_err(|e| e.to_string())?;
                    before = v;
                    if i == 0 {
                        above = v;
                    }
                }
            }
            blocks.push(z.finish().map_err(|e| e.to_string())?);
        }
    }
    let mut out = Vec::new();
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&u32::try_from(json.len()).map_err(|e| e.to_string())?.to_le_bytes());
    out.extend_from_slice(&json);
    out.extend_from_slice(&u32::try_from(blocks.len()).map_err(|e| e.to_string())?.to_le_bytes());
    let mut at = (out.len() + blocks.len() * 12) as u64;
    for b in &blocks {
        out.extend_from_slice(&at.to_le_bytes());
        out.extend_from_slice(&(b.len() as u32).to_le_bytes());
        at += b.len() as u64;
    }
    for b in &blocks {
        out.extend_from_slice(b);
    }
    let part = path.with_extension("part");
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    std::fs::write(&part, &out).map_err(|e| format!("{}: {e}", part.display()))?;
    std::fs::rename(&part, path).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(out.len() as u64)
}

/// A tile file opened for reading: its header and where its blocks are. Blocks are read and
/// decoded on demand; [`crate::store::Store`] keeps the recent ones.
#[derive(Debug)]
pub struct TileFile {
    pub header: Header,
    file: File,
    index: Vec<(u64, u32)>,
}

impl TileFile {
    /// Opens a tile file.
    ///
    /// # Errors
    ///
    /// It cannot be read, or is not a tile file.
    pub fn open(path: &Path) -> Result<Self, String> {
        let bad = |why: &str| format!("{}: {why}", path.display());
        let mut file = File::open(path).map_err(|e| bad(&e.to_string()))?;
        let mut magic = [0u8; 8];
        file.read_exact(&mut magic).map_err(|e| bad(&e.to_string()))?;
        if &magic != MAGIC {
            return Err(bad("not a terrain tile"));
        }
        let mut n = [0u8; 4];
        file.read_exact(&mut n).map_err(|e| bad(&e.to_string()))?;
        let mut json = vec![0u8; u32::from_le_bytes(n) as usize];
        file.read_exact(&mut json).map_err(|e| bad(&e.to_string()))?;
        let header: Header = serde_json::from_slice(&json).map_err(|e| bad(&e.to_string()))?;
        file.read_exact(&mut n).map_err(|e| bad(&e.to_string()))?;
        let count = u32::from_le_bytes(n) as usize;
        let want = header.width.div_ceil(header.block) * header.height.div_ceil(header.block);
        if count != want || header.block == 0 {
            return Err(bad(&format!("{count} blocks, {want} expected")));
        }
        let mut raw = vec![0u8; count * 12];
        file.read_exact(&mut raw).map_err(|e| bad(&e.to_string()))?;
        let index = raw
            .chunks(12)
            .map(|c| {
                (
                    u64::from_le_bytes(c[..8].try_into().expect("eight bytes")),
                    u32::from_le_bytes(c[8..].try_into().expect("four bytes")),
                )
            })
            .collect();
        Ok(TileFile { header, file, index })
    }

    /// The block holding sample (`row`, `col`): its number, and the sample's place in it.
    #[must_use]
    pub fn locate(&self, row: usize, col: usize) -> (usize, usize) {
        let b = self.header.block;
        let across = self.header.width.div_ceil(b);
        let w = b.min(self.header.width - (col / b) * b);
        ((row / b) * across + col / b, (row % b) * w + col % b)
    }

    /// Reads and decodes block `k`: its heights in half-metres, row by row, as wide as the block
    /// is (the last column of blocks may be narrower than the others).
    ///
    /// # Errors
    ///
    /// The file cannot be read there, or the block is damaged.
    pub fn block(&mut self, k: usize) -> Result<Vec<i16>, String> {
        let (at, len) = *self.index.get(k).ok_or_else(|| format!("no block {k}"))?;
        let mut raw = vec![0u8; len as usize];
        self.file
            .seek(SeekFrom::Start(at))
            .and_then(|_| self.file.read_exact(&mut raw))
            .map_err(|e| format!("block {k}: {e}"))?;
        let mut bytes = Vec::new();
        flate2::read::ZlibDecoder::new(raw.as_slice())
            .read_to_end(&mut bytes)
            .map_err(|e| format!("block {k} does not inflate: {e}"))?;
        let b = self.header.block;
        let across = self.header.width.div_ceil(b);
        let (bx, by) = (k % across, k / across);
        let w = b.min(self.header.width - bx * b);
        let h = b.min(self.header.height - by * b);
        if bytes.len() != w * h * 2 {
            return Err(format!("block {k} holds {} bytes, not {}", bytes.len(), w * h * 2));
        }
        let mut out = Vec::with_capacity(w * h);
        let mut above = 0i16;
        for y in 0..h {
            let mut before = above;
            for x in 0..w {
                let i = (y * w + x) * 2;
                let v = before.wrapping_add(i16::from_le_bytes([bytes[i], bytes[i + 1]]));
                out.push(v);
                before = v;
                if x == 0 {
                    above = v;
                }
            }
        }
        Ok(out)
    }
}
