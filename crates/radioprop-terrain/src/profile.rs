// SPDX-License-Identifier: MIT OR Apache-2.0
//! The ground along a path: heights sampled along the great circle from one end to the other, at
//! the elevation data's own spacing (about 31 m for GLO-30), each read bilinearly.

use std::collections::BTreeSet;

use radioprop_geo::{distance_km, intermediate, LatLon};

use crate::store::{Height, Source, Store, TileId};

/// Kilometres in a degree of a great circle (a 6 371 km sphere).
const KM_PER_DEGREE: f64 = 6371.0 * std::f64::consts::PI / 180.0;

/// Ground heights along a path, from `from` (distance 0) to `to` (`length_km`).
#[derive(Clone, Debug, PartialEq)]
pub struct Profile {
    pub from: LatLon,
    pub to: LatLon,
    pub length_km: f64,
    /// The distance of each sample from `from`, km: 0 first, `length_km` last, evenly spaced.
    pub d_km: Vec<f64>,
    /// The ground height at each, metres above the geoid.
    pub h_m: Vec<f64>,
    pub source: Source,
}

/// How many samples a path of `length_km` takes at `source`'s spacing: one at each end, and none
/// further apart than the data's samples.
#[must_use]
pub fn samples(length_km: f64, source: Source) -> usize {
    let step = source.dlat() * KM_PER_DEGREE;
    ((length_km / step).ceil() as usize + 1).max(2)
}

/// Every tile a profile from `from` to `to` reads: those the path crosses, and those just south
/// or east of it, whose first row or column a height near the edge is read from.
#[must_use]
pub fn tiles(from: LatLon, to: LatLon) -> Vec<TileId> {
    let length = distance_km(from, to);
    // a point every kilometre finds every tile crossed: south of 80° a tile is over 19 km wide
    let n = (length.ceil() as usize + 1).max(2);
    let mut out = BTreeSet::new();
    for i in 0..n {
        let p = intermediate(from, to, i as f64 / (n - 1) as f64);
        out.insert(TileId::of(p));
        out.insert(TileId::of(LatLon { lat: (p.lat - 0.002).max(-90.0), lon: p.lon }));
        let east = if p.lon + 0.002 >= 180.0 { -180.0 } else { p.lon + 0.002 };
        out.insert(TileId::of(LatLon { lat: p.lat, lon: east }));
    }
    out.into_iter().collect()
}

/// The profile from `from` to `to`, read from `store`.
///
/// # Errors
///
/// `Err(Ok(tiles))` names the tiles still to fetch; `Err(Err(why))` is a tile file that could not
/// be read.
pub fn profile(store: &Store, from: LatLon, to: LatLon) -> Result<Profile, Result<Vec<TileId>, String>> {
    let length_km = distance_km(from, to);
    let n = samples(length_km, store.source());
    let mut d_km = Vec::with_capacity(n);
    let mut h_m = Vec::with_capacity(n);
    let mut missing = BTreeSet::new();
    for i in 0..n {
        let f = i as f64 / (n - 1) as f64;
        let p = intermediate(from, to, f);
        match store.height(p).map_err(Err)? {
            Height::Metres(h) => h_m.push(h),
            Height::Missing(t) => {
                missing.insert(t);
                h_m.push(f64::NAN);
            }
        }
        d_km.push(f * length_km);
    }
    if missing.is_empty() {
        Ok(Profile { from, to, length_km, d_km, h_m, source: store.source() })
    } else {
        Err(Ok(missing.into_iter().collect()))
    }
}
