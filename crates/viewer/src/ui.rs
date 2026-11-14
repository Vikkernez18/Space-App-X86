// AI-assisted
//! `bevy_egui` side panel and startup mode selector UI (100% English, zero NASA logos).
//!
//! Provides:
//! - Startup mode selector: direct **Enter Public Demo Mode** button + optional **Researcher Login** (`POST /api/login`).
//! - Site selector for Lunar, Martian, and Earth terrestrial analog sites + optional side-by-side comparison site.
//! - Vertical exaggeration slider (`Transform.scale = Vec3::new(1.0, exaggeration, 1.0)`) and UV Checkerboard toggle.
//! - Real-time scientific weight sliders (`POST /api/recalculate`) and comparative Analog Scores table.
//! - Authenticated Researcher controls (`PUT /api/weights` and `POST /api/sites`) unlocked via `GET /api/me` / `POST /api/login`.

use std::collections::BTreeMap;

use analog_core::scoring::recalculate_catalog;
use analog_core::{Body, Catalog, DemExtent, ScoredSite, SiteMetadata};
use bevy::prelude::*;
use bevy_egui::{EguiContexts, egui};

use crate::camera::OrbitCamera;
use crate::net::{NetBridge, TerrainSlot, fallback_synthetic_catalog};

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
    /// Login form username input.
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
            triangle_count: 2048,
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
            login_username_input: "researcher".to_string(),
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
            .fixed_size(egui::Vec2::new(430.0, 340.0))
            .show(ctx, |ui| {
                ui.heading("Welcome to Space-App-X86");
                ui.label(
                    "Explore Lunar, Martian, and Earth terrestrial analog sites in 3D (.stl & .tif DEMs with planar DemExtent UV mapping).",
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
                    .min_size(egui::Vec2::new(390.0, 38.0));

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
                        "Authenticate to unlock canonical weight persistence (PUT /api/weights) and site registration (POST /api/sites).",
                    );
                    ui.add_space(4.0);
                    egui::Grid::new("startup_login_grid")
                        .num_columns(2)
                        .spacing([8.0, 6.0])
                        .show(ui, |ui| {
                            ui.label("Username:");
                            ui.text_edit_singleline(&mut app_state.login_username_input);
                            ui.end_row();

                            ui.label("Password:");
                            ui.add(
                                egui::TextEdit::singleline(&mut app_state.login_password_input)
                                    .password(true),
                            );
                            ui.end_row();
                        });

                    ui.add_space(6.0);
                    if ui
                        .add_sized(
                            [390.0, 30.0],
                            egui::Button::new("Log In as Researcher (POST /api/login)"),
                        )
                        .clicked()
                    {
                        app_state.error_message = None;
                        let user = app_state.login_username_input.trim().to_string();
                        let pass = app_state.login_password_input.clone();
                        app_state.login_password_input.clear();
                        net.login(user, pass);
                        net.fetch_demo_mode();
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
        .default_width(390.0)
        .width_range(340.0..=500.0)
        .resizable(true)
        .show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.heading("Space-App-X86 Explorer");
                ui.label(
                    egui::RichText::new("Lunar & Martian Terrestrial Analogs 3D Viewer")
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
                            .unwrap_or("researcher");
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
                        if ui.small_button("Switch / Login").clicked() {
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

                ui.horizontal(|ui| {
                    ui.label("Primary Site:");
                    egui::ComboBox::from_id_source("primary_site_combo")
                        .selected_text(&app_state.primary_site_id)
                        .width(230.0)
                        .show_ui(ui, |ui| {
                            for (id, name, body) in &filtered_sites {
                                let label = format!("[{}] {} ({})", body.as_str().to_uppercase(), name, id);
                                if ui
                                    .selectable_label(app_state.primary_site_id == *id, label)
                                    .clicked()
                                    && app_state.primary_site_id != *id
                                {
                                    app_state.primary_site_id = id.clone();
                                    if let Some(s) =
                                        app_state.catalog.sites.iter().find(|s| s.id == *id).cloned()
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
                ui.horizontal(|ui| {
                    ui.label("Compare With:");
                    let current_comp_label = app_state
                        .comparison_site_id
                        .as_deref()
                        .unwrap_or("None (Single View)");
                    egui::ComboBox::from_id_source("comparison_site_combo")
                        .selected_text(current_comp_label)
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
                                app_state.pending_reload_comparison = true;
                            }
                            for s in &app_state.catalog.sites {
                                let label =
                                    format!("[{}] {} ({})", s.body.as_str().to_uppercase(), s.name, s.id);
                                let selected =
                                    app_state.comparison_site_id.as_deref() == Some(s.id.as_str());
                                if ui.selectable_label(selected, label).clicked() {
                                    let site_clone = s.clone();
                                    app_state.comparison_site_id = Some(site_clone.id.clone());
                                    net.fetch_site_assets(TerrainSlot::Comparison, &site_clone);
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
                    if ui.button("Recalculate via Server (POST /api/recalculate)").clicked() {
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

                // Section 5: Authenticated Researcher Controls (PUT /api/weights & POST /api/sites)
                ui.heading("5. Researcher Controls");
                if !app_state.authenticated {
                    ui.label(
                        egui::RichText::new(
                            "Locked in Public Demo Mode. Log in as Researcher (Argon2id) to persist canonical weights or register/edit sites.",
                        )
                        .small()
                        .color(egui::Color32::from_gray(165)),
                    );
                    if ui.button("Open Researcher Login...").clicked() {
                        app_state.mode_selected = false;
                    }
                } else {
                    ui.colored_label(
                        egui::Color32::from_rgb(80, 230, 150),
                        "Researcher Session Active — Write Operations Unlocked",
                    );

                    if ui
                        .button("Save Canonical Weights (PUT /api/weights)")
                        .clicked()
                    {
                        net.save_canonical_weights(app_state.active_weights.clone());
                    }

                    ui.add_space(6.0);
                    egui::CollapsingHeader::new("Register / Edit Site (POST /api/sites)")
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
