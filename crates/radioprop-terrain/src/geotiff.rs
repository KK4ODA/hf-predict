// SPDX-License-Identifier: MIT OR Apache-2.0
//! Reads the elevation GeoTIFFs Copernicus publishes: its DEM's Cloud-Optimized GeoTIFFs, one per
//! 1° tile, 32-bit floating-point heights in metres, tiled, DEFLATE-compressed with the
//! floating-point predictor (TIFF Technical Note 3), geographic coordinates (EPSG:4326).
//!
//! Only the full-resolution image (the first IFD) is read; the overviews after it are not. What
//! a file holds beyond that subset is refused with the tag and value named, never guessed at.

use std::io::Read;

/// A grid of heights: `heights[row * width + col]`, row 0 the northernmost. Sample `(0, 0)` is at
/// (`north`, `west`), each row `dlat` degrees south of the one before and each column `dlon`
/// degrees east. With `point` set (GeoTIFF's *pixel is point*, as Copernicus sets it) the
/// coordinates are the samples' own; otherwise they are their cells' north-west corners, and
/// [`Raster::read`] moves them to the cells' centres.
#[derive(Clone, Debug, PartialEq)]
pub struct Raster {
    pub width: usize,
    pub height: usize,
    pub west: f64,
    pub north: f64,
    pub dlon: f64,
    pub dlat: f64,
    pub heights: Vec<f32>,
}

// TIFF tags (TIFF 6.0, GeoTIFF 1.1)
const IMAGE_WIDTH: u16 = 256;
const IMAGE_LENGTH: u16 = 257;
const BITS_PER_SAMPLE: u16 = 258;
const COMPRESSION: u16 = 259;
const STRIP_OFFSETS: u16 = 273;
const SAMPLES_PER_PIXEL: u16 = 277;
const ROWS_PER_STRIP: u16 = 278;
const STRIP_BYTE_COUNTS: u16 = 279;
const PLANAR_CONFIGURATION: u16 = 284;
const PREDICTOR: u16 = 317;
const TILE_WIDTH: u16 = 322;
const TILE_LENGTH: u16 = 323;
const TILE_OFFSETS: u16 = 324;
const TILE_BYTE_COUNTS: u16 = 325;
const SAMPLE_FORMAT: u16 = 339;
const MODEL_PIXEL_SCALE: u16 = 33550;
const MODEL_TIEPOINT: u16 = 33922;
const GEO_KEY_DIRECTORY: u16 = 34735;
/// GeoTIFF's `GTRasterTypeGeoKey`: 1 is *pixel is area*, 2 *pixel is point*.
const RASTER_TYPE_KEY: u16 = 1025;
/// GeoTIFF's `GeographicTypeGeoKey`: 4326 is WGS 84.
const GEOGRAPHIC_TYPE_KEY: u16 = 2048;

struct Reader<'a> {
    data: &'a [u8],
    little: bool,
}

impl Reader<'_> {
    fn bytes(&self, at: usize, n: usize) -> Result<&[u8], String> {
        self.data
            .get(at..at.checked_add(n).ok_or("an offset past the end")?)
            .ok_or_else(|| format!("the file ends before byte {}", at + n))
    }
    fn u16(&self, at: usize) -> Result<u16, String> {
        let b: [u8; 2] = self.bytes(at, 2)?.try_into().expect("two bytes");
        Ok(if self.little { u16::from_le_bytes(b) } else { u16::from_be_bytes(b) })
    }
    fn u32(&self, at: usize) -> Result<u32, String> {
        let b: [u8; 4] = self.bytes(at, 4)?.try_into().expect("four bytes");
        Ok(if self.little { u32::from_le_bytes(b) } else { u32::from_be_bytes(b) })
    }
    fn f64(&self, at: usize) -> Result<f64, String> {
        let b: [u8; 8] = self.bytes(at, 8)?.try_into().expect("eight bytes");
        Ok(if self.little { f64::from_le_bytes(b) } else { f64::from_be_bytes(b) })
    }
}

/// One IFD entry's values, as numbers.
struct Entry {
    tag: u16,
    values: Vec<f64>,
}

fn entries(r: &Reader, ifd: usize) -> Result<Vec<Entry>, String> {
    let n = usize::from(r.u16(ifd)?);
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let at = ifd + 2 + 12 * i;
        let (tag, kind, count) = (r.u16(at)?, r.u16(at + 2)?, r.u32(at + 4)? as usize);
        let size = match kind {
            1 | 2 | 6 | 7 => 1, // BYTE, ASCII, SBYTE, UNDEFINED
            3 | 8 => 2,         // SHORT, SSHORT
            4 | 9 | 11 => 4,    // LONG, SLONG, FLOAT
            12 => 8,            // DOUBLE
            _ => continue,      // RATIONALs and the like: no tag read here uses them
        };
        let total = size * count;
        let base = if total <= 4 { at + 8 } else { r.u32(at + 8)? as usize };
        let mut values = Vec::with_capacity(count);
        if kind != 2 {
            for k in 0..count {
                let p = base + k * size;
                values.push(match kind {
                    1 | 6 | 7 => f64::from(r.bytes(p, 1)?[0]),
                    3 | 8 => f64::from(r.u16(p)?),
                    4 | 9 => f64::from(r.u32(p)?),
                    11 => f64::from(f32::from_bits(r.u32(p)?)),
                    _ => r.f64(p)?,
                });
            }
        }
        out.push(Entry { tag, values });
    }
    Ok(out)
}

impl Raster {
    /// Reads a Copernicus DEM GeoTIFF's full-resolution heights.
    ///
    /// # Errors
    ///
    /// Not a TIFF, or a TIFF outside what Copernicus writes: not 32-bit floats, a compression
    /// other than none or DEFLATE, a predictor other than none or floating point, more than one
    /// sample a pixel, not WGS 84 geographic coordinates; or damaged.
    pub fn read(data: &[u8]) -> Result<Self, String> {
        let little = match data.get(..4) {
            Some([b'I', b'I', 42, 0]) => true,
            Some([b'M', b'M', 0, 42]) => false,
            _ => return Err("not a TIFF (or a BigTIFF, which is not read)".into()),
        };
        let r = Reader { data, little };
        let list = entries(&r, r.u32(4)? as usize)?;
        let get = |tag: u16| list.iter().find(|e| e.tag == tag).map(|e| e.values.as_slice());
        let one = |tag: u16, default: Option<f64>| -> Result<f64, String> {
            get(tag)
                .and_then(|v| v.first().copied())
                .or(default)
                .ok_or_else(|| format!("no TIFF tag {tag}"))
        };
        let width = one(IMAGE_WIDTH, None)? as usize;
        let height = one(IMAGE_LENGTH, None)? as usize;
        let want = |tag: u16, default: f64, allowed: &[f64], what: &str| -> Result<f64, String> {
            let v = one(tag, Some(default))?;
            if allowed.contains(&v) {
                Ok(v)
            } else {
                Err(format!("{what} {v} (TIFF tag {tag}) is not read"))
            }
        };
        want(BITS_PER_SAMPLE, 1.0, &[32.0], "bits per sample")?;
        want(SAMPLE_FORMAT, 1.0, &[3.0], "sample format")?;
        want(SAMPLES_PER_PIXEL, 1.0, &[1.0], "samples per pixel")?;
        want(PLANAR_CONFIGURATION, 1.0, &[1.0], "planar configuration")?;
        // 8 is DEFLATE as TIFF 6.0's supplement gives it, 32946 the older code for the same
        let deflate = want(COMPRESSION, 1.0, &[1.0, 8.0, 32946.0], "compression")? != 1.0;
        let float_predictor = want(PREDICTOR, 1.0, &[1.0, 3.0], "predictor")? == 3.0;

        let keys = get(GEO_KEY_DIRECTORY).ok_or("no GeoTIFF key directory")?;
        let key = |id: u16| {
            keys.chunks(4)
                .skip(1)
                .find(|k| k.first() == Some(&f64::from(id)) && k.get(1) == Some(&0.0))
                .and_then(|k| k.get(3).copied())
        };
        if key(GEOGRAPHIC_TYPE_KEY) != Some(4326.0) {
            return Err("not WGS 84 geographic coordinates (EPSG:4326)".into());
        }
        let point = key(RASTER_TYPE_KEY) == Some(2.0);
        let scale = get(MODEL_PIXEL_SCALE).ok_or("no model pixel scale")?;
        let tie = get(MODEL_TIEPOINT).ok_or("no model tiepoint")?;
        if scale.len() < 2 || tie.len() < 6 {
            return Err("a short pixel scale or tiepoint".into());
        }
        let (dlon, dlat) = (scale[0], scale[1]);
        // the tiepoint ties raster (i, j) to (lon, lat); with pixel is area it is a cell's corner
        let half = if point { 0.0 } else { 0.5 };
        let west = tie[3] - (tie[0] - half) * dlon;
        let north = tie[4] + (tie[1] - half) * dlat;

        // the image's pieces: tiles, or strips as tiles the image's width wide
        let (piece_w, piece_h, offsets, counts) = match get(TILE_OFFSETS) {
            Some(offsets) => (
                one(TILE_WIDTH, None)? as usize,
                one(TILE_LENGTH, None)? as usize,
                offsets,
                get(TILE_BYTE_COUNTS).ok_or("no tile byte counts")?,
            ),
            None => (
                width,
                one(ROWS_PER_STRIP, Some(height as f64))?.min(height as f64) as usize,
                get(STRIP_OFFSETS).ok_or("neither tiles nor strips")?,
                get(STRIP_BYTE_COUNTS).ok_or("no strip byte counts")?,
            ),
        };
        if piece_w == 0 || piece_h == 0 || width == 0 || height == 0 {
            return Err("an empty image or tile".into());
        }
        let across = width.div_ceil(piece_w);
        let down = height.div_ceil(piece_h);
        if offsets.len() < across * down || counts.len() < across * down {
            return Err(format!("{} pieces listed, {} needed", offsets.len(), across * down));
        }
        let mut heights = vec![0f32; width * height];
        let piece_bytes = piece_w * piece_h * 4;
        for (k, (&at, &n)) in offsets.iter().zip(counts).enumerate().take(across * down) {
            let raw = r.bytes(at as usize, n as usize)?;
            let mut bytes = Vec::with_capacity(piece_bytes);
            if deflate {
                flate2::read::ZlibDecoder::new(raw)
                    .read_to_end(&mut bytes)
                    .map_err(|e| format!("piece {k} does not inflate: {e}"))?;
            } else {
                bytes.extend_from_slice(raw);
            }
            // a strip at the image's foot may be shorter than the others
            let rows = (bytes.len() / (piece_w * 4)).min(piece_h);
            if rows == 0 {
                return Err(format!("piece {k} holds {} bytes", bytes.len()));
            }
            let (pr, pc) = (k / across, k % across);
            for row in 0..rows {
                let line = &mut bytes[row * piece_w * 4..(row + 1) * piece_w * 4];
                let values = if float_predictor {
                    undo_float_predictor(line)
                } else {
                    line.chunks(4)
                        .map(|b| {
                            let b: [u8; 4] = b.try_into().expect("four bytes");
                            if little { f32::from_le_bytes(b) } else { f32::from_be_bytes(b) }
                        })
                        .collect()
                };
                let y = pr * piece_h + row;
                if y >= height {
                    break;
                }
                let x0 = pc * piece_w;
                let n = piece_w.min(width - x0);
                heights[y * width + x0..y * width + x0 + n].copy_from_slice(&values[..n]);
            }
        }
        Ok(Raster { width, height, west, north, dlon, dlat, heights })
    }
}

/// Undoes TIFF Technical Note 3's floating-point predictor on one row of 32-bit samples: the
/// bytes were differenced along the whole row, after being laid out as four planes, the most
/// significant byte of every sample first, whatever the file's byte order.
fn undo_float_predictor(line: &mut [u8]) -> Vec<f32> {
    for i in 1..line.len() {
        line[i] = line[i].wrapping_add(line[i - 1]);
    }
    let n = line.len() / 4;
    (0..n)
        .map(|k| f32::from_be_bytes([line[k], line[n + k], line[2 * n + k], line[3 * n + k]]))
        .collect()
}
