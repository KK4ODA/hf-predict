// SPDX-License-Identifier: MIT OR Apache-2.0
//! A VHF or UHF path over its profile: the Earth's bulge, the direct ray between the antennas, the
//! ground's clearance below it, the first Fresnel zone, what the path is called, and its
//! diffraction loss by ITU-R P.526-16 § 4.5.
//!
//! The class explains; it does not decide. Most VHF paths of tens of kilometres are "partly
//! obstructed" (their Fresnel zones are 70 to 125 m across at mid-path) and work: what decides is
//! the loss against the link budget ([`crate::budget`]).

use crate::p526::{self, Diffraction, Ground, Polarization, Radio};
use crate::profile::Profile;

/// The median effective Earth radius factor of the standard atmosphere (ITU-R P.310), which
/// ITU-R P.530 takes for clearance.
pub const K_STANDARD: f64 = 4.0 / 3.0;
/// The Earth's mean radius, km.
pub const EARTH_KM: f64 = 6371.0;

/// What the path is called (the gateway selection design, § 8.4).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Class {
    /// The ground clears the direct ray by 60 % of the first Fresnel zone everywhere.
    Clear,
    /// Below the ray nowhere, but inside 60 % of the first Fresnel zone somewhere: where the
    /// clearance is the smallest share of the zone.
    Partly { at_km: f64 },
    /// The ground cuts the direct ray: where it rises highest above it, and by how much.
    Blocked { at_km: f64, above_ray_m: f64 },
}

/// The path, worked out.
#[derive(Clone, Debug, PartialEq)]
pub struct Path {
    pub length_km: f64,
    pub f_mhz: f64,
    /// The antennas' heights above their ground, and above sea level.
    pub agl_m: (f64, f64),
    pub ends_m: (f64, f64),
    /// The profile's distances from the first end, km.
    pub d_km: Vec<f64>,
    /// At each profile point: the ground with the Earth's bulge added, the direct ray, the
    /// clearance (the ray less the ground), and the first Fresnel zone's radius, all metres.
    pub ground_m: Vec<f64>,
    pub ray_m: Vec<f64>,
    pub clearance_m: Vec<f64>,
    pub fresnel_m: Vec<f64>,
    pub class: Class,
    pub diffraction: Diffraction,
    /// The effective Earth radius used, km.
    pub re_km: f64,
}

/// Works out the path over `profile` at `f_mhz`, the first end's antenna `tx_agl_m` and the last
/// end's `rx_agl_m` above their ground, with effective Earth radius factor `k`.
#[must_use]
pub fn analyse(profile: &Profile, f_mhz: f64, tx_agl_m: f64, rx_agl_m: f64, k: f64) -> Path {
    let re_km = k * EARTH_KM;
    let (d, h) = (&profile.d_km, &profile.h_m);
    let n = d.len();
    let length = profile.length_km;
    let ends_m = (h[0] + tx_agl_m, h[n - 1] + rx_agl_m);
    let f_ghz = f_mhz / 1000.0;
    let mut ground_m = Vec::with_capacity(n);
    let mut ray_m = Vec::with_capacity(n);
    let mut clearance_m = Vec::with_capacity(n);
    let mut fresnel_m = Vec::with_capacity(n);
    for i in 0..n {
        let (d1, d2) = (d[i], length - d[i]);
        // the bulge, d1 d2 / 2 ae: km² over km is km
        let bulge = d1 * d2 / (2.0 * re_km) * 1000.0;
        let g = h[i] + bulge;
        let ray = ends_m.0 + (ends_m.1 - ends_m.0) * d1 / length;
        ground_m.push(g);
        ray_m.push(ray);
        clearance_m.push(ray - g);
        fresnel_m.push(17.3 * (d1 * d2 / (f_ghz * length)).max(0.0).sqrt());
    }
    let inner = 1..n.saturating_sub(1);
    let class = match inner
        .clone()
        .map(|i| (i, -clearance_m[i]))
        .fold(None::<(usize, f64)>, |a, b| match a {
            Some(a) if a.1 >= b.1 => Some(a),
            _ => Some(b),
        }) {
        Some((i, above)) if above > 0.0 => Class::Blocked { at_km: d[i], above_ray_m: above },
        _ => inner
            .filter(|&i| fresnel_m[i] > 0.0)
            .map(|i| (i, clearance_m[i] / fresnel_m[i]))
            .filter(|&(_, share)| share < 0.6)
            .fold(None::<(usize, f64)>, |a, b| match a {
                Some(a) if a.1 <= b.1 => Some(a),
                _ => Some(b),
            })
            .map_or(Class::Clear, |(i, _)| Class::Partly { at_km: d[i] }),
    };
    let radio = Radio { f_mhz, re_km, ground: Ground::LAND, pol: Polarization::Vertical };
    let diffraction = p526::general_path(d, h, tx_agl_m, rx_agl_m, radio);
    Path {
        length_km: length,
        f_mhz,
        d_km: d.clone(),
        agl_m: (tx_agl_m, rx_agl_m),
        ends_m,
        ground_m,
        ray_m,
        clearance_m,
        fresnel_m,
        class,
        diffraction,
        re_km,
    }
}

impl Path {
    /// The profile point nearest `km` from the first end.
    #[must_use]
    pub fn index_at(&self, km: f64) -> usize {
        let n = self.d_km.len();
        if n < 2 || self.length_km <= 0.0 {
            return 0;
        }
        ((km / self.length_km * (n - 1) as f64).round() as usize).min(n - 1)
    }

    /// The path in words: "Clear terrain profile", "Fresnel zone partly obstructed at 8.2 km",
    /// "Blocked by terrain 41 m above the direct ray at 9.0 km".
    #[must_use]
    pub fn words(&self) -> String {
        match self.class {
            Class::Clear => "Clear terrain profile".into(),
            Class::Partly { at_km } => format!("Fresnel zone partly obstructed at {at_km:.1} km"),
            Class::Blocked { at_km, above_ray_m } => {
                format!("Blocked by terrain {above_ray_m:.0} m above the direct ray at {at_km:.1} km")
            }
        }
    }
}
