// SPDX-License-Identifier: MIT OR Apache-2.0
//! Diffraction loss by ITU-R P.526-16 (11/2025), written from the Recommendation's text: no
//! reference implementation of its § 4.5 was found.
//!
//! * [`j`]: a single knife edge's loss J(ν), § 4.1, equation (31).
//! * [`spherical_earth`]: a smooth spherical Earth at any distance, § 3.2, using § 3.1.1's first
//!   term of the residue series beyond the horizon.
//! * [`general_path`]: § 4.5's method for a general terrestrial path ("delta-Bullington"): the
//!   Bullington construction on the real profile and on a smooth one fitted to it, combined with
//!   the spherical Earth so that a completely smooth path gives the spherical Earth's loss.
//!
//! Units are the Recommendation's: distances in km, heights in metres above sea level (the
//! geoid), frequencies in MHz, wavelengths in metres. Losses are positive decibels.

/// The ground's electrical properties, for § 3.1.1's surface admittance K.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ground {
    /// Relative permittivity εr.
    pub permittivity: f64,
    /// Conductivity σ, S/m.
    pub conductivity: f64,
}

impl Ground {
    /// Land as ITU-R P.452 and P.1812 take it for their spherical-Earth term: εr 22, σ 0.003 S/m.
    pub const LAND: Ground = Ground { permittivity: 22.0, conductivity: 0.003 };
    /// Sea, likewise: εr 80, σ 5 S/m.
    pub const SEA: Ground = Ground { permittivity: 80.0, conductivity: 5.0 };
}

/// The wave's polarization.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Polarization {
    Horizontal,
    /// FM voice and packet at VHF and UHF are mostly vertical.
    Vertical,
}

/// The wave and the ground a path is worked out for.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Radio {
    pub f_mhz: f64,
    /// The effective Earth radius, km: k × 6 371.
    pub re_km: f64,
    pub ground: Ground,
    pub pol: Polarization,
}

/// The wavelength in metres at `f_mhz`.
#[must_use]
pub fn wavelength_m(f_mhz: f64) -> f64 {
    299.792_458 / f_mhz
}

/// A single knife edge's diffraction loss J(ν), dB: equation (31) for ν above −0.78, where it is
/// within about 1 dB of equation (30)'s Fresnel integrals; 0 below.
#[must_use]
pub fn j(nu: f64) -> f64 {
    if nu > -0.78 {
        6.9 + 20.0 * (((nu - 0.1).powi(2) + 1.0).sqrt() + nu - 0.1).log10()
    } else {
        0.0
    }
}

/// § 3.1.1: the diffracted field relative to free space over a smooth Earth of effective radius
/// `ae_km`, 20 log10(E/E0) = F(X) + G(Y1) + G(Y2), dB (negative where the field is weaker), from
/// the first term of the residue series.
#[must_use]
pub fn first_term(d_km: f64, h1_m: f64, h2_m: f64, f_mhz: f64, ae_km: f64, ground: Ground, pol: Polarization) -> f64 {
    let (eps, sigma) = (ground.permittivity, ground.conductivity);
    let loss = (18_000.0 * sigma / f_mhz).powi(2);
    // (11a) and (12a): the normalized surface admittance
    let kh = 0.36 * (ae_km * f_mhz).powf(-1.0 / 3.0) * ((eps - 1.0).powi(2) + loss).powf(-0.25);
    let k = match pol {
        Polarization::Horizontal => kh,
        Polarization::Vertical => kh * (eps * eps + loss).sqrt(),
    };
    // (16)
    let (k2, k4) = (k * k, k.powi(4));
    let beta = (1.0 + 1.6 * k2 + 0.67 * k4) / (1.0 + 4.5 * k2 + 1.53 * k4);
    // (14a), (15a)
    let x = 2.188 * beta * f_mhz.powf(1.0 / 3.0) * ae_km.powf(-2.0 / 3.0) * d_km;
    let y = |h: f64| 9.575e-3 * beta * f_mhz.powf(2.0 / 3.0) * ae_km.powf(-1.0 / 3.0) * h;
    // (17a), (17b)
    let f = if x >= 1.6 {
        11.0 + 10.0 * x.log10() - 17.6 * x
    } else {
        -20.0 * x.log10() - 5.6488 * x.powf(1.425)
    };
    // (18), (18a), (18b), and the floor under them
    let floor = 2.0 + 20.0 * k.log10();
    let g = |h: f64| {
        let b = beta * y(h);
        let g = if b > 2.0 {
            17.6 * (b - 1.1).sqrt() - 5.0 * (b - 1.1).log10() - 8.0
        } else {
            20.0 * (b + 0.1 * b.powi(3)).log10()
        };
        if g.is_nan() || g < floor {
            floor
        } else {
            g
        }
    };
    f + g(h1_m) + g(h2_m)
}

/// § 3.2: the diffraction loss over a smooth spherical Earth of effective radius `ae_km`, at any
/// distance, for antennas `h1_m` and `h2_m` above it (both above zero), at 10 MHz and above. 0 dB
/// where the path clears the Earth by enough.
#[must_use]
pub fn spherical_earth(d_km: f64, h1_m: f64, h2_m: f64, f_mhz: f64, ae_km: f64, ground: Ground, pol: Polarization) -> f64 {
    // the section's own units are self-consistent: metres throughout
    let (h1, h2) = (h1_m.max(1e-3), h2_m.max(1e-3));
    let (d, ae) = (d_km * 1000.0, ae_km * 1000.0);
    let lambda = wavelength_m(f_mhz);
    // (21)
    let dlos = (2.0 * ae).sqrt() * (h1.sqrt() + h2.sqrt());
    if d >= dlos {
        return -first_term(d_km, h1, h2, f_mhz, ae_km, ground, pol);
    }
    // (22) to (22e): the smallest clearance between the curved Earth and the ray
    let c = (h1 - h2) / (h1 + h2);
    let m = d * d / (4.0 * ae * (h1 + h2));
    let arg = (1.5 * c * (3.0 * m / (m + 1.0).powi(3)).sqrt()).clamp(-1.0, 1.0);
    let b = 2.0 * ((m + 1.0) / (3.0 * m)).sqrt()
        * (std::f64::consts::PI / 3.0 + arg.acos() / 3.0).cos();
    let d1 = d / 2.0 * (1.0 + b);
    let d2 = d - d1;
    let h = ((h1 - d1 * d1 / (2.0 * ae)) * d2 + (h2 - d2 * d2 / (2.0 * ae)) * d1) / d;
    // (23)
    let hreq = 0.552 * (d1 * d2 * lambda / d).sqrt();
    if h > hreq {
        return 0.0;
    }
    // (24): the Earth radius that puts the path at marginal line of sight
    let aem = 0.5 * (d / (h1.sqrt() + h2.sqrt())).powi(2);
    let ah = -first_term(d_km, h1, h2, f_mhz, aem / 1000.0, ground, pol);
    if ah < 0.0 {
        return 0.0;
    }
    // (25)
    (1.0 - h / hreq) * ah
}

/// The Bullington construction's result (§ 4.5.1).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bullington {
    /// Lb, equation (57), dB.
    pub loss_db: f64,
    /// Luc, the knife edge's own loss, dB.
    pub knife_db: f64,
    /// The diffraction parameter ν of the Bullington point.
    pub nu: f64,
    /// The Bullington point's distance from the first end, km: the profile point with the
    /// highest ν on a line-of-sight path; where the two ends' horizon lines meet beyond it.
    pub at_km: f64,
    /// Whether the path is line of sight (`Stim` < `Str`).
    pub line_of_sight: bool,
}

/// § 4.5.1, the Bullington construction on profile (`d_km`, `h_m`) between antennas at `hts` and
/// `hrs` metres above sea level, with an effective Earth radius `re_km`.
#[must_use]
pub fn bullington(d_km: &[f64], h_m: &[f64], hts: f64, hrs: f64, f_mhz: f64, re_km: f64) -> Bullington {
    let n = d_km.len();
    let d = d_km[n - 1];
    let ce = 1.0 / re_km;
    let lambda = wavelength_m(f_mhz);
    let lift = |i: usize| h_m[i] + 500.0 * ce * d_km[i] * (d - d_km[i]);
    let inner = 1..n.saturating_sub(1);
    // (49), (50)
    let stim = inner.clone().map(|i| (lift(i) - hts) / d_km[i]).fold(f64::NEG_INFINITY, f64::max);
    let str = (hrs - hts) / d;
    let nu_of = |height_above_ray: f64, di: f64| height_above_ray * (0.002 * d / (lambda * di * (d - di))).sqrt();
    let (nu, at_km, line_of_sight) = if stim < str {
        // (51): line of sight, the profile point with the highest ν
        let best = inner
            .map(|i| {
                let above = lift(i) - (hts * (d - d_km[i]) + hrs * d_km[i]) / d;
                (nu_of(above, d_km[i]), d_km[i])
            })
            .fold((f64::NEG_INFINITY, 0.0), |a, b| if b.0 > a.0 { b } else { a });
        (best.0, best.1, true)
    } else {
        // (53), (54), (55): over the horizon, where the two ends' horizon lines meet
        let srim = inner.map(|i| (lift(i) - hrs) / (d - d_km[i])).fold(f64::NEG_INFINITY, f64::max);
        let db = (hrs - hts + srim * d) / (stim + srim);
        let above = hts + stim * db - (hts * (d - db) + hrs * db) / d;
        (nu_of(above, db), db, false)
    };
    // (52), (56), (57)
    let knife_db = j(nu);
    let loss_db = knife_db + (1.0 - (-knife_db / 6.0).exp()) * (10.0 + 0.02 * d);
    Bullington { loss_db, knife_db, nu, at_km, line_of_sight }
}

/// What § 4.5 found for a path.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Diffraction {
    /// L, equation (66): the path's diffraction loss, dB.
    pub loss_db: f64,
    /// Lba: the Bullington loss on the real profile.
    pub actual: Bullington,
    /// Lbs: the Bullington loss on the smooth profile fitted to it.
    pub smooth: Bullington,
    /// Lsph: the spherical Earth's loss for the antennas' heights above the fitted surface.
    pub spherical_db: f64,
    /// The antennas' heights above the fitted surface (hts′, hrs′), m.
    pub effective_m: (f64, f64),
}

/// § 4.5.2: the diffraction loss of a general terrestrial path. `d_km` and `h_m` are the profile
/// (its first and last points the ground under each antenna, at least three points), `tx_agl_m`
/// and `rx_agl_m` the antennas' heights above that ground.
#[must_use]
pub fn general_path(d_km: &[f64], h_m: &[f64], tx_agl_m: f64, rx_agl_m: f64, radio: Radio) -> Diffraction {
    let Radio { f_mhz, re_km, ground, pol } = radio;
    let n = d_km.len();
    let d = d_km[n - 1];
    let hts = h_m[0] + tx_agl_m;
    let hrs = h_m[n - 1] + rx_agl_m;
    let actual = bullington(d_km, h_m, hts, hrs, f_mhz, re_km);

    // (58) to (60b): a straight line fitted to the profile by least squares, at each end
    let (mut v1, mut v2) = (0.0, 0.0);
    for i in 1..n {
        let step = d_km[i] - d_km[i - 1];
        v1 += step * (h_m[i] + h_m[i - 1]);
        v2 += step * (h_m[i] * (2.0 * d_km[i] + d_km[i - 1]) + h_m[i - 1] * (d_km[i] + 2.0 * d_km[i - 1]));
    }
    let hstip = (2.0 * v1 * d - v2) / (d * d);
    let hsrip = (v2 - v1 * d) / (d * d);
    // (61a) to (61d): the highest obstruction above the straight line between the antennas
    let (mut hobs, mut aobt, mut aobr) = (f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
    for i in 1..n.saturating_sub(1) {
        let hobi = h_m[i] - (hts * (d - d_km[i]) + hrs * d_km[i]) / d;
        hobs = hobs.max(hobi);
        aobt = aobt.max(hobi / d_km[i]);
        aobr = aobr.max(hobi / (d - d_km[i]));
    }
    // (62a) to (62f)
    let (hstp, hsrp) = if hobs <= 0.0 {
        (hstip, hsrip)
    } else {
        let (gt, gr) = (aobt / (aobt + aobr), aobr / (aobt + aobr));
        (hstip - hobs * gt, hsrip - hobs * gr)
    };
    // (63a) to (63d): never above the ground under the antenna
    let hst = hstp.min(h_m[0]);
    let hsr = hsrp.min(h_m[n - 1]);
    // (64a), (64b): the antennas above the fitted surface, and the Bullington loss on it
    let (hts_s, hrs_s) = (hts - hst, hrs - hsr);
    let flat = vec![0.0; n];
    let smooth = bullington(d_km, &flat, hts_s, hrs_s, f_mhz, re_km);
    // (65a), (65b), (25)
    let spherical_db = spherical_earth(d, hts_s, hrs_s, f_mhz, re_km, ground, pol);
    // (66)
    let loss_db = actual.loss_db + (spherical_db - smooth.loss_db).max(0.0);
    Diffraction { loss_db, actual, smooth, spherical_db, effective_m: (hts_s, hrs_s) }
}
