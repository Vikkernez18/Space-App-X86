use std::fs;
use std::path::Path;

use crate::model::{GeoBounds, PlanetaryBody, TerrainGrid};

/// Read an uncompressed 8-bit, 24-bit, or 32-bit BMP image into a TerrainGrid
/// (mapping image luminance 0..255 to the body's elevation range while preserving exact width x height).
pub fn read_bmp_to_grid(
    path: &Path,
    body: PlanetaryBody,
    source_format_override: Option<&str>,
) -> Result<TerrainGrid, String> {
    let bytes = fs::read(path).map_err(|e| format!("Failed to read BMP {:?}: {}", path, e))?;
    if bytes.len() < 54 || &bytes[0..2] != b"BM" {
        return Err("Invalid BMP header".to_string());
    }

    let data_offset = u32::from_le_bytes([bytes[10], bytes[11], bytes[12], bytes[13]]) as usize;
    let width_i = i32::from_le_bytes([bytes[18], bytes[19], bytes[20], bytes[21]]);
    let height_i = i32::from_le_bytes([bytes[22], bytes[23], bytes[24], bytes[25]]);
    let bpp = u16::from_le_bytes([bytes[28], bytes[29]]) as usize;
    let compression = u32::from_le_bytes([bytes[30], bytes[31], bytes[32], bytes[33]]);

    if compression != 0 {
        return Err(format!("Unsupported compressed BMP (compression={})", compression));
    }
    if width_i <= 0 || height_i == 0 {
        return Err("Invalid BMP dimensions".to_string());
    }

    let width = width_i as usize;
    let top_down = height_i < 0;
    let height = height_i.unsigned_abs() as usize;

    if bpp != 24 && bpp != 32 && bpp != 8 {
        return Err(format!("Unsupported BMP bit depth: {} bpp", bpp));
    }

    let bytes_per_pixel = bpp / 8;
    let row_stride = ((width * bpp + 31) / 32) * 4;
    if data_offset + row_stride * height > bytes.len() {
        return Err("BMP pixel buffer truncated".to_string());
    }

    let (target_min, target_max) = match body {
        PlanetaryBody::Mars => (-2850.0f32, -1950.0f32),
        PlanetaryBody::Moon => (-1600.0f32, 2150.0f32),
    };

    let mut elevation = vec![0.0f32; width * height];
    for y in 0..height {
        let src_y = if top_down { y } else { height - 1 - y };
        let row_off = data_offset + src_y * row_stride;
        for x in 0..width {
            let p = row_off + x * bytes_per_pixel;
            let lum = if bpp == 8 {
                bytes[p] as f32
            } else {
                let b = bytes[p] as f32;
                let g = bytes[p + 1] as f32;
                let r = bytes[p + 2] as f32;
                0.299 * r + 0.587 * g + 0.114 * b
            };
            let norm = (lum / 255.0).clamp(0.0, 1.0);
            elevation[y * width + x] = target_min + norm * (target_max - target_min);
        }
    }

    let mut bounds = GeoBounds::default_for(body, width, height);
    let span_m = match body {
        PlanetaryBody::Mars => 47_200.0,
        PlanetaryBody::Moon => 32_000.0,
    };
    let m_per_px = span_m / (width.max(height).max(1) as f64);
    bounds.meters_per_pixel_x = m_per_px;
    bounds.meters_per_pixel_y = m_per_px;

    let source_format = source_format_override
        .map(|s| s.to_string())
        .unwrap_or_else(|| format!("Raster Image ({}-bit BMP)", bpp));

    Ok(TerrainGrid {
        width,
        height,
        elevation,
        bounds,
        source_format,
        has_geotiff_tags: false,
        nodata_value: None,
    })
}
