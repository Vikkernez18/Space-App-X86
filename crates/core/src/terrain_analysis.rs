// AI-assisted
//! Mathematical, Differential-Geometric, and Topological Data Analysis (TDA) engine
//! for planetary and terrestrial digital elevation models (`.tif`/`.tiff`) and 3D meshes (`.stl`).
//!
//! Capabilities:
//! 1. **Morphometry & Differential Geometry**:
//!    - Finite-difference gradient $\nabla z = (\partial z/\partial x, \partial z/\partial y)$
//!      with mean, standard deviation, and 90th-percentile slope in degrees.
//!    - Terrain Ruggedness Index (TRI, Riley et al. 1999) and local RMS roughness.
//!    - Strahler Hypsometric Integral $HI = (\mu_z - z_{\min}) / (z_{\max} - z_{\min})$.
//!    - 3D surface area to 2D planar area ratio (topographic tortuosity).
//! 2. **Topological Data Analysis (0D Persistent Homology & Simplicial Topology)**:
//!    - Automatic grid subsampling (`<= 1024x1024` cells) for dense `.tif` DEM rasters so TDA
//!      completes in a few seconds.
//!    - Sublevel-set filtration ($\beta_0$ basins/craters) and superlevel-set filtration
//!      ($\beta_0$ peaks/domes) via Union-Find with the Elder Rule.
//!    - Automatic deterministic mesh simplification for `.stl` files exceeding `50,000` triangles,
//!      persisting the optimized `<= 50,000`-triangle `.stl` and reporting both original and
//!      simplified triangle counts.
//!    - Simplicial complex invariants on the welded triangle mesh: Euler characteristic
//!      $\chi = V - E + F$, connected components $C$, boundary edges, and genus estimate.
//! 3. **Automatic Planetary Analog Proposal & Binary `multipart/form-data` Transport**:
//!    - Projects morphometric and topological invariants into normalized `[0.0, 1.0]`
//!      catalog feature space, computes `analog_score`, identifies the closest analog site
//!      and planetary body (`Moon`, `Mars`, `Earth`), and generates quantitative justification.
//!    - Zero-Base64 RFC 7578 `multipart/form-data` binary encoder and decoder for `.stl`, `.tif`,
//!      and `.png` uploads up to 512 MiB.

use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::Cursor;

use serde::{Deserialize, Serialize};
use tiff::decoder::{Decoder, DecodingResult};

use crate::models::{Body, Catalog, DemExtent, MeshFormat, SiteMetadata};
use crate::scoring::compute_weighted_score;
use crate::stl::{
    MAX_TRIANGLES, STL_HEADER_BYTES, STL_TRIANGLE_BYTES, WeldedMesh, encode_binary_stl,
    parse_and_weld_stl_with_stats,
};
use crate::tiff_dem::{encode_uncompressed_f32_tiff, parse_dem_tiff};

/// Maximum grid dimension per axis (`1024x1024` cells) before running 0D Persistent Homology
/// and morphometric analysis on dense GeoTIFF DEMs so TDA completes in a few seconds.
pub const MAX_TDA_GRID_DIM: usize = 1024;

/// Differential-geometric and morphometric descriptors of a terrain surface.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MorphometryMetrics {
    /// Minimum elevation $z_{\min}$ in meters.
    pub min_elevation_m: f64,
    /// Maximum elevation $z_{\max}$ in meters.
    pub max_elevation_m: f64,
    /// Total vertical relief $z_{\max} - z_{\min}$ in meters.
    pub elevation_range_m: f64,
    /// Mean elevation $\mu_z$ in meters.
    pub mean_elevation_m: f64,
    /// Standard deviation of elevation $\sigma_z$ in meters.
    pub std_elevation_m: f64,
    /// Mean slope angle $\langle \arctan(\|\nabla z\|) \rangle$ in degrees.
    pub slope_mean_deg: f64,
    /// Standard deviation of slope angle in degrees.
    pub slope_std_deg: f64,
    /// 90th-percentile slope angle ($P_{90}$) in degrees.
    pub slope_p90_deg: f64,
    /// Mean Terrain Ruggedness Index (TRI) in meters.
    pub tri_mean_m: f64,
    /// Root-mean-square (RMS) local detrended elevation residual in meters.
    pub roughness_rms_m: f64,
    /// Strahler Hypsometric Integral $HI = (\mu_z - z_{\min}) / (z_{\max} - z_{\min}) \in [0, 1]$.
    pub hypsometric_integral: f64,
    /// True 3D surface area in square meters.
    pub surface_area_3d_m2: f64,
    /// Projected 2D planar area in square meters.
    pub planar_area_2d_m2: f64,
    /// Ratio of 3D surface area to 2D planar area (tortuosity $\ge 1.0$).
    pub surface_to_planar_ratio: f64,
}

/// 0D Persistent Homology (TDA) and simplicial mesh topology invariants.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TopologyMetrics {
    /// Number of persistent sublevel-set components ($\beta_0$ basins / craters).
    pub persistent_basins_b0: usize,
    /// Number of persistent superlevel-set components ($\beta_0$ peaks / domes).
    pub persistent_peaks_b0: usize,
    /// Maximum persistence (birth-death vertical prominence) in meters.
    pub max_persistence_m: f64,
    /// Mean persistence of significant topological features in meters.
    pub mean_persistence_m: f64,
    /// Persistence significance threshold $\tau$ in meters ($5\%$ of total relief).
    pub persistence_threshold_m: f64,
    /// Original triangle count of the uploaded mesh prior to automatic simplification.
    #[serde(default)]
    pub original_triangle_count: usize,
    /// Simplified triangle count (`<= 50,000`) persisted to `terrain.stl`.
    #[serde(default)]
    pub simplified_triangle_count: usize,
    /// Number of unique welded vertices $V$ in the simplicial mesh.
    pub vertex_count: usize,
    /// Number of unique undirected edges $E$ in the simplicial mesh.
    pub edge_count: usize,
    /// Number of triangular faces $F$ in the simplicial mesh.
    pub face_count: usize,
    /// Euler characteristic $\chi = V - E + F$.
    pub euler_characteristic: i64,
    /// Number of connected mesh components $\beta_0^{\text{mesh}}$.
    pub connected_components: usize,
    /// Number of exterior boundary edges (incident to exactly 1 triangle).
    pub boundary_edges: usize,
    /// Whether every edge is shared by at most 2 faces (2-manifold with boundary).
    pub is_manifold_with_boundary: bool,
    /// Estimated topological genus $g \ge 0$.
    pub estimated_genus: usize,
}

/// Quantitative planetary analog match and scientific justification.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AnalogProposal {
    /// Proposed planetary or terrestrial body (`moon`, `mars`, `earth`).
    pub proposed_body: Body,
    /// Identifier of the closest existing site in `catalog.json`.
    pub closest_site_id: String,
    /// Display name of the closest existing site in `catalog.json`.
    pub closest_site_name: String,
    /// Body of the closest catalog site.
    pub closest_site_body: Body,
    /// Feature-space similarity in `[0.0, 1.0]` ($1 - \|\Delta \mathbf{f}\|_2 / \sqrt{k}$).
    pub similarity_score: f64,
    /// Weighted composite `analog_score` in `[0.0, 1.0]`.
    pub analog_score: f64,
    /// Normalized `[0.0, 1.0]` feature vector projected into catalog space.
    pub normalized_features: BTreeMap<String, f64>,
    /// Quantitative scientific explanation of the analog classification.
    pub justification: String,
}

/// Complete mathematical, geometric, and topological report for an analyzed terrain.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TerrainAnalysisReport {
    pub site_id: String,
    pub site_name: String,
    pub dem_extent: DemExtent,
    /// Original triangle count of the uploaded `.stl` before automatic simplification.
    #[serde(default)]
    pub original_triangle_count: u32,
    /// Final simplified triangle count (`<= 50,000`) saved to `data/processed/<site_id>/terrain.stl`.
    #[serde(default)]
    pub simplified_triangle_count: u32,
    pub morphometry: MorphometryMetrics,
    pub topology: TopologyMetrics,
    pub proposal: AnalogProposal,
    pub generated_site_metadata: SiteMetadata,
    pub generated_by: String,
}

/// Complete output bundle produced by [`analyze_and_build_site_bundle`], including
/// normalized binary `.stl` (`<= 50,000` triangles), `.tif`, and `.png` buffers ready to be saved to disk.
#[derive(Debug, Clone)]
pub struct ProcessedSiteBundle {
    pub report: TerrainAnalysisReport,
    pub stl_bytes: Vec<u8>,
    pub tiff_bytes: Vec<u8>,
    pub png_bytes: Vec<u8>,
}

/// Request payload for `POST /api/sites/upload`.
///
/// Supports both binary `multipart/form-data` transport (`stl_bytes`, `tiff_bytes`, `png_bytes`,
/// zero Base64 overhead) and legacy JSON Base64 fallback (`stl_base64`, `tiff_base64`, `png_base64`).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UploadSiteRequest {
    /// Optional slug ID (auto-generated from `site_name` if empty).
    #[serde(default)]
    pub site_id: String,
    /// Human-readable site name.
    #[serde(default)]
    pub site_name: String,
    /// Optional explicit body override (`moon`, `mars`, `earth`). If `None`, uses the TDA/morphometric proposal.
    #[serde(default)]
    pub body: Option<Body>,
    /// Optional scientific description.
    #[serde(default)]
    pub description: String,
    /// Optional explicit `DemExtent`. If omitted, inferred from the STL bounding box or raster dimensions.
    #[serde(default)]
    pub dem_extent: Option<DemExtent>,
    /// Optional Base64-encoded binary `.stl` file bytes (JSON fallback).
    #[serde(default)]
    pub stl_base64: Option<String>,
    /// Optional Base64-encoded `.tif`/`.tiff` DEM file bytes (JSON fallback).
    #[serde(default)]
    pub tiff_base64: Option<String>,
    /// Optional Base64-encoded `.png` surface albedo texture bytes (JSON fallback).
    #[serde(default)]
    pub png_base64: Option<String>,
    /// Raw binary `.stl` file bytes sent via `multipart/form-data` (no Base64).
    #[serde(skip)]
    pub stl_bytes: Option<Vec<u8>>,
    /// Raw binary `.tif`/`.tiff` file bytes sent via `multipart/form-data` (no Base64).
    #[serde(skip)]
    pub tiff_bytes: Option<Vec<u8>>,
    /// Raw binary `.png` file bytes sent via `multipart/form-data` (no Base64).
    #[serde(skip)]
    pub png_bytes: Option<Vec<u8>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct MultipartUploadMetadata {
    #[serde(default)]
    pub site_id: String,
    #[serde(default)]
    pub site_name: String,
    #[serde(default)]
    pub body: Option<Body>,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub dem_extent: Option<DemExtent>,
}

/// Response payload returned by `POST /api/sites/upload`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UploadSiteResponse {
    pub report: TerrainAnalysisReport,
    pub updated_catalog: Catalog,
    pub generated_by: String,
}

/// Converts a site name into a filesystem-safe lowercase slug identifier.
pub fn slugify_site_id(input: &str) -> String {
    let mut slug = String::with_capacity(input.len());
    let mut prev_underscore = false;
    for ch in input.trim().chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
            prev_underscore = false;
        } else if !prev_underscore && !slug.is_empty() {
            slug.push('_');
            prev_underscore = true;
        }
    }
    let clean = slug.trim_matches('_').to_string();
    if clean.is_empty() {
        "uploaded_site".to_string()
    } else {
        clean
    }
}

/// Standard RFC 4648 Base64 encoder (no external crate required).
pub fn base64_encode(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };
        let triple = (b0 << 16) | (b1 << 8) | b2;

        out.push(TABLE[((triple >> 18) & 0x3F) as usize] as char);
        out.push(TABLE[((triple >> 12) & 0x3F) as usize] as char);
        if chunk.len() > 1 {
            out.push(TABLE[((triple >> 6) & 0x3F) as usize] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(TABLE[(triple & 0x3F) as usize] as char);
        } else {
            out.push('=');
        }
    }
    out
}

/// Standard RFC 4648 Base64 decoder (ignores whitespace and optional data-URL prefixes).
pub fn base64_decode(input: &str) -> Result<Vec<u8>, String> {
    let payload = if let Some((_, after_comma)) = input.split_once(',') {
        after_comma
    } else {
        input
    };

    let mut clean: Vec<u8> = Vec::with_capacity(payload.len());
    for b in payload.bytes() {
        if !b.is_ascii_whitespace() {
            clean.push(b);
        }
    }
    if clean.is_empty() {
        return Ok(Vec::new());
    }
    if !clean.len().is_multiple_of(4) {
        return Err("Invalid Base64 length (must be a multiple of 4)".to_string());
    }

    let val = |c: u8| -> Result<u32, String> {
        match c {
            b'A'..=b'Z' => Ok(u32::from(c - b'A')),
            b'a'..=b'z' => Ok(u32::from(c - b'a' + 26)),
            b'0'..=b'9' => Ok(u32::from(c - b'0' + 52)),
            b'+' | b'-' => Ok(62),
            b'/' | b'_' => Ok(63),
            b'=' => Ok(0),
            _ => Err(format!("Invalid Base64 character byte: {c}")),
        }
    };

    let mut out = Vec::with_capacity((clean.len() / 4) * 3);
    for chunk in clean.chunks_exact(4) {
        let v0 = val(chunk[0])?;
        let v1 = val(chunk[1])?;
        let v2 = val(chunk[2])?;
        let v3 = val(chunk[3])?;
        let triple = (v0 << 18) | (v1 << 12) | (v2 << 6) | v3;

        out.push(((triple >> 16) & 0xFF) as u8);
        if chunk[2] != b'=' {
            out.push(((triple >> 8) & 0xFF) as u8);
        }
        if chunk[3] != b'=' {
            out.push((triple & 0xFF) as u8);
        }
    }
    Ok(out)
}

#[inline]
fn find_subslice(haystack: &[u8], needle: &[u8], start: usize) -> Option<usize> {
    if needle.is_empty() || start.saturating_add(needle.len()) > haystack.len() {
        return None;
    }
    let first = needle[0];
    let max_pos = haystack.len() - needle.len();
    let mut pos = start;
    while pos <= max_pos {
        let offset = haystack[pos..=max_pos].iter().position(|&b| b == first)?;
        let candidate = pos + offset;
        if &haystack[candidate..candidate + needle.len()] == needle {
            return Some(candidate);
        }
        pos = candidate + 1;
    }
    None
}

/// Encodes an [`UploadSiteRequest`] into an RFC 7578 `multipart/form-data` binary payload
/// `(content_type_header, body_bytes)` without Base64 expansion.
pub fn encode_multipart_upload_request(req: &UploadSiteRequest) -> (String, Vec<u8>) {
    let mut boundary = "----SpaceAppX86MultipartBoundary9f8e7d6c5b4a3f2e".to_string();
    let check_slices: [&[u8]; 3] = [
        req.stl_bytes.as_deref().unwrap_or(&[]),
        req.tiff_bytes.as_deref().unwrap_or(&[]),
        req.png_bytes.as_deref().unwrap_or(&[]),
    ];
    let mut suffix = 0u32;
    while check_slices
        .iter()
        .any(|s| find_subslice(s, boundary.as_bytes(), 0).is_some())
    {
        suffix += 1;
        boundary = format!("----SpaceAppX86MultipartBoundary9f8e7d6c5b4a_{suffix}");
    }

    let meta = MultipartUploadMetadata {
        site_id: req.site_id.clone(),
        site_name: req.site_name.clone(),
        body: req.body,
        description: req.description.clone(),
        dem_extent: req.dem_extent,
    };
    let meta_json = serde_json::to_vec(&meta).unwrap_or_default();

    let stl_len = req.stl_bytes.as_ref().map_or(0, Vec::len);
    let tiff_len = req.tiff_bytes.as_ref().map_or(0, Vec::len);
    let png_len = req.png_bytes.as_ref().map_or(0, Vec::len);
    let total_cap = meta_json.len() + stl_len + tiff_len + png_len + 1024;

    let mut body = Vec::with_capacity(total_cap);

    // Part 1: JSON metadata part
    body.extend_from_slice(b"--");
    body.extend_from_slice(boundary.as_bytes());
    body.extend_from_slice(
        b"\r\nContent-Disposition: form-data; name=\"metadata\"\r\nContent-Type: application/json\r\n\r\n",
    );
    body.extend_from_slice(&meta_json);
    body.extend_from_slice(b"\r\n");

    // Helper closure for binary file parts
    let mut push_file_part = |field_name: &str, filename: &str, mime: &str, bytes: &[u8]| {
        body.extend_from_slice(b"--");
        body.extend_from_slice(boundary.as_bytes());
        body.extend_from_slice(b"\r\nContent-Disposition: form-data; name=\"");
        body.extend_from_slice(field_name.as_bytes());
        body.extend_from_slice(b"\"; filename=\"");
        body.extend_from_slice(filename.as_bytes());
        body.extend_from_slice(b"\"\r\nContent-Type: ");
        body.extend_from_slice(mime.as_bytes());
        body.extend_from_slice(b"\r\n\r\n");
        body.extend_from_slice(bytes);
        body.extend_from_slice(b"\r\n");
    };

    if let Some(ref stl) = req.stl_bytes
        && !stl.is_empty()
    {
        push_file_part("stl", "terrain.stl", "application/octet-stream", stl);
    }
    if let Some(ref tiff) = req.tiff_bytes
        && !tiff.is_empty()
    {
        push_file_part("tiff", "dem.tif", "image/tiff", tiff);
    }
    if let Some(ref png) = req.png_bytes
        && !png.is_empty()
    {
        push_file_part("png", "albedo.png", "image/png", png);
    }

    body.extend_from_slice(b"--");
    body.extend_from_slice(boundary.as_bytes());
    body.extend_from_slice(b"--\r\n");

    let content_type = format!("multipart/form-data; boundary={boundary}");
    (content_type, body)
}

/// Parses an RFC 7578 `multipart/form-data` request body into an [`UploadSiteRequest`].
pub fn decode_multipart_upload_request(
    content_type: &str,
    body: &[u8],
) -> Result<UploadSiteRequest, String> {
    let boundary = content_type
        .split(';')
        .skip(1)
        .find_map(|param| {
            let (k, v) = param.trim().split_once('=')?;
            if k.trim().eq_ignore_ascii_case("boundary") {
                Some(v.trim().trim_matches('"').to_string())
            } else {
                None
            }
        })
        .filter(|b| !b.is_empty())
        .ok_or_else(|| "Missing boundary parameter in multipart/form-data Content-Type".to_string())?;

    let delimiter = format!("--{boundary}").into_bytes();
    let Some(mut part_start) = find_subslice(body, &delimiter, 0) else {
        return Err("Multipart body does not contain expected boundary delimiter".to_string());
    };

    let mut req = UploadSiteRequest {
        site_name: "Uploaded Analog Site".to_string(),
        ..Default::default()
    };

    loop {
        let after_delim = part_start + delimiter.len();
        if after_delim + 2 <= body.len() && &body[after_delim..after_delim + 2] == b"--" {
            // Reached closing `--boundary--`
            break;
        }

        // Advance past `\r\n` or `\n` following `--boundary`
        let headers_start = if after_delim + 2 <= body.len()
            && &body[after_delim..after_delim + 2] == b"\r\n"
        {
            after_delim + 2
        } else if after_delim < body.len() && body[after_delim] == b'\n' {
            after_delim + 1
        } else {
            after_delim
        };

        let Some(next_delim) = find_subslice(body, &delimiter, headers_start) else {
            break;
        };

        // Locate end of part headers (`\r\n\r\n` or `\n\n`)
        let part_slice = &body[headers_start..next_delim];
        let (headers_bytes, raw_payload) =
            if let Some(h_end) = find_subslice(part_slice, b"\r\n\r\n", 0) {
                (&part_slice[..h_end], &part_slice[h_end + 4..])
            } else if let Some(h_end) = find_subslice(part_slice, b"\n\n", 0) {
                (&part_slice[..h_end], &part_slice[h_end + 2..])
            } else {
                part_start = next_delim;
                continue;
            };

        // Strip trailing `\r\n` or `\n` that precedes `--boundary`
        let payload = if raw_payload.ends_with(b"\r\n") {
            &raw_payload[..raw_payload.len() - 2]
        } else if raw_payload.ends_with(b"\n") {
            &raw_payload[..raw_payload.len() - 1]
        } else {
            raw_payload
        };

        let headers_str = String::from_utf8_lossy(headers_bytes);
        let mut field_name = String::new();
        let mut file_name = String::new();

        for line in headers_str.lines() {
            if let Some((h_name, h_val)) = line.split_once(':')
                && h_name.trim().eq_ignore_ascii_case("content-disposition")
            {
                for param in h_val.split(';') {
                    if let Some((pk, pv)) = param.trim().split_once('=') {
                        let clean_k = pk.trim();
                        let clean_v = pv.trim().trim_matches('"');
                        if clean_k.eq_ignore_ascii_case("name") {
                            field_name = clean_v.to_ascii_lowercase();
                        } else if clean_k.eq_ignore_ascii_case("filename") {
                            file_name = clean_v.to_ascii_lowercase();
                        }
                    }
                }
            }
        }

        match field_name.as_str() {
            "metadata" => {
                let meta: MultipartUploadMetadata = serde_json::from_slice(payload)
                    .map_err(|e| format!("Invalid multipart metadata JSON: {e}"))?;
                if !meta.site_id.is_empty() {
                    req.site_id = meta.site_id;
                }
                if !meta.site_name.is_empty() {
                    req.site_name = meta.site_name;
                }
                if meta.body.is_some() {
                    req.body = meta.body;
                }
                if !meta.description.is_empty() {
                    req.description = meta.description;
                }
                if meta.dem_extent.is_some() {
                    req.dem_extent = meta.dem_extent;
                }
            }
            "site_id" => {
                req.site_id = String::from_utf8_lossy(payload).trim().to_string();
            }
            "site_name" => {
                req.site_name = String::from_utf8_lossy(payload).trim().to_string();
            }
            "description" => {
                req.description = String::from_utf8_lossy(payload).trim().to_string();
            }
            "body" => {
                let b_str = String::from_utf8_lossy(payload).trim().to_ascii_lowercase();
                req.body = match b_str.as_str() {
                    "moon" => Some(Body::Moon),
                    "mars" => Some(Body::Mars),
                    "earth" => Some(Body::Earth),
                    _ => None,
                };
            }
            "stl" => {
                if !payload.is_empty() {
                    req.stl_bytes = Some(payload.to_vec());
                }
            }
            "tiff" | "tif" | "dem" => {
                if !payload.is_empty() {
                    req.tiff_bytes = Some(payload.to_vec());
                }
            }
            "png" | "texture" | "albedo" => {
                if !payload.is_empty() {
                    req.png_bytes = Some(payload.to_vec());
                }
            }
            _ => {
                if !payload.is_empty() {
                    if file_name.ends_with(".stl") {
                        req.stl_bytes = Some(payload.to_vec());
                    } else if file_name.ends_with(".tif") || file_name.ends_with(".tiff") {
                        req.tiff_bytes = Some(payload.to_vec());
                    } else if file_name.ends_with(".png") {
                        req.png_bytes = Some(payload.to_vec());
                    }
                }
            }
        }

        part_start = next_delim;
    }

    Ok(req)
}

/// Inspects raw binary STL bytes to infer horizontal `DemExtent` (`min_x`, `max_x`, `min_y`, `max_y`).
/// Supports meshes of any size (including `> 50,000` triangles prior to simplification).
pub fn infer_extent_from_stl_bytes(bytes: &[u8]) -> Result<DemExtent, String> {
    if bytes.len() < STL_HEADER_BYTES + 4 {
        return Err("STL buffer too short to infer DemExtent".to_string());
    }
    let tri_count = u32::from_le_bytes([
        bytes[STL_HEADER_BYTES],
        bytes[STL_HEADER_BYTES + 1],
        bytes[STL_HEADER_BYTES + 2],
        bytes[STL_HEADER_BYTES + 3],
    ]);
    if tri_count == 0 {
        return Err("STL contains zero triangles".to_string());
    }
    let expected = STL_HEADER_BYTES
        .saturating_add(4)
        .saturating_add((tri_count as usize).saturating_mul(STL_TRIANGLE_BYTES));
    if bytes.len() != expected {
        return Err(format!(
            "Binary STL byte length mismatch: expected {expected}, got {}",
            bytes.len()
        ));
    }

    let mut min_x = f32::INFINITY;
    let mut max_x = f32::NEG_INFINITY;
    let mut min_y = f32::INFINITY;
    let mut max_y = f32::NEG_INFINITY;

    let mut offset = STL_HEADER_BYTES + 4;
    for _ in 0..tri_count {
        for v in 0..3 {
            let base = offset + 12 + v * 12;
            let x = f32::from_le_bytes([
                bytes[base],
                bytes[base + 1],
                bytes[base + 2],
                bytes[base + 3],
            ]);
            let y = f32::from_le_bytes([
                bytes[base + 4],
                bytes[base + 5],
                bytes[base + 6],
                bytes[base + 7],
            ]);
            if x.is_finite() && y.is_finite() {
                min_x = min_x.min(x);
                max_x = max_x.max(x);
                min_y = min_y.min(y);
                max_y = max_y.max(y);
            }
        }
        offset += STL_TRIANGLE_BYTES;
    }

    if !(max_x > min_x && max_y > min_y) {
        return Err("Degenerate horizontal extent in STL vertices".to_string());
    }
    Ok(DemExtent::new(min_x, max_x, min_y, max_y))
}

/// Subsamples a 2D elevation grid `(width, height)` so neither dimension exceeds `max_dim`
/// (e.g. `1024x1024` for 0D Persistent Homology), preserving boundary rows/columns and
/// global elevation extremes (`min_z`, `max_z`).
pub fn subsample_grid_max_dim(
    width: usize,
    height: usize,
    grid: &[f32],
    max_dim: usize,
) -> (usize, usize, Vec<f32>) {
    let safe_max = max_dim.max(2);
    if width <= safe_max && height <= safe_max {
        return (width, height, grid.to_vec());
    }

    let scale = (width as f64 / safe_max as f64)
        .max(height as f64 / safe_max as f64)
        .max(1.0);
    let out_w = ((width as f64 / scale).round() as usize).clamp(2, safe_max);
    let out_h = ((height as f64 / scale).round() as usize).clamp(2, safe_max);

    let mut out = Vec::with_capacity(out_w * out_h);
    let max_r = height.saturating_sub(1);
    let max_c = width.saturating_sub(1);

    for or in 0..out_h {
        let src_r = if or + 1 == out_h {
            max_r
        } else {
            ((or * max_r) / (out_h - 1)).min(max_r)
        };
        let row_offset = src_r * width;
        for oc in 0..out_w {
            let src_c = if oc + 1 == out_w {
                max_c
            } else {
                ((oc * max_c) / (out_w - 1)).min(max_c)
            };
            out.push(grid[row_offset + src_c]);
        }
    }

    (out_w, out_h, out)
}

/// Reads only the `(width, height)` header from a TIFF byte buffer without decoding all pixels.
fn read_tiff_dimensions(bytes: &[u8]) -> Result<(usize, usize), String> {
    let mut decoder =
        Decoder::new(Cursor::new(bytes)).map_err(|e| format!("TIFF decode error: {e}"))?;
    let (w, h) = decoder
        .dimensions()
        .map_err(|e| format!("TIFF dimensions error: {e}"))?;
    if w < 2 || h < 2 {
        return Err(format!("TIFF dimensions {w}x{h} too small (need >= 2x2)"));
    }
    Ok((w as usize, h as usize))
}

/// Decodes a TIFF elevation grid into `(width, height, Vec<f32>)`, automatically subsampling
/// dense rasters to at most [`MAX_TDA_GRID_DIM`] (`1024x1024`) cells in a single pass so
/// TDA and morphometric calculations complete in a few seconds.
fn decode_tiff_grid(bytes: &[u8]) -> Result<(usize, usize, Vec<f32>), String> {
    let mut decoder =
        Decoder::new(Cursor::new(bytes)).map_err(|e| format!("TIFF decode error: {e}"))?;
    let (w, h) = decoder
        .dimensions()
        .map_err(|e| format!("TIFF dimensions error: {e}"))?;
    if w < 2 || h < 2 {
        return Err(format!("TIFF dimensions {w}x{h} too small (need >= 2x2)"));
    }
    let width = w as usize;
    let height = h as usize;
    let expected = width.saturating_mul(height);
    let decoded = decoder
        .read_image()
        .map_err(|e| format!("TIFF image read error: {e}"))?;

    let scale = (width as f64 / MAX_TDA_GRID_DIM as f64)
        .max(height as f64 / MAX_TDA_GRID_DIM as f64)
        .max(1.0);
    let out_w = if width > MAX_TDA_GRID_DIM || height > MAX_TDA_GRID_DIM {
        ((width as f64 / scale).round() as usize).clamp(2, MAX_TDA_GRID_DIM)
    } else {
        width
    };
    let out_h = if width > MAX_TDA_GRID_DIM || height > MAX_TDA_GRID_DIM {
        ((height as f64 / scale).round() as usize).clamp(2, MAX_TDA_GRID_DIM)
    } else {
        height
    };

    macro_rules! extract_subsampled_f32 {
        ($vec:expr, $conv:expr) => {{
            if $vec.len() < expected {
                return Err("Truncated TIFF raster band".to_string());
            }
            let spp = $vec.len() / expected;
            let mut out = Vec::with_capacity(out_w * out_h);
            let max_r = height - 1;
            let max_c = width - 1;
            for or in 0..out_h {
                let src_r = if or + 1 == out_h {
                    max_r
                } else {
                    ((or * max_r) / (out_h - 1)).min(max_r)
                };
                let row_base = src_r * width;
                for oc in 0..out_w {
                    let src_c = if oc + 1 == out_w {
                        max_c
                    } else {
                        ((oc * max_c) / (out_w - 1)).min(max_c)
                    };
                    let v: f32 = $conv($vec[(row_base + src_c) * spp]);
                    out.push(if v.is_finite() && v.abs() < 1.0e15 {
                        v
                    } else {
                        0.0
                    });
                }
            }
            out
        }};
    }

    let grid = match decoded {
        DecodingResult::F32(v) => extract_subsampled_f32!(v, |x: f32| x),
        DecodingResult::I16(v) => extract_subsampled_f32!(v, |x: i16| f32::from(x)),
        DecodingResult::U16(v) => extract_subsampled_f32!(v, |x: u16| f32::from(x)),
        DecodingResult::F64(v) => extract_subsampled_f32!(v, |x: f64| x as f32),
        DecodingResult::I32(v) => extract_subsampled_f32!(v, |x: i32| x as f32),
        DecodingResult::U32(v) => extract_subsampled_f32!(v, |x: u32| x as f32),
        DecodingResult::U8(v) => extract_subsampled_f32!(v, |x: u8| f32::from(x)),
        DecodingResult::I8(v) => extract_subsampled_f32!(v, |x: i8| f32::from(x)),
        DecodingResult::U64(v) => extract_subsampled_f32!(v, |x: u64| x as f32),
        DecodingResult::I64(v) => extract_subsampled_f32!(v, |x: i64| x as f32),
    };

    Ok((out_w, out_h, grid))
}

/// Rasterizes/interpolates a [`WeldedMesh`] onto a regular `grid_w x grid_h` elevation grid
/// in Z-up meter coordinates when only an `.stl` file is provided.
fn rasterize_welded_mesh_to_grid(
    mesh: &WeldedMesh,
    extent: &DemExtent,
    grid_w: usize,
    grid_h: usize,
) -> Vec<f32> {
    let mut accum_z = vec![0.0_f64; grid_w * grid_h];
    let mut counts = vec![0_u32; grid_w * grid_h];

    let span_x = extent.width().max(1e-3);
    let span_y = extent.height().max(1e-3);

    // Project every vertex into its corresponding grid cell.
    for pos in &mesh.positions {
        // Bevy Y-up back to STL Z-up: X_stl = pos[0], Y_stl = -pos[2], Z_stl = pos[1]
        let x_stl = pos[0];
        let y_stl = -pos[2];
        let z_stl = f64::from(pos[1]);

        let u = ((x_stl - extent.min_x) / span_x).clamp(0.0, 1.0);
        let v = ((extent.max_y - y_stl) / span_y).clamp(0.0, 1.0);
        let gx = ((u * ((grid_w - 1) as f32)).round() as usize).min(grid_w - 1);
        let gy = ((v * ((grid_h - 1) as f32)).round() as usize).min(grid_h - 1);
        let idx = gy * grid_w + gx;
        accum_z[idx] += z_stl;
        counts[idx] += 1;
    }

    // Fill occupied cells and compute global mean for fallback
    let mut sum_valid = 0.0_f64;
    let mut num_valid = 0_usize;
    let mut grid = vec![f32::NAN; grid_w * grid_h];
    for i in 0..(grid_w * grid_h) {
        if counts[i] > 0 {
            let z = accum_z[i] / f64::from(counts[i]);
            grid[i] = z as f32;
            sum_valid += z;
            num_valid += 1;
        }
    }
    let fallback_mean = if num_valid > 0 {
        (sum_valid / (num_valid as f64)) as f32
    } else {
        0.0
    };

    // Nearest-neighbor / iterative harmonic inpainting for any empty grid cells
    for _pass in 0..8 {
        let prev = grid.clone();
        let mut any_nan = false;
        for r in 0..grid_h {
            for c in 0..grid_w {
                let idx = r * grid_w + c;
                if prev[idx].is_finite() {
                    continue;
                }
                any_nan = true;
                let mut n_sum = 0.0_f32;
                let mut n_cnt = 0_u32;
                for dr in -1_isize..=1 {
                    for dc in -1_isize..=1 {
                        if dr == 0 && dc == 0 {
                            continue;
                        }
                        let nr = r as isize + dr;
                        let nc = c as isize + dc;
                        if nr >= 0
                            && nr < (grid_h as isize)
                            && nc >= 0
                            && nc < (grid_w as isize)
                        {
                            let v = prev[(nr as usize) * grid_w + (nc as usize)];
                            if v.is_finite() {
                                n_sum += v;
                                n_cnt += 1;
                            }
                        }
                    }
                }
                if n_cnt > 0 {
                    grid[idx] = n_sum / (n_cnt as f32);
                }
            }
        }
        if !any_nan {
            break;
        }
    }

    for val in &mut grid {
        if !val.is_finite() {
            *val = fallback_mean;
        }
    }
    grid
}

/// Converts a `WeldedMesh` (in Bevy Y-up space) back into a binary Z-up STL byte buffer.
fn welded_mesh_to_binary_stl(mesh: &WeldedMesh) -> Vec<u8> {
    let mut tris: Vec<[[f32; 3]; 3]> = Vec::with_capacity(mesh.indices.len() / 3);
    for tri in mesh.indices.chunks_exact(3) {
        let p0 = mesh.positions[tri[0] as usize];
        let p1 = mesh.positions[tri[1] as usize];
        let p2 = mesh.positions[tri[2] as usize];
        // Bevy Y-up [X_stl, Z_stl, -Y_stl] -> Z-up [X_stl, Y_stl, Z_stl]
        tris.push([
            [p0[0], -p0[2], p0[1]],
            [p1[0], -p1[2], p1[1]],
            [p2[0], -p2[2], p2[1]],
        ]);
    }
    encode_binary_stl(&tris)
}

/// Computes differential-geometric and morphometric metrics over an elevation grid and 3D mesh.
pub fn compute_morphometry_metrics(
    width: usize,
    height: usize,
    grid: &[f32],
    extent: &DemExtent,
    mesh: &WeldedMesh,
) -> MorphometryMetrics {
    let n = width * height;
    let mut min_z = f64::INFINITY;
    let mut max_z = f64::NEG_INFINITY;
    let mut sum_z = 0.0_f64;

    for &v in grid.iter().take(n) {
        let z = f64::from(v);
        min_z = min_z.min(z);
        max_z = max_z.max(z);
        sum_z += z;
    }

    if !min_z.is_finite() {
        min_z = 0.0;
        max_z = 0.0;
    }
    let mean_z = if n > 0 { sum_z / (n as f64) } else { 0.0 };
    let range_z = (max_z - min_z).max(0.0);

    let mut var_z = 0.0_f64;
    for &v in grid.iter().take(n) {
        let d = f64::from(v) - mean_z;
        var_z += d * d;
    }
    let std_z = if n > 1 {
        (var_z / (n as f64)).sqrt()
    } else {
        0.0
    };

    let dx = (f64::from(extent.width()) / ((width.max(2) - 1) as f64)).max(1e-3);
    let dy = (f64::from(extent.height()) / ((height.max(2) - 1) as f64)).max(1e-3);

    let mut slopes_deg: Vec<f64> = Vec::with_capacity(n);
    let mut tri_sum = 0.0_f64;
    let mut rms_residual_sq_sum = 0.0_f64;

    for r in 0..height {
        let r_prev = r.saturating_sub(1);
        let r_next = (r + 1).min(height - 1);
        let span_y = ((r_next - r_prev).max(1) as f64) * dy;

        for c in 0..width {
            let c_prev = c.saturating_sub(1);
            let c_next = (c + 1).min(width - 1);
            let span_x = ((c_next - c_prev).max(1) as f64) * dx;

            let z_center = f64::from(grid[r * width + c]);
            let z_e = f64::from(grid[r * width + c_next]);
            let z_w = f64::from(grid[r * width + c_prev]);
            // Note: row 0 is North (max_y), row height-1 is South (min_y)
            let z_n = f64::from(grid[r_prev * width + c]);
            let z_s = f64::from(grid[r_next * width + c]);

            let dz_dx = (z_e - z_w) / span_x;
            let dz_dy = (z_n - z_s) / span_y;
            let grad_mag = (dz_dx * dz_dx + dz_dy * dz_dy).sqrt();
            let slope_deg = grad_mag.atan().to_degrees();
            slopes_deg.push(slope_deg);

            // 8-neighborhood for TRI (Riley et al. 1999) and local 3x3 mean residual
            let mut sq_diff_neighbors = 0.0_f64;
            let mut neigh_sum = 0.0_f64;
            let mut neigh_count = 0_u32;

            for nr in r_prev..=r_next {
                for nc in c_prev..=c_next {
                    let zn = f64::from(grid[nr * width + nc]);
                    neigh_sum += zn;
                    neigh_count += 1;
                    if nr != r || nc != c {
                        let diff = zn - z_center;
                        sq_diff_neighbors += diff * diff;
                    }
                }
            }

            tri_sum += sq_diff_neighbors.sqrt();
            let local_mean = neigh_sum / f64::from(neigh_count.max(1));
            let residual = z_center - local_mean;
            rms_residual_sq_sum += residual * residual;
        }
    }

    let slope_mean_deg = if !slopes_deg.is_empty() {
        slopes_deg.iter().sum::<f64>() / (slopes_deg.len() as f64)
    } else {
        0.0
    };
    let slope_var = if slopes_deg.len() > 1 {
        slopes_deg
            .iter()
            .map(|&s| (s - slope_mean_deg).powi(2))
            .sum::<f64>()
            / (slopes_deg.len() as f64)
    } else {
        0.0
    };
    let slope_std_deg = slope_var.sqrt();

    slopes_deg.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let p90_idx = ((slopes_deg.len() as f64) * 0.90).floor() as usize;
    let slope_p90_deg = slopes_deg
        .get(p90_idx.min(slopes_deg.len().saturating_sub(1)))
        .copied()
        .unwrap_or(0.0);

    let tri_mean_m = if n > 0 { tri_sum / (n as f64) } else { 0.0 };
    let roughness_rms_m = if n > 0 {
        (rms_residual_sq_sum / (n as f64)).sqrt()
    } else {
        0.0
    };

    let hypsometric_integral = if range_z > 1e-6 {
        ((mean_z - min_z) / range_z).clamp(0.0, 1.0)
    } else {
        0.5
    };

    // Compute true 3D surface area from the welded triangle mesh
    let mut area_3d = 0.0_f64;
    for tri in mesh.indices.chunks_exact(3) {
        let p0 = mesh.positions[tri[0] as usize];
        let p1 = mesh.positions[tri[1] as usize];
        let p2 = mesh.positions[tri[2] as usize];
        let e1 = [
            f64::from(p1[0] - p0[0]),
            f64::from(p1[1] - p0[1]),
            f64::from(p1[2] - p0[2]),
        ];
        let e2 = [
            f64::from(p2[0] - p0[0]),
            f64::from(p2[1] - p0[1]),
            f64::from(p2[2] - p0[2]),
        ];
        let cx = e1[1] * e2[2] - e1[2] * e2[1];
        let cy = e1[2] * e2[0] - e1[0] * e2[2];
        let cz = e1[0] * e2[1] - e1[1] * e2[0];
        area_3d += 0.5 * (cx * cx + cy * cy + cz * cz).sqrt();
    }

    let planar_area = (f64::from(extent.width()) * f64::from(extent.height())).max(1.0);
    let surface_to_planar_ratio = (area_3d / planar_area).max(1.0);

    MorphometryMetrics {
        min_elevation_m: round4(min_z),
        max_elevation_m: round4(max_z),
        elevation_range_m: round4(range_z),
        mean_elevation_m: round4(mean_z),
        std_elevation_m: round4(std_z),
        slope_mean_deg: round4(slope_mean_deg),
        slope_std_deg: round4(slope_std_deg),
        slope_p90_deg: round4(slope_p90_deg),
        tri_mean_m: round4(tri_mean_m),
        roughness_rms_m: round4(roughness_rms_m),
        hypsometric_integral: round4(hypsometric_integral),
        surface_area_3d_m2: round4(area_3d),
        planar_area_2d_m2: round4(planar_area),
        surface_to_planar_ratio: round4(surface_to_planar_ratio),
    }
}

/// Computes 0D Persistent Homology persistence pairs (`|death - birth|`) over a 2D scalar field
/// using Union-Find with the Elder Rule.
///
/// When `invert == false`, performs sublevel-set filtration (basins / craters).
/// When `invert == true`, performs superlevel-set filtration (peaks / domes).
fn persistent_homology_0d_grid(width: usize, height: usize, grid: &[f32], invert: bool) -> Vec<f64> {
    let n = width * height;
    if n == 0 {
        return Vec::new();
    }

    let values: Vec<f64> = grid
        .iter()
        .take(n)
        .map(|&v| {
            let z = f64::from(v);
            if invert { -z } else { z }
        })
        .collect();

    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by(|&a, &b| {
        values[a]
            .partial_cmp(&values[b])
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.cmp(&b))
    });

    let mut rank_of = vec![0usize; n];
    for (rank, &idx) in order.iter().enumerate() {
        rank_of[idx] = rank;
    }

    let mut parent: Vec<usize> = (0..n).collect();
    let mut birth_val: Vec<f64> = values.clone();
    let birth_rank: Vec<usize> = rank_of.clone();
    let mut persistences: Vec<f64> = Vec::new();

    fn find(parent: &mut [usize], mut x: usize) -> usize {
        while parent[x] != x {
            let p = parent[x];
            parent[x] = parent[p];
            x = p;
        }
        x
    }

    for &u in &order {
        let r = u / width;
        let c = u % width;
        let current_val = values[u];

        let r_min = r.saturating_sub(1);
        let r_max = (r + 1).min(height - 1);
        let c_min = c.saturating_sub(1);
        let c_max = (c + 1).min(width - 1);

        for nr in r_min..=r_max {
            for nc in c_min..=c_max {
                if nr == r && nc == c {
                    continue;
                }
                let v = nr * width + nc;
                if rank_of[v] < rank_of[u] {
                    let ru = find(&mut parent, u);
                    let rv = find(&mut parent, v);
                    if ru != rv {
                        // Elder Rule: the component born earlier (smaller birth_rank) survives;
                        // the younger component dies at `current_val`.
                        let (survivor, dying) = if birth_rank[ru] < birth_rank[rv] {
                            (ru, rv)
                        } else {
                            (rv, ru)
                        };
                        let persistence = (current_val - birth_val[dying]).max(0.0);
                        if persistence > 0.0 {
                            persistences.push(persistence);
                        }
                        parent[dying] = survivor;
                        birth_val[survivor] = birth_val[survivor].min(birth_val[dying]);
                    }
                }
            }
        }
    }

    // Also include the global essential class span
    if let (Some(&first), Some(&last)) = (order.first(), order.last()) {
        let global_span = (values[last] - values[first]).max(0.0);
        if global_span > 0.0 {
            persistences.push(global_span);
        }
    }

    persistences
}

/// Computes 0D Persistent Homology and simplicial mesh topology metrics.
///
/// Automatically subsamples dense elevation grids to at most [`MAX_TDA_GRID_DIM`] (`1024x1024`)
/// before running 0D Persistent Homology so TDA finishes in a few seconds.
pub fn compute_topology_metrics(
    width: usize,
    height: usize,
    grid: &[f32],
    elevation_range_m: f64,
    mesh: &WeldedMesh,
) -> TopologyMetrics {
    compute_topology_metrics_with_counts(
        width,
        height,
        grid,
        elevation_range_m,
        mesh,
        mesh.triangle_count,
    )
}

/// Computes 0D Persistent Homology and simplicial mesh topology metrics while recording the
/// `original_triangle_count` prior to automatic mesh simplification.
pub fn compute_topology_metrics_with_counts(
    width: usize,
    height: usize,
    grid: &[f32],
    elevation_range_m: f64,
    mesh: &WeldedMesh,
    original_triangle_count: u32,
) -> TopologyMetrics {
    // Subsample dense grids to <= 1024x1024 cells before 0D Persistent Homology
    let subsampled_owned;
    let (ph_w, ph_h, ph_grid): (usize, usize, &[f32]) =
        if width > MAX_TDA_GRID_DIM || height > MAX_TDA_GRID_DIM {
            let (sw, sh, sg) = subsample_grid_max_dim(width, height, grid, MAX_TDA_GRID_DIM);
            subsampled_owned = sg;
            (sw, sh, &subsampled_owned)
        } else {
            (width, height, grid)
        };

    // Significance threshold: 5% of total elevation relief (minimum 0.5 m)
    let threshold = (elevation_range_m * 0.05).max(0.5);

    let sublevel_pairs = persistent_homology_0d_grid(ph_w, ph_h, ph_grid, false);
    let superlevel_pairs = persistent_homology_0d_grid(ph_w, ph_h, ph_grid, true);

    let sig_basins: Vec<f64> = sublevel_pairs
        .into_iter()
        .filter(|&p| p >= threshold)
        .collect();
    let sig_peaks: Vec<f64> = superlevel_pairs
        .into_iter()
        .filter(|&p| p >= threshold)
        .collect();

    let mut all_sig = Vec::with_capacity(sig_basins.len() + sig_peaks.len());
    all_sig.extend_from_slice(&sig_basins);
    all_sig.extend_from_slice(&sig_peaks);

    let max_persistence_m = all_sig.iter().copied().fold(0.0_f64, f64::max);
    let mean_persistence_m = if !all_sig.is_empty() {
        all_sig.iter().sum::<f64>() / (all_sig.len() as f64)
    } else {
        0.0
    };

    // Simplicial complex analysis on WeldedMesh
    let vertex_count = mesh.positions.len();
    let face_count = mesh.indices.len() / 3;

    let mut edge_incidence: HashMap<(u32, u32), u32> = HashMap::with_capacity(face_count * 2);
    let mut uf_parent: Vec<usize> = (0..vertex_count).collect();

    fn uf_find(parent: &mut [usize], mut x: usize) -> usize {
        while parent[x] != x {
            let p = parent[x];
            parent[x] = parent[p];
            x = p;
        }
        x
    }

    for tri in mesh.indices.chunks_exact(3) {
        let a = tri[0];
        let b = tri[1];
        let c = tri[2];
        for (u, v) in [(a, b), (b, c), (c, a)] {
            let key = if u < v { (u, v) } else { (v, u) };
            *edge_incidence.entry(key).or_insert(0) += 1;

            let ru = uf_find(&mut uf_parent, u as usize);
            let rv = uf_find(&mut uf_parent, v as usize);
            if ru != rv {
                uf_parent[rv] = ru;
            }
        }
    }

    let edge_count = edge_incidence.len();
    let euler_characteristic =
        (vertex_count as i64) - (edge_count as i64) + (face_count as i64);

    let mut roots: HashSet<usize> = HashSet::new();
    for v in 0..vertex_count {
        roots.insert(uf_find(&mut uf_parent, v));
    }
    let connected_components = roots.len();

    let mut boundary_edges = 0_usize;
    let mut non_manifold_edges = 0_usize;
    let mut boundary_uf: HashMap<u32, u32> = HashMap::new();

    for (&(u, v), &count) in &edge_incidence {
        if count == 1 {
            boundary_edges += 1;
            boundary_uf.entry(u).or_insert(u);
            boundary_uf.entry(v).or_insert(v);
        } else if count > 2 {
            non_manifold_edges += 1;
        }
    }

    fn b_find(map: &mut HashMap<u32, u32>, mut x: u32) -> u32 {
        while let Some(&p) = map.get(&x) {
            if p == x {
                break;
            }
            x = p;
        }
        x
    }

    for (&(u, v), &count) in &edge_incidence {
        if count == 1 {
            let ru = b_find(&mut boundary_uf, u);
            let rv = b_find(&mut boundary_uf, v);
            if ru != rv {
                boundary_uf.insert(rv, ru);
            }
        }
    }

    let mut boundary_loops_set: HashSet<u32> = HashSet::new();
    let boundary_keys: Vec<u32> = boundary_uf.keys().copied().collect();
    for k in boundary_keys {
        boundary_loops_set.insert(b_find(&mut boundary_uf, k));
    }
    let boundary_loops = boundary_loops_set.len().max(usize::from(boundary_edges > 0));

    let is_manifold_with_boundary = non_manifold_edges == 0;
    // From χ = 2C - 2g - B => 2g = 2C - B - χ
    let two_g = (2 * connected_components as i64) - (boundary_loops as i64) - euler_characteristic;
    let estimated_genus = if two_g > 0 { (two_g / 2) as usize } else { 0 };

    TopologyMetrics {
        persistent_basins_b0: sig_basins.len(),
        persistent_peaks_b0: sig_peaks.len(),
        max_persistence_m: round4(max_persistence_m),
        mean_persistence_m: round4(mean_persistence_m),
        persistence_threshold_m: round4(threshold),
        original_triangle_count: original_triangle_count as usize,
        simplified_triangle_count: face_count,
        vertex_count,
        edge_count,
        face_count,
        euler_characteristic,
        connected_components,
        boundary_edges,
        is_manifold_with_boundary,
        estimated_genus,
    }
}

/// Projects morphometric and topological metrics into the normalized `[0.0, 1.0]` feature space
/// and proposes the closest planetary analog site and body.
pub fn propose_planetary_analog(
    morph: &MorphometryMetrics,
    topo: &TopologyMetrics,
    catalog: &Catalog,
    body_override: Option<Body>,
) -> AnalogProposal {
    // 1. Normalize physical & topological invariants to [0.0, 1.0]
    let slope_safety = (-morph.slope_p90_deg / 28.0).exp().clamp(0.05, 0.99);
    let roughness_similarity = (1.0 / (1.0 + morph.tri_mean_m / 14.0)).clamp(0.05, 0.99);
    let relief_factor = (morph.elevation_range_m / 350.0).clamp(0.08, 1.0);
    let hi_balance = 1.0 - (morph.hypsometric_integral - 0.48).abs().min(0.45);
    let elevation_relief = (0.65 * relief_factor + 0.35 * hi_balance).clamp(0.05, 0.99);

    let total_persistent = (topo.persistent_basins_b0 + topo.persistent_peaks_b0) as f64;
    let tda_score = (1.0 - (-total_persistent / 6.0).exp()).clamp(0.10, 0.98);
    let tortuosity_score =
        (1.0 / (1.0 + (morph.surface_to_planar_ratio - 1.0) * 4.0)).clamp(0.10, 1.0);
    let geomorphology_match = (0.65 * tda_score + 0.35 * tortuosity_score).clamp(0.05, 0.99);

    let mut normalized_features = BTreeMap::new();
    normalized_features.insert("slope_safety".to_string(), round4(slope_safety));
    normalized_features.insert(
        "roughness_similarity".to_string(),
        round4(roughness_similarity),
    );
    normalized_features.insert("elevation_relief".to_string(), round4(elevation_relief));
    normalized_features.insert(
        "geomorphology_match".to_string(),
        round4(geomorphology_match),
    );
    for key in catalog.default_weights.keys() {
        if !normalized_features.contains_key(key) {
            let mapped = match key.as_str() {
                "slope" => slope_safety,
                "roughness" => roughness_similarity,
                "relief" => elevation_relief,
                "topological_complexity" => geomorphology_match,
                _ => (slope_safety + roughness_similarity + elevation_relief + geomorphology_match)
                    * 0.25,
            };
            normalized_features.insert(key.clone(), round4(mapped));
        }
    }

    let weights = if catalog.default_weights.is_empty() {
        let mut w = BTreeMap::new();
        w.insert("slope_safety".to_string(), 0.35);
        w.insert("roughness_similarity".to_string(), 0.25);
        w.insert("elevation_relief".to_string(), 0.20);
        w.insert("geomorphology_match".to_string(), 0.20);
        w
    } else {
        catalog.default_weights.clone()
    };

    let analog_score = round4(compute_weighted_score(&normalized_features, &weights));

    // 2. Find closest site in Catalog by Euclidean distance over shared normalized features
    let mut best_site_id = "test_site".to_string();
    let mut best_site_name = "Synthetic Crater & Ridge UV Verification Site".to_string();
    let mut best_site_body = Body::Moon;
    let mut best_similarity = 0.85_f64;

    if !catalog.sites.is_empty() {
        let mut max_sim = -1.0_f64;
        for candidate in &catalog.sites {
            let mut sq_dist = 0.0_f64;
            let mut count = 0_usize;
            for (k, &v) in &normalized_features {
                if let Some(&cv) = candidate.feature_scores.get(k) {
                    sq_dist += (v - cv).powi(2);
                    count += 1;
                }
            }
            let sim = if count > 0 {
                (1.0 - (sq_dist / (count as f64)).sqrt()).clamp(0.0, 1.0)
            } else {
                0.5
            };
            if sim > max_sim {
                max_sim = sim;
                best_site_id = candidate.id.clone();
                best_site_name = candidate.name.clone();
                best_site_body = candidate.body;
                best_similarity = round4(sim);
            }
        }
    }

    let inferred_body = body_override.unwrap_or_else(|| {
        if topo.persistent_basins_b0 > topo.persistent_peaks_b0
            && morph.hypsometric_integral < 0.48
        {
            Body::Moon
        } else if morph.elevation_range_m > 180.0 || morph.tri_mean_m > 4.5 {
            Body::Mars
        } else {
            best_site_body
        }
    });

    let decimation_note = if topo.original_triangle_count > topo.simplified_triangle_count {
        format!(
            "Mesh decimated from {} to {} triangles (<= {} budget). ",
            topo.original_triangle_count, topo.simplified_triangle_count, MAX_TRIANGLES
        )
    } else {
        format!(
            "Mesh triangles: {} (within {} budget). ",
            topo.simplified_triangle_count, MAX_TRIANGLES
        )
    };

    let justification = format!(
        "Morphometric & TDA classification -> Proposed Body: {} (Analog Score: {:.4}, Similarity to '{}' [{}]: {:.1}%). \
         {}Differential geometry shows mean slope {:.2}° (P90 = {:.2}°), TRI roughness {:.2} m, and Hypsometric Integral HI = {:.3}. \
         0D Persistent Homology detects β0 = {} persistent basins/craters and β0 = {} persistent peaks/domes (max persistence {:.1} m). \
         Simplicial mesh topology: χ = V({}) - E({}) + F({}) = {}, {} connected component(s), manifold={}.",
        inferred_body.as_str().to_uppercase(),
        analog_score,
        best_site_name,
        best_site_body.as_str().to_uppercase(),
        best_similarity * 100.0,
        decimation_note,
        morph.slope_mean_deg,
        morph.slope_p90_deg,
        morph.tri_mean_m,
        morph.hypsometric_integral,
        topo.persistent_basins_b0,
        topo.persistent_peaks_b0,
        topo.max_persistence_m,
        topo.vertex_count,
        topo.edge_count,
        topo.face_count,
        topo.euler_characteristic,
        topo.connected_components,
        topo.is_manifold_with_boundary,
    );

    AnalogProposal {
        proposed_body: inferred_body,
        closest_site_id: best_site_id,
        closest_site_name: best_site_name,
        closest_site_body: best_site_body,
        similarity_score: best_similarity,
        analog_score,
        normalized_features,
        justification,
    }
}

/// Generates a valid PNG image visualizing hypsometric elevation + hillshade relief
/// aligned 1:1 with `DemExtent` (`NW = (0,0)`, `SE = (1,1)`), used when no `.png` is uploaded.
pub fn generate_shaded_hypsometric_png(width: usize, height: usize, grid: &[f32]) -> Vec<u8> {
    let sub_owned;
    let (w, h, active_grid): (usize, usize, &[f32]) = if width > 512 || height > 512 {
        let (sw, sh, sg) = subsample_grid_max_dim(width, height, grid, 512);
        sub_owned = sg;
        (sw.max(2), sh.max(2), &sub_owned)
    } else {
        (width.max(2), height.max(2), grid)
    };

    let mut min_z = f32::INFINITY;
    let mut max_z = f32::NEG_INFINITY;
    for &z in active_grid.iter().take(w * h) {
        if z.is_finite() {
            min_z = min_z.min(z);
            max_z = max_z.max(z);
        }
    }
    if !min_z.is_finite() || max_z <= min_z {
        min_z = 0.0;
        max_z = 1.0;
    }
    let span_z = (max_z - min_z).max(1e-3);

    // Raw scanlines: 1 filter byte (0) + 3 RGB bytes per pixel
    let mut raw_scanlines = Vec::with_capacity(h * (1 + w * 3));
    for r in 0..h {
        raw_scanlines.push(0u8); // Filter type 0 (None)
        let r_prev = r.saturating_sub(1);
        let r_next = (r + 1).min(h - 1);
        for c in 0..w {
            let c_prev = c.saturating_sub(1);
            let c_next = (c + 1).min(w - 1);
            let z = active_grid[r * w + c];
            let t = ((z - min_z) / span_z).clamp(0.0, 1.0);

            // Azimuthal hillshade from NW
            let dz_x = active_grid[r * w + c_next] - active_grid[r * w + c_prev];
            let dz_y = active_grid[r_prev * w + c] - active_grid[r_next * w + c];
            let shade = (0.72 + (-0.015 * dz_x + 0.015 * dz_y)).clamp(0.35, 1.25);

            // Planetary hypsometric color ramp (deep basalt -> ochre -> highland ivory)
            let base_r = 48.0 + t * 175.0;
            let base_g = 54.0 + t * 155.0;
            let base_b = 68.0 + (1.0 - (t - 0.5).abs() * 1.4) * 110.0;

            let pr = (base_r * shade).clamp(0.0, 255.0) as u8;
            let pg = (base_g * shade).clamp(0.0, 255.0) as u8;
            let pb = (base_b * shade).clamp(0.0, 255.0) as u8;
            raw_scanlines.extend_from_slice(&[pr, pg, pb]);
        }
    }

    encode_uncompressed_rgb_png(w as u32, h as u32, &raw_scanlines)
}

/// Encodes raw PNG scanlines (`filter_byte + RGB` per row) into a valid RFC 2083 PNG using
/// stored RFC 1950/1951 zlib blocks (zero external dependencies).
fn encode_uncompressed_rgb_png(width: u32, height: u32, raw_scanlines: &[u8]) -> Vec<u8> {
    fn crc32(chunk_type: &[u8; 4], data: &[u8]) -> u32 {
        let mut crc = 0xFFFF_FFFF_u32;
        for &b in chunk_type.iter().chain(data.iter()) {
            crc ^= u32::from(b);
            for _ in 0..8 {
                if crc & 1 != 0 {
                    crc = (crc >> 1) ^ 0xEDB8_8320;
                } else {
                    crc >>= 1;
                }
            }
        }
        !crc
    }

    fn adler32(data: &[u8]) -> u32 {
        let mut a = 1_u32;
        let mut b = 0_u32;
        for &byte in data {
            a = (a + u32::from(byte)) % 65_521;
            b = (b + a) % 65_521;
        }
        (b << 16) | a
    }

    let mut zlib_stream = Vec::with_capacity(raw_scanlines.len() + 64);
    zlib_stream.extend_from_slice(&[0x78, 0x01]); // zlib header (CM=8, CINFO=7, no preset dict)

    let chunks: Vec<&[u8]> = raw_scanlines.chunks(65_535).collect();
    if chunks.is_empty() {
        zlib_stream.extend_from_slice(&[0x01, 0x00, 0x00, 0xFF, 0xFF]);
    } else {
        for (i, chunk) in chunks.iter().enumerate() {
            let is_final = u8::from(i + 1 == chunks.len());
            let len = chunk.len() as u16;
            let nlen = !len;
            zlib_stream.push(is_final);
            zlib_stream.extend_from_slice(&len.to_le_bytes());
            zlib_stream.extend_from_slice(&nlen.to_le_bytes());
            zlib_stream.extend_from_slice(chunk);
        }
    }
    zlib_stream.extend_from_slice(&adler32(raw_scanlines).to_be_bytes());

    let mut png = Vec::with_capacity(zlib_stream.len() + 64);
    png.extend_from_slice(b"\x89PNG\r\n\x1a\n");

    let write_chunk = |out: &mut Vec<u8>, ctype: &[u8; 4], data: &[u8]| {
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        out.extend_from_slice(ctype);
        out.extend_from_slice(data);
        out.extend_from_slice(&crc32(ctype, data).to_be_bytes());
    };

    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]); // 8-bit RGB, non-interlaced
    write_chunk(&mut png, b"IHDR", &ihdr);
    write_chunk(&mut png, b"IDAT", &zlib_stream);
    write_chunk(&mut png, b"IEND", &[]);
    png
}

/// High-level pipeline that takes an [`UploadSiteRequest`] and the existing [`Catalog`],
/// validates/parses `.stl` and/or `.tif`/`.tiff` bytes, automatically simplifies meshes exceeding
/// `50,000` triangles, subsamples dense DEM grids (`<= 1024x1024`) before running 0D Persistent
/// Homology, and returns a [`ProcessedSiteBundle`] ready for storage and 3D rendering.
pub fn analyze_and_build_site_bundle(
    req: &UploadSiteRequest,
    catalog: &Catalog,
) -> Result<ProcessedSiteBundle, String> {
    let site_name = if req.site_name.trim().is_empty() {
        "Uploaded Analog Site".to_string()
    } else {
        req.site_name.trim().to_string()
    };
    let site_id = if req.site_id.trim().is_empty() {
        slugify_site_id(&site_name)
    } else {
        slugify_site_id(&req.site_id)
    };

    let raw_stl: Option<Cow<'_, [u8]>> = match req.stl_bytes.as_deref().filter(|b| !b.is_empty()) {
        Some(bytes) => Some(Cow::Borrowed(bytes)),
        None => match req.stl_base64.as_deref().filter(|s| !s.trim().is_empty()) {
            Some(b64) => Some(Cow::Owned(base64_decode(b64)?)),
            None => None,
        },
    };
    let raw_tiff: Option<Cow<'_, [u8]>> =
        match req.tiff_bytes.as_deref().filter(|b| !b.is_empty()) {
            Some(bytes) => Some(Cow::Borrowed(bytes)),
            None => match req.tiff_base64.as_deref().filter(|s| !s.trim().is_empty()) {
                Some(b64) => Some(Cow::Owned(base64_decode(b64)?)),
                None => None,
            },
        };
    let raw_png: Option<Cow<'_, [u8]>> = match req.png_bytes.as_deref().filter(|b| !b.is_empty()) {
        Some(bytes) => Some(Cow::Borrowed(bytes)),
        None => match req.png_base64.as_deref().filter(|s| !s.trim().is_empty()) {
            Some(b64) => Some(Cow::Owned(base64_decode(b64)?)),
            None => None,
        },
    };

    if raw_stl.is_none() && raw_tiff.is_none() {
        return Err(
            "At least one terrain file (.stl mesh or .tif/.tiff DEM) must be provided."
                .to_string(),
        );
    }

    // 1. Resolve DemExtent (explicit -> inferred from STL -> inferred from TIFF dimensions -> default 2km x 2km)
    let extent = if let Some(ext) = req.dem_extent {
        if !ext.is_valid() {
            return Err("Provided DemExtent is invalid (require min_x < max_x and min_y < max_y)."
                .to_string());
        }
        ext
    } else if let Some(ref stl_bytes) = raw_stl {
        infer_extent_from_stl_bytes(stl_bytes)?
    } else if let Some(ref tiff_bytes) = raw_tiff {
        let (w, h) = read_tiff_dimensions(tiff_bytes)?;
        let half_w = (w as f32) * 10.0;
        let half_h = (h as f32) * 10.0;
        DemExtent::new(-half_w, half_w, -half_h, half_h)
    } else {
        DemExtent::new(-1000.0, 1000.0, -1000.0, 1000.0)
    };

    // 2. Build both WeldedMesh (<= 50,000 triangles) and subsampled 2D elevation grid (<= 1024x1024)
    let (
        welded_mesh,
        original_tri_count,
        grid_w,
        grid_h,
        elevation_grid,
        final_stl_bytes,
        final_tiff_bytes,
    ) = match (raw_stl, raw_tiff) {
        (Some(stl_bytes), Some(tiff_bytes)) => {
            let (mesh, orig_tris) = parse_and_weld_stl_with_stats(&stl_bytes, &extent)
                .map_err(|e| format!("Invalid binary STL: {e}"))?;
            // If the uploaded .stl exceeded MAX_TRIANGLES (50,000), save the decimated .stl to disk.
            let out_stl = if orig_tris > MAX_TRIANGLES || mesh.triangle_count < orig_tris {
                welded_mesh_to_binary_stl(&mesh)
            } else {
                stl_bytes.into_owned()
            };
            // Decode (and automatically subsample if > 1024x1024) the DEM TIFF; fall back to
            // rasterizing the welded mesh if the GeoTIFF uses an unsupported tag/compression.
            let (w, h, grid) = match decode_tiff_grid(&tiff_bytes) {
                Ok(res) => res,
                Err(_) => {
                    let rw = 256_usize;
                    let rh = 256_usize;
                    let rgrid = rasterize_welded_mesh_to_grid(&mesh, &extent, rw, rh);
                    (rw, rh, rgrid)
                }
            };
            (mesh, orig_tris, w, h, grid, out_stl, tiff_bytes.into_owned())
        }
        (Some(stl_bytes), None) => {
            let (mesh, orig_tris) = parse_and_weld_stl_with_stats(&stl_bytes, &extent)
                .map_err(|e| format!("Invalid binary STL: {e}"))?;
            let out_stl = if orig_tris > MAX_TRIANGLES || mesh.triangle_count < orig_tris {
                welded_mesh_to_binary_stl(&mesh)
            } else {
                stl_bytes.into_owned()
            };
            let w = if orig_tris > 10_000 { 160_usize } else { 64_usize };
            let h = if orig_tris > 10_000 { 160_usize } else { 64_usize };
            let grid = rasterize_welded_mesh_to_grid(&mesh, &extent, w, h);
            let tiff_bytes = encode_uncompressed_f32_tiff(w as u32, h as u32, &grid);
            (mesh, orig_tris, w, h, grid, out_stl, tiff_bytes)
        }
        (None, Some(tiff_bytes)) => {
            let (orig_w, orig_h) = read_tiff_dimensions(&tiff_bytes)?;
            let orig_tris =
                (2_u64 * (orig_w.saturating_sub(1) as u64) * (orig_h.saturating_sub(1) as u64))
                    .min(u64::from(u32::MAX)) as u32;
            let mesh = parse_dem_tiff(&tiff_bytes, &extent, 1.0)
                .map_err(|e| format!("Invalid DEM TIFF: {e}"))?;
            let (w, h, grid) = decode_tiff_grid(&tiff_bytes)?;
            let stl_bytes = welded_mesh_to_binary_stl(&mesh);
            (
                mesh,
                orig_tris,
                w,
                h,
                grid,
                stl_bytes,
                tiff_bytes.into_owned(),
            )
        }
        (None, None) => unreachable!(),
    };

    // 3. Generate shaded hypsometric PNG texture if no .png was provided
    let final_png_bytes = match raw_png {
        Some(png) if png.starts_with(b"\x89PNG\r\n\x1a\n") => png.into_owned(),
        _ => generate_shaded_hypsometric_png(grid_w, grid_h, &elevation_grid),
    };

    // 4. Compute Morphometry, Differential Geometry, and 0D Persistent Homology (TDA)
    let morphometry =
        compute_morphometry_metrics(grid_w, grid_h, &elevation_grid, &extent, &welded_mesh);
    let topology = compute_topology_metrics_with_counts(
        grid_w,
        grid_h,
        &elevation_grid,
        morphometry.elevation_range_m,
        &welded_mesh,
        original_tri_count,
    );
    let proposal = propose_planetary_analog(&morphometry, &topology, catalog, req.body);

    let description = if req.description.trim().is_empty() {
        proposal.justification.clone()
    } else {
        format!(
            "{} | {}",
            req.description.trim(),
            proposal.justification
        )
    };

    let simplified_tri_count = welded_mesh.triangle_count;

    let metadata = SiteMetadata {
        id: site_id.clone(),
        name: site_name.clone(),
        body: proposal.proposed_body,
        description,
        center_lat_lon: None,
        dem_extent: extent,
        elevation_range_m: Some([
            morphometry.min_elevation_m as f32,
            morphometry.max_elevation_m as f32,
        ]),
        triangle_count: simplified_tri_count,
        mesh_path: format!("{site_id}/terrain.stl"),
        mesh_format: Some(MeshFormat::Stl),
        texture_path: format!("{site_id}/albedo.png"),
        feature_scores: proposal.normalized_features.clone(),
        canonical_score: proposal.analog_score,
        generated_by: Some("analog_core::terrain_analysis".to_string()),
    };

    let report = TerrainAnalysisReport {
        site_id,
        site_name,
        dem_extent: extent,
        original_triangle_count: original_tri_count,
        simplified_triangle_count: simplified_tri_count,
        morphometry,
        topology,
        proposal,
        generated_site_metadata: metadata,
        generated_by: "analog_core::terrain_analysis".to_string(),
    };

    Ok(ProcessedSiteBundle {
        report,
        stl_bytes: final_stl_bytes,
        tiff_bytes: final_tiff_bytes,
        png_bytes: final_png_bytes,
    })
}

#[inline]
fn round4(val: f64) -> f64 {
    if val.is_finite() {
        (val * 10_000.0).round() / 10_000.0
    } else {
        0.0
    }
}
