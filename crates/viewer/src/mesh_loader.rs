// AI-assisted
//! Terrain mesh (`.stl` and `.tif`/`.tiff`) and surface texture (`.png` and procedural
//! UV checkerboard) loader for the Bevy 3D viewer.
//!
//! Strictly enforces `AGENTS.md` §2:
//! - Converts Z-up STL / DEM coordinates to Bevy's right-handed Y-up space via `analog_core`.
//! - Uses planar UVs derived strictly from `DemExtent` (`min_x`, `max_x`, `min_y`, `max_y`).
//! - Uses `Indices::U32` and enforces `<= 50,000` triangles per site (automatically simplifying
//!   any `.stl` or `.tif` exceeding `50,000` triangles).

use std::io::Cursor;

use analog_core::models::{DemExtent, MeshFormat, SiteMetadata};
use analog_core::stl::{WeldedMesh, encode_binary_stl, parse_and_weld_stl};
use analog_core::tiff_dem::parse_dem_tiff;
use bevy::prelude::*;
use bevy::render::mesh::{Indices, PrimitiveTopology};
use bevy::render::render_asset::RenderAssetUsages;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

/// Maximum GPU texture dimension per axis in the 3D viewer (`2048x2048`) so huge 50+ MiB
/// `.png` textures never exceed WebGL2 `MAX_TEXTURE_SIZE` or exhaust WASM memory.
pub const MAX_VIEWER_TEXTURE_DIM: u32 = 2048;

/// Centers all vertices of a [`WeldedMesh`] at the origin `(0.0, 0.0, 0.0)` in Bevy Y-up space
/// by computing the 3D bounding-box centroid `(center_x, center_y, center_z)` in `f64` and
/// translating every vertex so the terrain sits at `(0.0, 0.0, 0.0)` with its mid-elevation at `Y = 0`.
pub fn center_welded_mesh_at_origin(mesh: &mut WeldedMesh) {
    if mesh.positions.is_empty() {
        return;
    }

    let mut min_x = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_y = f64::NEG_INFINITY;
    let mut min_z = f64::INFINITY;
    let mut max_z = f64::NEG_INFINITY;

    for pos in &mesh.positions {
        let x = f64::from(pos[0]);
        let y = f64::from(pos[1]);
        let z = f64::from(pos[2]);
        if x.is_finite() && y.is_finite() && z.is_finite() {
            min_x = min_x.min(x);
            max_x = max_x.max(x);
            min_y = min_y.min(y);
            max_y = max_y.max(y);
            min_z = min_z.min(z);
            max_z = max_z.max(z);
        }
    }

    if !(min_x.is_finite() && min_y.is_finite() && min_z.is_finite()) {
        return;
    }

    let center_x = 0.5 * (min_x + max_x);
    let center_y = 0.5 * (min_y + max_y);
    let center_z = 0.5 * (min_z + max_z);

    if center_x.abs() > 1e-6 || center_y.abs() > 1e-6 || center_z.abs() > 1e-6 {
        for pos in &mut mesh.positions {
            pos[0] = (f64::from(pos[0]) - center_x) as f32;
            pos[1] = (f64::from(pos[1]) - center_y) as f32;
            pos[2] = (f64::from(pos[2]) - center_z) as f32;
        }
    }
}

/// Computes the bounding sphere radius (`bounding_radius`) in meters of a centered [`WeldedMesh`]
/// in double precision (`f64`) around the origin `(0.0, 0.0, 0.0)`.
pub fn compute_mesh_bounding_radius(mesh: &WeldedMesh) -> f32 {
    if mesh.positions.is_empty() {
        return 1000.0;
    }

    let mut max_r2 = 0.0_f64;
    for pos in &mesh.positions {
        let x = f64::from(pos[0]);
        let y = f64::from(pos[1]);
        let z = f64::from(pos[2]);
        let r2 = x * x + y * y + z * z;
        if r2.is_finite() && r2 > max_r2 {
            max_r2 = r2;
        }
    }

    let radius = max_r2.sqrt() as f32;
    if radius.is_finite() && radius > 1.0 {
        radius
    } else {
        1000.0
    }
}

/// Parses either a binary `.stl` or a `.tif`/`.tiff` elevation DEM into a [`WeldedMesh`]
/// in Bevy right-handed Y-up space centered at `(0.0, 0.0, 0.0)` with `DemExtent` planar UVs.
///
/// Note: For `.tif`/`.tiff` DEMs, `vertical_exaggeration` is passed as `1.0` during mesh
/// generation because Bevy scales the vertical axis dynamically via
/// `Transform.scale = Vec3::new(1.0, exaggeration, 1.0)` per `AGENTS.md` §2.3.
pub fn parse_site_terrain_bytes(
    bytes: &[u8],
    site: &SiteMetadata,
) -> Result<WeldedMesh, String> {
    let is_tiff_magic = bytes.starts_with(b"II*\x00") || bytes.starts_with(b"MM\x00*");
    let format = if is_tiff_magic {
        MeshFormat::Tiff
    } else {
        site.resolved_mesh_format()
    };

    let mut welded = match format {
        MeshFormat::Stl => parse_and_weld_stl(bytes, &site.dem_extent)
            .map_err(|e| format!("STL parse error for site '{}': {e}", site.id))?,
        MeshFormat::Tiff => parse_dem_tiff(bytes, &site.dem_extent, 1.0)
            .map_err(|e| format!("DEM TIFF parse error for site '{}': {e}", site.id))?,
    };

    center_welded_mesh_at_origin(&mut welded);
    Ok(welded)
}

/// Converts an `analog_core::WeldedMesh` into a GPU-ready Bevy [`Mesh`] with `Indices::U32`.
pub fn welded_mesh_to_bevy_mesh(welded: WeldedMesh) -> Mesh {
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, welded.positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, welded.normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, welded.uvs);
    mesh.insert_indices(Indices::U32(welded.indices));
    mesh
}

/// Decodes an in-memory `.png` byte buffer into an `Rgba8UnormSrgb` Bevy [`Image`],
/// automatically downsampling images larger than [`MAX_VIEWER_TEXTURE_DIM`] (`2048x2048`)
/// so 50+ MiB PNGs render cleanly in WebGL2 / WASM.
pub fn decode_png_to_bevy_image(png_bytes: &[u8]) -> Result<Image, String> {
    let mut decoder = png::Decoder::new(Cursor::new(png_bytes));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);

    let mut reader = decoder
        .read_info()
        .map_err(|e| format!("Failed to read PNG header: {e}"))?;
    let mut buf = vec![0u8; reader.output_buffer_size()];
    let info = reader
        .next_frame(&mut buf)
        .map_err(|e| format!("Failed to decode PNG frame: {e}"))?;

    let raw = &buf[..info.buffer_size()];
    let src_w = info.width as usize;
    let src_h = info.height as usize;

    let scale = (info.width as f64 / MAX_VIEWER_TEXTURE_DIM as f64)
        .max(info.height as f64 / MAX_VIEWER_TEXTURE_DIM as f64)
        .max(1.0);
    let out_w = ((info.width as f64 / scale).round() as u32).clamp(1, MAX_VIEWER_TEXTURE_DIM);
    let out_h = ((info.height as f64 / scale).round() as u32).clamp(1, MAX_VIEWER_TEXTURE_DIM);

    let bpp = match info.color_type {
        png::ColorType::Rgba => 4usize,
        png::ColorType::Rgb => 3usize,
        png::ColorType::GrayscaleAlpha => 2usize,
        png::ColorType::Grayscale | png::ColorType::Indexed => 1usize,
    };

    let mut rgba = Vec::with_capacity((out_w as usize) * (out_h as usize) * 4);
    let max_r = src_h.saturating_sub(1);
    let max_c = src_w.saturating_sub(1);

    for or in 0..(out_h as usize) {
        let sr = if out_h <= 1 {
            0
        } else if or + 1 == out_h as usize {
            max_r
        } else {
            (or * max_r) / ((out_h as usize) - 1)
        };
        let row_base = sr * src_w;
        for oc in 0..(out_w as usize) {
            let sc = if out_w <= 1 {
                0
            } else if oc + 1 == out_w as usize {
                max_c
            } else {
                (oc * max_c) / ((out_w as usize) - 1)
            };
            let idx = (row_base + sc) * bpp;
            match info.color_type {
                png::ColorType::Rgba => {
                    rgba.extend_from_slice(&raw[idx..idx + 4]);
                }
                png::ColorType::Rgb => {
                    rgba.extend_from_slice(&[raw[idx], raw[idx + 1], raw[idx + 2], 255]);
                }
                png::ColorType::GrayscaleAlpha => {
                    let g = raw[idx];
                    let a = raw[idx + 1];
                    rgba.extend_from_slice(&[g, g, g, a]);
                }
                png::ColorType::Grayscale | png::ColorType::Indexed => {
                    let g = raw[idx];
                    rgba.extend_from_slice(&[g, g, g, 255]);
                }
            }
        }
    }

    Ok(Image::new(
        Extent3d {
            width: out_w,
            height: out_h,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        rgba,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    ))
}

/// Builds a procedural 8x8 UV verification checkerboard [`Image`] (`AGENTS.md` §2.4):
/// - North-West corner tile (`u=0, v=0`) is highlighted in Cyan/Teal.
/// - South-East corner tile (`u=1, v=1`) is highlighted in Amber/Orange.
/// - Alternating interior tiles are Light Silver and Dark Slate.
pub fn build_checkerboard_bevy_image(width: u32, height: u32, tiles: u32) -> Image {
    let safe_tiles = tiles.max(2);
    let tile_w = (width / safe_tiles).max(1);
    let tile_h = (height / safe_tiles).max(1);
    let mut rgba = Vec::with_capacity((width * height * 4) as usize);

    for y in 0..height {
        let ty = (y / tile_h).min(safe_tiles - 1);
        for x in 0..width {
            let tx = (x / tile_w).min(safe_tiles - 1);
            let color: [u8; 4] = if tx == 0 && ty == 0 {
                // North-West corner (u=0, v=0)
                [0, 210, 220, 255]
            } else if tx == safe_tiles - 1 && ty == safe_tiles - 1 {
                // South-East corner (u=1, v=1)
                [245, 140, 30, 255]
            } else if (tx + ty) % 2 == 0 {
                [220, 226, 234, 255]
            } else {
                [36, 44, 58, 255]
            };
            rgba.extend_from_slice(&color);
        }
    }

    Image::new(
        Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        rgba,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    )
}

/// Generates the synthetic crater-and-ridge [`WeldedMesh`] in memory for `test_site`
/// so that the 3D viewer always has an immediate fallback mesh even before disk assets load.
pub fn generate_synthetic_test_site_welded_mesh(extent: &DemExtent) -> WeldedMesh {
    let grid_cells = 32usize;
    let dx = extent.width() / (grid_cells as f32);
    let dy = extent.height() / (grid_cells as f32);
    let mut triangles: Vec<[[f32; 3]; 3]> = Vec::with_capacity(grid_cells * grid_cells * 2);

    let elev = |x: f32, y: f32| -> f32 {
        let r = (x * x + y * y).sqrt();
        let bowl = -65.0 * (-(r / 320.0).powi(2)).exp();
        let rim = 85.0 * (-((r - 450.0) / 140.0).powi(2)).exp();
        let ridge = 20.0 * (x / 350.0).sin() * (y / 400.0).cos();
        (bowl + rim + ridge + 15.0).clamp(-45.0, 120.0)
    };

    for iy in 0..grid_cells {
        let y0 = extent.min_y + (iy as f32) * dy;
        let y1 = if iy + 1 == grid_cells {
            extent.max_y
        } else {
            extent.min_y + ((iy + 1) as f32) * dy
        };
        for ix in 0..grid_cells {
            let x0 = extent.min_x + (ix as f32) * dx;
            let x1 = if ix + 1 == grid_cells {
                extent.max_x
            } else {
                extent.min_x + ((ix + 1) as f32) * dx
            };

            let v00 = [x0, y0, elev(x0, y0)];
            let v10 = [x1, y0, elev(x1, y0)];
            let v11 = [x1, y1, elev(x1, y1)];
            let v01 = [x0, y1, elev(x0, y1)];

            triangles.push([v00, v10, v11]);
            triangles.push([v00, v11, v01]);
        }
    }

    let stl_bytes = encode_binary_stl(&triangles);
    parse_and_weld_stl(&stl_bytes, extent).expect("synthetic test_site STL must be valid")
}
