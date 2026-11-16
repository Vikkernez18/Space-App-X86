use std::fs;
use std::path::Path;

use crate::math::DerivedLayers;
use crate::model::{PlanetaryBody, TerrainGrid};

/// Write binary telemetry grid (little-endian: u32 width, u32 height, then 5 f32 arrays:
/// elevation, slope_deg, roughness_m, viability_score, residual_diff_m) for fast Java GUI & 3D rendering
pub fn write_telemetry_bin(
    path: &Path,
    grid: &TerrainGrid,
    layers: &DerivedLayers,
) -> Result<(), String> {
    let w = grid.width as u32;
    let h = grid.height as u32;
    let n = (w as usize) * (h as usize);
    let mut buf = Vec::with_capacity(8 + n * 4 * 5);
    buf.extend_from_slice(&w.to_le_bytes());
    buf.extend_from_slice(&h.to_le_bytes());

    for arr in [
        &grid.elevation,
        &layers.slope_deg,
        &layers.roughness_m,
        &layers.viability_score,
        &layers.residual_diff_m,
    ] {
        for &v in arr.iter().take(n) {
            buf.extend_from_slice(&v.to_le_bytes());
        }
    }

    fs::write(path, &buf).map_err(|e| format!("Failed to write telemetry bin {:?}: {}", path, e))
}

/// Write 24-bit uncompressed BMP image of a colormapped raster layer
pub fn write_bmp_rgb(path: &Path, width: usize, height: usize, rgb: &[u8]) -> Result<(), String> {
    let row_stride = (width * 3 + 3) & !3;
    let image_size = row_stride * height;
    let file_size = 54 + image_size;
    let mut buf = Vec::with_capacity(file_size);

    // BMP File Header (14 bytes)
    buf.extend_from_slice(b"BM");
    buf.extend_from_slice(&(file_size as u32).to_le_bytes());
    buf.extend_from_slice(&0u32.to_le_bytes());
    buf.extend_from_slice(&54u32.to_le_bytes());

    // DIB Header BITMAPINFOHEADER (40 bytes)
    buf.extend_from_slice(&40u32.to_le_bytes());
    buf.extend_from_slice(&(width as i32).to_le_bytes());
    buf.extend_from_slice(&(height as i32).to_le_bytes()); // bottom-up rows
    buf.extend_from_slice(&1u16.to_le_bytes());
    buf.extend_from_slice(&24u16.to_le_bytes());
    buf.extend_from_slice(&0u32.to_le_bytes());
    buf.extend_from_slice(&(image_size as u32).to_le_bytes());
    buf.extend_from_slice(&2835u32.to_le_bytes());
    buf.extend_from_slice(&2835u32.to_le_bytes());
    buf.extend_from_slice(&0u32.to_le_bytes());
    buf.extend_from_slice(&0u32.to_le_bytes());

    let pad = row_stride - width * 3;
    for y_inv in 0..height {
        let y = height - 1 - y_inv;
        for x in 0..width {
            let idx = (y * width + x) * 3;
            let r = rgb[idx];
            let g = rgb[idx + 1];
            let b = rgb[idx + 2];
            buf.push(b);
            buf.push(g);
            buf.push(r);
        }
        for _ in 0..pad {
            buf.push(0);
        }
    }

    fs::write(path, &buf).map_err(|e| format!("Failed to write BMP {:?}: {}", path, e))
}

/// Export colormapped BMP images for Elevation, Slope, and Viability Score to `out_dir`
pub fn export_report_bmps(
    out_dir: &Path,
    body: PlanetaryBody,
    grid: &TerrainGrid,
    layers: &DerivedLayers,
) -> Result<(), String> {
    let w = grid.width;
    let h = grid.height;
    let (min_z, max_z, _) = grid.min_max_mean();
    let span_z = (max_z - min_z).max(1.0);

    let mut elev_rgb = vec![0u8; w * h * 3];
    let mut viability_rgb = vec![0u8; w * h * 3];

    for i in 0..(w * h) {
        let t_z = ((grid.elevation[i] - min_z) / span_z).clamp(0.0, 1.0);
        let (er, eg, eb) = match body {
            PlanetaryBody::Mars => mars_hypsometric_rgb(t_z),
            PlanetaryBody::Moon => moon_hypsometric_rgb(t_z),
        };
        elev_rgb[i * 3] = er;
        elev_rgb[i * 3 + 1] = eg;
        elev_rgb[i * 3 + 2] = eb;

        let t_v = (layers.viability_score[i] / 100.0).clamp(0.0, 1.0);
        let (vr, vg, vb) = viability_heatmap_rgb(t_v);
        viability_rgb[i * 3] = vr;
        viability_rgb[i * 3 + 1] = vg;
        viability_rgb[i * 3 + 2] = vb;
    }

    write_bmp_rgb(&out_dir.join("latest_elevation.bmp"), w, h, &elev_rgb)?;
    write_bmp_rgb(&out_dir.join("latest_viability.bmp"), w, h, &viability_rgb)?;
    Ok(())
}

fn lerp_u8(a: u8, b: u8, t: f32) -> u8 {
    ((a as f32) + ((b as f32) - (a as f32)) * t.clamp(0.0, 1.0)).round() as u8
}

fn mars_hypsometric_rgb(t: f32) -> (u8, u8, u8) {
    if t < 0.35 {
        let s = t / 0.35;
        (lerp_u8(38, 122, s), lerp_u8(48, 72, s), lerp_u8(92, 64, s))
    } else if t < 0.70 {
        let s = (t - 0.35) / 0.35;
        (lerp_u8(122, 204, s), lerp_u8(72, 110, s), lerp_u8(64, 58, s))
    } else {
        let s = (t - 0.70) / 0.30;
        (lerp_u8(204, 246, s), lerp_u8(110, 205, s), lerp_u8(58, 145, s))
    }
}

fn moon_hypsometric_rgb(t: f32) -> (u8, u8, u8) {
    if t < 0.4 {
        let s = t / 0.4;
        (lerp_u8(28, 95, s), lerp_u8(36, 108, s), lerp_u8(68, 130, s))
    } else if t < 0.75 {
        let s = (t - 0.4) / 0.35;
        (lerp_u8(95, 178, s), lerp_u8(108, 188, s), lerp_u8(130, 202, s))
    } else {
        let s = (t - 0.75) / 0.25;
        (lerp_u8(178, 245, s), lerp_u8(188, 246, s), lerp_u8(202, 250, s))
    }
}

fn viability_heatmap_rgb(t: f32) -> (u8, u8, u8) {
    if t < 0.50 {
        let s = t / 0.50;
        (lerp_u8(190, 235, s), lerp_u8(38, 165, s), lerp_u8(45, 35, s))
    } else {
        let s = (t - 0.50) / 0.50;
        (lerp_u8(235, 32, s), lerp_u8(165, 205, s), lerp_u8(35, 115, s))
    }
}
