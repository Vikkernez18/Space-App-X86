// AI-assisted
//! `analog_viewer` — Bevy + `bevy_egui` 3D WebAssembly and Native terrain explorer
//! for the Space-App-X86 platform.
//!
//! Enforces `AGENTS.md` §2 & §4:
//! - Loads binary `.stl` and `.tif`/`.tiff` DEMs via `analog_core` with Z-up to Y-up conversion.
//! - Applies planar UVs strictly from `DemExtent` (`min_x`, `max_x`, `min_y`, `max_y`).
//! - Scales vertical exaggeration exclusively along Bevy's `+Y` up axis:
//!   `Transform.scale = Vec3::new(1.0, exaggeration, 1.0)`.
//! - Includes the procedural 8x8 NW/SE UV verification checkerboard toggle.
//! - Supports passwordless Public Demo Mode for judges and Argon2id Researcher Mode
//!   with binary `multipart/form-data` 3D/DEM terrain upload and Topological Data Analysis (TDA).

pub mod camera;
pub mod mesh_loader;
pub mod net;
pub mod ui;

use analog_core::scoring::recalculate_catalog;
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::prelude::*;
#[cfg(not(target_arch = "wasm32"))]
use bevy::window::FileDragAndDrop;
use bevy_egui::EguiPlugin;

use camera::{OrbitCamera, orbit_camera_system};
use mesh_loader::{
    build_checkerboard_bevy_image, decode_png_to_bevy_image,
    generate_synthetic_test_site_welded_mesh, parse_site_terrain_bytes, welded_mesh_to_bevy_mesh,
};
#[cfg(not(target_arch = "wasm32"))]
use net::BrowserSelectedFile;
use net::{
    NetBridge, NetEvent, TerrainSlot, install_browser_file_drop_listener,
    poll_browser_uploaded_files,
};
use ui::{SiteEditorForm, ViewerAppState, viewer_ui_system};

/// Marker component for the primary 3D terrain mesh entity.
#[derive(Component)]
pub struct PrimaryTerrain;

/// Marker component for the optional side-by-side comparison 3D terrain mesh entity.
#[derive(Component)]
pub struct ComparisonTerrain;

/// Resource storing GPU handles for the primary and comparison terrain meshes, materials, and textures.
#[derive(Resource)]
pub struct TerrainSceneHandles {
    pub checkerboard_texture: Handle<Image>,
    pub primary_site_texture: Option<Handle<Image>>,
    pub comparison_site_texture: Option<Handle<Image>>,
    pub primary_mesh: Handle<Mesh>,
    pub comparison_mesh: Handle<Mesh>,
    pub primary_material: Handle<StandardMaterial>,
    pub comparison_material: Handle<StandardMaterial>,
}

fn main() {
    App::new()
        .insert_resource(ClearColor(Color::srgb(0.04, 0.055, 0.08)))
        .insert_resource(AmbientLight {
            color: Color::srgb(0.78, 0.82, 0.92),
            brightness: 350.0,
        })
        .init_resource::<NetBridge>()
        .init_resource::<ViewerAppState>()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Space-App-X86 — Lunar & Martian Terrestrial Analogs 3D Explorer"
                    .to_string(),
                canvas: Some("#bevy-canvas".to_string()),
                fit_canvas_to_parent: true,
                prevent_default_event_handling: false,
                ..default()
            }),
            ..default()
        }))
        .add_plugins(EguiPlugin)
        .add_systems(Startup, setup_scene_system)
        .add_systems(
            Update,
            (
                process_net_events_system,
                viewer_ui_system,
                sync_terrain_visuals_system,
                orbit_camera_system,
            ),
        )
        .run();
}

/// Initializes lighting, the orbital 3D camera, the UV verification checkerboard texture,
/// the browser file drop listener, and the initial `test_site` terrain mesh.
fn setup_scene_system(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    app_state: Res<ViewerAppState>,
    net: Res<NetBridge>,
) {
    install_browser_file_drop_listener();

    // Primary solar directional light
    commands.spawn(DirectionalLightBundle {
        directional_light: DirectionalLight {
            illuminance: 11_000.0,
            shadows_enabled: false,
            color: Color::srgb(1.0, 0.98, 0.94),
            ..default()
        },
        transform: Transform::from_xyz(1400.0, 2200.0, 1200.0).looking_at(Vec3::ZERO, Vec3::Y),
        ..default()
    });

    // Secondary sky/rim fill light to reveal shadow detail in deep craters
    commands.spawn(DirectionalLightBundle {
        directional_light: DirectionalLight {
            illuminance: 3_200.0,
            shadows_enabled: false,
            color: Color::srgb(0.65, 0.75, 0.95),
            ..default()
        },
        transform: Transform::from_xyz(-1400.0, 1500.0, -1200.0).looking_at(Vec3::ZERO, Vec3::Y),
        ..default()
    });

    // Orbital 3D camera
    let orbit_cam = OrbitCamera::default();
    let cam_transform = orbit_cam.compute_transform();
    commands.spawn((
        Camera3dBundle {
            transform: cam_transform,
            tonemapping: Tonemapping::None,
            ..default()
        },
        orbit_cam,
    ));

    // Procedural 8x8 NW/SE UV alignment checkerboard texture
    let checker_img = build_checkerboard_bevy_image(512, 512, 8);
    let checker_handle = images.add(checker_img);

    // Initial synthetic test_site terrain mesh
    let default_extent = app_state
        .primary_site()
        .map(|s| s.dem_extent)
        .unwrap_or_else(|| analog_core::DemExtent::new(-1000.0, 1000.0, -1000.0, 1000.0));
    let initial_welded = generate_synthetic_test_site_welded_mesh(&default_extent);
    let primary_mesh_handle = meshes.add(welded_mesh_to_bevy_mesh(initial_welded.clone()));
    let comparison_mesh_handle = meshes.add(welded_mesh_to_bevy_mesh(initial_welded));

    let primary_mat_handle = materials.add(StandardMaterial {
        base_color_texture: Some(checker_handle.clone()),
        perceptual_roughness: 0.88,
        metallic: 0.04,
        reflectance: 0.25,
        ..default()
    });

    let comparison_mat_handle = materials.add(StandardMaterial {
        base_color_texture: Some(checker_handle.clone()),
        perceptual_roughness: 0.88,
        metallic: 0.04,
        reflectance: 0.25,
        ..default()
    });

    commands.spawn((
        PbrBundle {
            mesh: primary_mesh_handle.clone(),
            material: primary_mat_handle.clone(),
            transform: Transform::from_scale(Vec3::new(1.0, app_state.vertical_exaggeration, 1.0)),
            ..default()
        },
        PrimaryTerrain,
    ));

    commands.spawn((
        PbrBundle {
            mesh: comparison_mesh_handle.clone(),
            material: comparison_mat_handle.clone(),
            transform: Transform::from_xyz(2600.0, 0.0, 0.0)
                .with_scale(Vec3::new(1.0, app_state.vertical_exaggeration, 1.0)),
            visibility: Visibility::Hidden,
            ..default()
        },
        ComparisonTerrain,
    ));

    commands.insert_resource(TerrainSceneHandles {
        checkerboard_texture: checker_handle,
        primary_site_texture: None,
        comparison_site_texture: None,
        primary_mesh: primary_mesh_handle,
        comparison_mesh: comparison_mesh_handle,
        primary_material: primary_mat_handle,
        comparison_material: comparison_mat_handle,
    });

    // Query existing session status and initial catalog/assets
    net.check_session();
    if let Some(site) = app_state.primary_site() {
        net.fetch_site_assets(TerrainSlot::Primary, site);
    }
}

/// Processes asynchronous HTTP / asset loading responses from [`NetBridge`] as well as
/// browser/native file selections for `.stl`, `.tif`/`.tiff`, and `.png` uploads.
fn process_net_events_system(
    net: Res<NetBridge>,
    mut app_state: ResMut<ViewerAppState>,
    mut handles: ResMut<TerrainSceneHandles>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut cam_query: Query<&mut OrbitCamera>,
    #[cfg(not(target_arch = "wasm32"))] mut dnd_events: EventReader<FileDragAndDrop>,
) {
    // 1. Poll files selected or dropped in the browser (WASM)
    for file in poll_browser_uploaded_files() {
        match app_state.upload_form.ingest_selected_file(file) {
            Ok(msg) => {
                app_state.status_message = msg;
                app_state.error_message = None;
            }
            Err(err) => {
                app_state.error_message = Some(err);
            }
        }
    }

    // 2. Poll native desktop drag-and-drop files (raw binary Vec<u8>, no Base64)
    #[cfg(not(target_arch = "wasm32"))]
    for ev in dnd_events.read() {
        if let FileDragAndDrop::DroppedFile { path_buf, .. } = ev {
            if let Ok(bytes) = std::fs::read(path_buf) {
                let name = path_buf
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("dropped.stl")
                    .to_string();
                let size = bytes.len();
                let selected = BrowserSelectedFile { name, size, bytes };
                match app_state.upload_form.ingest_selected_file(selected) {
                    Ok(msg) => {
                        app_state.status_message = msg;
                        app_state.error_message = None;
                    }
                    Err(err) => {
                        app_state.error_message = Some(err);
                    }
                }
            }
        }
    }

    // 3. Process HTTP / asset events
    for event in net.drain_events() {
        match event {
            NetEvent::DemoLoaded { catalog, message } => {
                app_state.status_message = message;
                if app_state.mode_selected {
                    app_state.error_message = None;
                }
                app_state.apply_catalog(catalog);
                if let Some(site) = app_state.primary_site().cloned() {
                    net.fetch_site_assets(TerrainSlot::Primary, &site);
                }
            }
            NetEvent::CatalogLoaded(catalog) => {
                app_state.status_message =
                    format!("Loaded catalog with {} site(s).", catalog.sites.len());
                if app_state.mode_selected {
                    app_state.error_message = None;
                }
                app_state.apply_catalog(catalog);
                if let Some(site) = app_state.primary_site().cloned() {
                    net.fetch_site_assets(TerrainSlot::Primary, &site);
                }
            }
            NetEvent::SessionStatus {
                authenticated,
                username,
                mode: _,
                message,
            } => {
                app_state.authenticated = authenticated;
                app_state.researcher_username = username;
                if !message.is_empty() {
                    app_state.status_message = message;
                }
                if authenticated {
                    app_state.mode_selected = true;
                    app_state.error_message = None;
                }
            }
            NetEvent::Recalculated(recalc) => {
                app_state.ranked_scores = recalc.results;
            }
            NetEvent::CanonicalWeightsSaved(catalog) => {
                app_state.status_message =
                    "Canonical weights persisted to server (PUT /api/weights).".to_string();
                app_state.error_message = None;
                app_state.apply_catalog(catalog);
            }
            NetEvent::SiteSaved(saved_site) => {
                app_state.status_message = format!(
                    "Saved site '{}' to catalog (POST /api/sites).",
                    saved_site.id
                );
                app_state.error_message = None;
                let saved_id = saved_site.id.clone();
                if let Some(existing) = app_state
                    .catalog
                    .sites
                    .iter_mut()
                    .find(|s| s.id == saved_id)
                {
                    *existing = saved_site.clone();
                } else {
                    app_state.catalog.sites.push(saved_site.clone());
                }
                let recalc = recalculate_catalog(&app_state.catalog, &app_state.active_weights);
                app_state.ranked_scores = recalc.results;
                app_state.primary_site_id = saved_id;
                app_state.site_editor = SiteEditorForm::from_site(&saved_site);
                net.fetch_site_assets(TerrainSlot::Primary, &saved_site);
            }
            NetEvent::SiteUploaded(uploaded) => {
                app_state.upload_form.is_uploading = false;
                app_state.error_message = None;
                let new_site = uploaded.report.generated_site_metadata.clone();
                app_state.status_message = format!(
                    "Analyzed '{}': proposed {} analog (score={:.4}, tris: {}->{}, χ={}, β₀ basins={}).",
                    new_site.name,
                    uploaded.report.proposal.proposed_body.as_str().to_uppercase(),
                    uploaded.report.proposal.analog_score,
                    uploaded.report.original_triangle_count,
                    uploaded.report.simplified_triangle_count,
                    uploaded.report.topology.euler_characteristic,
                    uploaded.report.topology.persistent_basins_b0
                );
                app_state.last_analysis_report = Some(uploaded.report);
                app_state.primary_site_id = new_site.id.clone();
                app_state.apply_catalog(uploaded.updated_catalog);
                app_state.primary_site_id = new_site.id.clone();
                app_state.site_editor = SiteEditorForm::from_site(&new_site);
                app_state.show_checkerboard_uv = false;
                net.fetch_site_assets(TerrainSlot::Primary, &new_site);
            }
            NetEvent::SiteMeshBytesLoaded {
                slot,
                site_id,
                bytes,
            } => {
                let Some(site) = app_state
                    .catalog
                    .sites
                    .iter()
                    .find(|s| s.id == site_id)
                    .cloned()
                else {
                    continue;
                };

                match parse_site_terrain_bytes(&bytes, &site) {
                    Ok(welded) => {
                        let tri_count = welded.triangle_count;
                        let bevy_mesh = welded_mesh_to_bevy_mesh(welded);
                        match slot {
                            TerrainSlot::Primary => {
                                if let Some(m) = meshes.get_mut(&handles.primary_mesh) {
                                    *m = bevy_mesh;
                                }
                                app_state.loaded_primary_triangles = tri_count;
                                app_state.status_message = format!(
                                    "Loaded 3D mesh for '{}' ({} triangles, DemExtent UV mapped).",
                                    site.name, tri_count
                                );
                                let span = site.dem_extent.width().max(site.dem_extent.height());
                                for mut cam in &mut cam_query {
                                    cam.frame_terrain(Vec3::ZERO, span);
                                }
                            }
                            TerrainSlot::Comparison => {
                                if let Some(m) = meshes.get_mut(&handles.comparison_mesh) {
                                    *m = bevy_mesh;
                                }
                            }
                        }
                    }
                    Err(err) => {
                        app_state.error_message = Some(err);
                    }
                }
            }
            NetEvent::SiteTextureBytesLoaded {
                slot,
                site_id,
                bytes,
            } => match decode_png_to_bevy_image(&bytes) {
                Ok(img) => {
                    let tex_handle = images.add(img);
                    match slot {
                        TerrainSlot::Primary => {
                            handles.primary_site_texture = Some(tex_handle);
                            app_state.status_message = format!(
                                "Loaded surface PNG texture for '{site_id}' (toggle Checkerboard UV to compare)."
                            );
                        }
                        TerrainSlot::Comparison => {
                            handles.comparison_site_texture = Some(tex_handle);
                        }
                    }
                }
                Err(err) => {
                    app_state.error_message =
                        Some(format!("Texture decode error for '{site_id}': {err}"));
                }
            },
            NetEvent::Error(err) => {
                app_state.upload_form.is_uploading = false;
                app_state.error_message = Some(err);
            }
        }
    }
}

/// Synchronizes vertical exaggeration (`Transform.scale = Vec3::new(1.0, exaggeration, 1.0)`),
/// side-by-side comparison placement, and active surface/checkerboard textures.
fn sync_terrain_visuals_system(
    app_state: Res<ViewerAppState>,
    handles: Res<TerrainSceneHandles>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut primary_q: Query<&mut Transform, (With<PrimaryTerrain>, Without<ComparisonTerrain>)>,
    mut comp_q: Query<
        (&mut Transform, &mut Visibility),
        (With<ComparisonTerrain>, Without<PrimaryTerrain>),
    >,
) {
    let scale = Vec3::new(1.0, app_state.vertical_exaggeration, 1.0);

    for mut transform in &mut primary_q {
        transform.scale = scale;
    }

    let primary_span_x = app_state
        .primary_site()
        .map(|s| s.dem_extent.width())
        .unwrap_or(2000.0);
    let comp_span_x = app_state
        .comparison_site()
        .map(|s| s.dem_extent.width())
        .unwrap_or(2000.0);
    let offset_x = (primary_span_x * 0.5 + comp_span_x * 0.5) * 1.25;

    for (mut transform, mut vis) in &mut comp_q {
        transform.scale = scale;
        transform.translation = Vec3::new(offset_x.max(500.0), 0.0, 0.0);
        *vis = if app_state.comparison_site_id.is_some() {
            Visibility::Visible
        } else {
            Visibility::Hidden
        }
    }

    // Update primary material texture (Checkerboard vs loaded PNG)
    if let Some(mat) = materials.get_mut(&handles.primary_material) {
        let target_tex = if app_state.show_checkerboard_uv {
            handles.checkerboard_texture.clone()
        } else {
            handles
                .primary_site_texture
                .clone()
                .unwrap_or_else(|| handles.checkerboard_texture.clone())
        };
        if mat.base_color_texture.as_ref() != Some(&target_tex) {
            mat.base_color_texture = Some(target_tex);
        }
    }

    // Update comparison material texture
    if let Some(mat) = materials.get_mut(&handles.comparison_material) {
        let target_tex = if app_state.show_checkerboard_uv {
            handles.checkerboard_texture.clone()
        } else {
            handles
                .comparison_site_texture
                .clone()
                .unwrap_or_else(|| handles.checkerboard_texture.clone())
        };
        if mat.base_color_texture.as_ref() != Some(&target_tex) {
            mat.base_color_texture = Some(target_tex);
        }
    }
}

#[cfg(test)]
mod tests {
    use analog_core::tiff_dem::encode_uncompressed_f32_tiff;
    use analog_core::{Body, DemExtent, MeshFormat, SiteMetadata};
    use std::collections::BTreeMap;

    use crate::mesh_loader::{
        build_checkerboard_bevy_image, generate_synthetic_test_site_welded_mesh,
        parse_site_terrain_bytes,
    };

    #[test]
    fn test_viewer_synthetic_test_site_mesh_and_checkerboard() {
        let extent = DemExtent::new(-1000.0, 1000.0, -1000.0, 1000.0);
        let welded = generate_synthetic_test_site_welded_mesh(&extent);
        assert_eq!(welded.triangle_count, 2048);
        assert!(welded.triangle_count <= 50_000);

        let img = build_checkerboard_bevy_image(64, 64, 8);
        assert_eq!(img.width(), 64);
        assert_eq!(img.height(), 64);
    }

    #[test]
    fn test_viewer_parses_both_stl_and_tiff_dem_sites() {
        let extent = DemExtent::new(0.0, 500.0, 0.0, 500.0);
        let tiff_bytes = encode_uncompressed_f32_tiff(4, 4, &[12.0_f32; 16]);

        let site = SiteMetadata {
            id: "mars_dem_test".to_string(),
            name: "Mars DEM Test".to_string(),
            body: Body::Mars,
            description: String::new(),
            center_lat_lon: None,
            dem_extent: extent,
            elevation_range_m: None,
            triangle_count: 18,
            mesh_path: "sites/mars_dem_test/elevation.tif".to_string(),
            mesh_format: Some(MeshFormat::Tiff),
            texture_path: "sites/mars_dem_test/texture.png".to_string(),
            feature_scores: BTreeMap::new(),
            canonical_score: 0.75,
            generated_by: None,
        };

        let welded = parse_site_terrain_bytes(&tiff_bytes, &site).expect("DEM TIFF should parse");
        assert_eq!(welded.triangle_count, 18);
        assert_eq!(welded.positions.len(), 16);
    }
}
