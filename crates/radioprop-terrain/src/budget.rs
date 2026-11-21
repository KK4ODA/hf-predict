// SPDX-License-Identifier: MIT OR Apache-2.0
//! The link budget, one direction at a time: what the far end receives, its margin over what its
//! modem needs, and the chance that margin holds where a vehicle stands, read against a spread.

/// Free-space path loss between isotropic antennas, dB.
#[must_use]
pub fn fspl_db(f_mhz: f64, d_km: f64) -> f64 {
    32.45 + 20.0 * f_mhz.log10() + 20.0 * d_km.max(1e-6).log10()
}

/// One transmitter and one receiver, as their station profiles give them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ends {
    /// The transmitter's power, watts.
    pub power_w: f64,
    /// Each antenna's gain, dBi, and each feed line's loss, dB.
    pub tx_gain_dbi: f64,
    pub tx_feed_db: f64,
    pub rx_gain_dbi: f64,
    pub rx_feed_db: f64,
}

/// What the receiving end gets.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Budget {
    /// The power at the receiver's input, dBm.
    pub received_dbm: f64,
    /// Received less the threshold, dB.
    pub margin_db: f64,
    /// Φ(margin / spread): the chance the path carries at a given spot.
    pub p_viable: f64,
}

/// The budget of one direction: `path_loss_db` is free space plus diffraction (and any clutter),
/// `threshold_dbm` what the receiving modem needs, `spread_db` the location variability's
/// standard deviation (10 dB to start, the design's § 8.6).
#[must_use]
pub fn one_way(ends: Ends, path_loss_db: f64, threshold_dbm: f64, spread_db: f64) -> Budget {
    let tx_dbm = 10.0 * (ends.power_w * 1000.0).log10();
    let received_dbm = tx_dbm + ends.tx_gain_dbi - ends.tx_feed_db - path_loss_db + ends.rx_gain_dbi
        - ends.rx_feed_db;
    let margin_db = received_dbm - threshold_dbm;
    Budget { received_dbm, margin_db, p_viable: phi(margin_db / spread_db) }
}

/// The standard normal distribution's cumulative probability Φ(x).
#[must_use]
pub fn phi(x: f64) -> f64 {
    0.5 * erfc(-x / std::f64::consts::SQRT_2)
}

/// The complementary error function, to a relative error under 1.2 × 10⁻⁷ (the Chebyshev fit of
/// *Numerical Recipes*, 2nd ed., § 6.2, `erfcc`).
#[must_use]
pub fn erfc(x: f64) -> f64 {
    let z = x.abs();
    let t = 1.0 / (1.0 + 0.5 * z);
    let poly = -z * z - 1.265_512_23
        + t * (1.000_023_68
            + t * (0.374_091_96
                + t * (0.096_784_18
                    + t * (-0.186_288_06
                        + t * (0.278_868_07
                            + t * (-1.135_203_98
                                + t * (1.488_515_87 + t * (-0.822_152_23 + t * 0.170_872_77))))))));
    let r = t * poly.exp();
    if x >= 0.0 {
        r
    } else {
        2.0 - r
    }
}
