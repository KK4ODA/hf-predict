// SPDX-License-Identifier: MIT OR Apache-2.0
//! Terrain for VHF and UHF paths, shared by hf-predict and RadioPost (RadioPost's design,
//! *Choosing a gateway*, §§ 8 and 9).
//!
//! * [`geotiff`] reads the Copernicus DEM's GeoTIFFs, and [`tile`] keeps each 1° tile on disk in a
//!   compact form; [`store::Store`] holds them and reads heights from them. Fetching is the
//!   program's: the store names the URL and converts what was fetched.
//! * [`profile`] samples the ground along the great circle between two places.
//! * [`path`] works out the Earth's bulge, the direct ray, clearance, the first Fresnel zone and
//!   what the path is called, and [`p526`] its diffraction loss by ITU-R P.526-16.
//! * [`budget`] turns a loss into what the far end receives, and its margin.
//!
//! Copernicus asks that its notice and its liability sentence go with every view of what is made
//! from its data: [`store::NOTICE_GLO30`], [`store::NOTICE_GLO90`].

pub mod budget;
pub mod geotiff;
pub mod p526;
pub mod path;
pub mod profile;
pub mod store;
pub mod tile;

pub use path::{analyse, Class, Path};
pub use profile::{profile, Profile};
pub use store::{Have, Height, Source, Store, TileId};
