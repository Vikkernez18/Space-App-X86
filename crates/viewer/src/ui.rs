// AI-assisted
//! `bevy_egui` side panel, startup mode selector, and Researcher Terrain Upload + TDA UI
//! (100% English, zero NASA logos).
//!
//! Provides:
//! - Startup mode selector: direct **Enter Public Demo Mode** button + **Researcher Login** (`POST /api/login`).
//! - Site selector for Lunar, Martian, and Earth terrestrial analog sites + optional side-by-side comparison site.
//! - Vertical exaggeration slider (`Transform.scale = Vec3::new(1.0, exaggeration, 1.0)`) and UV Checkerboard toggle.
//! - Real-time scientific weight sliders (`POST /api/recalculate`) and comparative Analog Scores table.
//! - Authenticated Researcher controls:
//!   - **Upload & Analyze Terrain (`.stl` / `.tif` / `.png`)** (`POST /api/sites/upload`) with browser file picker
//!     and drag-and-drop support, displaying the full morphometric ($\nabla z$, TRI, $HI$), 0D persistent homology
//!     ($\beta_0$ basins/peaks), and Euler characteristic ($\chi = V - E + F$) report.
//!   - Canonical weight persistence (`PUT /api/weights`) and manual site metadata editor (`POST /api/sites`).

use std::collections::BTreeMap;

use analog_core::scoring::recalculate_catalog;
use analog_core::stl::encode_binary_stl;
use analog_core::terrain_analysis::{
    TerrainAnalysisReport, UploadSiteRequest, base64_encode,
};
use analog_core::{Body, Catalog, DemExtent, ScoredSite, SiteMetadata};
use bevy::prelude::*;
use bevy_egui::{EguiContexts, egui};

use crate::camera::OrbitCamera;
use crate::mesh_loader::generate_synthetic_test_site_welded_mesh;
use crate::net::{
    BrowserSelectedFile, NetBridge, TerrainSlot, fallback_synthetic_catalog,
    open_browser_file_dialog,
};

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

/// Editable form state for uploading and analyzing `.stl` / `.tif` / `.png` terrain files (`POST /api/sites/upload`).
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
    pub stl_base64: Option<String>,
    pub tiff_filename: Option<String>,
    pub tiff_size_bytes: usize,
    pub tiff_base64: Option<String>,
    pub png_filename: Option<String>,
    pub png_size_bytes: usize,
    pub png_base64: Option<String>,
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
            stl_base64: None,
            tiff_filename: None,
            tiff_size_bytes: 0,
            tiff_base64: None,
            png_filename: None,
            png_size_bytes: 0,
            png_base64: None,
            is_uploading: false,
        }
    }
}

impl TerrainUploadForm {
    /// Assigns a selected or dropped file to the `.stl`, `.tif`/`.tiff`, or `.png` slot based on extension.
    pub fn ingest_selected_file(&mut self, file: BrowserSelectedFile) -> Result<String, String> {
        let lower = file.name.to_ascii_lowercase();
        if self.name.trim().is_empty() || self.name == "Uploaded Candidate Terrain" {
            if let Some(stem) = file.name.rsplit_once('.').map(|(s, _)| s)
                && !stem.trim().is_empty()
            {
                self.name = stem.replace('_', " ");
            }
        }

        if lower.ends_with(".stl") {
            let msg = format!("Staged STL mesh '{}' ({} KB)", file.name, file.size / 1024);
            self.stl_filename = Some(file.name);
            self.stl_size_bytes = file.size;
            self.stl_base64 = Some(file.base64);
            Ok(msg)
        } else if lower.ends_with(".tif") || lower.ends_with(".tiff") {
            let msg = format!(
                "Staged GeoTIFF DEM '{}' ({} KB)",
                file.name,
                file.size / 1024
            );
            self.tiff_filename = Some(file.name);
            self.tiff_size_bytes = file.size;
            self.tiff_base64 = Some(file.base64);
            Ok(msg)
        } else if lower.ends_with(".png") {
            let msg = format!(
                "Staged PNG texture '{}' ({} KB)",
                file.name,
                file.size / 1024
            );
            self.png_filename = Some(file.name);
            self.png_size_bytes = file.size;
            self.png_base64 = Some(file.base64);
            Ok(msg)
        } else {
            Err(format!(
                "Unsupported file '{}': please select a .stl, .tif/.tiff, or .png file.",
                file.name
            ))
        }
    }

    pub fn has_elevation_source(&self) -> bool {
        self.stl_base64.is_some() || self.tiff_base64.is_some()
    }

    pub fn clear_files(&mut self) {
        self.stl_filename = None;
        self.stl_size_bytes = 0;
        self.stl_base64 = None;
        self.tiff_filename = None;
        self.tiff_size_bytes = 0;
        self.tiff_base64 = None;
        self.png_filename = None;
        self.png_size_bytes = 0;
        self.png_base64 = None;
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
        self.stl_base64 = Some(base64_encode(&stl_bytes));
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
            stl_base64: self.stl_base64.clone(),
            tiff_base64: self.tiff_base64.clone(),
            png_base64: self.png_base64.clone(),
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

/// Global UI & application state resource for the viewer.
#[derive(Resource)]
pub struct ViewerAppState {
    /// Whether the user has dismissed the initial welcome/mode-selection dialog.
    pub mode_selected: bool,
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
    /// Optional secondary site ID for side-by-side 3D comparison.
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
            elevation_range_m: None,
            triangle_count: 8192,
            mesh_path: "test_site/terrain.stl".to_string(),
            mesh_format: None,
            texture_path: "test_site/checkerboard.png".to_string(),
            feature_scores: BTreeMap::new(),
            canonical_score: 0.8485,
            generated_by: None,
        });

        Self {
            mode_selected: false,
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
        } else if let Some(current) = catalog.sites.iter().find(|s| s.id == self.primary_site_id) {
            self.site_editor = SiteEditorForm::from_site(current);
        }

        self.catalog = catalog;
        self.pending_reload_primary = true;
    }
}

/// Renders the startup mode selection modal and the left side panel in `bevy_egui`.
pub fn viewer_ui_system(
    mut contexts: EguiContexts,
    mut app_state: ResMut<ViewerAppState>,
    net: Res<NetBridge>,
    mut cam_query: Query<&mut OrbitCamera>,
) {
    let ctx = contexts.ctx_mut();

    // Apply clean dark scientific theme
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = egui::Color32::from_rgba_unmultiplied(15, 20, 28, 242);
    ctx.set_visuals(visuals);

    // 1. Startup Mode Selector Modal (Enter Public Demo Mode vs Researcher Login)
    if !app_state.mode_selected {
        egui::Window::new("Space-App-X86 — Terrestrial Analogs 3D Platform")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .fixed_size(egui::Vec2::new(440.0, 360.0))
            .show(ctx, |ui| {
                ui.heading("Welcome to Space-App-X86");
                ui.label(
                    "Explore Lunar, Martian, and Earth terrestrial analog sites in 3D (.stl & .tif DEMs with planar DemExtent UV mapping and TDA scoring).",
                );
                ui.add_space(10.0);

                ui.group(|ui| {
                    ui.heading("1. Judges & Visitors (No Password Required)");
                    ui.label(
                        "Inspect 3D terrain meshes, verify NW/SE checkerboard UV alignment, and test custom scoring weights immediately.",
                    );
                    ui.add_space(6.0);
                    let demo_btn = egui::Button::new(
                        egui::RichText::new("Enter Public Demo Mode")
                            .strong()
                            .size(16.0)
                            .color(egui::Color32::from_rgb(10, 25, 30)),
                    )
                    .fill(egui::Color32::from_rgb(0, 210, 220))
                    .min_size(egui::Vec2::new(400.0, 38.0));

                    if ui.add(demo_btn).clicked() {
                        app_state.mode_selected = true;
                        app_state.error_message = None;
                        app_state.status_message =
                            "Entering Public Demo Mode (fetching /api/demo & /api/catalog)..."
                                .to_string();
                        net.fetch_demo_mode();
                    }
                });

                ui.add_space(10.0);

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
                            [400.0, 32.0],
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

    // 2. Left Side Panel
    egui::SidePanel::left("analog_control_panel")
        .default_width(420.0)
        .width_range(350.0..=560.0)
        .resizable(true)
        .show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.heading("Space-App-X86 Explorer");
                ui.label(
                    egui::RichText::new(
                        "Lunar & Martian Terrestrial Analogs 3D Viewer & TDA Analyzer",
                    )
                    .small()
                    .color(egui::Color32::from_gray(180)),
                );
                ui.separator();

                // Mode & Session Banner
                ui.horizontal(|ui| {
                    if app_state.authenticated {
                        let user = app_state
                            .researcher_username
                            .as_deref()
                            .unwrap_or("equipo_analog");
                        ui.colored_label(
                            egui::Color32::from_rgb(80, 230, 150),
                            format!("Mode: Researcher ({user})"),
                        );
                        if ui.small_button("Log Out").clicked() {
                            net.logout();
                        }
                    } else {
                        ui.colored_label(
                            egui::Color32::from_rgb(0, 210, 220),
                            "Mode: Public Demo (No Password)",
                        );
                        if ui.small_button("Researcher Login").clicked() {
                            app_state.mode_selected = false;
                        }
                    }

                    if ui.small_button("Reload Catalog").clicked() {
                        net.fetch_demo_mode();
                    }
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

                // Section 1: Site Selector & Body Filter
                ui.heading("1. Planetary & Terrestrial Sites");
                ui.horizontal(|ui| {
                    ui.label("Filter Body:");
                    ui.selectable_value(&mut app_state.body_filter, BodyFilter::All, "All");
                    ui.selectable_value(&mut app_state.body_filter, BodyFilter::Moon, "Moon");
                    ui.selectable_value(&mut app_state.body_filter, BodyFilter::Mars, "Mars");
                    ui.selectable_value(&mut app_state.body_filter, BodyFilter::Earth, "Earth");
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
                        .width(240.0)
                        .show_ui(ui, |ui| {
                            for (id, name, body) in &filtered_sites {
                                let label = format!(
                                    "[{}] {} ({})",
                                    body.as_str().to_uppercase(),
                                    name,
                                    id
                                );
                                if ui
                                    .selectable_label(app_state.primary_site_id == *id, label)
                                    .clicked()
                                    && app_state.primary_site_id != *id
                                {
                                    app_state.primary_site_id = id.clone();
                                    if let Some(s) = app_state
                                        .catalog
                                        .sites
                                        .iter()
                                        .find(|s| s.id == *id)
                                        .cloned()
                                    {
                                        app_state.site_editor = SiteEditorForm::from_site(&s);
                                        net.fetch_site_assets(TerrainSlot::Primary, &s);
                                    }
                                    app_state.pending_reload_primary = true;
                                }
                            }
                        });
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
                        .width(240.0)
                        .show_ui(ui, |ui| {
                            if ui
                                .selectable_label(
                                    app_state.comparison_site_id.is_none(),
                                    "None (Single View)",
                                )
                                .clicked()
                            {
                                app_state.comparison_site_id = None;
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
                                    app_state.comparison_site_id.as_deref() == Some(s.id.as_str());
                                if ui.selectable_label(selected, label).clicked() {
                                    app_state.comparison_site_id = Some(s.id.clone());
                                    net.fetch_site_assets(TerrainSlot::Comparison, s);
                                    app_state.pending_reload_comparison = true;
                                }
                            }
                        });
                });

                if let Some(site) = app_state.primary_site().cloned() {
                    ui.add_space(4.0);
                    ui.group(|ui| {
                        ui.strong(format!(
                            "{} ({})",
                            site.name,
                            site.body.as_str().to_uppercase()
                        ));
                        if !site.description.is_empty() {
                            ui.label(egui::RichText::new(&site.description).small());
                        }
                        ui.label(
                            egui::RichText::new(format!(
                                "DemExtent (m): X=[{:.1}, {:.1}], Y=[{:.1}, {:.1}]",
                                site.dem_extent.min_x,
                                site.dem_extent.max_x,
                                site.dem_extent.min_y,
                                site.dem_extent.max_y
                            ))
                            .monospace()
                            .small(),
                        );
                        ui.label(
                            egui::RichText::new(format!(
                                "Format: {:?} | Triangles: {} / 50,000 | Mesh: {}",
                                site.resolved_mesh_format(),
                                app_state.loaded_primary_triangles,
                                site.mesh_path
                            ))
                            .monospace()
                            .small(),
                        );
                    });
                }

                ui.separator();

                // Section 2: 3D Terrain & Planar UV Controls
                ui.heading("2. 3D Terrain & UV Verification");
                ui.horizontal(|ui| {
                    ui.label("Vertical Exaggeration (Y-up):");
                    ui.add(
                        egui::Slider::new(&mut app_state.vertical_exaggeration, 0.2..=8.0)
                            .suffix("x"),
                    );
                });

                ui.checkbox(
                    &mut app_state.show_checkerboard_uv,
                    "Checkerboard UV Verification (NW=Cyan u=0,v=0 | SE=Amber u=1,v=1)",
                );

                ui.horizontal(|ui| {
                    if ui.button("Reset 3D Camera (R)").clicked() {
                        let span = app_state
                            .primary_site()
                            .map(|s| s.dem_extent.width().max(s.dem_extent.height()))
                            .unwrap_or(2000.0);
                        for mut cam in &mut cam_query {
                            cam.frame_terrain(Vec3::ZERO, span);
                        }
                    }
                    if ui.button("Re-fetch Site Mesh & PNG").clicked()
                        && let Some(site) = app_state.primary_site().cloned()
                    {
                        net.fetch_site_assets(TerrainSlot::Primary, &site);
                    }
                });
                ui.label(
                    egui::RichText::new(
                        "Camera: Left-drag = Orbit | Right-drag or Shift+Left = Pan | Scroll = Zoom",
                    )
                    .small()
                    .color(egui::Color32::from_gray(165)),
                );

                ui.separator();

                // Section 3: Scientific Feature Weights & Live Recalculation
                ui.heading("3. Scientific Feature Weights");
                ui.label(
                    egui::RichText::new(
                        "Adjust weights to recalculate weighted sums over pre-normalized [0.0, 1.0] site features:",
                    )
                    .small(),
                );

                let weight_keys: Vec<String> = app_state.active_weights.keys().cloned().collect();
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
                    let local_recalc =
                        recalculate_catalog(&app_state.catalog, &app_state.active_weights);
                    app_state.ranked_scores = local_recalc.results;
                    net.recalculate_scores(app_state.active_weights.clone());
                }

                ui.horizontal(|ui| {
                    if ui
                        .button("Recalculate via Server (POST /api/recalculate)")
                        .clicked()
                    {
                        let local_recalc =
                            recalculate_catalog(&app_state.catalog, &app_state.active_weights);
                        app_state.ranked_scores = local_recalc.results;
                        net.recalculate_scores(app_state.active_weights.clone());
                    }
                    if ui.button("Reset to Canonical Weights").clicked() {
                        app_state.active_weights = app_state.catalog.default_weights.clone();
                        let local_recalc =
                            recalculate_catalog(&app_state.catalog, &app_state.active_weights);
                        app_state.ranked_scores = local_recalc.results;
                    }
                });

                ui.separator();

                // Section 4: Comparative Analog Scores Table
                ui.heading("4. Comparative Analog Scores");
                egui::Grid::new("analog_scores_table")
                    .striped(true)
                    .num_columns(5)
                    .spacing([10.0, 6.0])
                    .show(ui, |ui| {
                        ui.strong("#");
                        ui.strong("Site");
                        ui.strong("Body");
                        ui.strong("Live Score");
                        ui.strong("Canonical");
                        ui.end_row();

                        let rows = app_state.ranked_scores.clone();
                        for (idx, row) in rows.iter().enumerate() {
                            ui.label(format!("{}", idx + 1));
                            let is_current = app_state.primary_site_id == row.site_id;
                            if ui
                                .selectable_label(is_current, &row.name)
                                .on_hover_text("Click to inspect this site in 3D")
                                .clicked()
                                && !is_current
                            {
                                app_state.primary_site_id = row.site_id.clone();
                                if let Some(s) = app_state
                                    .catalog
                                    .sites
                                    .iter()
                                    .find(|s| s.id == row.site_id)
                                    .cloned()
                                {
                                    app_state.site_editor = SiteEditorForm::from_site(&s);
                                    net.fetch_site_assets(TerrainSlot::Primary, &s);
                                }
                                app_state.pending_reload_primary = true;
                            }
                            ui.label(row.body.as_str().to_uppercase());
                            ui.monospace(format!("{:.4}", row.score));
                            ui.monospace(format!("{:.4}", row.canonical_score));
                            ui.end_row();
                        }
                    });

                ui.separator();

                // Section 5: Authenticated Researcher Controls & Terrain Upload + TDA
                ui.heading("5. Researcher Controls & Terrain TDA");
                if !app_state.authenticated {
                    ui.label(
                        egui::RichText::new(
                            "Log in as Researcher (Argon2id) to upload & analyze .stl / .tif / .png terrains, run 0D Persistent Homology & Euler topology analysis, or persist weights.",
                        )
                        .small()
                        .color(egui::Color32::from_gray(175)),
                    );

                    ui.group(|ui| {
                        ui.strong("Quick Researcher Login (POST /api/login)");
                        let mut inline_login = false;
                        egui::Grid::new("inline_login_grid")
                            .num_columns(2)
                            .spacing([6.0, 4.0])
                            .show(ui, |ui| {
                                ui.label("Username:");
                                ui.text_edit_singleline(&mut app_state.login_username_input);
                                ui.end_row();

                                ui.label("Password:");
                                let r = ui.add(
                                    egui::TextEdit::singleline(&mut app_state.login_password_input)
                                        .password(true),
                                );
                                if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                                    inline_login = true;
                                }
                                ui.end_row();
                            });

                        if ui.button("Log In as Researcher").clicked() {
                            inline_login = true;
                        }
                        if inline_login {
                            app_state.error_message = None;
                            let user = app_state.login_username_input.trim().to_string();
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

                    // 5A. Upload & Analyze Terrain (.stl / .tif / .png)
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
                                "Select or drag-and-drop binary .stl (3D mesh), .tif/.tiff (DEM elevation), and/or .png (surface texture) to compute slope ∇z, TRI, Hypsometric Integral HI, 0D Persistent Homology (β₀), and Euler characteristic χ = V - E + F:",
                            )
                            .small(),
                        );

                        ui.add_space(4.0);
                        ui.horizontal_wrapped(|ui| {
                            if ui
                                .button("Browse .STL / .TIF / .PNG...")
                                .on_hover_text("Open browser file picker for .stl, .tif, .tiff, or .png")
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

                        // Staged files summary box
                        ui.group(|ui| {
                            let stl_status = app_state
                                .upload_form
                                .stl_filename
                                .as_deref()
                                .map(|n| {
                                    format!(
                                        "{} ({:.1} KB)",
                                        n,
                                        app_state.upload_form.stl_size_bytes as f64 / 1024.0
                                    )
                                })
                                .unwrap_or_else(|| "None (auto-generated if .tif provided)".to_string());
                            let tiff_status = app_state
                                .upload_form
                                .tiff_filename
                                .as_deref()
                                .map(|n| {
                                    format!(
                                        "{} ({:.1} KB)",
                                        n,
                                        app_state.upload_form.tiff_size_bytes as f64 / 1024.0
                                    )
                                })
                                .unwrap_or_else(|| "None (auto-rasterized if .stl provided)".to_string());
                            let png_status = app_state
                                .upload_form
                                .png_filename
                                .as_deref()
                                .map(|n| {
                                    format!(
                                        "{} ({:.1} KB)",
                                        n,
                                        app_state.upload_form.png_size_bytes as f64 / 1024.0
                                    )
                                })
                                .unwrap_or_else(|| {
                                    "None (shaded hypsometric relief PNG auto-generated)".to_string()
                                });

                            ui.monospace(format!("STL Mesh : {stl_status}"));
                            ui.monospace(format!("DEM TIFF : {tiff_status}"));
                            ui.monospace(format!("Albedo PNG: {png_status}"));
                        });

                        egui::Grid::new("upload_terrain_meta_grid")
                            .num_columns(2)
                            .spacing([8.0, 5.0])
                            .show(ui, |ui| {
                                ui.label("Site Name:");
                                ui.text_edit_singleline(&mut app_state.upload_form.name);
                                ui.end_row();

                                ui.label("Site ID (optional):");
                                ui.text_edit_singleline(&mut app_state.upload_form.id);
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
                                        egui::DragValue::new(&mut app_state.upload_form.min_x)
                                            .prefix("min: "),
                                    );
                                    ui.add(
                                        egui::DragValue::new(&mut app_state.upload_form.max_x)
                                            .prefix("max: "),
                                    );
                                });
                                ui.end_row();

                                ui.label("DemExtent Y (m):");
                                ui.horizontal(|ui| {
                                    ui.add(
                                        egui::DragValue::new(&mut app_state.upload_form.min_y)
                                            .prefix("min: "),
                                    );
                                    ui.add(
                                        egui::DragValue::new(&mut app_state.upload_form.max_y)
                                            .prefix("max: "),
                                    );
                                });
                                ui.end_row();
                            });

                        ui.add_space(6.0);
                        let can_analyze = app_state.upload_form.has_elevation_source()
                            && !app_state.upload_form.is_uploading;
                        let analyze_btn = egui::Button::new(
                            egui::RichText::new("Analyze & Propose Analog (POST /api/sites/upload)")
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
                        .min_size(egui::Vec2::new(360.0, 32.0));

                        if ui.add_enabled(can_analyze, analyze_btn).clicked() {
                            let req = app_state.upload_form.to_upload_request();
                            app_state.upload_form.is_uploading = true;
                            app_state.error_message = None;
                            app_state.status_message = format!(
                                "Uploading '{}' & computing morphometry + 0D persistent homology...",
                                req.site_name
                            );
                            net.upload_and_analyze_site(req);
                        }
                    });

                    // 5B. Mathematical, Differential-Geometric & Topological Report Viewer
                    if let Some(report) = app_state.last_analysis_report.clone() {
                        ui.add_space(6.0);
                        egui::CollapsingHeader::new(
                            egui::RichText::new(
                                "Mathematical & Topological Analysis Report (TDA)",
                            )
                            .strong()
                            .color(egui::Color32::from_rgb(120, 240, 170)),
                        )
                        .default_open(true)
                        .show(ui, |ui| {
                            // 1. Analog Proposal Box
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
                                    report.proposal.closest_site_name,
                                    report.proposal.closest_site_id
                                ));
                                ui.monospace(format!(
                                    "Feature Similarity: {:.1}%",
                                    report.proposal.similarity_score * 100.0
                                ));
                                ui.add_space(2.0);
                                ui.label(
                                    egui::RichText::new(&report.proposal.justification).small(),
                                );
                            });

                            // 2. Differential Geometry & Morphometry
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

                            // 3. Topological Data Analysis (0D Persistent Homology + Euler Characteristic)
                            ui.group(|ui| {
                                ui.strong(
                                    "Topological Data Analysis (0D Persistent Homology & χ)",
                                );
                                let t = &report.topology;
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
                                    t.max_persistence_m,
                                    t.mean_persistence_m,
                                    t.persistence_threshold_m
                                ));
                                ui.monospace(format!(
                                    "Simplicial Mesh   : V={} | E={} | F={}",
                                    t.vertex_count, t.edge_count, t.face_count
                                ));
                                ui.monospace(format!(
                                    "Euler χ = V-E+F   : {} | Components (β₀)={} | Genus≈{}",
                                    t.euler_characteristic,
                                    t.connected_components,
                                    t.estimated_genus
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

                    ui.add_space(6.0);
                    if ui
                        .button("Save Canonical Weights (PUT /api/weights)")
                        .clicked()
                    {
                        net.save_canonical_weights(app_state.active_weights.clone());
                    }

                    ui.add_space(4.0);
                    egui::CollapsingHeader::new("Manual Site Metadata Editor (POST /api/sites)")
                        .default_open(false)
                        .show(ui, |ui| {
                            egui::Grid::new("site_editor_grid")
                                .num_columns(2)
                                .spacing([8.0, 5.0])
                                .show(ui, |ui| {
                                    ui.label("Site ID:");
                                    ui.text_edit_singleline(&mut app_state.site_editor.id);
                                    ui.end_row();

                                    ui.label("Display Name:");
                                    ui.text_edit_singleline(&mut app_state.site_editor.name);
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
                                    ui.text_edit_singleline(&mut app_state.site_editor.description);
                                    ui.end_row();

                                    ui.label("Mesh (.stl/.tif):");
                                    ui.text_edit_singleline(&mut app_state.site_editor.mesh_path);
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
                                            egui::DragValue::new(&mut app_state.site_editor.min_x)
                                                .prefix("min: "),
                                        );
                                        ui.add(
                                            egui::DragValue::new(&mut app_state.site_editor.max_x)
                                                .prefix("max: "),
                                        );
                                    });
                                    ui.end_row();

                                    ui.label("DemExtent Y (m):");
                                    ui.horizontal(|ui| {
                                        ui.add(
                                            egui::DragValue::new(&mut app_state.site_editor.min_y)
                                                .prefix("min: "),
                                        );
                                        ui.add(
                                            egui::DragValue::new(&mut app_state.site_editor.max_y)
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
                                        egui::Slider::new(score_entry, 0.0..=1.0).step_by(0.01),
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
            });
        });
}
