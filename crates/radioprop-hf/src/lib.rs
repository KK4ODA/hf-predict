// SPDX-License-Identifier: MIT OR Apache-2.0
//! HF propagation as hf-predict predicts it, with no app around it: the VOACAP engine (`voacapl`)
//! run as a subprocess ([`engine`], [`voacap`]), the request and prediction types and the engine
//! trait ([`propagation`]), the station model VOACAP is given ([`station`]), the smoothed sunspot
//! table ([`solar`]), and predictions over one path ([`predictor`]).
//!
//! The engine itself is not built here: hf-predict's `engines/voacapl/build.sh` builds it, and a
//! program points [`voacap::VoacaplEngine`] at the folder it was installed in.

pub use radioprop_geo as geo;

pub mod engine;
pub mod predictor;
pub mod propagation;
pub mod solar;
pub mod station;
pub mod timeutil;
pub mod voacap;

#[cfg(test)]
pub(crate) mod testing {
    use std::path::PathBuf;

    /// The engine the tests run: `HFP_ENGINE_DIR`, or the one `engines/voacapl/build.sh` builds.
    pub fn engine_root() -> PathBuf {
        std::env::var_os("HFP_ENGINE_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../.work/engine"))
    }

    pub fn scratch_dir(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("hfp-{name}-{}", std::process::id()))
    }
}
