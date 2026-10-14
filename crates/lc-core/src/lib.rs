//! lc-core: platform independent logic for BloomAnywhere.
//!
//! CONTRACT FILE: the public signatures below are relied on by `lc-app`.
//! Implementations may be moved into sub-modules, but keep these paths/signatures.

use serde::{Deserialize, Serialize};

pub mod cooldown;
pub mod engine;
pub mod geo;
pub mod gpx;
pub mod profile;

/// WGS-84 coordinate in decimal degrees.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
pub struct LatLng {
    pub lat: f64,
    pub lng: f64,
}

impl LatLng {
    pub const fn new(lat: f64, lng: f64) -> Self {
        Self { lat, lng }
    }
    /// lat in [-90,90], lng in [-180,180], both finite.
    pub fn is_valid(&self) -> bool {
        self.lat.is_finite()
            && self.lng.is_finite()
            && (-90.0..=90.0).contains(&self.lat)
            && (-180.0..=180.0).contains(&self.lng)
    }
}
