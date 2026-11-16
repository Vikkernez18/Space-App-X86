// AI-assisted
//! Shared data models for catalog metadata, DEM extents, and researcher scoring.
//!
//! This module is strictly WASM-compatible (`no_std`-friendly standard library types only;
//! no filesystem, network, or async runtime dependencies).

use std::collections::BTreeMap;
use serde::{Deserialize, Serialize};

/// Planetary or terrestrial body identifier for an analog or target site.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Body {
    Moon,
    Mars,
    Earth,
}

impl Body {
    /// Returns the canonical lowercase string label for this body.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Moon => "moon",
            Self::Mars => "mars",
            Self::Earth => "earth",
        }
    }
}

/// Supported terrain mesh / elevation raster source formats (`stl` or `tiff`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MeshFormat {
    /// Binary STL 3D triangle mesh (`.stl`).
    Stl,
    /// Elevation raster DEM in TIFF/GeoTIFF format (`.tif` or `.tiff`).
    #[serde(alias = "tif", alias = "geotiff")]
    Tiff,
}

impl MeshFormat {
    /// Auto-detects [`MeshFormat`] from a file path or URL extension (`.stl`, `.tif`, `.tiff`).
    pub fn from_path(path: &str) -> Option<Self> {
        let clean = path.split('?').next().unwrap_or(path).trim();
        let ext = clean.rsplit('.').next()?.to_ascii_lowercase();
        match ext.as_str() {
            "stl" => Some(Self::Stl),
            "tif" | "tiff" => Some(Self::Tiff),
            _ => None,
        }
    }
}

/// Horizontal bounding box of the local projected Digital Elevation Model (DEM) in meters.
///
/// Coordinates follow the pipeline/STL horizontal convention:
/// - `+X` = East (`min_x` to `max_x`)
/// - `+Y` = North (`min_y` to `max_y`)
///
/// Planar UV coordinates MUST be computed strictly from `DemExtent`, never from the mesh AABB.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct DemExtent {
    pub min_x: f32,
    pub max_x: f32,
    pub min_y: f32,
    pub max_y: f32,
}

impl DemExtent {
    /// Creates a new `DemExtent`.
    pub const fn new(min_x: f32, max_x: f32, min_y: f32, max_y: f32) -> Self {
        Self {
            min_x,
            max_x,
            min_y,
            max_y,
        }
    }

    /// Horizontal East-West span in meters (`max_x - min_x`).
    #[inline]
    pub fn width(&self) -> f32 {
        self.max_x - self.min_x
    }

    /// Horizontal South-North span in meters (`max_y - min_y`).
    #[inline]
    pub fn height(&self) -> f32 {
        self.max_y - self.min_y
    }

    /// Returns `true` if all bounds are finite and strictly positive in width and height.
    #[inline]
    pub fn is_valid(&self) -> bool {
        self.min_x.is_finite()
            && self.max_x.is_finite()
            && self.min_y.is_finite()
            && self.max_y.is_finite()
            && self.max_x > self.min_x
            && self.max_y > self.min_y
    }

    /// Computes planar UV coordinates `[u, v]` from horizontal STL coordinates `(x_stl, y_stl)`:
    /// - `u = (x_stl - min_x) / (max_x - min_x)` (West `min_x` -> `0.0`, East `max_x` -> `1.0`)
    /// - `v = (max_y - y_stl) / (max_y - min_y)` (North `max_y` -> `0.0`, South `min_y` -> `1.0`)
    #[inline]
    pub fn compute_uv(&self, x_stl: f32, y_stl: f32) -> [f32; 2] {
        let u = (x_stl - self.min_x) / (self.max_x - self.min_x);
        let v = (self.max_y - y_stl) / (self.max_y - self.min_y);
        [u, v]
    }
}

/// Metadata for a single lunar, Martian, or terrestrial analog site (`metadata.json`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SiteMetadata {
    /// Unique slug identifier (e.g., `"test_site"`).
    pub id: String,
    /// Human-readable display name in English.
    pub name: String,
    /// Planetary body (`moon`, `mars`, or `earth`).
    pub body: Body,
    /// Short scientific or operational summary of the site.
    #[serde(default)]
    pub description: String,
    /// Optional geographic/planetographic center `[latitude_deg, longitude_deg]`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub center_lat_lon: Option<[f64; 2]>,
    /// Local meter-based horizontal DEM extent used for planar UV projection.
    pub dem_extent: DemExtent,
    /// Minimum and maximum elevation in meters (`[min_z, max_z]`) in STL space.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub elevation_range_m: Option<[f32; 2]>,
    /// Triangle count of the terrain mesh (must not exceed 50,000).
    #[serde(default)]
    pub triangle_count: u32,
    /// Relative path to the binary STL mesh or DEM TIFF under `data/processed/`.
    pub mesh_path: String,
    /// Optional explicit mesh/DEM format (`"stl"` or `"tiff"`).
    /// When `None`, auto-detected from the extension of `mesh_path`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mesh_format: Option<MeshFormat>,
    /// Relative path to the aligned top-down PNG texture under `data/processed/`.
    pub texture_path: String,
    /// Pre-normalized `[0.0, 1.0]` feature scores keyed by criteria name.
    #[serde(default)]
    pub feature_scores: BTreeMap<String, f64>,
    /// Canonical composite score computed offline by `pipeline/build_catalog.py`.
    #[serde(default)]
    pub canonical_score: f64,
    /// Compliance provenance marker when generated or assisted by AI tools.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generated_by: Option<String>,
}

impl SiteMetadata {
    /// Resolves the terrain format using `mesh_format` if explicitly configured,
    /// or auto-detecting from `mesh_path` (`.stl`, `.tif`, `.tiff`), defaulting to [`MeshFormat::Stl`].
    pub fn resolved_mesh_format(&self) -> MeshFormat {
        self.mesh_format
            .or_else(|| MeshFormat::from_path(&self.mesh_path))
            .unwrap_or(MeshFormat::Stl)
    }
}

/// Top-level catalog index (`data/processed/catalog.json`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Catalog {
    /// Schema or catalog version string.
    pub version: String,
    /// Default feature weights from `pipeline/weights.json`.
    #[serde(default)]
    pub default_weights: BTreeMap<String, f64>,
    /// List of available sites in the catalog.
    pub sites: Vec<SiteMetadata>,
    /// Compliance provenance marker when generated or assisted by AI tools.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generated_by: Option<String>,
}

/// Researcher weight configuration matching `pipeline/weights.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WeightsConfig {
    /// Map of feature name to non-negative weight value.
    pub weights: BTreeMap<String, f64>,
    /// Compliance provenance marker when generated or assisted by AI tools.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generated_by: Option<String>,
}

/// Request payload for authenticated researcher score recalculation (`POST /api/recalculate`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RecalculateRequest {
    /// Custom researcher feature weights.
    pub weights: BTreeMap<String, f64>,
}

/// Result entry for a single site after weighted score recalculation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScoredSite {
    /// Site identifier matching `SiteMetadata::id`.
    pub site_id: String,
    /// Display name of the site.
    pub name: String,
    /// Planetary body of the site.
    pub body: Body,
    /// Newly recalculated weighted score in `[0.0, 1.0]`.
    pub score: f64,
    /// Original canonical score from `catalog.json`.
    pub canonical_score: f64,
}

/// Response payload returned by `POST /api/recalculate`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RecalculateResponse {
    /// Ranked list of sites (highest score first).
    pub results: Vec<ScoredSite>,
    /// Normalized weights actually applied during recalculation.
    pub normalized_weights: BTreeMap<String, f64>,
    /// Compliance provenance marker.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generated_by: Option<String>,
}
