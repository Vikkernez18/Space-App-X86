// AI-assisted
//! `bevy_egui` side panel, 14" laptop compact tabbed layout, keyboard + touchpad navigation,
//! Planetary Analog Similarity Breakdown (`Earth ↔ Moon/Mars`), Split-Screen 3D HUD overlays,
//! and Researcher Terrain Upload + TDA UI (100% English, zero NASA logos).
//!
//! Provides:
//! - Top-left `[H] Hide/Show Panel` toggle button and `H` keyboard shortcut for fullscreen 3D inspection.
//! - Compact 4-tab layout (`1. Sites & Compare`, `2. 3D & Camera`, `3. Weights & TDA`, `4. Researcher Upload`)
//!   designed to avoid long scrolling on 14" laptop screens.
//! - Keyboard navigation (`ArrowUp`/`ArrowDown` to switch sites, `C` to toggle Split View, `+`/`-` to zoom,
//!   `R` to reset camera, `1`–`4` to switch tabs) while keeping all mouse/touchpad controls intact.
//! - Highlighted **"Planetary Analog Similarity Breakdown (Earth ↔ Moon/Mars)"** card with global
//!   similarity (`0–100%`), per-dimension progress bars (*Elevation Relief*, *Geomorphology Match*,
//!   *Roughness Similarity*, *Slope Safety*), and topological difference ($\Delta \beta_0$ in 0D
//!   Persistent Homology and simplicial Euler characteristic $\chi = V - E + F$).
//! - Synchronized Split-Screen 3D overlay labels above the left and right viewport halves displaying
//!   site name, planetary body (`EARTH`, `MOON`, `MARS`), and min/max elevation.

use std::collections::BTreeMap;

use analog_core::scoring::recalculate_catalog;
use analog_core::stl::{MAX_TRIANGLES, encode_binary_stl, read_stl_triangle_count};
use analog_core::terrain_analysis::{
    TerrainAnalysisReport, UploadSiteRequest, infer_extent_from_stl_bytes,
};
use analog_core::{Body, Catalog, DemExtent, ScoredSite, SiteMetadata};
use bevy::prelude::*;
use bevy_egui::{EguiContexts, egui};

use crate::camera::OrbitCamera;
use crate::mesh_loader::{
    SiteMeshSummary, estimate_site_summary_from_metadata, generate_synthetic_test_site_welded_mesh,
};
use crate::net::{
    BrowserSelectedFile, NetBridge, TerrainSlot, fallback_synthetic_catalog,
    open_browser_file_dialog,
};

#[inline]
fn bytes_to_mib(bytes: usize) -> f64 {
    (bytes as f64) / (1024.0 * 1024.0)
}

/// Returns a distinct high-contrast accent color for each planetary body (`EARTH`, `MOON`, `MARS`).
#[inline]
fn body_accent_color(body: Body) -> egui::Color32 {
    match body {
        Body::Earth => egui::Color32::from_rgb(55, 225, 155),
        Body::Moon => egui::Color32::from_rgb(0, 210, 235),
        Body::Mars => egui::Color32::from_rgb(255, 145, 65),
    }
}

/// Filter for the planetary body selector in the UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BodyFilter {
    All,
    Moon,
    Mars,
    Earth,
}

impl BodyFilter {
    pub fn matches(self, body: Body) -> bool {
        match self {
            Self::All => true,
            Self::Moon => body == Body::Moon,
            Self::Mars => body == Body::Mars,
            Self::Earth => body == Body::Earth,
        }
    }
}

/// Compact navigation tabs for the left control panel (optimized for 14" laptop screens).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanelTab {
    /// `1. Sites & Compare`: Site selector, Split-Screen toggle, and Planetary Analog Similarity Breakdown.
    SitesAndCompare,
    /// `2. 3D & Camera`: Vertical exaggeration, Checkerboard UV toggle, camera controls, and keyboard shortcuts.
    View3dAndCamera,
    /// `3. Weights & TDA`: Live feature weight sliders, ranked scores table, and TDA reports.
    WeightsAndTda,
    /// `4. Researcher Upload`: Argon2id login, binary `.stl`/`.tif`/`.png` upload, TDA report, and site editor.
    ResearcherUpload,
}

/// Editable form state for uploading and analyzing `.stl` / `.tif` / `.png` terrain files
/// via binary `multipart/form-data` (`POST /api/sites/upload`, zero Base64).
#[derive(Debug, Clone)]
pub struct TerrainUploadForm {
    pub id: String,
    pub name: String,
    /// `None` = auto-propose planetary body from morphometric & topological similarity.
    pub body_override: Option<Body>,
    pub description: String,
    pub min_x: f32,
    pub max_x: f32,
    pub min_y: f32,
    pub max_y: f32,
    pub stl_filename: Option<String>,
    pub stl_size_bytes: usize,
    pub stl_detected_triangles: Option<u32>,
    pub stl_bytes: Option<Vec<u8>>,
    pub tiff_filename: Option<String>,
    pub tiff_size_bytes: usize,
    pub tiff_bytes: Option<Vec<u8>>,
    pub png_filename: Option<String>,
    pub png_size_bytes: usize,
    pub png_bytes: Option<Vec<u8>>,
    pub is_uploading: bool,
}

impl Default for TerrainUploadForm {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: "Uploaded Candidate Terrain".to_string(),
            body_override: None,
            description: String::new(),
            min_x: -1000.0,
            max_x: 1000.0,
            min_y: -1000.0,
            max_y: 1000.0,
            stl_filename: None,
            stl_size_bytes: 0,
            stl_detected_triangles: None,
            stl_bytes: None,
            tiff_filename: None,
            tiff_size_bytes: 0,
            tiff_bytes: None,
            png_filename: None,
            png_size_bytes: 0,
            png_bytes: None,
            is_uploading: false,
        }
    }
}

impl TerrainUploadForm {
    /// Assigns a selected or dropped binary file to the `.stl`, `.tif`/`.tiff`, or `.png` slot.
    pub fn ingest_selected_file(&mut self, file: BrowserSelectedFile) -> Result<String, String> {
        let lower = file.name.to_ascii_lowercase();
        if self.name.trim().is_empty() || self.name == "Uploaded Candidate Terrain" {
            if let Some(stem) = file.name.rsplit_once('.').map(|(s, _)| s)
                && !stem.trim().is_empty()
            {
                self.name = stem.replace('_', " ");
            }
        }

        let mib = bytes_to_mib(file.size);

        if lower.ends_with(".stl") {
            let tri_count = read_stl_triangle_count(&file.bytes).ok();
            if let Ok(ext) = infer_extent_from_stl_bytes(&file.bytes) {
                self.min_x = ext.min_x;
                self.max_x = ext.max_x;
                self.min_y = ext.min_y;
                self.max_y = ext.max_y;
            }
            let tri_note = match tri_count {
                Some(c) if c > MAX_TRIANGLES => {
                    format!(", {c} triangles -> will auto-simplify to <= {MAX_TRIANGLES}")
                }
                Some(c) => format!(", {c} triangles"),
                None => String::new(),
            };
            let msg = format!(
                "Staged STL mesh '{}' ({:.2} MiB{tri_note})",
                file.name, mib
            );
            self.stl_filename = Some(file.name);
            self.stl_size_bytes = file.size;
            self.stl_detected_triangles = tri_count;
            self.stl_bytes = Some(file.bytes);
            Ok(msg)
        } else if lower.ends_with(".tif") || lower.ends_with(".tiff") {
            let msg = format!("Staged GeoTIFF DEM '{}' ({:.2} MiB)", file.name, mib);
            self.tiff_filename = Some(file.name);
            self.tiff_size_bytes = file.size;
            self.tiff_bytes = Some(file.bytes);
            Ok(msg)
        } else if lower.ends_with(".png") {
            let msg = format!("Staged PNG texture '{}' ({:.2} MiB)", file.name, mib);
            self.png_filename = Some(file.name);
            self.png_size_bytes = file.size;
            self.png_bytes = Some(file.bytes);
            Ok(msg)
        } else {
            Err(format!(
                "Unsupported file '{}': please select a .stl, .tif/.tiff, or .png file.",
                file.name
            ))
        }
    }

    pub fn has_elevation_source(&self) -> bool {
        self.stl_bytes.is_some() || self.tiff_bytes.is_some()
    }

    pub fn total_staged_bytes(&self) -> usize {
        self.stl_size_bytes + self.tiff_size_bytes + self.png_size_bytes
    }

    pub fn clear_files(&mut self) {
        self.stl_filename = None;
        self.stl_size_bytes = 0;
        self.stl_detected_triangles = None;
        self.stl_bytes = None;
        self.tiff_filename = None;
        self.tiff_size_bytes = 0;
        self.tiff_bytes = None;
        self.png_filename = None;
        self.png_size_bytes = 0;
        self.png_bytes = None;
        self.is_uploading = false;
    }

    /// Populates the upload form with an in-memory synthetic crater STL so researchers can test
    /// the full upload + TDA pipeline in one click.
    pub fn stage_synthetic_demo_stl(&mut self) {
        let extent = DemExtent::new(self.min_x, self.max_x, self.min_y, self.max_y);
        let welded = generate_synthetic_test_site_welded_mesh(&extent);
        let mut tris = Vec::with_capacity(welded.indices.len() / 3);
        for tri in welded.indices.chunks_exact(3) {
            let p0 = welded.positions[tri[0] as usize];
            let p1 = welded.positions[tri[1] as usize];
            let p2 = welded.positions[tri[2] as usize];
            // Convert Bevy Y-up back to STL Z-up: X_stl = X_bevy, Y_stl = -Z_bevy, Z_stl = Y_bevy
            tris.push([
                [p0[0], -p0[2], p0[1]],
                [p1[0], -p1[2], p1[1]],
                [p2[0], -p2[2], p2[1]],
            ]);
        }
        let stl_bytes = encode_binary_stl(&tris);
        self.stl_filename = Some("synthetic_crater_ridge.stl".to_string());
        self.stl_size_bytes = stl_bytes.len();
        self.stl_detected_triangles = Some(tris.len() as u32);
        self.stl_bytes = Some(stl_bytes);
        if self.name.trim().is_empty() {
            self.name = "Synthetic Crater Candidate".to_string();
        }
    }

    pub fn to_upload_request(&self) -> UploadSiteRequest {
        UploadSiteRequest {
            site_id: self.id.trim().to_string(),
            site_name: self.name.trim().to_string(),
            body: self.body_override,
            description: self.description.trim().to_string(),
            dem_extent: Some(DemExtent::new(
                self.min_x, self.max_x, self.min_y, self.max_y,
            )),
            stl_base64: None,
            tiff_base64: None,
            png_base64: None,
            stl_bytes: self.stl_bytes.clone(),
            tiff_bytes: self.tiff_bytes.clone(),
            png_bytes: self.png_bytes.clone(),
        }
    }
}

/// Editable form state for registering or updating a site in authenticated Researcher Mode (`POST /api/sites`).
#[derive(Debug, Clone)]
pub struct SiteEditorForm {
    pub id: String,
    pub name: String,
    pub body: Body,
    pub description: String,
    pub min_x: f32,
    pub max_x: f32,
    pub min_y: f32,
    pub max_y: f32,
    pub triangle_count: u32,
    pub mesh_path: String,
    pub texture_path: String,
    pub feature_scores: BTreeMap<String, f64>,
}

impl SiteEditorForm {
    pub fn from_site(site: &SiteMetadata) -> Self {
        Self {
            id: site.id.clone(),
            name: site.name.clone(),
            body: site.body,
            description: site.description.clone(),
            min_x: site.dem_extent.min_x,
            max_x: site.dem_extent.max_x,
            min_y: site.dem_extent.min_y,
            max_y: site.dem_extent.max_y,
            triangle_count: site.triangle_count,
            mesh_path: site.mesh_path.clone(),
            texture_path: site.texture_path.clone(),
            feature_scores: site.feature_scores.clone(),
        }
    }

    pub fn into_site_metadata(&self) -> SiteMetadata {
        SiteMetadata {
            id: self.id.trim().to_string(),
            name: self.name.trim().to_string(),
            body: self.body,
            description: self.description.trim().to_string(),
            center_lat_lon: None,
            dem_extent: DemExtent::new(self.min_x, self.max_x, self.min_y, self.max_y),
            elevation_range_m: None,
            triangle_count: self.triangle_count,
            mesh_path: self.mesh_path.trim().to_string(),
            mesh_format: None,
            texture_path: self.texture_path.trim().to_string(),
            feature_scores: self.feature_scores.clone(),
            canonical_score: 0.0,
            generated_by: Some("analog_viewer researcher editor".to_string()),
        }
    }
}

/// Quantitative similarity breakdown between a primary site and a comparison site.
#[derive(Debug, Clone)]
pub struct PairwiseSimilarityBreakdown {
    pub global_similarity_pct: f64,
    pub elevation_relief_sim: f64,
    pub primary_relief_score: f64,
    pub compare_relief_score: f64,
    pub geomorphology_sim: f64,
    pub primary_geom_score: f64,
    pub compare_geom_score: f64,
    pub roughness_sim: f64,
    pub primary_rough_score: f64,
    pub compare_rough_score: f64,
    pub slope_safety_sim: f64,
    pub primary_slope_score: f64,
    pub compare_slope_score: f64,
    pub topology_sim: f64,
    pub primary_summary: SiteMeshSummary,
    pub compare_summary: SiteMeshSummary,
    pub delta_basins_b0: usize,
    pub delta_peaks_b0: usize,
    pub delta_euler_chi: i64,
}

/// Global UI & application state resource for the viewer.
#[derive(Resource)]
pub struct ViewerAppState {
    /// Whether the user has dismissed the initial welcome/mode-selection dialog.
    pub mode_selected: bool,
    /// Whether the left side panel is expanded (`true`) or collapsed for fullscreen 3D (`false`, shortcut `H`).
    pub panel_visible: bool,
    /// Active tab in the compact left control panel (`1`–`4`).
    pub active_tab: PanelTab,
    /// Fraction `[0.0, 0.75]` of the window width occupied by the left side panel (`0.0` when hidden).
    pub viewport_left_fraction: f32,
    /// Whether the user is authenticated as a researcher (`GET /api/me` or `POST /api/login`).
    pub authenticated: bool,
    /// Active researcher username when `authenticated == true`.
    pub researcher_username: Option<String>,
    /// Login form username input (defaults to `"equipo_analog"`).
    pub login_username_input: String,
    /// Login form password input.
    pub login_password_input: String,
    /// Status banner message displayed at the top of the side panel.
    pub status_message: String,
    /// Optional last error message from an API call or parser.
    pub error_message: Option<String>,
    /// Loaded site catalog.
    pub catalog: Catalog,
    /// Active planetary body filter in the site selector.
    pub body_filter: BodyFilter,
    /// Currently selected primary site ID (starts at `"test_site"`).
    pub primary_site_id: String,
    /// Optional secondary site ID for side-by-side 3D Split-Screen comparison (`C` shortcut).
    pub comparison_site_id: Option<String>,
    /// Vertical exaggeration factor (`Transform.scale = Vec3::new(1.0, exaggeration, 1.0)`).
    pub vertical_exaggeration: f32,
    /// When `true`, forces the procedural 8x8 NW/SE UV verification checkerboard texture on the 3D mesh.
    pub show_checkerboard_uv: bool,
    /// Live scientific feature weights adjusted via UI sliders.
    pub active_weights: BTreeMap<String, f64>,
    /// Ranked analog scores computed from `active_weights`.
    pub ranked_scores: Vec<ScoredSite>,
    /// Triangle count of the currently rendered primary 3D mesh.
    pub loaded_primary_triangles: u32,
    /// Bounding sphere radius in meters of the currently rendered primary 3D mesh.
    pub loaded_primary_bounding_radius: f32,
    /// Triangle count of the currently rendered comparison 3D mesh.
    pub loaded_comparison_triangles: u32,
    /// Bounding sphere radius in meters of the currently rendered comparison 3D mesh.
    pub loaded_comparison_bounding_radius: f32,
    /// Live elevation & topological summary for the primary site.
    pub loaded_primary_summary: SiteMeshSummary,
    /// Live elevation & topological summary for the comparison site (when loaded).
    pub loaded_comparison_summary: Option<SiteMeshSummary>,
    /// Editable site form for authenticated researchers (`POST /api/sites`).
    pub site_editor: SiteEditorForm,
    /// Upload & TDA analysis form for `.stl` / `.tif` / `.png` files (`POST /api/sites/upload`).
    pub upload_form: TerrainUploadForm,
    /// Most recent mathematical & topological analysis report returned by `POST /api/sites/upload`.
    pub last_analysis_report: Option<TerrainAnalysisReport>,
    /// Request flag signaling the 3D scene system to reload the primary site mesh/texture.
    pub pending_reload_primary: bool,
    /// Request flag signaling the 3D scene system to reload the comparison site mesh/texture.
    pub pending_reload_comparison: bool,
}

impl Default for ViewerAppState {
    fn default() -> Self {
        let catalog = fallback_synthetic_catalog();
        let active_weights = catalog.default_weights.clone();
        let recalc = recalculate_catalog(&catalog, &active_weights);
        let first_site = catalog.sites.first().cloned().unwrap_or(SiteMetadata {
            id: "test_site".to_string(),
            name: "Synthetic Crater & Ridge UV Verification Site".to_string(),
            body: Body::Moon,
            description: String::new(),
            center_lat_lon: None,
            dem_extent: DemExtent::new(-1000.0, 1000.0, -1000.0, 1000.0),
            elevation_range_m: Some([-45.0, 120.0]),
            triangle_count: 2048,
            mesh_path: "test_site/terrain.stl".to_string(),
            mesh_format: None,
            texture_path: "test_site/checkerboard.png".to_string(),
            feature_scores: BTreeMap::new(),
            canonical_score: 0.8485,
            generated_by: None,
        });
        let primary_summary = estimate_site_summary_from_metadata(&first_site);

        Self {
            mode_selected: false,
            panel_visible: true,
            active_tab: PanelTab::SitesAndCompare,
            viewport_left_fraction: 0.30,
            authenticated: false,
            researcher_username: None,
            login_username_input: "equipo_analog".to_string(),
            login_password_input: String::new(),
            status_message: "Select Public Demo Mode or log in as Researcher to begin.".to_string(),
            error_message: None,
            primary_site_id: first_site.id.clone(),
            comparison_site_id: None,
            vertical_exaggeration: 1.5,
            show_checkerboard_uv: true,
            active_weights,
            ranked_scores: recalc.results,
            loaded_primary_triangles: first_site.triangle_count,
            loaded_primary_bounding_radius: 1415.0,
            loaded_comparison_triangles: first_site.triangle_count,
            loaded_comparison_bounding_radius: 1415.0,
            loaded_primary_summary: primary_summary,
            loaded_comparison_summary: None,
            site_editor: SiteEditorForm::from_site(&first_site),
            upload_form: TerrainUploadForm::default(),
            last_analysis_report: None,
            catalog,
            body_filter: BodyFilter::All,
            pending_reload_primary: true,
            pending_reload_comparison: false,
        }
    }
}

impl ViewerAppState {
    /// Returns the currently selected primary [`SiteMetadata`].
    pub fn primary_site(&self) -> Option<&SiteMetadata> {
        self.catalog
            .sites
            .iter()
            .find(|s| s.id == self.primary_site_id)
            .or_else(|| self.catalog.sites.first())
    }

    /// Returns the currently selected comparison [`SiteMetadata`], if any.
    pub fn comparison_site(&self) -> Option<&SiteMetadata> {
        let comp_id = self.comparison_site_id.as_deref()?;
        self.catalog.sites.iter().find(|s| s.id == comp_id)
    }

    /// Automatically finds the closest analog site in `catalog.sites` to `primary`,
    /// prioritizing sites on a different planetary body (`Earth ↔ Moon` or `Earth ↔ Mars`).
    pub fn find_best_comparison_site(&self) -> Option<SiteMetadata> {
        let primary = self.primary_site()?;
        let feature_dist = |cand: &SiteMetadata| -> f64 {
            let keys = [
                "elevation_relief",
                "geomorphology_match",
                "roughness_similarity",
                "slope_safety",
            ];
            let mut sum_sq = 0.0_f64;
            for k in keys {
                let a = primary.feature_scores.get(k).copied().unwrap_or(0.5);
                let b = cand.feature_scores.get(k).copied().unwrap_or(0.5);
                let d = a - b;
                sum_sq += d * d;
            }
            (sum_sq / (keys.len() as f64)).sqrt()
        };

        // 1. Prefer a site on a different planetary body (e.g. Earth vs Moon/Mars)
        let best_cross_body = self
            .catalog
            .sites
            .iter()
            .filter(|s| s.id != primary.id && s.body != primary.body)
            .min_by(|a, b| {
                feature_dist(a)
                    .partial_cmp(&feature_dist(b))
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .cloned();

        if best_cross_body.is_some() {
            return best_cross_body;
        }

        // 2. Otherwise pick the closest distinct site in the catalog
        self.catalog
            .sites
            .iter()
            .filter(|s| s.id != primary.id)
            .min_by(|a, b| {
                feature_dist(a)
                    .partial_cmp(&feature_dist(b))
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .cloned()
    }

    /// Returns `(target_site, is_split_screen_active)` for the Planetary Analog Similarity Breakdown card.
    pub fn resolved_similarity_target(&self) -> Option<(SiteMetadata, bool)> {
        if let Some(comp) = self.comparison_site().cloned() {
            return Some((comp, true));
        }
        if let Some(auto_site) = self.find_best_comparison_site() {
            return Some((auto_site, false));
        }
        self.primary_site().cloned().map(|s| (s, false))
    }

    /// Returns the [`SiteMeshSummary`] for any target site (using live mesh stats if loaded).
    pub fn summary_for_site(&self, site: &SiteMetadata) -> SiteMeshSummary {
        if site.id == self.primary_site_id {
            return self.loaded_primary_summary;
        }
        if self.comparison_site_id.as_deref() == Some(site.id.as_str())
            && let Some(summary) = self.loaded_comparison_summary
        {
            return summary;
        }
        estimate_site_summary_from_metadata(site)
    }

    /// Computes the quantitative [`PairwiseSimilarityBreakdown`] between `primary` and `compare`.
    pub fn compute_pairwise_similarity(
        &self,
        primary: &SiteMetadata,
        compare: &SiteMetadata,
    ) -> PairwiseSimilarityBreakdown {
        let get_f = |s: &SiteMetadata, k: &str| -> f64 {
            s.feature_scores
                .get(k)
                .copied()
                .unwrap_or(0.5)
                .clamp(0.0, 1.0)
        };

        let primary_relief_score = get_f(primary, "elevation_relief");
        let compare_relief_score = get_f(compare, "elevation_relief");
        let elevation_relief_sim =
            (1.0 - (primary_relief_score - compare_relief_score).abs()).clamp(0.0, 1.0);

        let primary_geom_score = get_f(primary, "geomorphology_match");
        let compare_geom_score = get_f(compare, "geomorphology_match");
        let geomorphology_sim =
            (1.0 - (primary_geom_score - compare_geom_score).abs()).clamp(0.0, 1.0);

        let primary_rough_score = get_f(primary, "roughness_similarity");
        let compare_rough_score = get_f(compare, "roughness_similarity");
        let roughness_sim =
            (1.0 - (primary_rough_score - compare_rough_score).abs()).clamp(0.0, 1.0);

        let primary_slope_score = get_f(primary, "slope_safety");
        let compare_slope_score = get_f(compare, "slope_safety");
        let slope_safety_sim =
            (1.0 - (primary_slope_score - compare_slope_score).abs()).clamp(0.0, 1.0);

        let w_er = self
            .active_weights
            .get("elevation_relief")
            .copied()
            .unwrap_or(0.20)
            .max(0.0);
        let w_gm = self
            .active_weights
            .get("geomorphology_match")
            .copied()
            .unwrap_or(0.20)
            .max(0.0);
        let w_rs = self
            .active_weights
            .get("roughness_similarity")
            .copied()
            .unwrap_or(0.25)
            .max(0.0);
        let w_ss = self
            .active_weights
            .get("slope_safety")
            .copied()
            .unwrap_or(0.35)
            .max(0.0);
        let w_sum = w_er + w_gm + w_rs + w_ss;

        let weighted_feature_sim = if w_sum > 1e-9 {
            (w_er * elevation_relief_sim
                + w_gm * geomorphology_sim
                + w_rs * roughness_sim
                + w_ss * slope_safety_sim)
                / w_sum
        } else {
            0.25 * (elevation_relief_sim + geomorphology_sim + roughness_sim + slope_safety_sim)
        };

        let primary_summary = self.summary_for_site(primary);
        let compare_summary = self.summary_for_site(compare);

        let delta_basins_b0 = primary_summary
            .persistent_basins_b0
            .abs_diff(compare_summary.persistent_basins_b0);
        let delta_peaks_b0 = primary_summary
            .persistent_peaks_b0
            .abs_diff(compare_summary.persistent_peaks_b0);
        let delta_euler_chi =
            (primary_summary.euler_characteristic - compare_summary.euler_characteristic).abs();

        // Normalized log-scale topological similarity so both small and large β₀ counts compare smoothly
        let b1_ln = (1.0 + primary_summary.persistent_basins_b0 as f64).ln();
        let b2_ln = (1.0 + compare_summary.persistent_basins_b0 as f64).ln();
        let p1_ln = (1.0 + primary_summary.persistent_peaks_b0 as f64).ln();
        let p2_ln = (1.0 + compare_summary.persistent_peaks_b0 as f64).ln();
        let b0_diff = (b1_ln - b2_ln).abs() + 0.7 * (p1_ln - p2_ln).abs();
        let chi_penalty = (delta_euler_chi as f64) * 0.08;
        let topology_sim = (1.0 / (1.0 + 0.18 * b0_diff + chi_penalty)).clamp(0.0, 1.0);

        let global_similarity_pct =
            ((0.82 * weighted_feature_sim + 0.18 * topology_sim) * 100.0).clamp(0.0, 100.0);

        PairwiseSimilarityBreakdown {
            global_similarity_pct,
            elevation_relief_sim,
            primary_relief_score,
            compare_relief_score,
            geomorphology_sim,
            primary_geom_score,
            compare_geom_score,
            roughness_sim,
            primary_rough_score,
            compare_rough_score,
            slope_safety_sim,
            primary_slope_score,
            compare_slope_score,
            topology_sim,
            primary_summary,
            compare_summary,
            delta_basins_b0,
            delta_peaks_b0,
            delta_euler_chi,
        }
    }

    /// Replaces the catalog, synchronizes weights if empty, and updates rankings.
    pub fn apply_catalog(&mut self, catalog: Catalog) {
        if !catalog.default_weights.is_empty() {
            self.active_weights = catalog.default_weights.clone();
        }
        let recalc = recalculate_catalog(&catalog, &self.active_weights);
        self.ranked_scores = recalc.results;

        let primary_exists = catalog.sites.iter().any(|s| s.id == self.primary_site_id);
        if !primary_exists
            && let Some(first) = catalog.sites.first()
        {
            self.primary_site_id = first.id.clone();
            self.site_editor = SiteEditorForm::from_site(first);
            self.loaded_primary_summary = estimate_site_summary_from_metadata(first);
        } else if let Some(current) = catalog.sites.iter().find(|s| s.id == self.primary_site_id) {
            self.site_editor = SiteEditorForm::from_site(current);
            self.loaded_primary_summary = estimate_site_summary_from_metadata(current);
        }

        self.catalog = catalog;
        self.pending_reload_primary = true;
    }

    /// Selects a new primary site and triggers asynchronous mesh + texture loading.
    pub fn select_primary_site(&mut self, site_id: &str, net: &NetBridge) {
        if let Some(site) = self.catalog.sites.iter().find(|s| s.id == site_id).cloned() {
            self.primary_site_id = site.id.clone();
            self.site_editor = SiteEditorForm::from_site(&site);
            self.loaded_primary_summary = estimate_site_summary_from_metadata(&site);
            self.pending_reload_primary = true;
            net.fetch_site_assets(TerrainSlot::Primary, &site);
        }
    }

    /// Cycles the primary site forward (`step = 1`) or backward (`step = -1`) via `ArrowDown` / `ArrowUp`.
    pub fn cycle_primary_site(&mut self, step: isize, net: &NetBridge) {
        let mut pool: Vec<String> = self
            .catalog
            .sites
            .iter()
            .filter(|s| self.body_filter.matches(s.body))
            .map(|s| s.id.clone())
            .collect();
        if pool.is_empty() {
            pool = self.catalog.sites.iter().map(|s| s.id.clone()).collect();
        }
        if pool.is_empty() {
            return;
        }
        let cur_idx = pool
            .iter()
            .position(|id| *id == self.primary_site_id)
            .unwrap_or(0);
        let len = pool.len() as isize;
        let next_idx = (cur_idx as isize + step).rem_euclid(len) as usize;
        let next_id = pool[next_idx].clone();
        self.select_primary_site(&next_id, net);
        if let Some(site) = self.primary_site() {
            self.status_message = format!(
                "Switched Primary Site to '{}' [{}] (ArrowUp/ArrowDown).",
                site.name,
                site.body.as_str().to_uppercase()
            );
        }
    }

    /// Toggles synchronized side-by-side 3D Split-Screen comparison (`C` shortcut).
    pub fn toggle_split_screen_comparison(&mut self, net: &NetBridge) {
        if self.comparison_site_id.is_some() {
            self.comparison_site_id = None;
            self.loaded_comparison_summary = None;
            self.pending_reload_comparison = true;
            self.status_message = "Exited Split-Screen (Single 3D View). Press [C] to compare again.".to_string();
        } else {
            let target = self
                .find_best_comparison_site()
                .or_else(|| self.primary_site().cloned());
            if let Some(comp) = target {
                self.comparison_site_id = Some(comp.id.clone());
                self.loaded_comparison_summary = Some(estimate_site_summary_from_metadata(&comp));
                self.pending_reload_comparison = true;
                self.status_message = format!(
                    "Split-Screen active: comparing '{}' vs '{}' [{}]. Orbit/Zoom are synchronized!",
                    self.primary_site()
                        .map(|s| s.name.as_str())
                        .unwrap_or("Primary"),
                    comp.name,
                    comp.body.as_str().to_uppercase()
                );
                net.fetch_site_assets(TerrainSlot::Comparison, &comp);
            }
        }
    }
}

/// Renders the highlighted **"Planetary Analog Similarity Breakdown (Earth ↔ Moon/Mars)"** card.
fn render_similarity_breakdown_card(
    ui: &mut egui::Ui,
    app_state: &mut ViewerAppState,
    net: &NetBridge,
) {
    let Some(primary) = app_state.primary_site().cloned() else {
        return;
    };
    let Some((target, is_split_active)) = app_state.resolved_similarity_target() else {
        return;
    };

    let sim = app_state.compute_pairwise_similarity(&primary, &target);

    egui::Frame::group(ui.style())
        .fill(egui::Color32::from_rgba_unmultiplied(18, 28, 40, 245))
        .stroke(egui::Stroke::new(
            1.5,
            if is_split_active {
                egui::Color32::from_rgb(0, 220, 180)
            } else {
                egui::Color32::from_rgb(0, 185, 220)
            },
        ))
        .inner_margin(egui::Margin::same(8.0))
        .show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.colored_label(
                    egui::Color32::from_rgb(0, 225, 235),
                    egui::RichText::new(
                        "Planetary Analog Similarity Breakdown (Earth ↔ Moon/Mars)",
                    )
                    .strong(),
                );
                if is_split_active {
                    ui.colored_label(
                        egui::Color32::from_rgb(80, 240, 165),
                        egui::RichText::new("[SPLIT-SCREEN ON]").small().strong(),
                    );
                } else {
                    ui.colored_label(
                        egui::Color32::from_gray(175),
                        egui::RichText::new("[AUTO-MATCHED ANALOG]").small(),
                    );
                }
            });

            // Pair Header: Primary [BODY] <-> Compare [BODY]
            ui.horizontal_wrapped(|ui| {
                ui.colored_label(
                    body_accent_color(primary.body),
                    egui::RichText::new(format!(
                        "{} [{}]",
                        primary.name,
                        primary.body.as_str().to_uppercase()
                    ))
                    .strong()
                    .small(),
                );
                ui.label(egui::RichText::new("↔").strong());
                ui.colored_label(
                    body_accent_color(target.body),
                    egui::RichText::new(format!(
                        "{} [{}]",
                        target.name,
                        target.body.as_str().to_uppercase()
                    ))
                    .strong()
                    .small(),
                );
            });

            ui.add_space(4.0);

            // Global Similarity Progress Bar (0-100%)
            let global_frac = (sim.global_similarity_pct / 100.0).clamp(0.0, 1.0) as f32;
            let global_color = if sim.global_similarity_pct >= 75.0 {
                egui::Color32::from_rgb(35, 205, 145)
            } else if sim.global_similarity_pct >= 50.0 {
                egui::Color32::from_rgb(0, 185, 225)
            } else {
                egui::Color32::from_rgb(235, 145, 50)
            };
            ui.add(
                egui::ProgressBar::new(global_frac)
                    .fill(global_color)
                    .text(
                        egui::RichText::new(format!(
                            "Global Analog Similarity: {:.1}%",
                            sim.global_similarity_pct
                        ))
                        .strong()
                        .color(egui::Color32::WHITE),
                    ),
            );

            ui.add_space(4.0);

            // Per-Dimension Similarity Progress Bars
            let dz_p = (sim.primary_summary.max_elevation_m - sim.primary_summary.min_elevation_m)
                .abs();
            let dz_c = (sim.compare_summary.max_elevation_m - sim.compare_summary.min_elevation_m)
                .abs();

            let dimensions = [
                (
                    "Elevation Relief",
                    sim.elevation_relief_sim,
                    format!(
                        "{:.0}% ({:.2} vs {:.2} | Δz: {:.0}m vs {:.0}m)",
                        sim.elevation_relief_sim * 100.0,
                        sim.primary_relief_score,
                        sim.compare_relief_score,
                        dz_p,
                        dz_c
                    ),
                ),
                (
                    "Geomorphology Match",
                    sim.geomorphology_sim,
                    format!(
                        "{:.0}% ({:.2} vs {:.2})",
                        sim.geomorphology_sim * 100.0,
                        sim.primary_geom_score,
                        sim.compare_geom_score
                    ),
                ),
                (
                    "Roughness Similarity",
                    sim.roughness_sim,
                    format!(
                        "{:.0}% ({:.2} vs {:.2})",
                        sim.roughness_sim * 100.0,
                        sim.primary_rough_score,
                        sim.compare_rough_score
                    ),
                ),
                (
                    "Slope Safety",
                    sim.slope_safety_sim,
                    format!(
                        "{:.0}% ({:.2} vs {:.2})",
                        sim.slope_safety_sim * 100.0,
                        sim.primary_slope_score,
                        sim.compare_slope_score
                    ),
                ),
                (
                    "Topology (0D PH & χ)",
                    sim.topology_sim,
                    format!(
                        "{:.0}% (Δβ₀ basins={}, Δχ={})",
                        sim.topology_sim * 100.0,
                        sim.delta_basins_b0,
                        sim.delta_euler_chi
                    ),
                ),
            ];

            egui::Grid::new("similarity_dimensions_grid")
                .num_columns(2)
                .spacing([8.0, 4.0])
                .show(ui, |ui| {
                    for (label, frac, bar_text) in dimensions {
                        ui.label(egui::RichText::new(label).small());
                        ui.add(
                            egui::ProgressBar::new(frac as f32)
                                .desired_width(215.0)
                                .fill(egui::Color32::from_rgb(30, 145, 195))
                                .text(egui::RichText::new(bar_text).small()),
                        );
                        ui.end_row();
                    }
                });

            ui.add_space(4.0);

            // Topological Difference Summary (0D Persistent Homology Δβ₀ and Euler χ)
            ui.group(|ui| {
                ui.label(
                    egui::RichText::new(
                        "Topological Invariant Difference (0D Persistent Homology Δβ₀ & Euler χ):",
                    )
                    .small()
                    .strong(),
                );
                ui.monospace(format!(
                    "β₀ Basins : {} vs {} (Δβ₀ = {}) | β₀ Peaks: {} vs {} (Δβ₀ = {})",
                    sim.primary_summary.persistent_basins_b0,
                    sim.compare_summary.persistent_basins_b0,
                    sim.delta_basins_b0,
                    sim.primary_summary.persistent_peaks_b0,
                    sim.compare_summary.persistent_peaks_b0,
                    sim.delta_peaks_b0,
                ));
                ui.monospace(format!(
                    "Euler χ   : χ₁ = {} vs χ₂ = {} (Δχ = {})",
                    sim.primary_summary.euler_characteristic,
                    sim.compare_summary.euler_characteristic,
                    sim.delta_euler_chi,
                ));
            });

            ui.add_space(4.0);
            ui.horizontal(|ui| {
                if is_split_active {
                    if ui
                        .button("[C] Exit 3D Split-Screen (Single View)")
                        .clicked()
                    {
                        app_state.toggle_split_screen_comparison(net);
                    }
                } else {
                    let btn = egui::Button::new(
                        egui::RichText::new(format!(
                            "[C] Launch Synchronized 3D Split-Screen ({} ↔ {})",
                            primary.name, target.name
                        ))
                        .strong()
                        .color(egui::Color32::from_rgb(10, 25, 30)),
                    )
                    .fill(egui::Color32::from_rgb(0, 215, 185));
                    if ui.add(btn).clicked() {
                        app_state.comparison_site_id = Some(target.id.clone());
                        app_state.loaded_comparison_summary =
                            Some(estimate_site_summary_from_metadata(&target));
                        app_state.pending_reload_comparison = true;
                        app_state.status_message = format!(
                            "Split-Screen active: '{}' [{}] ↔ '{}' [{}].",
                            primary.name,
                            primary.body.as_str().to_uppercase(),
                            target.name,
                            target.body.as_str().to_uppercase()
                        );
                        net.fetch_site_assets(TerrainSlot::Comparison, &target);
                    }
                }
            });
        });
}

/// Renders the Mathematical, Differential-Geometric & Topological Report (TDA) panel.
fn render_tda_report_panel(ui: &mut egui::Ui, report: &TerrainAnalysisReport) {
    egui::CollapsingHeader::new(
        egui::RichText::new("Mathematical & Topological Analysis Report (TDA)")
            .strong()
            .color(egui::Color32::from_rgb(120, 240, 170)),
    )
    .default_open(true)
    .show(ui, |ui| {
        ui.group(|ui| {
            ui.colored_label(
                egui::Color32::from_rgb(0, 220, 230),
                format!(
                    "Proposed Planetary Analog: {} (Score: {:.4})",
                    report.proposal.proposed_body.as_str().to_uppercase(),
                    report.proposal.analog_score
                ),
            );
            ui.monospace(format!(
                "Closest Reference : {} ({})",
                report.proposal.closest_site_name, report.proposal.closest_site_id
            ));
            ui.monospace(format!(
                "Feature Similarity: {:.1}%",
                report.proposal.similarity_score * 100.0
            ));
            ui.add_space(2.0);
            ui.label(egui::RichText::new(&report.proposal.justification).small());
        });

        ui.group(|ui| {
            ui.strong("Differential Geometry & Morphometry (∇z, TRI, HI)");
            let m = &report.morphometry;
            ui.monospace(format!(
                "Elevation z (m)   : [{:.2}, {:.2}] (Δz = {:.2} m)",
                m.min_elevation_m, m.max_elevation_m, m.elevation_range_m
            ));
            ui.monospace(format!(
                "Mean / Std z (m)  : {:.2} ± {:.2} m",
                m.mean_elevation_m, m.std_elevation_m
            ));
            ui.monospace(format!(
                "Slope |∇z| (deg)  : mean={:.2}° | std={:.2}° | P90={:.2}°",
                m.slope_mean_deg, m.slope_std_deg, m.slope_p90_deg
            ));
            ui.monospace(format!(
                "Roughness TRI/RMS : TRI={:.3} m | Laplacian RMS={:.3} m",
                m.tri_mean_m, m.roughness_rms_m
            ));
            ui.monospace(format!(
                "Hypsometric Int HI: {:.4} | Tortuosity A3D/A2D: {:.5}",
                m.hypsometric_integral, m.surface_to_planar_ratio
            ));
        });

        ui.group(|ui| {
            ui.strong("Topological Data Analysis (0D Persistent Homology & χ)");
            let t = &report.topology;
            let orig_tris = t.original_triangle_count.max(t.face_count);
            let simp_tris = t.simplified_triangle_count.max(t.face_count);
            ui.monospace(format!(
                "Mesh Triangles    : {orig_tris} original -> {simp_tris} simplified (<= 50,000)"
            ));
            ui.monospace(format!(
                "β₀ Sublevel Basins: {} craters/depressions",
                t.persistent_basins_b0
            ));
            ui.monospace(format!(
                "β₀ Superlevel Peak: {} peaks/domes",
                t.persistent_peaks_b0
            ));
            ui.monospace(format!(
                "Persistence (m)   : max={:.2} m | mean={:.2} m (τ={:.2} m)",
                t.max_persistence_m, t.mean_persistence_m, t.persistence_threshold_m
            ));
            ui.monospace(format!(
                "Simplicial Mesh   : V={} | E={} | F={}",
                t.vertex_count, t.edge_count, t.face_count
            ));
            ui.monospace(format!(
                "Euler χ = V-E+F   : {} | Components (β₀)={} | Genus≈{}",
                t.euler_characteristic, t.connected_components, t.estimated_genus
            ));
            ui.monospace(format!(
                "Boundary Edges    : {} | 2-Manifold: {}",
                t.boundary_edges,
                if t.is_manifold_with_boundary {
                    "Yes"
                } else {
                    "No"
                }
            ));
        });
    });
}

/// Renders the startup mode selection modal, compact 14" laptop tabbed side panel,
/// top-left `[H] Hide/Show Panel` button, and synchronized Split-Screen 3D overlays.
pub fn viewer_ui_system(
    mut contexts: EguiContexts,
    mut app_state: ResMut<ViewerAppState>,
    net: Res<NetBridge>,
    mut cam_query: Query<(&mut OrbitCamera, &mut Projection)>,
) {
    let ctx = contexts.ctx_mut();

    // Apply clean dark scientific theme
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = egui::Color32::from_rgba_unmultiplied(15, 20, 28, 242);
    ctx.set_visuals(visuals);

    // 0. Global Keyboard Navigation (when no text input field is focused)
    if app_state.mode_selected && !ctx.wants_keyboard_input() {
        let mut toggle_panel = false;
        let mut cycle_up = false;
        let mut cycle_down = false;
        let mut toggle_split = false;
        let mut zoom_in_step = false;
        let mut zoom_out_step = false;
        let mut tab_select: Option<PanelTab> = None;

        ctx.input(|i| {
            if i.key_pressed(egui::Key::H) {
                toggle_panel = true;
            }
            if i.key_pressed(egui::Key::ArrowUp) {
                cycle_up = true;
            }
            if i.key_pressed(egui::Key::ArrowDown) {
                cycle_down = true;
            }
            if i.key_pressed(egui::Key::C) {
                toggle_split = true;
            }
            if i.key_pressed(egui::Key::Plus) || i.key_pressed(egui::Key::Equals) {
                zoom_in_step = true;
            }
            if i.key_pressed(egui::Key::Minus) {
                zoom_out_step = true;
            }
            if i.key_pressed(egui::Key::Num1) {
                tab_select = Some(PanelTab::SitesAndCompare);
            } else if i.key_pressed(egui::Key::Num2) {
                tab_select = Some(PanelTab::View3dAndCamera);
            } else if i.key_pressed(egui::Key::Num3) {
                tab_select = Some(PanelTab::WeightsAndTda);
            } else if i.key_pressed(egui::Key::Num4) {
                tab_select = Some(PanelTab::ResearcherUpload);
            }
            for ev in &i.events {
                if let egui::Event::Text(text) = ev {
                    if text == "+" || text == "=" {
                        zoom_in_step = true;
                    } else if text == "-" || text == "_" {
                        zoom_out_step = true;
                    }
                }
            }
        });

        if toggle_panel {
            app_state.panel_visible = !app_state.panel_visible;
        }
        if let Some(tab) = tab_select {
            app_state.active_tab = tab;
            app_state.panel_visible = true;
        }
        if cycle_up {
            app_state.cycle_primary_site(-1, &net);
        } else if cycle_down {
            app_state.cycle_primary_site(1, &net);
        }
        if toggle_split {
            app_state.toggle_split_screen_comparison(&net);
        }
        if zoom_in_step {
            for (mut cam, _) in &mut cam_query {
                cam.zoom_by_factor(0.86);
            }
        } else if zoom_out_step {
            for (mut cam, _) in &mut cam_query {
                cam.zoom_by_factor(1.16);
            }
        }
    }

    // 1. Startup Mode Selector Modal (Enter Public Demo Mode vs Researcher Login)
    if !app_state.mode_selected {
        egui::Window::new("Space-App-X86 — Terrestrial Analogs 3D Platform")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .fixed_size(egui::Vec2::new(430.0, 355.0))
            .show(ctx, |ui| {
                ui.heading("Welcome to Space-App-X86");
                ui.label(
                    "Explore Lunar, Martian, and Earth terrestrial analog sites in 3D (.stl & .tif DEMs with planar DemExtent UV mapping and TDA scoring).",
                );
                ui.add_space(8.0);

                ui.group(|ui| {
                    ui.heading("1. Judges & Visitors (No Password Required)");
                    ui.label(
                        "Inspect 3D terrain meshes, compare Earth ↔ Moon/Mars analogs in Split-Screen, and test custom weights immediately.",
                    );
                    ui.add_space(6.0);
                    let demo_btn = egui::Button::new(
                        egui::RichText::new("Enter Public Demo Mode")
                            .strong()
                            .size(16.0)
                            .color(egui::Color32::from_rgb(10, 25, 30)),
                    )
                    .fill(egui::Color32::from_rgb(0, 210, 220))
                    .min_size(egui::Vec2::new(395.0, 36.0));

                    if ui.add(demo_btn).clicked() {
                        app_state.mode_selected = true;
                        app_state.error_message = None;
                        app_state.status_message =
                            "Entering Public Demo Mode (fetching /api/demo & /api/catalog)..."
                                .to_string();
                        net.fetch_demo_mode();
                    }
                });

                ui.add_space(8.0);

                ui.group(|ui| {
                    ui.heading("2. Researcher Login (Argon2id Session)");
                    ui.label(
                        "Authenticate (default user: equipo_analog) to upload & analyze .stl/.tif/.png terrains, persist weights, and register sites.",
                    );
                    ui.add_space(4.0);
                    let mut submit_login = false;
                    egui::Grid::new("startup_login_grid")
                        .num_columns(2)
                        .spacing([8.0, 6.0])
                        .show(ui, |ui| {
                            ui.label("Username:");
                            ui.text_edit_singleline(&mut app_state.login_username_input);
                            ui.end_row();

                            ui.label("Password:");
                            let pass_resp = ui.add(
                                egui::TextEdit::singleline(&mut app_state.login_password_input)
                                    .password(true),
                            );
                            if pass_resp.lost_focus()
                                && ui.input(|i| i.key_pressed(egui::Key::Enter))
                            {
                                submit_login = true;
                            }
                            ui.end_row();
                        });

                    ui.add_space(6.0);
                    if ui
                        .add_sized(
                            [395.0, 30.0],
                            egui::Button::new("Log In as Researcher (POST /api/login)"),
                        )
                        .clicked()
                    {
                        submit_login = true;
                    }

                    if submit_login {
                        app_state.error_message = None;
                        let user = app_state.login_username_input.trim().to_string();
                        let pass = app_state.login_password_input.clone();
                        app_state.login_password_input.clear();
                        app_state.status_message = format!(
                            "Authenticating researcher '{}' via Argon2id (POST /api/login)...",
                            user
                        );
                        net.login(user, pass);
                    }
                });

                if let Some(err) = &app_state.error_message {
                    ui.add_space(6.0);
                    ui.colored_label(egui::Color32::from_rgb(255, 110, 110), err);
                }
            });
    }

    // 2. Left Side Panel (when `panel_visible == true`) OR Floating Top-Left Bar (when collapsed)
    if app_state.panel_visible {
        egui::SidePanel::left("analog_control_panel")
            .default_width(395.0)
            .width_range(330.0..=520.0)
            .resizable(true)
            .show(ctx, |ui| {
                // Top-left bar with [H] Hide/Show Panel button and quick keyboard controls
                ui.horizontal_wrapped(|ui| {
                    let hide_btn = egui::Button::new(
                        egui::RichText::new("[H] Hide/Show Panel")
                            .strong()
                            .color(egui::Color32::from_rgb(10, 25, 30)),
                    )
                    .fill(egui::Color32::from_rgb(0, 210, 220));
                    if ui
                        .add(hide_btn)
                        .on_hover_text("Collapse panel for fullscreen 3D view (Shortcut: H)")
                        .clicked()
                    {
                        app_state.panel_visible = false;
                    }

                    let split_active = app_state.comparison_site_id.is_some();
                    let split_btn = egui::Button::new(
                        egui::RichText::new(if split_active {
                            "[C] Split View: ON"
                        } else {
                            "[C] Split View: OFF"
                        })
                        .strong(),
                    )
                    .fill(if split_active {
                        egui::Color32::from_rgb(25, 135, 105)
                    } else {
                        egui::Color32::from_rgb(40, 52, 68)
                    });
                    if ui
                        .add(split_btn)
                        .on_hover_text("Toggle synchronized 3D Split-Screen comparison (Shortcut: C)")
                        .clicked()
                    {
                        app_state.toggle_split_screen_comparison(&net);
                    }

                    if ui
                        .button("[R] Center Cam")
                        .on_hover_text("Reset 3D camera to origin (Shortcut: R)")
                        .clicked()
                    {
                        let br = app_state.loaded_primary_bounding_radius.max(10.0);
                        for (mut cam, mut proj) in &mut cam_query {
                            cam.frame_terrain(Vec3::ZERO, br);
                            cam.apply_perspective_clip_planes(&mut proj);
                        }
                    }
                });

                ui.add_space(2.0);
                ui.horizontal(|ui| {
                    if app_state.authenticated {
                        let user = app_state
                            .researcher_username
                            .as_deref()
                            .unwrap_or("equipo_analog");
                        ui.colored_label(
                            egui::Color32::from_rgb(80, 230, 150),
                            egui::RichText::new(format!("Researcher ({user})")).small().strong(),
                        );
                        if ui.small_button("Log Out").clicked() {
                            net.logout();
                        }
                    } else {
                        ui.colored_label(
                            egui::Color32::from_rgb(0, 210, 220),
                            egui::RichText::new("Public Demo Mode").small().strong(),
                        );
                        if ui.small_button("Researcher Login").clicked() {
                            app_state.active_tab = PanelTab::ResearcherUpload;
                        }
                    }

                    if ui.small_button("Reload Catalog").clicked() {
                        net.fetch_demo_mode();
                    }
                    ui.label(
                        egui::RichText::new("Keys: H | ↑/↓ | C | +/- | R")
                            .small()
                            .color(egui::Color32::from_gray(145)),
                    );
                });

                ui.label(
                    egui::RichText::new(&app_state.status_message)
                        .small()
                        .color(egui::Color32::from_gray(190)),
                );

                if let Some(err) = app_state.error_message.clone() {
                    ui.horizontal(|ui| {
                        ui.colored_label(egui::Color32::from_rgb(255, 105, 105), &err);
                        if ui.small_button("Dismiss").clicked() {
                            app_state.error_message = None;
                        }
                    });
                }

                ui.separator();

                // Compact 4-Tab Selector Bar for 14" Laptop Ergonomics
                ui.horizontal_wrapped(|ui| {
                    ui.selectable_value(
                        &mut app_state.active_tab,
                        PanelTab::SitesAndCompare,
                        "1. Sites & Compare",
                    );
                    ui.selectable_value(
                        &mut app_state.active_tab,
                        PanelTab::View3dAndCamera,
                        "2. 3D & Camera",
                    );
                    ui.selectable_value(
                        &mut app_state.active_tab,
                        PanelTab::WeightsAndTda,
                        "3. Weights & TDA",
                    );
                    ui.selectable_value(
                        &mut app_state.active_tab,
                        PanelTab::ResearcherUpload,
                        "4. Researcher Upload",
                    );
                });

                ui.separator();

                // Scrollable body for the active tab
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| match app_state.active_tab {
                        // =========================================================================
                        // TAB 1: SITES & COMPARE + PLANETARY ANALOG SIMILARITY BREAKDOWN
                        // =========================================================================
                        PanelTab::SitesAndCompare => {
                            ui.horizontal(|ui| {
                                ui.label("Filter Body:");
                                ui.selectable_value(
                                    &mut app_state.body_filter,
                                    BodyFilter::All,
                                    "All",
                                );
                                ui.selectable_value(
                                    &mut app_state.body_filter,
                                    BodyFilter::Moon,
                                    "Moon",
                                );
                                ui.selectable_value(
                                    &mut app_state.body_filter,
                                    BodyFilter::Mars,
                                    "Mars",
                                );
                                ui.selectable_value(
                                    &mut app_state.body_filter,
                                    BodyFilter::Earth,
                                    "Earth",
                                );
                            });

                            let filtered_sites: Vec<(String, String, Body)> = app_state
                                .catalog
                                .sites
                                .iter()
                                .filter(|s| app_state.body_filter.matches(s.body))
                                .map(|s| (s.id.clone(), s.name.clone(), s.body))
                                .collect();

                            let current_primary_label = app_state.primary_site_id.clone();
                            ui.horizontal(|ui| {
                                ui.label("Primary Site:");
                                egui::ComboBox::from_id_source("primary_site_combo")
                                    .selected_text(current_primary_label)
                                    .width(205.0)
                                    .show_ui(ui, |ui| {
                                        for (id, name, body) in &filtered_sites {
                                            let label = format!(
                                                "[{}] {} ({})",
                                                body.as_str().to_uppercase(),
                                                name,
                                                id
                                            );
                                            if ui
                                                .selectable_label(
                                                    app_state.primary_site_id == *id,
                                                    label,
                                                )
                                                .clicked()
                                                && app_state.primary_site_id != *id
                                            {
                                                let id_clone = id.clone();
                                                app_state.select_primary_site(&id_clone, &net);
                                            }
                                        }
                                    });
                                if ui
                                    .small_button("↑")
                                    .on_hover_text("Previous site (Shortcut: ArrowUp)")
                                    .clicked()
                                {
                                    app_state.cycle_primary_site(-1, &net);
                                }
                                if ui
                                    .small_button("↓")
                                    .on_hover_text("Next site (Shortcut: ArrowDown)")
                                    .clicked()
                                {
                                    app_state.cycle_primary_site(1, &net);
                                }
                            });

                            // Side-by-side comparison site selector
                            let all_sites: Vec<SiteMetadata> = app_state.catalog.sites.clone();
                            let current_comp_label = app_state
                                .comparison_site_id
                                .clone()
                                .unwrap_or_else(|| "None (Single View)".to_string());
                            ui.horizontal(|ui| {
                                ui.label("Compare With:");
                                egui::ComboBox::from_id_source("comparison_site_combo")
                                    .selected_text(current_comp_label)
                                    .width(205.0)
                                    .show_ui(ui, |ui| {
                                        if ui
                                            .selectable_label(
                                                app_state.comparison_site_id.is_none(),
                                                "None (Single View)",
                                            )
                                            .clicked()
                                        {
                                            app_state.comparison_site_id = None;
                                            app_state.loaded_comparison_summary = None;
                                            app_state.pending_reload_comparison = true;
                                        }
                                        for s in &all_sites {
                                            let label = format!(
                                                "[{}] {} ({})",
                                                s.body.as_str().to_uppercase(),
                                                s.name,
                                                s.id
                                            );
                                            let selected =
                                                app_state.comparison_site_id.as_deref()
                                                    == Some(s.id.as_str());
                                            if ui.selectable_label(selected, label).clicked() {
                                                app_state.comparison_site_id =
                                                    Some(s.id.clone());
                                                app_state.loaded_comparison_summary = Some(
                                                    estimate_site_summary_from_metadata(s),
                                                );
                                                net.fetch_site_assets(
                                                    TerrainSlot::Comparison,
                                                    s,
                                                );
                                                app_state.pending_reload_comparison = true;
                                            }
                                        }
                                    });
                                if ui
                                    .small_button("[C] Split")
                                    .on_hover_text("Toggle 3D Split-Screen (Shortcut: C)")
                                    .clicked()
                                {
                                    app_state.toggle_split_screen_comparison(&net);
                                }
                            });

                            ui.add_space(4.0);

                            // Highlighted Planetary Analog Similarity Breakdown Card
                            render_similarity_breakdown_card(ui, &mut app_state, &net);

                            // Compact Primary Site Metadata Summary
                            if let Some(site) = app_state.primary_site().cloned() {
                                ui.add_space(4.0);
                                egui::CollapsingHeader::new(format!(
                                    "Primary Site Details: {} [{}]",
                                    site.name,
                                    site.body.as_str().to_uppercase()
                                ))
                                .default_open(false)
                                .show(ui, |ui| {
                                    if !site.description.is_empty() {
                                        ui.label(egui::RichText::new(&site.description).small());
                                    }
                                    ui.monospace(format!(
                                        "DemExtent (m): X=[{:.1}, {:.1}], Y=[{:.1}, {:.1}]",
                                        site.dem_extent.min_x,
                                        site.dem_extent.max_x,
                                        site.dem_extent.min_y,
                                        site.dem_extent.max_y
                                    ));
                                    ui.monospace(format!(
                                        "Format: {:?} | Triangles: {} / 50,000 | Mesh: {}",
                                        site.resolved_mesh_format(),
                                        app_state.loaded_primary_triangles,
                                        site.mesh_path
                                    ));
                                });
                            }
                        }

                        // =========================================================================
                        // TAB 2: 3D TERRAIN, UV CHECKERBOARD & KEYBOARD/TOUCHPAD CAMERA CONTROLS
                        // =========================================================================
                        PanelTab::View3dAndCamera => {
                            ui.heading("3D Terrain & UV Verification");
                            ui.horizontal(|ui| {
                                ui.label("Vertical Exaggeration (Y-up):");
                                ui.add(
                                    egui::Slider::new(
                                        &mut app_state.vertical_exaggeration,
                                        0.2..=8.0,
                                    )
                                    .suffix("x"),
                                );
                            });

                            ui.checkbox(
                                &mut app_state.show_checkerboard_uv,
                                "Checkerboard UV Verification (NW=Cyan u=0,v=0 | SE=Amber u=1,v=1)",
                            );

                            ui.add_space(4.0);
                            ui.horizontal_wrapped(|ui| {
                                if ui.button("[R] Reset 3D Camera").clicked() {
                                    let br = app_state.loaded_primary_bounding_radius.max(10.0);
                                    for (mut cam, mut proj) in &mut cam_query {
                                        cam.frame_terrain(Vec3::ZERO, br);
                                        cam.apply_perspective_clip_planes(&mut proj);
                                    }
                                }
                                if ui.button("[+] Zoom In").clicked() {
                                    for (mut cam, _) in &mut cam_query {
                                        cam.zoom_by_factor(0.82);
                                    }
                                }
                                if ui.button("[-] Zoom Out").clicked() {
                                    for (mut cam, _) in &mut cam_query {
                                        cam.zoom_by_factor(1.22);
                                    }
                                }
                                if ui.button("Re-fetch Mesh & PNG").clicked()
                                    && let Some(site) = app_state.primary_site().cloned()
                                {
                                    net.fetch_site_assets(TerrainSlot::Primary, &site);
                                }
                            });

                            ui.add_space(6.0);
                            ui.group(|ui| {
                                ui.strong("Keyboard & 14\" Laptop Touchpad Controls");
                                egui::Grid::new("keyboard_shortcuts_grid")
                                    .striped(true)
                                    .num_columns(2)
                                    .spacing([12.0, 4.0])
                                    .show(ui, |ui| {
                                        ui.monospace("[H]");
                                        ui.label("Hide / Show left panel (Fullscreen 3D view)");
                                        ui.end_row();

                                        ui.monospace("[ArrowUp] / [ArrowDown]");
                                        ui.label("Cycle Primary Site up / down in catalog");
                                        ui.end_row();

                                        ui.monospace("[C]");
                                        ui.label(
                                            "Toggle synchronized 3D Split-Screen (Primary ↔ Compare)",
                                        );
                                        ui.end_row();

                                        ui.monospace("[+] / [-]");
                                        ui.label("Smooth 3D camera Zoom In / Zoom Out");
                                        ui.end_row();

                                        ui.monospace("[R]");
                                        ui.label(
                                            "Center & fit 3D camera to terrain bounding sphere",
                                        );
                                        ui.end_row();

                                        ui.monospace("[1] [2] [3] [4]");
                                        ui.label("Switch directly between panel tabs");
                                        ui.end_row();

                                        ui.monospace("Left-Drag");
                                        ui.label("Orbit 3D camera (synchronized in Split View)");
                                        ui.end_row();

                                        ui.monospace("Right-Drag / Shift+Drag");
                                        ui.label("Pan 3D camera across terrain");
                                        ui.end_row();

                                        ui.monospace("Scroll / 2-Finger Touchpad");
                                        ui.label("Zoom 3D camera in / out");
                                        ui.end_row();
                                    });
                            });
                        }

                        // =========================================================================
                        // TAB 3: SCIENTIFIC WEIGHTS, RANKED SCORES & SIMILARITY / TDA
                        // =========================================================================
                        PanelTab::WeightsAndTda => {
                            ui.heading("Scientific Feature Weights");
                            ui.label(
                                egui::RichText::new(
                                    "Adjust weights to recalculate weighted sums over pre-normalized [0.0, 1.0] features:",
                                )
                                .small(),
                            );

                            let weight_keys: Vec<String> =
                                app_state.active_weights.keys().cloned().collect();
                            let mut weights_changed = false;
                            for key in weight_keys {
                                if let Some(val) = app_state.active_weights.get_mut(&key) {
                                    ui.horizontal(|ui| {
                                        ui.label(format!("{key}:"));
                                        if ui
                                            .add(egui::Slider::new(val, 0.0..=1.0).step_by(0.01))
                                            .changed()
                                        {
                                            weights_changed = true;
                                        }
                                    });
                                }
                            }

                            if weights_changed {
                                let local_recalc = recalculate_catalog(
                                    &app_state.catalog,
                                    &app_state.active_weights,
                                );
                                app_state.ranked_scores = local_recalc.results;
                                net.recalculate_scores(app_state.active_weights.clone());
                            }

                            ui.horizontal_wrapped(|ui| {
                                if ui.button("Recalculate via Server").clicked() {
                                    let local_recalc = recalculate_catalog(
                                        &app_state.catalog,
                                        &app_state.active_weights,
                                    );
                                    app_state.ranked_scores = local_recalc.results;
                                    net.recalculate_scores(app_state.active_weights.clone());
                                }
                                if ui.button("Reset to Canonical").clicked() {
                                    app_state.active_weights =
                                        app_state.catalog.default_weights.clone();
                                    let local_recalc = recalculate_catalog(
                                        &app_state.catalog,
                                        &app_state.active_weights,
                                    );
                                    app_state.ranked_scores = local_recalc.results;
                                }
                            });

                            ui.separator();

                            ui.heading("Comparative Analog Scores (Click to Load 3D)");
                            egui::Grid::new("analog_scores_table")
                                .striped(true)
                                .num_columns(5)
                                .spacing([8.0, 5.0])
                                .show(ui, |ui| {
                                    ui.strong("#");
                                    ui.strong("Site");
                                    ui.strong("Body");
                                    ui.strong("Live");
                                    ui.strong("Canonical");
                                    ui.end_row();

                                    let rows = app_state.ranked_scores.clone();
                                    for (idx, row) in rows.iter().enumerate() {
                                        ui.label(format!("{}", idx + 1));
                                        let is_current =
                                            app_state.primary_site_id == row.site_id;
                                        if ui
                                            .selectable_label(is_current, &row.name)
                                            .on_hover_text("Click to inspect this site in 3D")
                                            .clicked()
                                            && !is_current
                                        {
                                            app_state.select_primary_site(&row.site_id, &net);
                                        }
                                        ui.colored_label(
                                            body_accent_color(row.body),
                                            row.body.as_str().to_uppercase(),
                                        );
                                        ui.monospace(format!("{:.4}", row.score));
                                        ui.monospace(format!("{:.4}", row.canonical_score));
                                        ui.end_row();
                                    }
                                });

                            ui.separator();
                            render_similarity_breakdown_card(ui, &mut app_state, &net);

                            if let Some(report) = app_state.last_analysis_report.clone() {
                                ui.add_space(4.0);
                                render_tda_report_panel(ui, &report);
                            }
                        }

                        // =========================================================================
                        // TAB 4: RESEARCHER LOGIN, BINARY MULTIPART UPLOAD & TDA REPORT
                        // =========================================================================
                        PanelTab::ResearcherUpload => {
                            if !app_state.authenticated {
                                ui.label(
                                    egui::RichText::new(
                                        "Log in as Researcher (Argon2id) to upload & analyze .stl / .tif / .png terrains, run 0D Persistent Homology & Euler topology analysis, or persist weights.",
                                    )
                                    .small()
                                    .color(egui::Color32::from_gray(175)),
                                );

                                ui.group(|ui| {
                                    ui.strong("Researcher Login (POST /api/login)");
                                    let mut inline_login = false;
                                    egui::Grid::new("inline_login_grid")
                                        .num_columns(2)
                                        .spacing([6.0, 4.0])
                                        .show(ui, |ui| {
                                            ui.label("Username:");
                                            ui.text_edit_singleline(
                                                &mut app_state.login_username_input,
                                            );
                                            ui.end_row();

                                            ui.label("Password:");
                                            let r = ui.add(
                                                egui::TextEdit::singleline(
                                                    &mut app_state.login_password_input,
                                                )
                                                .password(true),
                                            );
                                            if r.lost_focus()
                                                && ui.input(|i| {
                                                    i.key_pressed(egui::Key::Enter)
                                                })
                                            {
                                                inline_login = true;
                                            }
                                            ui.end_row();
                                        });

                                    if ui.button("Log In as Researcher").clicked() {
                                        inline_login = true;
                                    }
                                    if inline_login {
                                        app_state.error_message = None;
                                        let user =
                                            app_state.login_username_input.trim().to_string();
                                        let pass = app_state.login_password_input.clone();
                                        app_state.login_password_input.clear();
                                        app_state.status_message = format!(
                                            "Authenticating researcher '{}' via Argon2id...",
                                            user
                                        );
                                        net.login(user, pass);
                                    }
                                });
                            } else {
                                ui.colored_label(
                                    egui::Color32::from_rgb(80, 230, 150),
                                    "Researcher Session Active — Upload, TDA & Write Operations Unlocked",
                                );

                                egui::CollapsingHeader::new(
                                    egui::RichText::new(
                                        "Upload & Analyze Terrain (.stl / .tif / .png)",
                                    )
                                    .strong()
                                    .color(egui::Color32::from_rgb(0, 225, 235)),
                                )
                                .default_open(true)
                                .show(ui, |ui| {
                                    ui.label(
                                        egui::RichText::new(
                                            "Select or drag-and-drop binary .stl (3D mesh), .tif/.tiff (DEM elevation), and/or .png (surface texture). Sent as raw binary multipart/form-data (up to 512 MiB, zero Base64). Meshes > 50,000 triangles are automatically decimated & centered:",
                                        )
                                        .small(),
                                    );

                                    ui.add_space(4.0);
                                    ui.horizontal_wrapped(|ui| {
                                        if ui
                                            .button("Browse .STL / .TIF / .PNG...")
                                            .on_hover_text(
                                                "Open browser file picker for .stl, .tif, .tiff, or .png",
                                            )
                                            .clicked()
                                        {
                                            open_browser_file_dialog(".stl,.tif,.tiff,.png");
                                        }
                                        if ui.small_button("+ .STL").clicked() {
                                            open_browser_file_dialog(".stl");
                                        }
                                        if ui.small_button("+ .TIF/.TIFF").clicked() {
                                            open_browser_file_dialog(".tif,.tiff");
                                        }
                                        if ui.small_button("+ .PNG").clicked() {
                                            open_browser_file_dialog(".png");
                                        }
                                        if ui.small_button("Demo Crater .STL").clicked() {
                                            app_state.upload_form.stage_synthetic_demo_stl();
                                            app_state.status_message =
                                                "Staged built-in synthetic crater .STL for TDA analysis."
                                                    .to_string();
                                        }
                                        if ui.small_button("Clear Files").clicked() {
                                            app_state.upload_form.clear_files();
                                        }
                                    });

                                    ui.group(|ui| {
                                        let stl_status = app_state
                                            .upload_form
                                            .stl_filename
                                            .as_deref()
                                            .map(|n| {
                                                let mib = bytes_to_mib(
                                                    app_state.upload_form.stl_size_bytes,
                                                );
                                                let tri_info = match app_state
                                                    .upload_form
                                                    .stl_detected_triangles
                                                {
                                                    Some(t) if t > MAX_TRIANGLES => {
                                                        format!(
                                                            " | {t} tris -> auto-decimates <= {MAX_TRIANGLES}"
                                                        )
                                                    }
                                                    Some(t) => format!(" | {t} tris"),
                                                    None => String::new(),
                                                };
                                                format!("{n} ({mib:.2} MiB{tri_info})")
                                            })
                                            .unwrap_or_else(|| {
                                                "None (0.00 MiB — auto-generated if .tif provided)"
                                                    .to_string()
                                            });
                                        let tiff_status = app_state
                                            .upload_form
                                            .tiff_filename
                                            .as_deref()
                                            .map(|n| {
                                                let mib = bytes_to_mib(
                                                    app_state.upload_form.tiff_size_bytes,
                                                );
                                                format!("{n} ({mib:.2} MiB)")
                                            })
                                            .unwrap_or_else(|| {
                                                "None (0.00 MiB — auto-rasterized if .stl provided)"
                                                    .to_string()
                                            });
                                        let png_status = app_state
                                            .upload_form
                                            .png_filename
                                            .as_deref()
                                            .map(|n| {
                                                let mib = bytes_to_mib(
                                                    app_state.upload_form.png_size_bytes,
                                                );
                                                format!("{n} ({mib:.2} MiB)")
                                            })
                                            .unwrap_or_else(|| {
                                                "None (0.00 MiB — shaded hypsometric PNG auto-generated)"
                                                    .to_string()
                                            });

                                        let total_mib = bytes_to_mib(
                                            app_state.upload_form.total_staged_bytes(),
                                        );

                                        ui.monospace(format!("STL Mesh  : {stl_status}"));
                                        ui.monospace(format!("DEM TIFF  : {tiff_status}"));
                                        ui.monospace(format!("Albedo PNG: {png_status}"));
                                        ui.monospace(format!(
                                            "Total Size: {total_mib:.2} MiB / 512.00 MiB (multipart/form-data)"
                                        ));
                                    });

                                    egui::Grid::new("upload_terrain_meta_grid")
                                        .num_columns(2)
                                        .spacing([8.0, 5.0])
                                        .show(ui, |ui| {
                                            ui.label("Site Name:");
                                            ui.text_edit_singleline(
                                                &mut app_state.upload_form.name,
                                            );
                                            ui.end_row();

                                            ui.label("Site ID (optional):");
                                            ui.text_edit_singleline(
                                                &mut app_state.upload_form.id,
                                            );
                                            ui.end_row();

                                            ui.label("Body Classification:");
                                            ui.horizontal(|ui| {
                                                ui.selectable_value(
                                                    &mut app_state.upload_form.body_override,
                                                    None,
                                                    "Auto-Propose (TDA)",
                                                );
                                                ui.selectable_value(
                                                    &mut app_state.upload_form.body_override,
                                                    Some(Body::Earth),
                                                    "Earth",
                                                );
                                                ui.selectable_value(
                                                    &mut app_state.upload_form.body_override,
                                                    Some(Body::Moon),
                                                    "Moon",
                                                );
                                                ui.selectable_value(
                                                    &mut app_state.upload_form.body_override,
                                                    Some(Body::Mars),
                                                    "Mars",
                                                );
                                            });
                                            ui.end_row();

                                            ui.label("DemExtent X (m):");
                                            ui.horizontal(|ui| {
                                                ui.add(
                                                    egui::DragValue::new(
                                                        &mut app_state.upload_form.min_x,
                                                    )
                                                    .prefix("min: "),
                                                );
                                                ui.add(
                                                    egui::DragValue::new(
                                                        &mut app_state.upload_form.max_x,
                                                    )
                                                    .prefix("max: "),
                                                );
                                            });
                                            ui.end_row();

                                            ui.label("DemExtent Y (m):");
                                            ui.horizontal(|ui| {
                                                ui.add(
                                                    egui::DragValue::new(
                                                        &mut app_state.upload_form.min_y,
                                                    )
                                                    .prefix("min: "),
                                                );
                                                ui.add(
                                                    egui::DragValue::new(
                                                        &mut app_state.upload_form.max_y,
                                                    )
                                                    .prefix("max: "),
                                                );
                                            });
                                            ui.end_row();
                                        });

                                    ui.add_space(6.0);
                                    let can_analyze = app_state
                                        .upload_form
                                        .has_elevation_source()
                                        && !app_state.upload_form.is_uploading;
                                    let analyze_btn = egui::Button::new(
                                        egui::RichText::new(
                                            "Analyze & Propose Analog (POST /api/sites/upload)",
                                        )
                                        .strong()
                                        .color(if can_analyze {
                                            egui::Color32::from_rgb(10, 25, 30)
                                        } else {
                                            egui::Color32::from_gray(140)
                                        }),
                                    )
                                    .fill(if can_analyze {
                                        egui::Color32::from_rgb(0, 215, 180)
                                    } else {
                                        egui::Color32::from_gray(55)
                                    })
                                    .min_size(egui::Vec2::new(340.0, 30.0));

                                    if ui.add_enabled(can_analyze, analyze_btn).clicked() {
                                        let total_mib = bytes_to_mib(
                                            app_state.upload_form.total_staged_bytes(),
                                        );
                                        let req = app_state.upload_form.to_upload_request();
                                        app_state.upload_form.is_uploading = true;
                                        app_state.error_message = None;
                                        app_state.status_message = format!(
                                            "Uploading '{}' ({total_mib:.2} MiB binary multipart/form-data) & running TDA...",
                                            req.site_name
                                        );
                                        net.upload_and_analyze_site(req);
                                    }
                                });

                                if let Some(report) = app_state.last_analysis_report.clone() {
                                    ui.add_space(6.0);
                                    render_tda_report_panel(ui, &report);
                                }

                                ui.add_space(6.0);
                                if ui
                                    .button("Save Canonical Weights (PUT /api/weights)")
                                    .clicked()
                                {
                                    net.save_canonical_weights(app_state.active_weights.clone());
                                }

                                ui.add_space(4.0);
                                egui::CollapsingHeader::new(
                                    "Manual Site Metadata Editor (POST /api/sites)",
                                )
                                .default_open(false)
                                .show(ui, |ui| {
                                    egui::Grid::new("site_editor_grid")
                                        .num_columns(2)
                                        .spacing([8.0, 5.0])
                                        .show(ui, |ui| {
                                            ui.label("Site ID:");
                                            ui.text_edit_singleline(
                                                &mut app_state.site_editor.id,
                                            );
                                            ui.end_row();

                                            ui.label("Display Name:");
                                            ui.text_edit_singleline(
                                                &mut app_state.site_editor.name,
                                            );
                                            ui.end_row();

                                            ui.label("Body:");
                                            ui.horizontal(|ui| {
                                                ui.selectable_value(
                                                    &mut app_state.site_editor.body,
                                                    Body::Moon,
                                                    "Moon",
                                                );
                                                ui.selectable_value(
                                                    &mut app_state.site_editor.body,
                                                    Body::Mars,
                                                    "Mars",
                                                );
                                                ui.selectable_value(
                                                    &mut app_state.site_editor.body,
                                                    Body::Earth,
                                                    "Earth",
                                                );
                                            });
                                            ui.end_row();

                                            ui.label("Description:");
                                            ui.text_edit_singleline(
                                                &mut app_state.site_editor.description,
                                            );
                                            ui.end_row();

                                            ui.label("Mesh (.stl/.tif):");
                                            ui.text_edit_singleline(
                                                &mut app_state.site_editor.mesh_path,
                                            );
                                            ui.end_row();

                                            ui.label("Texture (.png):");
                                            ui.text_edit_singleline(
                                                &mut app_state.site_editor.texture_path,
                                            );
                                            ui.end_row();

                                            ui.label("Triangles (<=50k):");
                                            ui.add(
                                                egui::DragValue::new(
                                                    &mut app_state.site_editor.triangle_count,
                                                )
                                                .range(1..=50_000),
                                            );
                                            ui.end_row();

                                            ui.label("DemExtent X (m):");
                                            ui.horizontal(|ui| {
                                                ui.add(
                                                    egui::DragValue::new(
                                                        &mut app_state.site_editor.min_x,
                                                    )
                                                    .prefix("min: "),
                                                );
                                                ui.add(
                                                    egui::DragValue::new(
                                                        &mut app_state.site_editor.max_x,
                                                    )
                                                    .prefix("max: "),
                                                );
                                            });
                                            ui.end_row();

                                            ui.label("DemExtent Y (m):");
                                            ui.horizontal(|ui| {
                                                ui.add(
                                                    egui::DragValue::new(
                                                        &mut app_state.site_editor.min_y,
                                                    )
                                                    .prefix("min: "),
                                                );
                                                ui.add(
                                                    egui::DragValue::new(
                                                        &mut app_state.site_editor.max_y,
                                                    )
                                                    .prefix("max: "),
                                                );
                                            });
                                            ui.end_row();
                                        });

                                    ui.label("Pre-normalized Feature Scores [0.0, 1.0]:");
                                    let weight_keys: Vec<String> =
                                        app_state.active_weights.keys().cloned().collect();
                                    for k in weight_keys {
                                        let score_entry = app_state
                                            .site_editor
                                            .feature_scores
                                            .entry(k.clone())
                                            .or_insert(0.5);
                                        ui.horizontal(|ui| {
                                            ui.label(format!("  {k}:"));
                                            ui.add(
                                                egui::Slider::new(score_entry, 0.0..=1.0)
                                                    .step_by(0.01),
                                            );
                                        });
                                    }

                                    if ui
                                        .button("Save Site to Catalog (POST /api/sites)")
                                        .clicked()
                                    {
                                        let meta = app_state.site_editor.into_site_metadata();
                                        net.save_site(meta);
                                    }
                                });
                            }
                        }
                    });
            });
    } else {
        // Floating top-left toolbar when the left panel is hidden (`[H] Hide/Show Panel`)
        egui::Area::new(egui::Id::new("floating_top_left_bar"))
            .fixed_pos(egui::pos2(12.0, 12.0))
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                egui::Frame::none()
                    .fill(egui::Color32::from_rgba_unmultiplied(15, 22, 32, 230))
                    .stroke(egui::Stroke::new(
                        1.0,
                        egui::Color32::from_rgb(0, 200, 220),
                    ))
                    .rounding(egui::Rounding::same(6.0))
                    .inner_margin(egui::Margin::symmetric(8.0, 6.0))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            let show_btn = egui::Button::new(
                                egui::RichText::new("[H] Hide/Show Panel")
                                    .strong()
                                    .color(egui::Color32::from_rgb(10, 25, 30)),
                            )
                            .fill(egui::Color32::from_rgb(0, 210, 220));
                            if ui.add(show_btn).clicked() {
                                app_state.panel_visible = true;
                            }

                            let split_on = app_state.comparison_site_id.is_some();
                            if ui
                                .button(if split_on {
                                    "[C] Split View: ON"
                                } else {
                                    "[C] Split View: OFF"
                                })
                                .clicked()
                            {
                                app_state.toggle_split_screen_comparison(&net);
                            }

                            if ui.button("↑").on_hover_text("Previous Site").clicked() {
                                app_state.cycle_primary_site(-1, &net);
                            }
                            if ui.button("↓").on_hover_text("Next Site").clicked() {
                                app_state.cycle_primary_site(1, &net);
                            }
                            if ui.button("+").on_hover_text("Zoom In").clicked() {
                                for (mut cam, _) in &mut cam_query {
                                    cam.zoom_by_factor(0.84);
                                }
                            }
                            if ui.button("-").on_hover_text("Zoom Out").clicked() {
                                for (mut cam, _) in &mut cam_query {
                                    cam.zoom_by_factor(1.18);
                                }
                            }
                            if ui.button("[R] Reset Cam").clicked() {
                                let br = app_state.loaded_primary_bounding_radius.max(10.0);
                                for (mut cam, mut proj) in &mut cam_query {
                                    cam.frame_terrain(Vec3::ZERO, br);
                                    cam.apply_perspective_clip_planes(&mut proj);
                                }
                            }

                            if let Some(primary) = app_state.primary_site() {
                                ui.colored_label(
                                    body_accent_color(primary.body),
                                    egui::RichText::new(format!(
                                        "{} [{}]",
                                        primary.name,
                                        primary.body.as_str().to_uppercase()
                                    ))
                                    .strong()
                                    .small(),
                                );
                            }
                        });
                    });
            });
    }

    // 3. Measure uncovered 3D viewport rectangle and render Split-Screen 3D overlays
    let screen_rect = ctx.screen_rect();
    let viewport_rect = ctx.available_rect();
    app_state.viewport_left_fraction = if app_state.panel_visible {
        (viewport_rect.left() / screen_rect.width().max(1.0)).clamp(0.0, 0.75)
    } else {
        0.0
    };

    if let (Some(primary), Some(compare)) = (
        app_state.primary_site().cloned(),
        app_state.comparison_site().cloned(),
    ) {
        let sim = app_state.compute_pairwise_similarity(&primary, &compare);
        let mid_x = viewport_rect.center().x;
        let left_center_x = 0.5 * (viewport_rect.left() + mid_x);
        let right_center_x = 0.5 * (mid_x + viewport_rect.right());
        let top_y = if app_state.panel_visible {
            viewport_rect.top() + 14.0
        } else {
            viewport_rect.top() + 56.0
        };

        // Draw subtle vertical Split-Screen divider line down the middle of the 3D viewport
        let painter = ctx.layer_painter(egui::LayerId::new(
            egui::Order::Background,
            egui::Id::new("split_screen_divider"),
        ));
        painter.line_segment(
            [
                egui::pos2(mid_x, viewport_rect.top()),
                egui::pos2(mid_x, viewport_rect.bottom()),
            ],
            egui::Stroke::new(
                1.5,
                egui::Color32::from_rgba_unmultiplied(0, 215, 225, 115),
            ),
        );

        // Left Half HUD Label: Primary Terrain
        egui::Area::new(egui::Id::new("split_left_hud_badge"))
            .pivot(egui::Align2::CENTER_TOP)
            .fixed_pos(egui::pos2(left_center_x, top_y))
            .order(egui::Order::Middle)
            .interactable(false)
            .show(ctx, |ui| {
                egui::Frame::none()
                    .fill(egui::Color32::from_rgba_unmultiplied(12, 18, 28, 220))
                    .stroke(egui::Stroke::new(1.2, body_accent_color(primary.body)))
                    .rounding(egui::Rounding::same(6.0))
                    .inner_margin(egui::Margin::symmetric(10.0, 5.0))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.colored_label(
                                body_accent_color(primary.body),
                                egui::RichText::new(format!(
                                    "[{}]",
                                    primary.body.as_str().to_uppercase()
                                ))
                                .strong(),
                            );
                            ui.strong(&primary.name);
                        });
                        ui.monospace(format!(
                            "Elev: [{:.1} m, {:.1} m] (Δz={:.1} m) | β₀={} basins | χ={}",
                            sim.primary_summary.min_elevation_m,
                            sim.primary_summary.max_elevation_m,
                            (sim.primary_summary.max_elevation_m
                                - sim.primary_summary.min_elevation_m)
                                .abs(),
                            sim.primary_summary.persistent_basins_b0,
                            sim.primary_summary.euler_characteristic
                        ));
                    });
            });

        // Right Half HUD Label: Comparison Terrain
        egui::Area::new(egui::Id::new("split_right_hud_badge"))
            .pivot(egui::Align2::CENTER_TOP)
            .fixed_pos(egui::pos2(right_center_x, top_y))
            .order(egui::Order::Middle)
            .interactable(false)
            .show(ctx, |ui| {
                egui::Frame::none()
                    .fill(egui::Color32::from_rgba_unmultiplied(12, 18, 28, 220))
                    .stroke(egui::Stroke::new(1.2, body_accent_color(compare.body)))
                    .rounding(egui::Rounding::same(6.0))
                    .inner_margin(egui::Margin::symmetric(10.0, 5.0))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.colored_label(
                                body_accent_color(compare.body),
                                egui::RichText::new(format!(
                                    "[{}]",
                                    compare.body.as_str().to_uppercase()
                                ))
                                .strong(),
                            );
                            ui.strong(&compare.name);
                        });
                        ui.monospace(format!(
                            "Elev: [{:.1} m, {:.1} m] (Δz={:.1} m) | β₀={} basins | χ={}",
                            sim.compare_summary.min_elevation_m,
                            sim.compare_summary.max_elevation_m,
                            (sim.compare_summary.max_elevation_m
                                - sim.compare_summary.min_elevation_m)
                                .abs(),
                            sim.compare_summary.persistent_basins_b0,
                            sim.compare_summary.euler_characteristic
                        ));
                    });
            });

        // Center Divider Similarity Pill
        egui::Area::new(egui::Id::new("split_center_sim_badge"))
            .pivot(egui::Align2::CENTER_BOTTOM)
            .fixed_pos(egui::pos2(mid_x, viewport_rect.bottom() - 14.0))
            .order(egui::Order::Middle)
            .interactable(false)
            .show(ctx, |ui| {
                egui::Frame::none()
                    .fill(egui::Color32::from_rgba_unmultiplied(14, 24, 34, 230))
                    .stroke(egui::Stroke::new(
                        1.2,
                        egui::Color32::from_rgb(0, 215, 190),
                    ))
                    .rounding(egui::Rounding::same(12.0))
                    .inner_margin(egui::Margin::symmetric(12.0, 5.0))
                    .show(ui, |ui| {
                        ui.colored_label(
                            egui::Color32::from_rgb(80, 240, 190),
                            egui::RichText::new(format!(
                                "Synchronized Split-Screen | Analog Similarity: {:.1}% | Δβ₀={} | Δχ={}",
                                sim.global_similarity_pct,
                                sim.delta_basins_b0,
                                sim.delta_euler_chi
                            ))
                            .strong()
                            .small(),
                        );
                    });
            });
    }
}
