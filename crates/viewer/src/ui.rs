// AI-assisted
//! `bevy_egui` interface for:
//! 1. Legible 14" Laptop Typography & High-Contrast Visual Hierarchy (`egui::Style` with
//!    explicit `Heading: 17.5px`, `Body/Button: 13.5px`, `Monospace: 13.0px`, `Small: 12.0px`,
//!    `#F1F5F9` primary text, `#CBD5E1` secondary text, generous spacing, and line wrapping).
//! 2. Homogeneous Startup Access Gate (`Space-App-X86 Access Gate` full-screen central panel
//!    before entering Guest Demo Mode or logging in as an authenticated Researcher, with clean
//!    return on `Log Out`).
//! 3. Clean ASCII-only UI controls (no `□` missing-glyph boxes), full mouse + touchpad +
//!    keyboard navigation (`H`, `A`, `C`, `R`, `Up`/`Down`, `+`/`-`), and 5 structured tabs:
//!    - `1. Sites & Compare`
//!    - `2. 3D & Camera`
//!    - `3. Charts & Math` (custom `egui::Painter` charts + collapsible Mathematical Foundation)
//!    - `4. Settlement Guide` (Lunar & Martian habitat engineering guide)
//!    - `5. Upload & Reset` (512 MiB binary `multipart/form-data` upload, TDA report, and
//!      Workspace Reset / Restore Reference Demo Site).
//! 4. Live Planetary Telemetry HUD (`MOON`, `MARS`, `EARTH`) computed from real-time UTC clock.

use std::collections::BTreeMap;

use analog_core::scoring::{normalize_weights, recalculate_catalog};
use analog_core::terrain_analysis::{TerrainAnalysisReport, UploadSiteRequest};
use analog_core::{Body, Catalog, DemExtent, ScoredSite, SiteMetadata};
use bevy::prelude::*;
use bevy_egui::{EguiContexts, egui};

use crate::camera::OrbitCamera;
use crate::mesh_loader::{
    SiteMeshSummary, SurfaceSkinMode, estimate_site_summary_from_metadata,
};
use crate::net::{
    BrowserSelectedFile, NetBridge, TerrainSlot, fallback_synthetic_catalog,
    open_browser_file_dialog,
};

/// High-contrast primary text color (`#F1F5F9`).
const COLOR_TEXT_PRIMARY: egui::Color32 = egui::Color32::from_rgb(241, 245, 249);
/// High-contrast secondary label color (`#CBD5E1`).
const COLOR_TEXT_SECONDARY: egui::Color32 = egui::Color32::from_rgb(203, 213, 225);
/// Dark slate surface background for cards (`#0F172A`).
const COLOR_CARD_BG: egui::Color32 = egui::Color32::from_rgb(15, 23, 42);
/// Elevated dark panel background (`#111827`).
const COLOR_PANEL_BG: egui::Color32 = egui::Color32::from_rgb(17, 24, 39);
/// Subtle border stroke color (`#334155`).
const COLOR_BORDER: egui::Color32 = egui::Color32::from_rgb(51, 65, 85);

/// Editable form state for authenticated researchers to register or update a site in the catalog.
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
            body: site.body.clone(),
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

    pub fn to_site_metadata(&self) -> SiteMetadata {
        SiteMetadata {
            id: self.id.trim().to_string(),
            name: self.name.trim().to_string(),
            body: self.body.clone(),
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
            generated_by: Some("analog_viewer researcher form".to_string()),
        }
    }
}

/// Form state for uploading `.stl`, `.tif`/`.tiff`, and `.png` files for automatic
/// morphometric + Topological Data Analysis (TDA) and site registration.
/// Stores raw `Vec<u8>` bytes for binary `multipart/form-data` transport (zero Base64).
#[derive(Debug, Clone)]
pub struct TerrainUploadForm {
    pub site_id: String,
    pub site_name: String,
    pub source_body: Body,
    pub description: String,
    pub horizontal_scale_m: f64,
    pub stl_file_name: Option<String>,
    pub stl_bytes: Option<Vec<u8>>,
    pub tif_file_name: Option<String>,
    pub tif_bytes: Option<Vec<u8>>,
    pub png_file_name: Option<String>,
    pub png_bytes: Option<Vec<u8>>,
    pub is_uploading: bool,
}

impl Default for TerrainUploadForm {
    fn default() -> Self {
        Self {
            site_id: String::new(),
            site_name: String::new(),
            source_body: Body::Earth,
            description: String::new(),
            horizontal_scale_m: 1.0,
            stl_file_name: None,
            stl_bytes: None,
            tif_file_name: None,
            tif_bytes: None,
            png_file_name: None,
            png_bytes: None,
            is_uploading: false,
        }
    }
}

impl TerrainUploadForm {
    pub fn has_terrain_file(&self) -> bool {
        self.stl_bytes.is_some() || self.tif_bytes.is_some()
    }

    pub fn format_mib(bytes_len: usize) -> String {
        let mib = bytes_len as f64 / (1024.0 * 1024.0);
        if mib >= 0.01 {
            format!("{mib:.2} MiB")
        } else {
            format!("{:.1} KiB", bytes_len as f64 / 1024.0)
        }
    }

    pub fn total_payload_bytes(&self) -> usize {
        self.stl_bytes.as_ref().map_or(0, Vec::len)
            + self.tif_bytes.as_ref().map_or(0, Vec::len)
            + self.png_bytes.as_ref().map_or(0, Vec::len)
    }

    pub fn ingest_selected_file(&mut self, file: BrowserSelectedFile) -> Result<String, String> {
        let lower = file.name.to_ascii_lowercase();
        if self.site_name.trim().is_empty()
            && let Some(stem) = file.name.rsplit('/').next().and_then(|s| s.split('.').next())
            && !stem.is_empty()
        {
            self.site_name = stem.replace(['_', '-'], " ");
            self.site_id = stem.to_ascii_lowercase();
        }

        let size_str = Self::format_mib(file.size);

        if lower.ends_with(".stl") {
            let msg = format!(
                "Attached 3D STL mesh '{}' ({size_str}, raw binary ready).",
                file.name
            );
            self.stl_file_name = Some(file.name);
            self.stl_bytes = Some(file.bytes);
            Ok(msg)
        } else if lower.ends_with(".tif") || lower.ends_with(".tiff") {
            let msg = format!(
                "Attached elevation DEM '{}' ({size_str}, raw binary ready).",
                file.name
            );
            self.tif_file_name = Some(file.name);
            self.tif_bytes = Some(file.bytes);
            Ok(msg)
        } else if lower.ends_with(".png") {
            let msg = format!(
                "Attached surface texture '{}' ({size_str}, raw binary ready).",
                file.name
            );
            self.png_file_name = Some(file.name);
            self.png_bytes = Some(file.bytes);
            Ok(msg)
        } else {
            Err(format!(
                "Unsupported file '{}'. Please select a .stl, .tif/.tiff, or .png file.",
                file.name
            ))
        }
    }

    pub fn to_upload_request(&self) -> UploadSiteRequest {
        UploadSiteRequest {
            site_id: self.site_id.trim().to_string(),
            site_name: self.site_name.trim().to_string(),
            body: Some(self.source_body),
            description: self.description.trim().to_string(),
            dem_extent: None,
            stl_base64: None,
            tiff_base64: None,
            png_base64: None,
            stl_bytes: self.stl_bytes.clone(),
            tiff_bytes: self.tif_bytes.clone(),
            png_bytes: self.png_bytes.clone(),
        }
    }
}

/// Compact tabs inside the left navigation panel for 14" laptop screens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanelTab {
    SitesAndCompare,
    ViewAndCamera,
    ChartsAndMath,
    SettlementGuide,
    UploadAndReset,
}

/// Pairwise similarity breakdown between the primary site and a comparison site (or top analog).
#[derive(Debug, Clone)]
pub struct PairwiseAnalogSimilarity {
    pub primary_name: String,
    pub primary_body: Body,
    pub primary_elev: Option<[f32; 2]>,
    pub compare_id: String,
    pub compare_name: String,
    pub compare_body: Body,
    pub compare_elev: Option<[f32; 2]>,
    pub overall_similarity_pct: f64,
    pub dimensions: Vec<(String, f64, f64, f64)>,
    pub delta_b0_basins: Option<i64>,
    pub delta_euler_chi: Option<i64>,
    pub primary_safe_landing_15_pct: f32,
    pub primary_optimal_pad_5_pct: f32,
    pub primary_mean_slope_deg: f32,
    pub primary_b0_basins: usize,
    pub compare_safe_landing_15_pct: f32,
    pub compare_optimal_pad_5_pct: f32,
    pub compare_mean_slope_deg: f32,
    pub compare_b0_basins: usize,
}

/// Global UI & application state resource for `analog_viewer`.
#[derive(Resource)]
pub struct ViewerAppState {
    pub catalog: Catalog,
    pub active_weights: BTreeMap<String, f64>,
    pub ranked_scores: Vec<ScoredSite>,
    pub primary_site_id: String,
    pub comparison_site_id: Option<String>,
    pub vertical_exaggeration: f32,
    pub show_checkerboard_uv: bool,
    pub mode_selected: bool,
    pub authenticated: bool,
    pub researcher_username: Option<String>,
    pub login_username: String,
    pub login_password: String,
    pub show_login_modal: bool,
    pub show_left_panel: bool,
    pub hide_all_ui: bool,
    pub show_similarity_charts_window: bool,
    pub show_planetary_telemetry: bool,
    pub viewport_left_fraction: f32,
    pub active_tab: PanelTab,
    pub status_message: String,
    pub error_message: Option<String>,
    pub loaded_primary_triangles: u32,
    pub loaded_comparison_triangles: u32,
    pub loaded_primary_bounding_radius: f32,
    pub loaded_comparison_bounding_radius: f32,
    pub loaded_primary_summary: SiteMeshSummary,
    pub loaded_comparison_summary: Option<SiteMeshSummary>,
    pub site_editor: SiteEditorForm,
    pub upload_form: TerrainUploadForm,
    pub last_analysis_report: Option<TerrainAnalysisReport>,
    pub style_configured: bool,
}

impl Default for ViewerAppState {
    fn default() -> Self {
        let catalog = fallback_synthetic_catalog();
        let active_weights = catalog.default_weights.clone();
        let recalc = recalculate_catalog(&catalog, &active_weights);
        let primary_site = catalog.sites.first().cloned().unwrap_or_else(|| {
            fallback_synthetic_catalog()
                .sites
                .into_iter()
                .next()
                .expect("synthetic catalog has 1 site")
        });
        let primary_site_id = primary_site.id.clone();
        let site_editor = SiteEditorForm::from_site(&primary_site);

        Self {
            catalog,
            active_weights,
            ranked_scores: recalc.results,
            primary_site_id,
            comparison_site_id: None,
            vertical_exaggeration: 1.5,
            show_checkerboard_uv: true,
            mode_selected: false,
            authenticated: false,
            researcher_username: None,
            login_username: "researcher".to_string(),
            login_password: String::new(),
            show_login_modal: false,
            show_left_panel: true,
            hide_all_ui: false,
            show_similarity_charts_window: false,
            show_planetary_telemetry: true,
            viewport_left_fraction: 0.28,
            active_tab: PanelTab::SitesAndCompare,
            status_message:
                "Select Guest Public Demo Mode or log in with Researcher credentials to begin."
                    .to_string(),
            error_message: None,
            loaded_primary_triangles: 2048,
            loaded_comparison_triangles: 0,
            loaded_primary_bounding_radius: 1414.2,
            loaded_comparison_bounding_radius: 1414.2,
            loaded_primary_summary: SiteMeshSummary::default(),
            loaded_comparison_summary: None,
            site_editor,
            upload_form: TerrainUploadForm::default(),
            last_analysis_report: None,
            style_configured: false,
        }
    }
}

impl ViewerAppState {
    pub fn apply_catalog(&mut self, catalog: Catalog) {
        self.active_weights = catalog.default_weights.clone();
        let recalc = recalculate_catalog(&catalog, &self.active_weights);
        self.ranked_scores = recalc.results;

        let primary_exists = catalog.sites.iter().any(|s| s.id == self.primary_site_id);
        if !primary_exists {
            if let Some(first) = catalog.sites.first() {
                self.primary_site_id = first.id.clone();
                self.site_editor = SiteEditorForm::from_site(first);
            } else {
                self.primary_site_id.clear();
            }
        } else if let Some(curr) = catalog.sites.iter().find(|s| s.id == self.primary_site_id) {
            self.site_editor = SiteEditorForm::from_site(curr);
        }

        if let Some(comp_id) = &self.comparison_site_id
            && !catalog.sites.iter().any(|s| &s.id == comp_id)
        {
            self.comparison_site_id = None;
        }

        self.catalog = catalog;
    }

    pub fn primary_site(&self) -> Option<&SiteMetadata> {
        self.catalog
            .sites
            .iter()
            .find(|s| s.id == self.primary_site_id)
            .or_else(|| self.catalog.sites.first())
    }

    pub fn comparison_site(&self) -> Option<&SiteMetadata> {
        let id = self.comparison_site_id.as_ref()?;
        self.catalog.sites.iter().find(|s| &s.id == id)
    }

    /// Finds the best cross-body analog partner (e.g., Earth <-> Moon/Mars) or falls back to
    /// the currently selected `comparison_site` so similarity breakdown is always available.
    pub fn best_analog_partner(&self) -> Option<&SiteMetadata> {
        if let Some(comp) = self.comparison_site() {
            return Some(comp);
        }
        let primary = self.primary_site()?;
        self.catalog
            .sites
            .iter()
            .filter(|s| s.id != primary.id && s.body != primary.body)
            .max_by(|a, b| {
                let sa = self.compute_site_pair_similarity(primary, a);
                let sb = self.compute_site_pair_similarity(primary, b);
                sa.partial_cmp(&sb).unwrap_or(std::cmp::Ordering::Equal)
            })
            .or_else(|| self.catalog.sites.iter().find(|s| s.id != primary.id))
    }

    pub fn compute_site_pair_similarity(&self, a: &SiteMetadata, b: &SiteMetadata) -> f64 {
        let norm_w = normalize_weights(&self.active_weights);
        let keys = [
            "elevation_relief",
            "geomorphology_match",
            "roughness_similarity",
            "slope_safety",
        ];
        let mut weighted_sim = 0.0;
        let mut w_sum = 0.0;
        for k in keys {
            let va = a.feature_scores.get(k).copied().unwrap_or(0.5);
            let vb = b.feature_scores.get(k).copied().unwrap_or(0.5);
            let w = norm_w.get(k).copied().unwrap_or(0.25);
            let sim = (1.0 - (va - vb).abs()).clamp(0.0, 1.0);
            weighted_sim += w * sim;
            w_sum += w;
        }
        if w_sum > 0.0 {
            (weighted_sim / w_sum).clamp(0.0, 1.0)
        } else {
            0.0
        }
    }

    pub fn build_pairwise_similarity(&self) -> Option<PairwiseAnalogSimilarity> {
        let primary = self.primary_site()?;
        let partner = self.best_analog_partner()?;

        let ordered_dims = [
            ("elevation_relief", "Elevation Relief"),
            ("geomorphology_match", "Geomorphology (HI)"),
            ("roughness_similarity", "Roughness (TRI)"),
            ("slope_safety", "Slope Safety"),
        ];

        let mut dimensions = Vec::with_capacity(ordered_dims.len());
        for (key, label) in ordered_dims {
            let va = primary.feature_scores.get(key).copied().unwrap_or(0.5);
            let vb = partner.feature_scores.get(key).copied().unwrap_or(0.5);
            let sim_pct = ((1.0 - (va - vb).abs()).clamp(0.0, 1.0)) * 100.0;
            dimensions.push((label.to_string(), va, vb, sim_pct));
        }

        let overall_similarity_pct = self.compute_site_pair_similarity(primary, partner) * 100.0;

        let prim_sum = self.loaded_primary_summary;
        let comp_sum = self
            .loaded_comparison_summary
            .unwrap_or_else(|| estimate_site_summary_from_metadata(partner));
        let delta_b0_basins = Some(
            (prim_sum.persistent_basins_b0 as i64 - comp_sum.persistent_basins_b0 as i64).abs(),
        );
        let delta_euler_chi =
            Some((prim_sum.euler_characteristic - comp_sum.euler_characteristic).abs());

        Some(PairwiseAnalogSimilarity {
            primary_name: primary.name.clone(),
            primary_body: primary.body.clone(),
            primary_elev: primary.elevation_range_m,
            compare_id: partner.id.clone(),
            compare_name: partner.name.clone(),
            compare_body: partner.body.clone(),
            compare_elev: partner.elevation_range_m,
            overall_similarity_pct,
            dimensions,
            delta_b0_basins,
            delta_euler_chi,
            primary_safe_landing_15_pct: prim_sum.safe_landing_pct_under_15deg,
            primary_optimal_pad_5_pct: prim_sum.optimal_pad_pct_under_5deg,
            primary_mean_slope_deg: prim_sum.mean_slope_deg,
            primary_b0_basins: prim_sum.persistent_basins_b0,
            compare_safe_landing_15_pct: comp_sum.safe_landing_pct_under_15deg,
            compare_optimal_pad_5_pct: comp_sum.optimal_pad_pct_under_5deg,
            compare_mean_slope_deg: comp_sum.mean_slope_deg,
            compare_b0_basins: comp_sum.persistent_basins_b0,
        })
    }
}

/// Applies explicit font sizes, high-contrast colors (`#F1F5F9` / `#CBD5E1`), button padding,
/// item spacing, and line wrapping on 14" laptop displays.
fn configure_legible_style(ctx: &egui::Context) {
    ctx.style_mut(|style| {
        use egui::{FontFamily, FontId, TextStyle};
        style.text_styles.insert(
            TextStyle::Heading,
            FontId::new(17.5, FontFamily::Proportional),
        );
        style
            .text_styles
            .insert(TextStyle::Body, FontId::new(13.5, FontFamily::Proportional));
        style.text_styles.insert(
            TextStyle::Button,
            FontId::new(13.5, FontFamily::Proportional),
        );
        style.text_styles.insert(
            TextStyle::Monospace,
            FontId::new(13.0, FontFamily::Monospace),
        );
        style.text_styles.insert(
            TextStyle::Small,
            FontId::new(12.0, FontFamily::Proportional),
        );

        style.spacing.item_spacing = egui::vec2(8.0, 6.0);
        style.spacing.button_padding = egui::vec2(10.0, 5.0);
        style.wrap_mode = Some(egui::TextWrapMode::Wrap);

        style.visuals.override_text_color = Some(COLOR_TEXT_PRIMARY);
        style.visuals.panel_fill = COLOR_PANEL_BG;
        style.visuals.window_fill = COLOR_PANEL_BG;
        style.visuals.extreme_bg_color = egui::Color32::from_rgb(9, 13, 22);
        style.visuals.widgets.noninteractive.bg_fill = COLOR_CARD_BG;
        style.visuals.widgets.noninteractive.fg_stroke =
            egui::Stroke::new(1.0, COLOR_TEXT_PRIMARY);
        style.visuals.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0, COLOR_BORDER);
        style.visuals.widgets.inactive.bg_fill = egui::Color32::from_rgb(30, 41, 59);
        style.visuals.widgets.inactive.fg_stroke = egui::Stroke::new(1.0, COLOR_TEXT_PRIMARY);
        style.visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(51, 65, 85);
        style.visuals.widgets.hovered.fg_stroke = egui::Stroke::new(1.5, egui::Color32::WHITE);
        style.visuals.widgets.active.bg_fill = egui::Color32::from_rgb(37, 99, 235);
        style.visuals.widgets.active.fg_stroke = egui::Stroke::new(1.5, egui::Color32::WHITE);
    });
}

/// Returns the current Unix timestamp in seconds (with subsecond precision) on both WASM and native.
fn current_unix_timestamp_secs() -> f64 {
    #[cfg(target_arch = "wasm32")]
    {
        js_sys::Date::now() / 1000.0
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs_f64())
            .unwrap_or(1_775_000_000.0)
    }
}

/// Formats seconds of day (`0..86400`) as `HH:MM:SS`.
fn format_hms(seconds_of_day: f64) -> String {
    let total = seconds_of_day.rem_euclid(86_400.0) as u64;
    let hh = total / 3600;
    let mm = (total % 3600) / 60;
    let ss = total % 60;
    format!("{hh:02}:{mm:02}:{ss:02}")
}

/// Main `bevy_egui` rendering system.
pub fn viewer_ui_system(
    mut contexts: EguiContexts,
    mut app_state: ResMut<ViewerAppState>,
    mut skin_mode: ResMut<SurfaceSkinMode>,
    net: Res<NetBridge>,
    keys: Res<ButtonInput<KeyCode>>,
    mut camera_query: Query<(&mut OrbitCamera, &mut Projection)>,
) {
    let ctx = contexts.ctx_mut();
    if !app_state.style_configured {
        configure_legible_style(ctx);
        app_state.style_configured = true;
    }

    // -------------------------------------------------------------------------
    // Startup Access Gate: Homogeneous Full-Screen Entry before 3D Workspace
    // -------------------------------------------------------------------------
    if !app_state.mode_selected {
        render_access_gate_screen(ctx, &mut app_state, &net);
        return;
    }

    // -------------------------------------------------------------------------
    // Keyboard Shortcuts (disabled while typing inside any text input field)
    // -------------------------------------------------------------------------
    let wants_kb = ctx.wants_keyboard_input();
    if !wants_kb {
        // [H] Toggle Left Panel visibility (or restore UI if hidden)
        if keys.just_pressed(KeyCode::KeyH) {
            if app_state.hide_all_ui {
                app_state.hide_all_ui = false;
                app_state.show_left_panel = true;
            } else {
                app_state.show_left_panel = !app_state.show_left_panel;
            }
        }

        // [A] Toggle Auto-Rotate orbital animation
        if keys.just_pressed(KeyCode::KeyA) {
            for (mut cam, _) in &mut camera_query {
                cam.auto_rotate = !cam.auto_rotate;
            }
        }

        // [R] Reset / Center 3D Camera on origin
        if keys.just_pressed(KeyCode::KeyR) {
            let r = app_state.loaded_primary_bounding_radius;
            for (mut cam, mut proj) in &mut camera_query {
                cam.frame_terrain(Vec3::ZERO, r);
                cam.apply_perspective_clip_planes(&mut proj);
            }
        }

        // [C] Toggle Split-Screen Comparison with best cross-body analog
        if keys.just_pressed(KeyCode::KeyC) {
            if app_state.comparison_site_id.is_some() {
                app_state.comparison_site_id = None;
            } else if let Some(partner) = app_state.best_analog_partner().cloned() {
                app_state.comparison_site_id = Some(partner.id.clone());
                net.fetch_site_assets(TerrainSlot::Comparison, &partner);
            }
        }

        // [Up] / [Down] Cycle through catalog sites
        let site_count = app_state.catalog.sites.len();
        if site_count > 1 {
            let current_idx = app_state
                .catalog
                .sites
                .iter()
                .position(|s| s.id == app_state.primary_site_id)
                .unwrap_or(0);

            if keys.just_pressed(KeyCode::ArrowDown) {
                let next_idx = (current_idx + 1) % site_count;
                let next_site = app_state.catalog.sites[next_idx].clone();
                app_state.primary_site_id = next_site.id.clone();
                app_state.site_editor = SiteEditorForm::from_site(&next_site);
                net.fetch_site_assets(TerrainSlot::Primary, &next_site);
            } else if keys.just_pressed(KeyCode::ArrowUp) {
                let prev_idx = (current_idx + site_count - 1) % site_count;
                let prev_site = app_state.catalog.sites[prev_idx].clone();
                app_state.primary_site_id = prev_site.id.clone();
                app_state.site_editor = SiteEditorForm::from_site(&prev_site);
                net.fetch_site_assets(TerrainSlot::Primary, &prev_site);
            }
        }
    }

    let is_auto_rotating = camera_query
        .get_single()
        .map(|(c, _)| c.auto_rotate)
        .unwrap_or(false);

    // -------------------------------------------------------------------------
    // Top Status & Session Bar (hidden only when `hide_all_ui` is active)
    // -------------------------------------------------------------------------
    if !app_state.hide_all_ui {
        egui::TopBottomPanel::top("top_status_bar")
            .frame(
                egui::Frame::none()
                    .fill(COLOR_CARD_BG)
                    .inner_margin(egui::Margin::symmetric(12.0, 7.0))
                    .stroke(egui::Stroke::new(1.0, COLOR_BORDER)),
            )
            .show(ctx, |ui| {
                ui.horizontal_wrapped(|ui| {
                    let toggle_label = if app_state.show_left_panel {
                        "[H] Hide Panel"
                    } else {
                        "[H] Show Panel"
                    };
                    if ui
                        .add(
                            egui::Button::new(
                                egui::RichText::new(toggle_label)
                                    .strong()
                                    .color(egui::Color32::from_rgb(125, 211, 252)),
                            )
                            .fill(egui::Color32::from_rgb(30, 41, 59)),
                        )
                        .on_hover_text("Collapse or expand the left panel (Shortcut: H)")
                        .clicked()
                    {
                        app_state.show_left_panel = !app_state.show_left_panel;
                    }

                    ui.separator();
                    ui.label(
                        egui::RichText::new("Space-App-X86")
                            .strong()
                            .color(COLOR_TEXT_PRIMARY),
                    );
                    ui.separator();

                    if app_state.authenticated {
                        let user = app_state
                            .researcher_username
                            .as_deref()
                            .unwrap_or("researcher");
                        ui.label(
                            egui::RichText::new(format!("[Researcher: {user}]"))
                                .color(egui::Color32::from_rgb(74, 222, 128))
                                .strong(),
                        );
                        if ui.button("Log Out").clicked() {
                            net.logout();
                            app_state.authenticated = false;
                            app_state.researcher_username = None;
                            app_state.mode_selected = false;
                            app_state.status_message =
                                "Logged out. Returned to Space-App-X86 Access Gate.".to_string();
                        }
                    } else {
                        ui.label(
                            egui::RichText::new("[Guest: Public Demo Mode]")
                                .color(egui::Color32::from_rgb(96, 165, 250))
                                .strong(),
                        );
                        if ui.button("Researcher Login").clicked() {
                            app_state.show_login_modal = true;
                        }
                        if ui
                            .button("Access Gate")
                            .on_hover_text("Return to the startup Access Gate screen")
                            .clicked()
                        {
                            app_state.mode_selected = false;
                        }
                    }

                    ui.separator();
                    ui.label(
                        egui::RichText::new(
                            "Keys: [H] Panel | [A] Auto-Rotate | [C] Split | [R] Center | [Up/Down] Site | [+/-] Zoom",
                        )
                        .small()
                        .color(COLOR_TEXT_SECONDARY),
                    );
                });
            });
    }

    // -------------------------------------------------------------------------
    // Resizable, Non-Clipping Left Explorer Panel (5 Homogeneous Tabs)
    // -------------------------------------------------------------------------
    if app_state.show_left_panel && !app_state.hide_all_ui {
        let panel_resp = egui::SidePanel::left("left_control_panel")
            .resizable(true)
            .default_width(405.0)
            .width_range(340.0..=680.0)
            .frame(
                egui::Frame::none()
                    .fill(COLOR_PANEL_BG)
                    .inner_margin(egui::Margin::symmetric(12.0, 10.0))
                    .stroke(egui::Stroke::new(1.0, COLOR_BORDER)),
            )
            .show(ctx, |ui| {
                ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);

                if let Some(err) = app_state.error_message.clone() {
                    egui::Frame::none()
                        .fill(egui::Color32::from_rgb(69, 10, 10))
                        .inner_margin(egui::Margin::same(8.0))
                        .rounding(egui::Rounding::same(5.0))
                        .stroke(egui::Stroke::new(
                            1.0,
                            egui::Color32::from_rgb(248, 113, 113),
                        ))
                        .show(ui, |ui| {
                            ui.horizontal_wrapped(|ui| {
                                ui.colored_label(
                                    egui::Color32::from_rgb(254, 202, 202),
                                    format!("Error: {err}"),
                                );
                                if ui.small_button("Dismiss").clicked() {
                                    app_state.error_message = None;
                                }
                            });
                        });
                    ui.add_space(6.0);
                }

                // 5 Standardized Homogeneous Navigation Tabs (2 rows for zero horizontal clipping)
                egui::Frame::none()
                    .fill(COLOR_CARD_BG)
                    .inner_margin(egui::Margin::same(6.0))
                    .rounding(egui::Rounding::same(6.0))
                    .stroke(egui::Stroke::new(1.0, COLOR_BORDER))
                    .show(ui, |ui| {
                        ui.columns(3, |cols| {
                            let t1 = cols[0].add_sized(
                                [cols[0].available_width(), 28.0],
                                egui::SelectableLabel::new(
                                    app_state.active_tab == PanelTab::SitesAndCompare,
                                    "1. Sites & Compare",
                                ),
                            );
                            if t1.clicked() {
                                app_state.active_tab = PanelTab::SitesAndCompare;
                            }

                            let t2 = cols[1].add_sized(
                                [cols[1].available_width(), 28.0],
                                egui::SelectableLabel::new(
                                    app_state.active_tab == PanelTab::ViewAndCamera,
                                    "2. 3D & Camera",
                                ),
                            );
                            if t2.clicked() {
                                app_state.active_tab = PanelTab::ViewAndCamera;
                            }

                            let t3 = cols[2].add_sized(
                                [cols[2].available_width(), 28.0],
                                egui::SelectableLabel::new(
                                    app_state.active_tab == PanelTab::ChartsAndMath,
                                    "3. Charts & Math",
                                ),
                            );
                            if t3.clicked() {
                                app_state.active_tab = PanelTab::ChartsAndMath;
                            }
                        });
                        ui.add_space(4.0);
                        ui.columns(2, |cols| {
                            let t4 = cols[0].add_sized(
                                [cols[0].available_width(), 28.0],
                                egui::SelectableLabel::new(
                                    app_state.active_tab == PanelTab::SettlementGuide,
                                    "4. Settlement Guide",
                                ),
                            );
                            if t4.clicked() {
                                app_state.active_tab = PanelTab::SettlementGuide;
                            }

                            let t5 = cols[1].add_sized(
                                [cols[1].available_width(), 28.0],
                                egui::SelectableLabel::new(
                                    app_state.active_tab == PanelTab::UploadAndReset,
                                    "5. Upload & Reset",
                                ),
                            );
                            if t5.clicked() {
                                app_state.active_tab = PanelTab::UploadAndReset;
                            }
                        });
                    });

                ui.add_space(6.0);
                ui.label(
                    egui::RichText::new(&app_state.status_message)
                        .small()
                        .color(COLOR_TEXT_SECONDARY),
                );
                ui.separator();

                egui::ScrollArea::both()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
                        match app_state.active_tab {
                            PanelTab::SitesAndCompare => {
                                render_tab_sites_and_compare(ui, &mut app_state, &net);
                            }
                            PanelTab::ViewAndCamera => {
                                render_tab_view_and_camera(
                                    ui,
                                    &mut app_state,
                                    &mut skin_mode,
                                    &mut camera_query,
                                );
                            }
                            PanelTab::ChartsAndMath => {
                                render_tab_charts_and_math(ui, &mut app_state, &net);
                            }
                            PanelTab::SettlementGuide => {
                                render_tab_settlement_guide(ui);
                            }
                            PanelTab::UploadAndReset => {
                                render_tab_upload_and_reset(ui, &mut app_state, &net);
                            }
                        }
                    });
            });

        let screen_w = ctx.screen_rect().width().max(1.0);
        app_state.viewport_left_fraction =
            (panel_resp.response.rect.width() / screen_w).clamp(0.0, 0.70);
    } else {
        app_state.viewport_left_fraction = 0.0;
    }

    // -------------------------------------------------------------------------
    // Persistent Top-Left Floating Quick-Action Toolbar (Always Accessible)
    // -------------------------------------------------------------------------
    let screen_rect = ctx.screen_rect();
    let left_offset = if app_state.show_left_panel && !app_state.hide_all_ui {
        (screen_rect.width() * app_state.viewport_left_fraction + 14.0)
            .min(screen_rect.width() - 280.0)
    } else {
        14.0
    };
    let top_offset = if app_state.hide_all_ui { 14.0 } else { 46.0 };

    egui::Window::new("quick_floating_toolbar")
        .title_bar(false)
        .resizable(false)
        .fixed_pos(egui::pos2(left_offset.max(10.0), top_offset))
        .frame(
            egui::Frame::window(&ctx.style())
                .fill(egui::Color32::from_rgba_unmultiplied(15, 23, 42, 238))
                .stroke(egui::Stroke::new(1.0, COLOR_BORDER))
                .rounding(egui::Rounding::same(6.0))
                .inner_margin(egui::Margin::symmetric(8.0, 6.0)),
        )
        .show(ctx, |ui| {
            ui.horizontal_wrapped(|ui| {
                if app_state.hide_all_ui {
                    if ui
                        .button(
                            egui::RichText::new("[H] Show UI")
                                .strong()
                                .color(egui::Color32::from_rgb(125, 211, 252)),
                        )
                        .clicked()
                    {
                        app_state.hide_all_ui = false;
                        app_state.show_left_panel = true;
                    }
                    return;
                }

                let panel_btn = if app_state.show_left_panel {
                    "[< Panel]"
                } else {
                    "[> Panel]"
                };
                if ui
                    .button(egui::RichText::new(panel_btn).strong())
                    .on_hover_text("Show or hide the left explorer panel (Key: H)")
                    .clicked()
                {
                    app_state.show_left_panel = !app_state.show_left_panel;
                }

                let charts_label = if app_state.show_similarity_charts_window {
                    "[Charts: ON]"
                } else {
                    "[Charts]"
                };
                if ui
                    .button(
                        egui::RichText::new(charts_label).color(
                            if app_state.show_similarity_charts_window {
                                egui::Color32::from_rgb(125, 211, 252)
                            } else {
                                COLOR_TEXT_PRIMARY
                            },
                        ),
                    )
                    .on_hover_text("Toggle floating Similarity & Math Breakdown window")
                    .clicked()
                {
                    app_state.show_similarity_charts_window =
                        !app_state.show_similarity_charts_window;
                }

                let rot_label = if is_auto_rotating {
                    "[Rotate: ON]"
                } else {
                    "[Rotate: OFF]"
                };
                if ui
                    .button(egui::RichText::new(rot_label).color(if is_auto_rotating {
                        egui::Color32::from_rgb(74, 222, 128)
                    } else {
                        COLOR_TEXT_PRIMARY
                    }))
                    .on_hover_text("Toggle smooth orbital Auto-Rotate (Key: A)")
                    .clicked()
                {
                    for (mut cam, _) in &mut camera_query {
                        cam.auto_rotate = !cam.auto_rotate;
                    }
                }

                let split_on = app_state.comparison_site_id.is_some();
                let split_label = if split_on {
                    "[Split: ON]"
                } else {
                    "[Split: OFF]"
                };
                if ui
                    .button(egui::RichText::new(split_label).color(if split_on {
                        egui::Color32::from_rgb(250, 204, 21)
                    } else {
                        COLOR_TEXT_PRIMARY
                    }))
                    .on_hover_text("Toggle synchronized Split-Screen 3D comparison (Key: C)")
                    .clicked()
                {
                    if split_on {
                        app_state.comparison_site_id = None;
                    } else if let Some(partner) = app_state.best_analog_partner().cloned() {
                        app_state.comparison_site_id = Some(partner.id.clone());
                        net.fetch_site_assets(TerrainSlot::Comparison, &partner);
                    }
                }

                let telem_label = if app_state.show_planetary_telemetry {
                    "[Telemetry: ON]"
                } else {
                    "[Telemetry: OFF]"
                };
                if ui
                    .button(egui::RichText::new(telem_label).color(
                        if app_state.show_planetary_telemetry {
                            egui::Color32::from_rgb(56, 189, 248)
                        } else {
                            COLOR_TEXT_PRIMARY
                        },
                    ))
                    .on_hover_text("Show or hide real-time Planetary Telemetry HUD")
                    .clicked()
                {
                    app_state.show_planetary_telemetry = !app_state.show_planetary_telemetry;
                }

                if ui
                    .button("[Center 3D]")
                    .on_hover_text("Reset 3D camera to terrain center (Key: R)")
                    .clicked()
                {
                    let r = app_state.loaded_primary_bounding_radius;
                    for (mut cam, mut proj) in &mut camera_query {
                        cam.frame_terrain(Vec3::ZERO, r);
                        cam.apply_perspective_clip_planes(&mut proj);
                    }
                }

                egui::ComboBox::from_id_source("toolbar_surface_skin_combo")
                    .selected_text(format!("Skin: {}", skin_mode.short_label()))
                    .width(138.0_f32)
                    .show_ui(ui, |ui| {
                        for candidate in SurfaceSkinMode::ALL {
                            if ui
                                .selectable_label(*skin_mode == candidate, candidate.label())
                                .clicked()
                            {
                                *skin_mode = candidate;
                                app_state.show_checkerboard_uv = false;
                            }
                        }
                    });

                if ui
                    .small_button("[Full 3D]")
                    .on_hover_text("Hide all panels for clean full-screen 3D view (Key: H)")
                    .clicked()
                {
                    app_state.hide_all_ui = true;
                }
            });
        });

    // -------------------------------------------------------------------------
    // Live Planetary Telemetry HUD Card (Top-Right of 3D Viewport)
    // -------------------------------------------------------------------------
    if app_state.show_planetary_telemetry && !app_state.hide_all_ui {
        render_planetary_telemetry_hud(ctx, &mut app_state);
    }

    // -------------------------------------------------------------------------
    // Floating Similarity Charts & Mathematical Foundation Window
    // -------------------------------------------------------------------------
    if app_state.show_similarity_charts_window && !app_state.hide_all_ui {
        let mut open = app_state.show_similarity_charts_window;
        egui::Window::new("Planetary Analog Similarity Charts & Mathematical Foundation")
            .open(&mut open)
            .resizable(true)
            .default_width(510.0)
            .default_pos(egui::pos2(left_offset.max(16.0), top_offset + 44.0))
            .show(ctx, |ui| {
                ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
                render_similarity_charts_and_math(ui, &app_state);
            });
        app_state.show_similarity_charts_window = open;
    }

    // -------------------------------------------------------------------------
    // Split-Screen 3D Viewport Overlay Badges (Left = Primary, Right = Compare)
    // -------------------------------------------------------------------------
    if !app_state.hide_all_ui
        && let Some(comp_site) = app_state.comparison_site().cloned()
        && let Some(prim_site) = app_state.primary_site().cloned()
    {
        let usable_left = screen_rect.width() * app_state.viewport_left_fraction;
        let usable_w = (screen_rect.width() - usable_left).max(200.0);
        let left_x = usable_left + usable_w * 0.22 - 135.0;
        let right_x = usable_left + usable_w * 0.72 - 135.0;
        let badge_y = screen_rect.height() - 86.0;

        egui::Window::new("split_left_badge")
            .title_bar(false)
            .resizable(false)
            .fixed_pos(egui::pos2(left_x.max(usable_left + 8.0), badge_y))
            .frame(
                egui::Frame::window(&ctx.style())
                    .fill(egui::Color32::from_rgba_unmultiplied(15, 23, 42, 230))
                    .stroke(egui::Stroke::new(
                        1.5,
                        egui::Color32::from_rgb(56, 189, 248),
                    )),
            )
            .show(ctx, |ui| {
                ui.label(
                    egui::RichText::new(format!(
                        "LEFT [{}]: {}",
                        prim_site.body.as_str().to_uppercase(),
                        prim_site.name
                    ))
                    .strong()
                    .color(egui::Color32::from_rgb(125, 211, 252)),
                );
                if let Some([min_z, max_z]) = prim_site.elevation_range_m {
                    ui.label(
                        egui::RichText::new(format!(
                            "Elev: {min_z:.1} m .. {max_z:.1} m | {} tris",
                            app_state.loaded_primary_triangles
                        ))
                        .small()
                        .color(COLOR_TEXT_SECONDARY),
                    );
                }
            });

        egui::Window::new("split_right_badge")
            .title_bar(false)
            .resizable(false)
            .fixed_pos(egui::pos2(right_x.max(usable_left + 180.0), badge_y))
            .frame(
                egui::Frame::window(&ctx.style())
                    .fill(egui::Color32::from_rgba_unmultiplied(15, 23, 42, 230))
                    .stroke(egui::Stroke::new(
                        1.5,
                        egui::Color32::from_rgb(250, 204, 21),
                    )),
            )
            .show(ctx, |ui| {
                ui.label(
                    egui::RichText::new(format!(
                        "RIGHT [{}]: {}",
                        comp_site.body.as_str().to_uppercase(),
                        comp_site.name
                    ))
                    .strong()
                    .color(egui::Color32::from_rgb(253, 224, 71)),
                );
                if let Some([min_z, max_z]) = comp_site.elevation_range_m {
                    ui.label(
                        egui::RichText::new(format!(
                            "Elev: {min_z:.1} m .. {max_z:.1} m | {} tris",
                            app_state.loaded_comparison_triangles
                        ))
                        .small()
                        .color(COLOR_TEXT_SECONDARY),
                    );
                }
            });
    }

    // -------------------------------------------------------------------------
    // Researcher Login Modal (when opened from Guest Demo Mode)
    // -------------------------------------------------------------------------
    if app_state.show_login_modal {
        egui::Window::new("Researcher Authentication (Argon2id)")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.set_width(370.0);
                ui.label(
                    "Authenticate with Researcher credentials to unlock 512 MiB binary terrain uploads (.stl, .tif/.tiff, .png), 0D Persistent Homology TDA, canonical weight persistence, and Workspace Reset.",
                );
                ui.add_space(8.0);

                egui::Grid::new("modal_login_grid")
                    .num_columns(2)
                    .spacing([10.0, 8.0])
                    .show(ui, |ui| {
                        ui.label("Username:");
                        ui.text_edit_singleline(&mut app_state.login_username);
                        ui.end_row();

                        ui.label("Password:");
                        let pass_resp = ui.add(
                            egui::TextEdit::singleline(&mut app_state.login_password)
                                .password(true),
                        );
                        if pass_resp.lost_focus()
                            && ui.input(|i| i.key_pressed(egui::Key::Enter))
                        {
                            net.login(
                                app_state.login_username.clone(),
                                app_state.login_password.clone(),
                            );
                            app_state.login_password.clear();
                            app_state.show_login_modal = false;
                        }
                        ui.end_row();
                    });

                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    if ui
                        .add(
                            egui::Button::new(
                                egui::RichText::new("Log in as Researcher")
                                    .strong()
                                    .color(egui::Color32::WHITE),
                            )
                            .fill(egui::Color32::from_rgb(37, 99, 235)),
                        )
                        .clicked()
                    {
                        net.login(
                            app_state.login_username.clone(),
                            app_state.login_password.clone(),
                        );
                        app_state.login_password.clear();
                        app_state.show_login_modal = false;
                    }
                    if ui.button("Cancel").clicked() {
                        app_state.show_login_modal = false;
                    }
                });
            });
    }
}

/// Renders the homogeneous full-screen startup **Space-App-X86 Access Gate** before entering
/// Guest Public Demo Mode or logging in as an authenticated Researcher.
fn render_access_gate_screen(
    ctx: &egui::Context,
    app_state: &mut ViewerAppState,
    net: &NetBridge,
) {
    egui::CentralPanel::default()
        .frame(
            egui::Frame::none()
                .fill(egui::Color32::from_rgb(10, 15, 26))
                .inner_margin(egui::Margin::same(24.0)),
        )
        .show(ctx, |ui| {
            let avail_h = ui.available_height();
            let top_pad = ((avail_h - 460.0) * 0.45).max(16.0);
            ui.add_space(top_pad);

            ui.vertical_centered(|ui| {
                egui::Frame::none()
                    .fill(COLOR_CARD_BG)
                    .stroke(egui::Stroke::new(
                        1.5,
                        egui::Color32::from_rgb(56, 189, 248),
                    ))
                    .rounding(egui::Rounding::same(10.0))
                    .inner_margin(egui::Margin::symmetric(28.0, 24.0))
                    .show(ui, |ui| {
                        ui.set_max_width(560.0);
                        ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);

                        ui.heading(
                            egui::RichText::new(
                                "Space-App-X86 Access Gate — Planetary Analogs 3D Platform",
                            )
                            .color(egui::Color32::from_rgb(125, 211, 252))
                            .strong(),
                        );
                        ui.add_space(6.0);
                        ui.label(
                            egui::RichText::new(
                                "3D Morphometric & Topological Data Analysis (0D Persistent Homology b0 & Euler Characteristic chi) for Lunar South Pole (Artemis) and Martian Landing & Habitat Site Selection.",
                            )
                            .color(COLOR_TEXT_SECONDARY),
                        );

                        if let Some(err) = app_state.error_message.clone() {
                            ui.add_space(8.0);
                            egui::Frame::none()
                                .fill(egui::Color32::from_rgb(69, 10, 10))
                                .inner_margin(egui::Margin::same(8.0))
                                .rounding(egui::Rounding::same(5.0))
                                .stroke(egui::Stroke::new(
                                    1.0,
                                    egui::Color32::from_rgb(248, 113, 113),
                                ))
                                .show(ui, |ui| {
                                    ui.colored_label(
                                        egui::Color32::from_rgb(254, 202, 202),
                                        format!("Authentication Error: {err}"),
                                    );
                                });
                        }

                        ui.add_space(14.0);
                        ui.separator();
                        ui.add_space(10.0);

                        // Option 1: Researcher Authentication Form
                        ui.label(
                            egui::RichText::new("Option 1 — Authenticated Researcher Workspace")
                                .strong()
                                .color(egui::Color32::from_rgb(74, 222, 128)),
                        );
                        ui.label(
                            egui::RichText::new(
                                "Unlocks 512 MiB raw binary terrain uploads (.stl, .tif/.tiff, .png), automated 50k mesh simplification, 0D Persistent Homology TDA, canonical weight saving, and Workspace Reset.",
                            )
                            .small()
                            .color(COLOR_TEXT_SECONDARY),
                        );
                        ui.add_space(6.0);

                        egui::Grid::new("access_gate_login_grid")
                            .num_columns(2)
                            .spacing([12.0, 8.0])
                            .show(ui, |ui| {
                                ui.label(
                                    egui::RichText::new("Username:").color(COLOR_TEXT_PRIMARY),
                                );
                                ui.add_sized(
                                    [320.0, 28.0],
                                    egui::TextEdit::singleline(&mut app_state.login_username),
                                );
                                ui.end_row();

                                ui.label(
                                    egui::RichText::new("Password:").color(COLOR_TEXT_PRIMARY),
                                );
                                let pass_resp = ui.add_sized(
                                    [320.0, 28.0],
                                    egui::TextEdit::singleline(&mut app_state.login_password)
                                        .password(true),
                                );
                                if pass_resp.lost_focus()
                                    && ui.input(|i| i.key_pressed(egui::Key::Enter))
                                {
                                    app_state.error_message = None;
                                    net.login(
                                        app_state.login_username.clone(),
                                        app_state.login_password.clone(),
                                    );
                                    app_state.login_password.clear();
                                }
                                ui.end_row();
                            });

                        ui.add_space(8.0);
                        if ui
                            .add_sized(
                                [ui.available_width(), 34.0],
                                egui::Button::new(
                                    egui::RichText::new("Log in as Researcher (Argon2id)")
                                        .strong()
                                        .color(egui::Color32::WHITE),
                                )
                                .fill(egui::Color32::from_rgb(37, 99, 235)),
                            )
                            .clicked()
                        {
                            app_state.error_message = None;
                            net.login(
                                app_state.login_username.clone(),
                                app_state.login_password.clone(),
                            );
                            app_state.login_password.clear();
                        }

                        ui.add_space(14.0);
                        ui.separator();
                        ui.add_space(10.0);

                        // Option 2: Guest Public Demo Mode
                        ui.label(
                            egui::RichText::new("Option 2 — Guest / Judge Public Demo Mode")
                                .strong()
                                .color(egui::Color32::from_rgb(125, 211, 252)),
                        );
                        ui.label(
                            egui::RichText::new(
                                "Passwordless access to 3D planetary terrains, planar UV checkerboard verification, synchronized Split-Screen comparison, live Planetary Telemetry HUD, and custom weight simulations.",
                            )
                            .small()
                            .color(COLOR_TEXT_SECONDARY),
                        );
                        ui.add_space(8.0);

                        if ui
                            .add_sized(
                                [ui.available_width(), 36.0],
                                egui::Button::new(
                                    egui::RichText::new("Enter as Guest (Public Demo Mode)")
                                        .strong()
                                        .color(egui::Color32::WHITE),
                                )
                                .fill(egui::Color32::from_rgb(22, 163, 74)),
                            )
                            .clicked()
                        {
                            app_state.mode_selected = true;
                            app_state.error_message = None;
                            net.fetch_demo_mode();
                        }
                    });
            });
        });
}

/// Renders the top-right real-time Planetary Telemetry HUD card (`MOON`, `MARS`, or `EARTH`).
fn render_planetary_telemetry_hud(ctx: &egui::Context, app_state: &mut ViewerAppState) {
    let Some(site) = app_state.primary_site().cloned() else {
        return;
    };

    let unix_secs = current_unix_timestamp_secs();
    let [lat_deg, lon_deg] = site.center_lat_lon.unwrap_or(match site.body {
        Body::Moon => [-89.5, 0.0],
        Body::Mars => [18.38, 77.58],
        Body::Earth => [19.47, -155.58],
    });
    let elev_range = site.elevation_range_m.unwrap_or([-45.0, 120.0]);
    let relief_m = (elev_range[1] - elev_range[0]).max(0.0);

    let screen_rect = ctx.screen_rect();
    egui::Window::new("planetary_telemetry_hud")
        .title_bar(false)
        .resizable(false)
        .fixed_pos(egui::pos2((screen_rect.width() - 340.0).max(20.0), 46.0))
        .frame(
            egui::Frame::window(&ctx.style())
                .fill(egui::Color32::from_rgba_unmultiplied(11, 18, 31, 238))
                .stroke(egui::Stroke::new(
                    1.2,
                    egui::Color32::from_rgb(56, 189, 248),
                ))
                .rounding(egui::Rounding::same(8.0))
                .inner_margin(egui::Margin::symmetric(12.0, 10.0)),
        )
        .show(ctx, |ui| {
            ui.set_width(316.0);
            ui.horizontal(|ui| {
                let badge_color = match site.body {
                    Body::Moon => egui::Color32::from_rgb(148, 163, 184),
                    Body::Mars => egui::Color32::from_rgb(248, 113, 113),
                    Body::Earth => egui::Color32::from_rgb(56, 189, 248),
                };
                ui.label(
                    egui::RichText::new(format!(
                        "[*] {} LIVE TELEMETRY HUD",
                        site.body.as_str().to_uppercase()
                    ))
                    .strong()
                    .color(badge_color),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .small_button("x")
                        .on_hover_text("Hide Telemetry HUD")
                        .clicked()
                    {
                        app_state.show_planetary_telemetry = false;
                    }
                });
            });

            ui.label(
                egui::RichText::new(&site.name)
                    .small()
                    .strong()
                    .color(COLOR_TEXT_PRIMARY),
            );
            ui.separator();

            match site.body {
                Body::Moon => {
                    let synodic_period_s = 29.530588853 * 86_400.0;
                    let ref_new_moon_s = 947_182_440.0;
                    let phase = ((unix_secs - ref_new_moon_s) / synodic_period_s
                        + lon_deg / 360.0)
                        .rem_euclid(1.0);
                    let lst_secs = phase * 86_400.0;
                    let sun_elev_factor =
                        (phase * std::f64::consts::TAU - std::f64::consts::FRAC_PI_2).sin();
                    let illum_pct = ((sun_elev_factor + 1.0) * 50.0).clamp(0.0, 100.0);

                    let is_polar = lat_deg.abs() >= 80.0;
                    let temp_range_str = if is_polar {
                        let rim_k = 110.0 + 115.0 * (illum_pct / 100.0);
                        format!("~40 K (-233 C PSR) .. {rim_k:.0} K (Rim)")
                    } else {
                        let curr_k = 100.0 + 290.0 * (illum_pct / 100.0);
                        format!("100 K .. 390 K (Now ~{curr_k:.0} K)")
                    };

                    egui::Grid::new("moon_telem_grid")
                        .num_columns(2)
                        .spacing([8.0, 4.0])
                        .show(ui, |ui| {
                            ui.label(
                                egui::RichText::new("Lunar Solar Time (LST):")
                                    .small()
                                    .color(COLOR_TEXT_SECONDARY),
                            );
                            ui.monospace(format!("{} LST", format_hms(lst_secs)));
                            ui.end_row();

                            ui.label(
                                egui::RichText::new("Solar Illumination:")
                                    .small()
                                    .color(COLOR_TEXT_SECONDARY),
                            );
                            ui.monospace(format!("{illum_pct:.1}% (Synodic 29.53 d)"));
                            ui.end_row();

                            ui.label(
                                egui::RichText::new("Surface Thermal:")
                                    .small()
                                    .color(COLOR_TEXT_SECONDARY),
                            );
                            ui.monospace(temp_range_str);
                            ui.end_row();

                            ui.label(
                                egui::RichText::new("Gravity & Pressure:")
                                    .small()
                                    .color(COLOR_TEXT_SECONDARY),
                            );
                            ui.monospace("1.62 m/s^2 (0.165 g) | ~3e-15 bar");
                            ui.end_row();

                            ui.label(
                                egui::RichText::new("Site Lat/Lon & Relief:")
                                    .small()
                                    .color(COLOR_TEXT_SECONDARY),
                            );
                            ui.monospace(format!(
                                "{lat_deg:.2} deg, {lon_deg:.2} deg | dz {relief_m:.0} m"
                            ));
                            ui.end_row();
                        });
                }
                Body::Mars => {
                    let jd_ut = 2_440_587.5 + (unix_secs / 86_400.0);
                    let jd_tt = jd_ut + 69.184 / 86_400.0;
                    let msd = (jd_tt - 2_451_549.5) / 1.0274912517 + 44_796.0 - 0.0009626;
                    let sol_number = msd.floor() as i64;
                    let mtc_hours = (msd.rem_euclid(1.0)) * 24.0;
                    let lmst_hours = (mtc_hours + lon_deg / 15.0).rem_euclid(24.0);
                    let lmst_secs = lmst_hours * 3600.0;

                    let diurnal_wave =
                        ((lmst_hours - 14.5) / 24.0 * std::f64::consts::TAU).cos();
                    let temp_c = -63.0 + 42.0 * diurnal_wave;
                    let temp_k = temp_c + 273.15;

                    egui::Grid::new("mars_telem_grid")
                        .num_columns(2)
                        .spacing([8.0, 4.0])
                        .show(ui, |ui| {
                            ui.label(
                                egui::RichText::new("Mars Sol & LMST:")
                                    .small()
                                    .color(COLOR_TEXT_SECONDARY),
                            );
                            ui.monospace(format!("Sol {sol_number} | {}", format_hms(lmst_secs)));
                            ui.end_row();

                            ui.label(
                                egui::RichText::new("Coordinated Mars Time:")
                                    .small()
                                    .color(COLOR_TEXT_SECONDARY),
                            );
                            ui.monospace(format!("{} MTC", format_hms(mtc_hours * 3600.0)));
                            ui.end_row();

                            ui.label(
                                egui::RichText::new("Surface Thermal:")
                                    .small()
                                    .color(COLOR_TEXT_SECONDARY),
                            );
                            ui.monospace(format!("{temp_c:.1} C ({temp_k:.0} K) [-125..+20 C]"));
                            ui.end_row();

                            ui.label(
                                egui::RichText::new("CO2 Atmos & Gravity:")
                                    .small()
                                    .color(COLOR_TEXT_SECONDARY),
                            );
                            ui.monospace("6.1 mbar (95% CO2) | 3.72 m/s^2");
                            ui.end_row();

                            ui.label(
                                egui::RichText::new("Site Lat/Lon & Relief:")
                                    .small()
                                    .color(COLOR_TEXT_SECONDARY),
                            );
                            ui.monospace(format!(
                                "{lat_deg:.2} deg, {lon_deg:.2} deg | dz {relief_m:.0} m"
                            ));
                            ui.end_row();
                        });
                }
                Body::Earth => {
                    let utc_secs = unix_secs.rem_euclid(86_400.0);
                    let lst_secs = (utc_secs + (lon_deg / 15.0) * 3600.0).rem_euclid(86_400.0);
                    let lst_hours = lst_secs / 3600.0;
                    let diurnal_wave =
                        (((lst_hours - 14.0) / 24.0 * std::f64::consts::TAU).cos()) as f32;
                    let mean_elev_km = ((elev_range[0] + elev_range[1]) * 0.5 / 1000.0).max(0.0);
                    let temp_c = 18.0_f32 - 6.5_f32 * mean_elev_km + 7.5_f32 * diurnal_wave;
                    let pressure_hpa =
                        1013.25_f32 * (1.0 - 0.0065 * mean_elev_km * 1000.0 / 288.15).powf(5.255);

                    egui::Grid::new("earth_telem_grid")
                        .num_columns(2)
                        .spacing([8.0, 4.0])
                        .show(ui, |ui| {
                            ui.label(
                                egui::RichText::new("UTC / Local Solar Time:")
                                    .small()
                                    .color(COLOR_TEXT_SECONDARY),
                            );
                            ui.monospace(format!(
                                "{} UTC | {} LST",
                                format_hms(utc_secs),
                                format_hms(lst_secs)
                            ));
                            ui.end_row();

                            ui.label(
                                egui::RichText::new("Analog Field Thermal:")
                                    .small()
                                    .color(COLOR_TEXT_SECONDARY),
                            );
                            ui.monospace(format!(
                                "{temp_c:.1} C ({:.0} K)",
                                temp_c + 273.15
                            ));
                            ui.end_row();

                            ui.label(
                                egui::RichText::new("Barometric & Gravity:")
                                    .small()
                                    .color(COLOR_TEXT_SECONDARY),
                            );
                            ui.monospace(format!("{pressure_hpa:.0} hPa | 9.81 m/s^2 (1.00 g)"));
                            ui.end_row();

                            ui.label(
                                egui::RichText::new("Site Lat/Lon & Relief:")
                                    .small()
                                    .color(COLOR_TEXT_SECONDARY),
                            );
                            ui.monospace(format!(
                                "{lat_deg:.2} deg, {lon_deg:.2} deg | dz {relief_m:.0} m"
                            ));
                            ui.end_row();
                        });
                }
            }
        });
}

/// Tab 1: Primary site selector, Side-by-Side Comparison selector, and Analog Similarity Card.
fn render_tab_sites_and_compare(
    ui: &mut egui::Ui,
    app_state: &mut ViewerAppState,
    net: &NetBridge,
) {
    ui.heading("1. Terrain Catalog & Split Comparison");
    ui.add_space(4.0);

    if app_state.catalog.sites.is_empty() {
        egui::Frame::none()
            .fill(COLOR_CARD_BG)
            .inner_margin(egui::Margin::same(12.0))
            .rounding(egui::Rounding::same(6.0))
            .stroke(egui::Stroke::new(
                1.0,
                egui::Color32::from_rgb(250, 204, 21),
            ))
            .show(ui, |ui| {
                ui.label(
                    egui::RichText::new("Workspace Catalog is Currently Empty (0 Maps Loaded)")
                        .strong()
                        .color(egui::Color32::from_rgb(253, 224, 71)),
                );
                ui.label(
                    "Open Tab '5. Upload & Reset' to upload your own .stl / .tif / .png planetary maps or click 'Restore Reference Demo Site'.",
                );
                ui.add_space(6.0);
                if ui.button("Go to 5. Upload & Reset").clicked() {
                    app_state.active_tab = PanelTab::UploadAndReset;
                }
            });
        return;
    }

    let current_primary_id = app_state.primary_site_id.clone();
    let sites_snapshot = app_state.catalog.sites.clone();

    ui.horizontal_wrapped(|ui| {
        ui.label(egui::RichText::new("Primary Site:").strong());
        egui::ComboBox::from_id_source("primary_site_combo")
            .selected_text(&current_primary_id)
            .width(230.0)
            .show_ui(ui, |ui| {
                for site in &sites_snapshot {
                    let label = format!(
                        "{} ({} - {})",
                        site.id,
                        site.name,
                        site.body.as_str().to_uppercase()
                    );
                    if ui
                        .selectable_label(app_state.primary_site_id == site.id, label)
                        .clicked()
                    {
                        app_state.primary_site_id = site.id.clone();
                        app_state.site_editor = SiteEditorForm::from_site(site);
                        net.fetch_site_assets(TerrainSlot::Primary, site);
                    }
                }
            });
    });

    // Keyboard-friendly Previous / Next site buttons
    if sites_snapshot.len() > 1 {
        ui.horizontal_wrapped(|ui| {
            let curr_idx = sites_snapshot
                .iter()
                .position(|s| s.id == app_state.primary_site_id)
                .unwrap_or(0);
            if ui
                .button("< Prev Site [Up]")
                .on_hover_text("Select previous site in catalog (ArrowUp)")
                .clicked()
            {
                let prev = &sites_snapshot[(curr_idx + sites_snapshot.len() - 1) % sites_snapshot.len()];
                app_state.primary_site_id = prev.id.clone();
                app_state.site_editor = SiteEditorForm::from_site(prev);
                net.fetch_site_assets(TerrainSlot::Primary, prev);
            }
            if ui
                .button("Next Site [Down] >")
                .on_hover_text("Select next site in catalog (ArrowDown)")
                .clicked()
            {
                let next = &sites_snapshot[(curr_idx + 1) % sites_snapshot.len()];
                app_state.primary_site_id = next.id.clone();
                app_state.site_editor = SiteEditorForm::from_site(next);
                net.fetch_site_assets(TerrainSlot::Primary, next);
            }
        });
    }

    ui.add_space(4.0);

    let comp_text = app_state
        .comparison_site_id
        .clone()
        .unwrap_or_else(|| "None (Single View)".to_string());
    ui.horizontal_wrapped(|ui| {
        ui.label(egui::RichText::new("Compare With:").strong());
        egui::ComboBox::from_id_source("comparison_site_combo")
            .selected_text(comp_text)
            .width(230.0)
            .show_ui(ui, |ui| {
                if ui
                    .selectable_label(
                        app_state.comparison_site_id.is_none(),
                        "None (Single View)",
                    )
                    .clicked()
                {
                    app_state.comparison_site_id = None;
                }
                for site in &sites_snapshot {
                    let label = format!(
                        "{} ({} - {})",
                        site.id,
                        site.name,
                        site.body.as_str().to_uppercase()
                    );
                    let is_selected =
                        app_state.comparison_site_id.as_deref() == Some(site.id.as_str());
                    if ui.selectable_label(is_selected, label).clicked() {
                        app_state.comparison_site_id = Some(site.id.clone());
                        net.fetch_site_assets(TerrainSlot::Comparison, site);
                    }
                }
            });
    });

    if let Some(site) = app_state.primary_site().cloned() {
        ui.add_space(6.0);
        egui::Frame::none()
            .fill(COLOR_CARD_BG)
            .inner_margin(egui::Margin::same(10.0))
            .rounding(egui::Rounding::same(6.0))
            .stroke(egui::Stroke::new(1.0, COLOR_BORDER))
            .show(ui, |ui| {
                ui.label(
                    egui::RichText::new(format!(
                        "{} [{}]",
                        site.name,
                        site.body.as_str().to_uppercase()
                    ))
                    .strong()
                    .color(egui::Color32::from_rgb(125, 211, 252)),
                );
                ui.label(
                    egui::RichText::new(&site.description)
                        .small()
                        .color(COLOR_TEXT_SECONDARY),
                );
                ui.add_space(4.0);
                let s = &app_state.loaded_primary_summary;
                let relief_dz = (s.max_elevation_m - s.min_elevation_m).max(0.0);
                ui.monospace(format!(
                    "Extent: {:.1}m × {:.1}m | Relief Δz: {:.1}m | β₀ basins: {} | χ: {} | {} tris",
                    site.dem_extent.width(),
                    site.dem_extent.height(),
                    relief_dz,
                    s.persistent_basins_b0,
                    s.euler_characteristic,
                    app_state.loaded_primary_triangles
                ));
                ui.monospace(format!(
                    "Landing Safety: <15° Safe = {:.1}% | <5° Optimal Pad = {:.1}% | Mean θ = {:.1}°",
                    s.safe_landing_pct_under_15deg,
                    s.optimal_pad_pct_under_5deg,
                    s.mean_slope_deg
                ));
            });
    }

    // -------------------------------------------------------------------------
    // Highlighted Card: Planetary Analog Similarity Breakdown (Earth <-> Moon/Mars)
    // -------------------------------------------------------------------------
    if let Some(sim) = app_state.build_pairwise_similarity() {
        ui.add_space(8.0);
        egui::Frame::none()
            .fill(COLOR_CARD_BG)
            .inner_margin(egui::Margin::same(10.0))
            .rounding(egui::Rounding::same(6.0))
            .stroke(egui::Stroke::new(
                1.5,
                egui::Color32::from_rgb(56, 189, 248),
            ))
            .show(ui, |ui| {
                ui.label(
                    egui::RichText::new(
                        "Planetary Analog Similarity Breakdown (Earth ↔ Moon/Mars)",
                    )
                    .strong()
                    .color(egui::Color32::from_rgb(125, 211, 252)),
                );
                ui.label(
                    egui::RichText::new(format!(
                        "{} [{}]  ↔  {} [{}]",
                        sim.primary_name,
                        sim.primary_body.as_str().to_uppercase(),
                        sim.compare_name,
                        sim.compare_body.as_str().to_uppercase()
                    ))
                    .small()
                    .strong()
                    .color(COLOR_TEXT_PRIMARY),
                );

                ui.add_space(4.0);
                let overall_frac = (sim.overall_similarity_pct / 100.0).clamp(0.0, 1.0) as f32;
                ui.add(
                    egui::ProgressBar::new(overall_frac)
                        .text(format!(
                            "Global Morphometric & TDA Similarity: {:.1}%",
                            sim.overall_similarity_pct
                        ))
                        .fill(egui::Color32::from_rgb(37, 99, 235)),
                );

                ui.add_space(6.0);
                for (dim_label, va, vb, sim_pct) in &sim.dimensions {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(egui::RichText::new(dim_label).small().strong());
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.monospace(format!(
                                "{sim_pct:.0}% ({va:.2} vs {vb:.2})"
                            ));
                        });
                    });
                    ui.add(
                        egui::ProgressBar::new((*sim_pct / 100.0) as f32)
                            .desired_height(6.0)
                            .fill(egui::Color32::from_rgb(16, 185, 129)),
                    );
                }

                if sim.delta_b0_basins.is_some() || sim.delta_euler_chi.is_some() {
                    ui.add_space(4.0);
                    ui.monospace(format!(
                        "TDA Invariants: Δβ₀ basins = {} | Δχ Euler = {}",
                        sim.delta_b0_basins
                            .map(|v| v.to_string())
                            .unwrap_or_else(|| "n/a".to_string()),
                        sim.delta_euler_chi
                            .map(|v| v.to_string())
                            .unwrap_or_else(|| "n/a".to_string()),
                    ));
                }

                // Mission Engineering Indicator: Landing Safety & Cold-Trap Proxy Comparison
                ui.add_space(6.0);
                egui::Frame::none()
                    .fill(egui::Color32::from_rgb(9, 14, 24))
                    .inner_margin(egui::Margin::same(8.0))
                    .rounding(egui::Rounding::same(5.0))
                    .stroke(egui::Stroke::new(
                        1.0,
                        egui::Color32::from_rgb(34, 197, 94),
                    ))
                    .show(ui, |ui| {
                        ui.label(
                            egui::RichText::new(
                                "Mission Engineering: Landing Safety & Cold-Trap Proxy",
                            )
                            .small()
                            .strong()
                            .color(egui::Color32::from_rgb(74, 222, 128)),
                        );
                        ui.monospace(format!(
                            "Safe Landing (< 15°): {:.1}% ({})  ↔  {:.1}% ({})",
                            sim.primary_safe_landing_15_pct,
                            sim.primary_body.as_str().to_uppercase(),
                            sim.compare_safe_landing_15_pct,
                            sim.compare_body.as_str().to_uppercase(),
                        ));
                        ui.monospace(format!(
                            "Optimal Pad  (<  5°): {:.1}% (θ̄ {:.1}°) ↔  {:.1}% (θ̄ {:.1}°)",
                            sim.primary_optimal_pad_5_pct,
                            sim.primary_mean_slope_deg,
                            sim.compare_optimal_pad_5_pct,
                            sim.compare_mean_slope_deg,
                        ));
                        ui.monospace(format!(
                            "Cold-Trap / Shielding Basins (β₀): {}  ↔  {}",
                            sim.primary_b0_basins, sim.compare_b0_basins
                        ));
                        ui.label(
                            egui::RichText::new(
                                "Artemis Lunar South Pole & Martian Habitat Utility: Quantifies rollover-safe lander touchdown zones (< 15°), level pressurized habitat foundations (< 5°), and β₀ depression cold-trap / radiation-shielding basins.",
                            )
                            .small()
                            .color(COLOR_TEXT_SECONDARY),
                        );
                    });

                ui.add_space(6.0);
                let split_active =
                    app_state.comparison_site_id.as_deref() == Some(sim.compare_id.as_str());
                ui.horizontal_wrapped(|ui| {
                    if split_active {
                        if ui.button("Exit Split-Screen [C]").clicked() {
                            app_state.comparison_site_id = None;
                        }
                    } else if ui
                        .button(
                            egui::RichText::new(format!(
                                "Split-Screen 3D Compare with '{}' [C]",
                                sim.compare_name
                            ))
                            .strong()
                            .color(egui::Color32::from_rgb(253, 224, 71)),
                        )
                        .clicked()
                    {
                        if let Some(partner) = app_state
                            .catalog
                            .sites
                            .iter()
                            .find(|s| s.id == sim.compare_id)
                            .cloned()
                        {
                            app_state.comparison_site_id = Some(partner.id.clone());
                            net.fetch_site_assets(TerrainSlot::Comparison, &partner);
                        }
                    }

                    if ui.button("Open Charts & Math").clicked() {
                        app_state.active_tab = PanelTab::ChartsAndMath;
                    }
                });
            });
    }
}

/// Tab 2: Real-Time 3D Surface Skin / Palette Selector, Mission Engineering Landing Safety Card,
/// Vertical Exaggeration, Planar UV Checkerboard test, and Mouse/Keyboard Camera controls.
fn render_tab_view_and_camera(
    ui: &mut egui::Ui,
    app_state: &mut ViewerAppState,
    skin_mode: &mut SurfaceSkinMode,
    camera_query: &mut Query<(&mut OrbitCamera, &mut Projection)>,
) {
    ui.heading("2. 3D Terrain, Surface Skins & Camera");
    ui.add_space(4.0);

    // -------------------------------------------------------------------------
    // Card A: Real-Time 3D Surface Skin / Palette Selector (6 Scientific Modes)
    // -------------------------------------------------------------------------
    egui::Frame::none()
        .fill(COLOR_CARD_BG)
        .inner_margin(egui::Margin::same(10.0))
        .rounding(egui::Rounding::same(6.0))
        .stroke(egui::Stroke::new(
            1.2,
            egui::Color32::from_rgb(56, 189, 248),
        ))
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new("Real-Time 3D Surface Skin / Palette Selector")
                    .strong()
                    .color(egui::Color32::from_rgb(125, 211, 252)),
            );
            ui.label(
                egui::RichText::new(
                    "Dynamically recolors Primary and Split-View 3D meshes via vertex attributes (elevation y or normal slope angle θ = arccos(n_y)) and material texture switching:",
                )
                .small()
                .color(COLOR_TEXT_SECONDARY),
            );
            ui.add_space(4.0);

            ui.horizontal_wrapped(|ui| {
                ui.label(egui::RichText::new("Active 3D Skin:").strong());
                egui::ComboBox::from_id_source("surface_skin_combo")
                    .selected_text(skin_mode.label())
                    .width(235.0_f32)
                    .show_ui(ui, |ui| {
                        for candidate in SurfaceSkinMode::ALL {
                            if ui
                                .selectable_label(*skin_mode == candidate, candidate.label())
                                .clicked()
                            {
                                *skin_mode = candidate;
                                app_state.show_checkerboard_uv = false;
                            }
                        }
                    });
            });

            ui.add_space(4.0);
            ui.horizontal_wrapped(|ui| {
                for candidate in SurfaceSkinMode::ALL {
                    let is_active = *skin_mode == candidate && !app_state.show_checkerboard_uv;
                    let btn = egui::Button::new(
                        egui::RichText::new(candidate.short_label())
                            .small()
                            .strong()
                            .color(if is_active {
                                egui::Color32::WHITE
                            } else {
                                COLOR_TEXT_PRIMARY
                            }),
                    )
                    .fill(if is_active {
                        egui::Color32::from_rgb(37, 99, 235)
                    } else {
                        egui::Color32::from_rgb(30, 41, 59)
                    });
                    if ui
                        .add(btn)
                        .on_hover_text(candidate.label())
                        .clicked()
                    {
                        *skin_mode = candidate;
                        app_state.show_checkerboard_uv = false;
                    }
                }
            });

            if *skin_mode == SurfaceSkinMode::LandingSlopeHazard {
                ui.add_space(4.0);
                ui.horizontal_wrapped(|ui| {
                    ui.colored_label(
                        egui::Color32::from_rgb(34, 197, 94),
                        "■ Green (#22C55E): Optimal Pad < 5°",
                    );
                    ui.colored_label(
                        egui::Color32::from_rgb(245, 158, 11),
                        "■ Amber (#F59E0B): Caution 5°–15°",
                    );
                    ui.colored_label(
                        egui::Color32::from_rgb(239, 68, 68),
                        "■ Red (#EF4444): Hazard > 15°",
                    );
                });
            }
        });

    ui.add_space(6.0);

    // -------------------------------------------------------------------------
    // Card B: Scientific Mission Engineering Indicator (Landing Safety & Cold-Trap Proxy)
    // -------------------------------------------------------------------------
    let prim_summary = app_state.loaded_primary_summary;
    egui::Frame::none()
        .fill(COLOR_CARD_BG)
        .inner_margin(egui::Margin::same(10.0))
        .rounding(egui::Rounding::same(6.0))
        .stroke(egui::Stroke::new(
            1.2,
            egui::Color32::from_rgb(34, 197, 94),
        ))
        .show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label(
                    egui::RichText::new("Mission Engineering: Landing Safety & Cold-Trap Proxy")
                        .strong()
                        .color(egui::Color32::from_rgb(74, 222, 128)),
                );
                if *skin_mode != SurfaceSkinMode::LandingSlopeHazard {
                    if ui
                        .small_button("Visualize Slope Hazard Map")
                        .on_hover_text("Switch 3D skin to Landing Slope Hazard (<5° / 15°)")
                        .clicked()
                    {
                        *skin_mode = SurfaceSkinMode::LandingSlopeHazard;
                        app_state.show_checkerboard_uv = false;
                    }
                }
            });

            ui.add_space(4.0);
            let safe_15_frac = (prim_summary.safe_landing_pct_under_15deg / 100.0_f32).clamp(0.0, 1.0);
            ui.add(
                egui::ProgressBar::new(safe_15_frac)
                    .text(format!(
                        "Safe Landing Area (θ < 15°): {:.1}% of surface",
                        prim_summary.safe_landing_pct_under_15deg
                    ))
                    .fill(egui::Color32::from_rgb(245, 158, 11)),
            );

            let opt_5_frac = (prim_summary.optimal_pad_pct_under_5deg / 100.0_f32).clamp(0.0, 1.0);
            ui.add(
                egui::ProgressBar::new(opt_5_frac)
                    .text(format!(
                        "Optimal Habitat Pad (θ < 5°): {:.1}% of surface",
                        prim_summary.optimal_pad_pct_under_5deg
                    ))
                    .fill(egui::Color32::from_rgb(34, 197, 94)),
            );

            ui.add_space(4.0);
            ui.monospace(format!(
                "Mean Slope θ̄: {:.2}° | Persistent Basins β₀: {} | Euler χ: {}",
                prim_summary.mean_slope_deg,
                prim_summary.persistent_basins_b0,
                prim_summary.euler_characteristic
            ));
            ui.label(
                egui::RichText::new(
                    "Mission Utility: Identifies level touchdown ellipses (θ < 15°) and pressurized habitat foundations (θ < 5°) while β₀ basins locate candidate Permanently Shadowed Region (PSR) ice cold-traps for Artemis (Lunar South Pole) and radiation-shielded depressions for Mars.",
                )
                .small()
                .color(COLOR_TEXT_SECONDARY),
            );
        });

    ui.add_space(6.0);

    ui.label("Vertical Exaggeration (scales Bevy +Y up axis only):");
    ui.add(
        egui::Slider::new(&mut app_state.vertical_exaggeration, 0.25..=10.0)
            .text("Y-scale")
            .step_by(0.05),
    );

    ui.add_space(4.0);
    ui.checkbox(
        &mut app_state.show_checkerboard_uv,
        "Checkerboard UV Alignment Test (NW = u:0,v:0 | SE = u:1,v:1)",
    );

    ui.add_space(6.0);
    if let Ok((mut cam, mut proj)) = camera_query.get_single_mut() {
        ui.checkbox(
            &mut cam.auto_rotate,
            "Auto-Rotate Terrain Orbit [Shortcut: A] (pauses while dragging)",
        );
        ui.add(
            egui::Slider::new(&mut cam.auto_rotate_speed, 0.05..=1.2)
                .text("Orbit Speed (rad/s)"),
        );

        ui.add_space(6.0);
        ui.label(egui::RichText::new("No-Mouse / Touchpad Camera Controls:").strong());

        let step_angle = 0.22_f32;
        ui.horizontal_wrapped(|ui| {
            if ui.button("< Orbit Left").clicked() {
                cam.target_yaw += step_angle;
            }
            if ui.button("Orbit Right >").clicked() {
                cam.target_yaw -= step_angle;
            }
            if ui.button("^ Tilt Up").clicked() {
                cam.target_pitch = (cam.target_pitch + 0.14).clamp(0.05, 1.52);
            }
            if ui.button("v Tilt Down").clicked() {
                cam.target_pitch = (cam.target_pitch - 0.14).clamp(0.05, 1.52);
            }
        });

        ui.horizontal_wrapped(|ui| {
            if ui.button("[+] Zoom In").clicked() {
                let min_r = cam.min_radius;
                let max_r = cam.max_radius;
                cam.target_radius = (cam.target_radius * 0.80).clamp(min_r, max_r);
            }
            if ui.button("[-] Zoom Out").clicked() {
                let min_r = cam.min_radius;
                let max_r = cam.max_radius;
                cam.target_radius = (cam.target_radius * 1.25).clamp(min_r, max_r);
            }
            if ui.button("[R] Reset & Center 3D Camera").clicked() {
                cam.frame_terrain(Vec3::ZERO, app_state.loaded_primary_bounding_radius);
                cam.apply_perspective_clip_planes(&mut proj);
            }
        });
    }

    ui.add_space(8.0);
    egui::Frame::none()
        .fill(COLOR_CARD_BG)
        .inner_margin(egui::Margin::same(10.0))
        .rounding(egui::Rounding::same(6.0))
        .stroke(egui::Stroke::new(1.0, COLOR_BORDER))
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new("Coordinate System & UV Laws (AGENTS.md Section 2)")
                    .strong()
                    .color(egui::Color32::from_rgb(125, 211, 252)),
            );
            ui.label(
                egui::RichText::new(
                    "• Blender / JMARS Z-up STL centered in f64 to (0,0,0) and mapped to Bevy Y-up: X_bevy = X_stl, Y_bevy = Z_stl (Up), Z_bevy = −Y_stl (South).\n\
                     • Planar UVs from centered DemExtent: u = (X − min_x)/(max_x − min_x), v = (max_y − Y)/(max_y − min_y).\n\
                     • Mouse / Touchpad: Left-drag = Orbit | Right-drag or Shift+Left-drag = Pan | Scroll = Zoom.",
                )
                .small()
                .color(COLOR_TEXT_SECONDARY),
            );
        });
}

/// Tab 3: Similarity Charts, Collapsible Mathematical Foundation, and Interactive Weights.
fn render_tab_charts_and_math(
    ui: &mut egui::Ui,
    app_state: &mut ViewerAppState,
    net: &NetBridge,
) {
    ui.heading("3. Similarity Charts, Math & Weights");
    ui.add_space(4.0);

    render_similarity_charts_and_math(ui, app_state);

    ui.add_space(8.0);
    ui.separator();
    ui.label(
        egui::RichText::new("Interactive Morphometric Feature Weights [0.0 .. 1.0]:").strong(),
    );

    let weight_keys: Vec<String> = app_state.active_weights.keys().cloned().collect();
    let mut weights_changed = false;

    for key in weight_keys {
        if let Some(val) = app_state.active_weights.get_mut(&key) {
            let resp = ui.add(
                egui::Slider::new(val, 0.0..=1.0)
                    .text(&key)
                    .step_by(0.01),
            );
            if resp.changed() {
                weights_changed = true;
            }
        }
    }

    if weights_changed {
        let recalc = recalculate_catalog(&app_state.catalog, &app_state.active_weights);
        app_state.ranked_scores = recalc.results;
    }

    ui.horizontal_wrapped(|ui| {
        if ui.button("Sync Server (/api/recalculate)").clicked() {
            net.recalculate_scores(app_state.active_weights.clone());
        }
        if ui.button("Reset Default Weights").clicked() {
            app_state.active_weights = app_state.catalog.default_weights.clone();
            let recalc = recalculate_catalog(&app_state.catalog, &app_state.active_weights);
            app_state.ranked_scores = recalc.results;
        }
        if app_state.authenticated
            && ui
                .button(
                    egui::RichText::new("Save Canonical (PUT /api/weights)")
                        .color(egui::Color32::from_rgb(74, 222, 128)),
                )
                .clicked()
        {
            net.save_canonical_weights(app_state.active_weights.clone());
        }
    });
}

/// Helper to render a single mathematical equation card using clean Unicode symbols,
/// cyan/amber `TextStyle::Monospace` formula boxes, and indented English variable explanations.
fn render_unicode_equation_card(
    ui: &mut egui::Ui,
    id_source: &str,
    title: &str,
    title_color: egui::Color32,
    formula: &str,
    formula_color: egui::Color32,
    variables: &[(&str, &str)],
) {
    egui::Frame::none()
        .fill(COLOR_CARD_BG)
        .inner_margin(egui::Margin::same(10.0))
        .rounding(egui::Rounding::same(6.0))
        .stroke(egui::Stroke::new(1.0, COLOR_BORDER))
        .show(ui, |ui| {
            ui.label(egui::RichText::new(title).strong().color(title_color));
            ui.add_space(4.0);

            // Highlighted Monospace Equation Banner
            egui::Frame::none()
                .fill(egui::Color32::from_rgb(9, 14, 24))
                .inner_margin(egui::Margin::symmetric(10.0, 7.0))
                .rounding(egui::Rounding::same(4.0))
                .stroke(egui::Stroke::new(1.0, formula_color))
                .show(ui, |ui| {
                    ui.label(
                        egui::RichText::new(formula)
                            .text_style(egui::TextStyle::Monospace)
                            .strong()
                            .color(formula_color),
                    );
                });

            ui.add_space(4.0);
            ui.indent(id_source, |ui| {
                for (symbol, explanation) in variables {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(
                            egui::RichText::new(*symbol)
                                .text_style(egui::TextStyle::Monospace)
                                .strong()
                                .color(egui::Color32::from_rgb(125, 211, 252)),
                        );
                        ui.label(
                            egui::RichText::new(format!("— {explanation}"))
                                .small()
                                .color(COLOR_TEXT_SECONDARY),
                        );
                    });
                }
            });
        });
}

/// Draws custom `egui::Painter` multi-bar charts, overall catalog analog ranking bars, and the
/// collapsible **"Mathematical Foundation & TDA Equations"** reference card.
fn render_similarity_charts_and_math(ui: &mut egui::Ui, app_state: &ViewerAppState) {
    if let Some(sim) = app_state.build_pairwise_similarity() {
        egui::Frame::none()
            .fill(COLOR_CARD_BG)
            .inner_margin(egui::Margin::same(10.0))
            .rounding(egui::Rounding::same(6.0))
            .stroke(egui::Stroke::new(1.0, COLOR_BORDER))
            .show(ui, |ui| {
                ui.label(
                    egui::RichText::new(format!(
                        "Pairwise Feature Chart: {} [{}] ↔ {} [{}] ({:.1}% Match)",
                        sim.primary_name,
                        sim.primary_body.as_str().to_uppercase(),
                        sim.compare_name,
                        sim.compare_body.as_str().to_uppercase(),
                        sim.overall_similarity_pct
                    ))
                    .strong()
                    .color(egui::Color32::from_rgb(125, 211, 252)),
                );

                ui.horizontal_wrapped(|ui| {
                    ui.colored_label(
                        egui::Color32::from_rgb(56, 189, 248),
                        format!("[#] Primary: {}", sim.primary_name),
                    );
                    ui.colored_label(
                        egui::Color32::from_rgb(250, 204, 21),
                        format!("[#] Partner: {}", sim.compare_name),
                    );
                    ui.colored_label(
                        egui::Color32::from_rgb(16, 185, 129),
                        "[#] Dimension Similarity (%)",
                    );
                });

                ui.add_space(4.0);

                let chart_w = ui.available_width().max(260.0);
                let row_h = 40.0;
                let chart_h = row_h * (sim.dimensions.len() as f32) + 12.0;
                let (rect, _) =
                    ui.allocate_exact_size(egui::vec2(chart_w, chart_h), egui::Sense::hover());
                let painter = ui.painter_at(rect);

                painter.rect_filled(
                    rect,
                    egui::Rounding::same(4.0),
                    egui::Color32::from_rgb(9, 14, 24),
                );

                let label_col_w = 132.0_f32.min(chart_w * 0.40);
                let bar_area_w = (chart_w - label_col_w - 64.0).max(80.0);

                for i in 0..=4 {
                    let frac = i as f32 * 0.25;
                    let gx = rect.min.x + label_col_w + bar_area_w * frac;
                    painter.line_segment(
                        [egui::pos2(gx, rect.min.y + 4.0), egui::pos2(gx, rect.max.y - 4.0)],
                        egui::Stroke::new(1.0, egui::Color32::from_rgb(30, 41, 59)),
                    );
                }

                for (idx, (label, va, vb, sim_pct)) in sim.dimensions.iter().enumerate() {
                    let y0 = rect.min.y + 6.0 + (idx as f32) * row_h;
                    painter.text(
                        egui::pos2(rect.min.x + 6.0, y0 + 10.0),
                        egui::Align2::LEFT_CENTER,
                        label,
                        egui::FontId::proportional(12.0),
                        COLOR_TEXT_PRIMARY,
                    );

                    let bx = rect.min.x + label_col_w;
                    let w_a = (*va as f32).clamp(0.0, 1.0) * bar_area_w;
                    let w_b = (*vb as f32).clamp(0.0, 1.0) * bar_area_w;
                    let w_sim = ((*sim_pct / 100.0) as f32).clamp(0.0, 1.0) * bar_area_w;

                    painter.rect_filled(
                        egui::Rect::from_min_size(egui::pos2(bx, y0 + 2.0), egui::vec2(w_a, 8.0)),
                        egui::Rounding::same(2.0),
                        egui::Color32::from_rgb(56, 189, 248),
                    );
                    painter.rect_filled(
                        egui::Rect::from_min_size(egui::pos2(bx, y0 + 12.0), egui::vec2(w_b, 8.0)),
                        egui::Rounding::same(2.0),
                        egui::Color32::from_rgb(250, 204, 21),
                    );
                    painter.rect_filled(
                        egui::Rect::from_min_size(
                            egui::pos2(bx, y0 + 22.0),
                            egui::vec2(w_sim, 8.0),
                        ),
                        egui::Rounding::same(2.0),
                        egui::Color32::from_rgb(16, 185, 129),
                    );

                    painter.text(
                        egui::pos2(bx + bar_area_w + 6.0, y0 + 16.0),
                        egui::Align2::LEFT_CENTER,
                        format!("{sim_pct:.0}%"),
                        egui::FontId::monospace(12.0),
                        egui::Color32::from_rgb(167, 243, 208),
                    );
                }
            });
    }

    // Overall Catalog Ranking Chart
    ui.add_space(6.0);
    egui::Frame::none()
        .fill(COLOR_CARD_BG)
        .inner_margin(egui::Margin::same(10.0))
        .rounding(egui::Rounding::same(6.0))
        .stroke(egui::Stroke::new(1.0, COLOR_BORDER))
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new("Catalog Analog Score Ranking (0% .. 100%)")
                    .strong()
                    .color(COLOR_TEXT_PRIMARY),
            );
            for (rank, scored) in app_state.ranked_scores.iter().enumerate() {
                let pct = (scored.score * 100.0).clamp(0.0, 100.0);
                let is_prim = scored.site_id == app_state.primary_site_id;
                let bar_color = if is_prim {
                    egui::Color32::from_rgb(56, 189, 248)
                } else {
                    egui::Color32::from_rgb(71, 85, 105)
                };
                ui.add(
                    egui::ProgressBar::new((pct / 100.0) as f32)
                        .text(format!(
                            "#{}: {} [{}] — {:.1}% (canonical {:.1}%)",
                            rank + 1,
                            scored.name,
                            scored.body.as_str().to_uppercase(),
                            pct,
                            scored.canonical_score * 100.0
                        ))
                        .fill(bar_color),
                );
            }
        });

    // Collapsible Mathematical Foundation & TDA Equations Card (Styled Unicode Equation Cards)
    ui.add_space(6.0);
    egui::CollapsingHeader::new(
        egui::RichText::new("Mathematical Foundation & TDA Equations (Unicode Reference)")
            .strong()
            .color(egui::Color32::from_rgb(125, 211, 252)),
    )
    .default_open(true)
    .show(ui, |ui| {
        let cyan = egui::Color32::from_rgb(56, 189, 248);
        let amber = egui::Color32::from_rgb(250, 204, 21);
        let green = egui::Color32::from_rgb(74, 222, 128);

        render_unicode_equation_card(
            ui,
            "eq_weighted_similarity",
            "1. Weighted Morphometric Analog Similarity (S_analog)",
            green,
            "S_analog(A, B) = ∑ᵢ wᵢ · (1 − |fᵢ(A) − fᵢ(B)|)     where ∑ᵢ wᵢ = 1",
            cyan,
            &[
                ("S_analog(A, B)", "Pairwise planetary analog similarity in [0, 1] (Earth ↔ Moon/Mars)."),
                ("wᵢ", "Normalized researcher weight for morphometric dimension i (∑ᵢ wᵢ = 1)."),
                ("fᵢ(A), fᵢ(B)", "Pre-normalized feature scores in [0, 1] computed from meter-projected DEMs."),
                ("|fᵢ(A) − fᵢ(B)|", "Absolute feature discrepancy between Primary site A and Partner site B."),
            ],
        );

        ui.add_space(6.0);
        render_unicode_equation_card(
            ui,
            "eq_relief_hypsometric",
            "2. Elevation Relief (Δz) & Hypsometric Integral (HI)",
            green,
            "Δz = z_max − z_min     |     HI = (z_mean − z_min) / (z_max − z_min)",
            cyan,
            &[
                ("Δz", "Total vertical relief in meters between peak elevation z_max and basin floor z_min."),
                ("z_mean", "Mean surface elevation across all welded mesh vertices or DEM cells."),
                ("HI ≈ 0.5", "Hypsometric Integral measuring basin vs. highland mass distribution in [0, 1]."),
            ],
        );

        ui.add_space(6.0);
        render_unicode_equation_card(
            ui,
            "eq_tri_roughness",
            "3. Terrain Ruggedness Index (TRI) & Surface Slope Safety (θ)",
            amber,
            "TRI = √( (1/8) · ∑ₙ₌₁..₈ (zₙ − z₀)² )     |     θ = arctan( ‖∇z‖ ) = arccos( n_y )",
            amber,
            &[
                ("z₀, zₙ", "Center cell elevation z₀ and its 8 immediate 3×3 neighbors zₙ (meters)."),
                ("∇z", "Horizontal surface gradient (∂z/∂x, ∂z/∂y) in local meter projection (+units=m)."),
                ("θ ≤ 5° / 15°", "Local slope angle from vertical normal n_y: θ < 5° optimal habitat pad, θ < 15° safe lander touchdown."),
            ],
        );

        ui.add_space(6.0);
        render_unicode_equation_card(
            ui,
            "eq_tda_homology",
            "4. 0D Persistent Homology (β₀) & Simplicial Euler Characteristic (χ)",
            amber,
            "χ = V − E + F     |     Persistence  pₖ = |dₖ − bₖ| ≥ τ_noise",
            amber,
            &[
                ("β₀", "0D Betti number counting persistent topological basins (craters/cold-traps) and peaks."),
                ("(bₖ, dₖ)", "Component birth height bₖ and merge death height dₖ tracked via Union-Find Elder Rule."),
                ("τ_noise", "Persistence threshold (5% of Δz) filtering micro-roughness from true craters/ridges."),
                ("χ = V − E + F", "Euler characteristic (Vertices − Edges + Faces); Δβ₀ and Δχ measure topological match."),
            ],
        );
    });
}

/// Tab 4: Lunar & Martian Settlement Engineering Guide (5 critical habitat needs & how Space-App-X86 solves them).
fn render_tab_settlement_guide(ui: &mut egui::Ui) {
    ui.heading("4. Lunar & Martian Settlement Engineering Guide");
    ui.label(
        egui::RichText::new(
            "How Space-App-X86's 3D morphometry, TDA invariants, and real-time telemetry solve the 5 core engineering requirements for human habitats on the Moon (Artemis South Pole) and Mars:",
        )
        .small()
        .color(COLOR_TEXT_SECONDARY),
    );
    ui.add_space(6.0);

    let items = [
        (
            "1. Landing & Habitat Slope Stability (θ < 5° .. 15°)",
            "Challenge: Lunar and Martian landers risk dynamic rollover on slopes exceeding 10°–15°, and pressurized habitat modules require stable, low-gradient foundations (θ < 5°).",
            "Solved by: Slope Safety & meter-projected DEM gradient analysis (θ = arctan(‖∇z‖) = arccos(n_y)), live Landing Slope Hazard 3D skin, and automated Safe Landing Area % (< 15° and < 5°).",
        ),
        (
            "2. Solar Power & Cryogenic Volatile Access (Lunar South Pole)",
            "Challenge: Equatorial lunar nights last 354 hours (~100 K), whereas South Pole crater rims ('peaks of eternal light') receive >80% sunlight right next to ~40 K Permanently Shadowed Regions (PSRs) rich in water ice.",
            "Solved by: Elevation Relief (Δz), Hypsometric Integral (HI), and the live Planetary Telemetry HUD (tracking Lunar Solar Time, synodic illumination %, and surface thermal range).",
        ),
        (
            "3. Rover Trafficability & Crew EVA Mobility",
            "Challenge: High boulder density and sharp decameter-scale ejecta ridges cause wheel slip, chassis high-centering, and elevated metabolic/battery energy consumption during EVAs.",
            "Solved by: Terrain Ruggedness Index (TRI = √((1/8) · ∑(zₙ − z₀)²)) & Roughness Similarity, quantifying local root-mean-square elevation steps across the entire site.",
        ),
        (
            "4. Natural Radiation & Micrometeorite Shielding",
            "Challenge: Outside Earth's magnetosphere, Galactic Cosmic Rays (GCR), Solar Particle Events (SPE), and secondary regolith ejecta require subterranean lava tubes, pit craters, or enclosed basin berms.",
            "Solved by: 0D Persistent Homology (β₀ sublevel filtration via Union-Find Elder Rule) & Euler Characteristic (χ = V − E + F), automatically counting persistent closed basins and depressions for natural regolith shielding.",
        ),
        (
            "5. Pre-Flight Terrestrial Analog Validation (Earth ↔ Moon/Mars)",
            "Challenge: Every habitat deployment mechanism, ISRU drill, and autonomous rover navigation stack must be field-tested on Earth in terrains that match target extraterrestrial geomorphology.",
            "Solved by: Cross-Body Analog Similarity Ranking (S_analog) & synchronized Split-Screen 3D Comparison, pairing terrestrial volcanic/impact sites directly with Lunar and Martian targets.",
        ),
    ];

    for (title, challenge, solution) in items {
        egui::Frame::none()
            .fill(COLOR_CARD_BG)
            .inner_margin(egui::Margin::same(10.0))
            .rounding(egui::Rounding::same(6.0))
            .stroke(egui::Stroke::new(1.0, COLOR_BORDER))
            .show(ui, |ui| {
                ui.label(
                    egui::RichText::new(title)
                        .strong()
                        .color(egui::Color32::from_rgb(125, 211, 252)),
                );
                ui.label(
                    egui::RichText::new(challenge)
                        .small()
                        .color(COLOR_TEXT_SECONDARY),
                );
                ui.add_space(2.0);
                ui.label(
                    egui::RichText::new(solution)
                        .small()
                        .strong()
                        .color(egui::Color32::from_rgb(74, 222, 128)),
                );
            });
        ui.add_space(6.0);
    }
}

/// Tab 5: Researcher 512 MiB Binary Terrain Upload, TDA Report, and Workspace Reset / Demo Restore.
fn render_tab_upload_and_reset(
    ui: &mut egui::Ui,
    app_state: &mut ViewerAppState,
    net: &NetBridge,
) {
    ui.heading("5. Researcher Upload, TDA & Workspace Reset");
    ui.add_space(4.0);

    if !app_state.authenticated {
        egui::Frame::none()
            .fill(COLOR_CARD_BG)
            .inner_margin(egui::Margin::same(12.0))
            .rounding(egui::Rounding::same(6.0))
            .stroke(egui::Stroke::new(1.0, COLOR_BORDER))
            .show(ui, |ui| {
                ui.label(
                    egui::RichText::new("Researcher Authentication Required")
                        .strong()
                        .color(egui::Color32::from_rgb(250, 204, 21)),
                );
                ui.label(
                    "Log in with Researcher credentials to upload 50-100+ MiB .stl/.tif/.png files via binary multipart/form-data, run 0D Persistent Homology TDA, or clear/reset preloaded catalog maps.",
                );
                ui.add_space(6.0);
                if ui
                    .add(
                        egui::Button::new(
                            egui::RichText::new("Open Researcher Login")
                                .strong()
                                .color(egui::Color32::WHITE),
                        )
                        .fill(egui::Color32::from_rgb(37, 99, 235)),
                    )
                    .clicked()
                {
                    app_state.show_login_modal = true;
                }
            });
        return;
    }

    // -------------------------------------------------------------------------
    // Section A: Workspace Catalog Reset (Clear Preloaded Maps / Restore Demo)
    // -------------------------------------------------------------------------
    egui::Frame::none()
        .fill(COLOR_CARD_BG)
        .inner_margin(egui::Margin::same(10.0))
        .rounding(egui::Rounding::same(6.0))
        .stroke(egui::Stroke::new(
            1.2,
            egui::Color32::from_rgb(248, 113, 113),
        ))
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new("Workspace Catalog Management (POST /api/sites/reset)")
                    .strong()
                    .color(egui::Color32::from_rgb(252, 165, 165)),
            );
            ui.label(
                egui::RichText::new(
                    "Clear all preloaded maps from data/processed/catalog.json so only your newly uploaded terrains are displayed, or restore the synthetic UV reference site (test_site).",
                )
                .small()
                .color(COLOR_TEXT_SECONDARY),
            );
            ui.add_space(6.0);
            ui.horizontal_wrapped(|ui| {
                if ui
                    .add(
                        egui::Button::new(
                            egui::RichText::new("Reset Workspace (Clear All Preloaded Maps)")
                                .strong()
                                .color(egui::Color32::WHITE),
                        )
                        .fill(egui::Color32::from_rgb(185, 28, 28)),
                    )
                    .on_hover_text(
                        "Empties catalog.json on the server so only your newly uploaded maps appear",
                    )
                    .clicked()
                {
                    net.reset_catalog(false);
                }

                if ui
                    .add(
                        egui::Button::new(
                            egui::RichText::new("Restore Reference Demo Site")
                                .strong()
                                .color(COLOR_TEXT_PRIMARY),
                        )
                        .fill(egui::Color32::from_rgb(30, 41, 59)),
                    )
                    .on_hover_text("Restores the synthetic test_site in catalog.json")
                    .clicked()
                {
                    net.reset_catalog(true);
                }
            });
        });

    ui.add_space(8.0);

    // -------------------------------------------------------------------------
    // Section B: Binary Multipart Terrain Upload & Automated TDA
    // -------------------------------------------------------------------------
    egui::Frame::none()
        .fill(COLOR_CARD_BG)
        .inner_margin(egui::Margin::same(10.0))
        .rounding(egui::Rounding::same(6.0))
        .stroke(egui::Stroke::new(1.0, COLOR_BORDER))
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new(
                    "Upload 3D Mesh (.stl), DEM (.tif/.tiff) & Texture (.png) [Max 512 MiB]",
                )
                .strong()
                .color(egui::Color32::from_rgb(74, 222, 128)),
            );
            ui.label(
                egui::RichText::new(
                    "Raw binary multipart/form-data (zero Base64). Meshes >50,000 triangles are automatically decimated via 3D vertex clustering and centered to (0,0,0) in f64.",
                )
                .small()
                .color(COLOR_TEXT_SECONDARY),
            );
            ui.add_space(6.0);

            ui.horizontal_wrapped(|ui| {
                if ui.button("Select .STL").clicked() {
                    open_browser_file_dialog(".stl");
                }
                if ui.button("Select .TIF/.TIFF").clicked() {
                    open_browser_file_dialog(".tif,.tiff");
                }
                if ui.button("Select .PNG").clicked() {
                    open_browser_file_dialog(".png");
                }
                if ui.button("Select Any (.stl,.tif,.png)").clicked() {
                    open_browser_file_dialog(".stl,.tif,.tiff,.png");
                }
            });

            egui::Grid::new("upload_files_status_grid")
                .num_columns(2)
                .spacing([8.0, 4.0])
                .show(ui, |ui| {
                    ui.label("3D Mesh (.stl):");
                    if let (Some(name), Some(bytes)) = (
                        app_state.upload_form.stl_file_name.clone(),
                        app_state.upload_form.stl_bytes.as_ref(),
                    ) {
                        let size_str = TerrainUploadForm::format_mib(bytes.len());
                        ui.horizontal(|ui| {
                            ui.colored_label(
                                egui::Color32::from_rgb(74, 222, 128),
                                format!("{name} ({size_str})"),
                            );
                            if ui.small_button("x").clicked() {
                                app_state.upload_form.stl_file_name = None;
                                app_state.upload_form.stl_bytes = None;
                            }
                        });
                    } else {
                        ui.label(
                            egui::RichText::new("None selected").color(COLOR_TEXT_SECONDARY),
                        );
                    }
                    ui.end_row();

                    ui.label("Elevation (.tif):");
                    if let (Some(name), Some(bytes)) = (
                        app_state.upload_form.tif_file_name.clone(),
                        app_state.upload_form.tif_bytes.as_ref(),
                    ) {
                        let size_str = TerrainUploadForm::format_mib(bytes.len());
                        ui.horizontal(|ui| {
                            ui.colored_label(
                                egui::Color32::from_rgb(74, 222, 128),
                                format!("{name} ({size_str})"),
                            );
                            if ui.small_button("x").clicked() {
                                app_state.upload_form.tif_file_name = None;
                                app_state.upload_form.tif_bytes = None;
                            }
                        });
                    } else {
                        ui.label(
                            egui::RichText::new("None selected").color(COLOR_TEXT_SECONDARY),
                        );
                    }
                    ui.end_row();

                    ui.label("Texture (.png):");
                    if let (Some(name), Some(bytes)) = (
                        app_state.upload_form.png_file_name.clone(),
                        app_state.upload_form.png_bytes.as_ref(),
                    ) {
                        let size_str = TerrainUploadForm::format_mib(bytes.len());
                        ui.horizontal(|ui| {
                            ui.colored_label(
                                egui::Color32::from_rgb(74, 222, 128),
                                format!("{name} ({size_str})"),
                            );
                            if ui.small_button("x").clicked() {
                                app_state.upload_form.png_file_name = None;
                                app_state.upload_form.png_bytes = None;
                            }
                        });
                    } else {
                        ui.label(
                            egui::RichText::new("Auto-generate hypsometric PNG")
                                .color(COLOR_TEXT_SECONDARY),
                        );
                    }
                    ui.end_row();
                });

            let total_bytes = app_state.upload_form.total_payload_bytes();
            if total_bytes > 0 {
                ui.monospace(format!(
                    "Total binary multipart payload: {}",
                    TerrainUploadForm::format_mib(total_bytes)
                ));
            }

            ui.add_space(6.0);
            egui::Grid::new("upload_meta_grid")
                .num_columns(2)
                .spacing([8.0, 5.0])
                .show(ui, |ui| {
                    ui.label("Site Name:");
                    ui.text_edit_singleline(&mut app_state.upload_form.site_name);
                    ui.end_row();

                    ui.label("Site ID:");
                    ui.text_edit_singleline(&mut app_state.upload_form.site_id);
                    ui.end_row();

                    ui.label("Origin Body:");
                    egui::ComboBox::from_id_source("upload_source_body_combo")
                        .selected_text(app_state.upload_form.source_body.as_str().to_uppercase())
                        .show_ui(ui, |ui| {
                            ui.selectable_value(
                                &mut app_state.upload_form.source_body,
                                Body::Earth,
                                "EARTH (Terrestrial Analog)",
                            );
                            ui.selectable_value(
                                &mut app_state.upload_form.source_body,
                                Body::Moon,
                                "MOON (Lunar Terrain)",
                            );
                            ui.selectable_value(
                                &mut app_state.upload_form.source_body,
                                Body::Mars,
                                "MARS (Martian Terrain)",
                            );
                        });
                    ui.end_row();

                    ui.label("DEM Cell Size (m):");
                    ui.add(
                        egui::DragValue::new(&mut app_state.upload_form.horizontal_scale_m)
                            .speed(0.5)
                            .range(0.01..=10_000.0),
                    );
                    ui.end_row();

                    ui.label("Notes:");
                    ui.text_edit_singleline(&mut app_state.upload_form.description);
                    ui.end_row();
                });

            ui.add_space(6.0);
            let can_upload =
                app_state.upload_form.has_terrain_file() && !app_state.upload_form.is_uploading;
            ui.add_enabled_ui(can_upload, |ui| {
                let btn_label = if app_state.upload_form.is_uploading {
                    "Uploading & Computing TDA..."
                } else {
                    "Analyze Terrain & Register in 3D Catalog (POST /api/sites/upload)"
                };
                if ui
                    .add_sized(
                        [ui.available_width(), 32.0],
                        egui::Button::new(
                            egui::RichText::new(btn_label)
                                .strong()
                                .color(egui::Color32::WHITE),
                        )
                        .fill(egui::Color32::from_rgb(22, 163, 74)),
                    )
                    .clicked()
                {
                    app_state.upload_form.is_uploading = true;
                    app_state.status_message = format!(
                        "Uploading {} via binary multipart/form-data & running TDA...",
                        TerrainUploadForm::format_mib(total_bytes)
                    );
                    let req = app_state.upload_form.to_upload_request();
                    net.upload_and_analyze_site(req);
                }
            });
        });

    if let Some(report) = &app_state.last_analysis_report {
        ui.add_space(8.0);
        egui::Frame::none()
            .fill(COLOR_CARD_BG)
            .inner_margin(egui::Margin::same(10.0))
            .rounding(egui::Rounding::same(6.0))
            .stroke(egui::Stroke::new(
                1.0,
                egui::Color32::from_rgb(56, 189, 248),
            ))
            .show(ui, |ui| {
                ui.label(
                    egui::RichText::new(format!(
                        "TDA Proposal: {} Analog ({:.1}% | Closest: {} [{}], {:.1}%)",
                        report.proposal.proposed_body.as_str().to_uppercase(),
                        report.proposal.analog_score * 100.0,
                        report.proposal.closest_site_name,
                        report.proposal.closest_site_body.as_str().to_uppercase(),
                        report.proposal.similarity_score * 100.0
                    ))
                    .strong()
                    .color(egui::Color32::from_rgb(125, 211, 252)),
                );
                if report.original_triangle_count > report.simplified_triangle_count {
                    ui.colored_label(
                        egui::Color32::from_rgb(250, 204, 21),
                        format!(
                            "Auto-simplified mesh: {} -> {} triangles (<= 50,000 budget)",
                            report.original_triangle_count, report.simplified_triangle_count
                        ),
                    );
                }
                ui.monospace(format!(
                    "Relief dz: {:.1}m | Slope mean: {:.2} deg | TRI: {:.2}m | HI: {:.3}",
                    report.morphometry.elevation_range_m,
                    report.morphometry.slope_mean_deg,
                    report.morphometry.tri_mean_m,
                    report.morphometry.hypsometric_integral
                ));
                ui.monospace(format!(
                    "Euler chi: {} | b0 Basins: {} | b0 Peaks: {}",
                    report.topology.euler_characteristic,
                    report.topology.persistent_basins_b0,
                    report.topology.persistent_peaks_b0
                ));
                ui.label(
                    egui::RichText::new(&report.proposal.justification)
                        .small()
                        .color(COLOR_TEXT_SECONDARY),
                );
            });
    }

    ui.add_space(8.0);
    egui::CollapsingHeader::new("Manual Site Metadata Editor (POST /api/sites)")
        .default_open(false)
        .show(ui, |ui| {
            egui::Grid::new("site_editor_grid")
                .num_columns(2)
                .spacing([8.0, 4.0])
                .show(ui, |ui| {
                    ui.label("Site ID:");
                    ui.text_edit_singleline(&mut app_state.site_editor.id);
                    ui.end_row();

                    ui.label("Name:");
                    ui.text_edit_singleline(&mut app_state.site_editor.name);
                    ui.end_row();

                    ui.label("Triangles (<=50k):");
                    ui.add(
                        egui::DragValue::new(&mut app_state.site_editor.triangle_count)
                            .range(1..=50_000),
                    );
                    ui.end_row();

                    ui.label("Mesh Path:");
                    ui.text_edit_singleline(&mut app_state.site_editor.mesh_path);
                    ui.end_row();

                    ui.label("Texture Path:");
                    ui.text_edit_singleline(&mut app_state.site_editor.texture_path);
                    ui.end_row();
                });

            if ui.button("Save Site Metadata (POST /api/sites)").clicked() {
                let metadata = app_state.site_editor.to_site_metadata();
                net.save_site(metadata);
            }
        });
}
