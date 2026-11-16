use std::fs;
use std::path::Path;

use crate::model::{GeoBounds, PlanetaryBody, TerrainGrid};

#[derive(Debug, Clone, Copy)]
pub struct Triangle3D {
    pub v0: [f32; 3],
    pub v1: [f32; 3],
    pub v2: [f32; 3],
}

fn cross_norm(a: [f32; 3], b: [f32; 3], c: [f32; 3]) -> [f32; 3] {
    let ux = b[0] - a[0];
    let uy = b[1] - a[1];
    let uz = b[2] - a[2];
    let vx = c[0] - a[0];
    let vy = c[1] - a[1];
    let vz = c[2] - a[2];
    let nx = uy * vz - uz * vy;
    let ny = uz * vx - ux * vz;
    let nz = ux * vy - uy * vx;
    let len = (nx * nx + ny * ny + nz * nz).sqrt().max(1e-6);
    [nx / len, ny / len, nz / len]
}

/// Export a TerrainGrid as a Binary .stl 3D mesh (2 triangles per grid quad)
pub fn write_binary_stl(path: &Path, grid: &TerrainGrid, step: usize) -> Result<(), String> {
    let step = step.max(1);
    let nx = (grid.width - 1) / step;
    let ny = (grid.height - 1) / step;
    if nx == 0 || ny == 0 {
        return Err("Grid too small to export as STL mesh".to_string());
    }
    let num_triangles = (nx * ny * 2) as u32;
    let mut buf = Vec::with_capacity(84 + (num_triangles as usize) * 50);

    // 80-byte header
    let mut header = [0u8; 80];
    let title = b"JMARS Planetary Settler Binary STL DEM Mesh";
    header[..title.len()].copy_from_slice(title);
    buf.extend_from_slice(&header);
    buf.extend_from_slice(&num_triangles.to_le_bytes());

    let dx = grid.bounds.meters_per_pixel_x as f32;
    let dy = grid.bounds.meters_per_pixel_y as f32;

    for gy in 0..ny {
        let y0 = gy * step;
        let y1 = (y0 + step).min(grid.height - 1);
        // Flip Y in 3D space so y=0 (North) is at +Y, or keep direct grid meters:
        // We use X = easting (m), Y = northing from top (so -y * dy or (H-1-y)*dy), Z = elevation (m)
        let py0 = ((grid.height - 1 - y0) as f32) * dy;
        let py1 = ((grid.height - 1 - y1) as f32) * dy;

        for gx in 0..nx {
            let x0 = gx * step;
            let x1 = (x0 + step).min(grid.width - 1);
            let px0 = (x0 as f32) * dx;
            let px1 = (x1 as f32) * dx;

            let z00 = grid.get(x0, y0);
            let z10 = grid.get(x1, y0);
            let z01 = grid.get(x0, y1);
            let z11 = grid.get(x1, y1);

            let p00 = [px0, py0, z00];
            let p10 = [px1, py0, z10];
            let p01 = [px0, py1, z01];
            let p11 = [px1, py1, z11];

            for (a, b, c) in [(p00, p01, p10), (p10, p01, p11)] {
                let n = cross_norm(a, b, c);
                for f in [n[0], n[1], n[2], a[0], a[1], a[2], b[0], b[1], b[2], c[0], c[1], c[2]]
                {
                    buf.extend_from_slice(&f.to_le_bytes());
                }
                buf.extend_from_slice(&0u16.to_le_bytes());
            }
        }
    }

    fs::write(path, &buf).map_err(|e| format!("Failed to write binary STL {:?}: {}", path, e))
}

/// Export a TerrainGrid as an ASCII .stl 3D mesh
pub fn write_ascii_stl(path: &Path, grid: &TerrainGrid, step: usize) -> Result<(), String> {
    let step = step.max(1);
    let nx = (grid.width - 1) / step;
    let ny = (grid.height - 1) / step;
    if nx == 0 || ny == 0 {
        return Err("Grid too small to export as ASCII STL".to_string());
    }
    let mut s = String::from("solid planetary_terrain\n");
    let dx = grid.bounds.meters_per_pixel_x as f32;
    let dy = grid.bounds.meters_per_pixel_y as f32;

    for gy in 0..ny {
        let y0 = gy * step;
        let y1 = (y0 + step).min(grid.height - 1);
        let py0 = ((grid.height - 1 - y0) as f32) * dy;
        let py1 = ((grid.height - 1 - y1) as f32) * dy;
        for gx in 0..nx {
            let x0 = gx * step;
            let x1 = (x0 + step).min(grid.width - 1);
            let px0 = (x0 as f32) * dx;
            let px1 = (x1 as f32) * dx;

            let p00 = [px0, py0, grid.get(x0, y0)];
            let p10 = [px1, py0, grid.get(x1, y0)];
            let p01 = [px0, py1, grid.get(x0, y1)];
            let p11 = [px1, py1, grid.get(x1, y1)];

            for (a, b, c) in [(p00, p01, p10), (p10, p01, p11)] {
                let n = cross_norm(a, b, c);
                s.push_str(&format!(
                    "  facet normal {:.4} {:.4} {:.4}\n    outer loop\n      vertex {:.3} {:.3} {:.3}\n      vertex {:.3} {:.3} {:.3}\n      vertex {:.3} {:.3} {:.3}\n    endloop\n  endfacet\n",
                    n[0], n[1], n[2], a[0], a[1], a[2], b[0], b[1], b[2], c[0], c[1], c[2]
                ));
            }
        }
    }
    s.push_str("endsolid planetary_terrain\n");
    fs::write(path, s).map_err(|e| format!("Failed to write ASCII STL {:?}: {}", path, e))
}

/// Read an ASCII or Binary .stl file and rasterize its 3D triangle mesh into a 2D DEM TerrainGrid
pub fn read_stl_to_grid(
    path: &Path,
    body: PlanetaryBody,
    target_w: usize,
    target_h: usize,
) -> Result<TerrainGrid, String> {
    let bytes = fs::read(path).map_err(|e| format!("Failed to read STL {:?}: {}", path, e))?;
    if bytes.len() < 15 {
        return Err("File too small to be a valid STL mesh".to_string());
    }

    let is_ascii = is_ascii_stl(&bytes);
    let triangles = if is_ascii {
        parse_ascii_stl(&bytes)?
    } else {
        parse_binary_stl(&bytes)?
    };

    if triangles.is_empty() {
        return Err("STL file contains 0 triangles".to_string());
    }

    // Compute 3D bounding box of mesh
    let mut min_x = f32::INFINITY;
    let mut max_x = f32::NEG_INFINITY;
    let mut min_y = f32::INFINITY;
    let mut max_y = f32::NEG_INFINITY;
    let mut min_z = f32::INFINITY;
    let mut max_z = f32::NEG_INFINITY;

    for t in &triangles {
        for v in [t.v0, t.v1, t.v2] {
            min_x = min_x.min(v[0]);
            max_x = max_x.max(v[0]);
            min_y = min_y.min(v[1]);
            max_y = max_y.max(v[1]);
            min_z = min_z.min(v[2]);
            max_z = max_z.max(v[2]);
        }
    }

    let span_x = (max_x - min_x).max(1e-5);
    let span_y = (max_y - min_y).max(1e-5);

    // Dynamically compute grid resolution from STL aspect ratio and triangle density
    // so non-square meshes preserve exact geometry, details, and resolution.
    let aspect = ((span_x as f64) / (span_y as f64)).clamp(0.08, 12.5);
    let total_quads = (triangles.len() as f64 * 0.5).max(1024.0);
    let mut est_w = (total_quads * aspect).sqrt().round() as usize;
    let mut est_h = (total_quads / aspect).sqrt().round() as usize;

    let min_side = est_w.min(est_h).max(1);
    if min_side < 64 {
        let scale = 64.0 / (min_side as f64);
        est_w = ((est_w as f64) * scale).round() as usize;
        est_h = ((est_h as f64) * scale).round() as usize;
    }
    let max_side = est_w.max(est_h).max(1);
    if max_side > 1600 {
        let scale = 1600.0 / (max_side as f64);
        est_w = ((est_w as f64) * scale).round() as usize;
        est_h = ((est_h as f64) * scale).round() as usize;
    }
    est_w = est_w.max(32);
    est_h = est_h.max(32);

    let w = if target_w > 0 { target_w } else { est_w };
    let h = if target_h > 0 { target_h } else { est_h };

    let mut z_buf = vec![f32::NAN; w * h];

    // Barycentric triangle rasterization onto (w x h) DEM grid
    // Note: in our coordinate convention, y_grid = 0 is North (+max_y in 3D space)
    for t in &triangles {
        let to_grid = |v: [f32; 3]| -> (f32, f32, f32) {
            let gx = ((v[0] - min_x) / span_x) * ((w - 1) as f32);
            let gy = ((max_y - v[1]) / span_y) * ((h - 1) as f32);
            (gx, gy, v[2])
        };
        let (x0, y0, z0) = to_grid(t.v0);
        let (x1, y1, z1) = to_grid(t.v1);
        let (x2, y2, z2) = to_grid(t.v2);

        let gx_min = x0.min(x1).min(x2).floor().max(0.0) as usize;
        let gx_max = (x0.max(x1).max(x2).ceil().max(0.0) as usize).min(w - 1);
        let gy_min = y0.min(y1).min(y2).floor().max(0.0) as usize;
        let gy_max = (y0.max(y1).max(y2).ceil().max(0.0) as usize).min(h - 1);

        let denom = (y1 - y2) * (x0 - x2) + (x2 - x1) * (y0 - y2);
        if denom.abs() < 1e-6 {
            continue;
        }

        let mut hit = false;
        for py in gy_min..=gy_max {
            let fy = py as f32;
            for px in gx_min..=gx_max {
                let fx = px as f32;
                let w0 = ((y1 - y2) * (fx - x2) + (x2 - x1) * (fy - y2)) / denom;
                let w1 = ((y2 - y0) * (fx - x2) + (x0 - x2) * (fy - y2)) / denom;
                let w2 = 1.0 - w0 - w1;
                if w0 >= -0.02 && w1 >= -0.02 && w2 >= -0.02 {
                    let z = w0 * z0 + w1 * z1 + w2 * z2;
                    let idx = py * w + px;
                    if z_buf[idx].is_nan() || z > z_buf[idx] {
                        z_buf[idx] = z;
                    }
                    hit = true;
                }
            }
        }
        if !hit {
            for (vx, vy, vz) in [(x0, y0, z0), (x1, y1, z1), (x2, y2, z2)] {
                let px = (vx.round() as isize).clamp(0, (w as isize) - 1) as usize;
                let py = (vy.round() as isize).clamp(0, (h as isize) - 1) as usize;
                let idx = py * w + px;
                if z_buf[idx].is_nan() || vz > z_buf[idx] {
                    z_buf[idx] = vz;
                }
            }
        }
    }

    // Fill any sparse/unhit grid pixels via iterative neighbor averaging
    fill_nan_cells(&mut z_buf, w, h, (min_z + max_z) * 0.5);

    let mut bounds = GeoBounds::default_for(body, w, h);
    // Preserve isotropic 3D mesh units so slope angles match the true STL geometry
    let m_per_px_x = (span_x as f64) / ((w.saturating_sub(1)).max(1) as f64);
    let m_per_px_y = (span_y as f64) / ((h.saturating_sub(1)).max(1) as f64);
    bounds.meters_per_pixel_x = m_per_px_x.max(1e-4);
    bounds.meters_per_pixel_y = m_per_px_y.max(1e-4);

    let source_format = if is_ascii {
        format!("3D Mesh STL (ASCII, {} tris, {}x{})", triangles.len(), w, h)
    } else {
        format!("3D Mesh STL (Binary, {} tris, {}x{})", triangles.len(), w, h)
    };

    Ok(TerrainGrid {
        width: w,
        height: h,
        elevation: z_buf,
        bounds,
        source_format,
        has_geotiff_tags: false,
        nodata_value: None,
    })
}

fn is_ascii_stl(bytes: &[u8]) -> bool {
    if bytes.len() >= 84 {
        let num_tris = u32::from_le_bytes([bytes[80], bytes[81], bytes[82], bytes[83]]) as usize;
        if 84 + num_tris * 50 == bytes.len() {
            return false;
        }
    }
    let prefix_len = bytes.len().min(512);
    if let Ok(s) = std::str::from_utf8(&bytes[..prefix_len]) {
        s.trim_start().starts_with("solid") && s.contains("facet")
    } else {
        false
    }
}

fn parse_binary_stl(bytes: &[u8]) -> Result<Vec<Triangle3D>, String> {
    if bytes.len() < 84 {
        return Err("Binary STL header truncated (< 84 bytes)".to_string());
    }
    let num_tris = u32::from_le_bytes([bytes[80], bytes[81], bytes[82], bytes[83]]) as usize;
    let max_possible = (bytes.len() - 84) / 50;
    let count = num_tris.min(max_possible);
    let mut tris = Vec::with_capacity(count);
    for i in 0..count {
        let off = 84 + i * 50;
        let rf = |k: usize| -> f32 {
            let p = off + k * 4;
            f32::from_le_bytes([bytes[p], bytes[p + 1], bytes[p + 2], bytes[p + 3]])
        };
        tris.push(Triangle3D {
            v0: [rf(3), rf(4), rf(5)],
            v1: [rf(6), rf(7), rf(8)],
            v2: [rf(9), rf(10), rf(11)],
        });
    }
    Ok(tris)
}

fn parse_ascii_stl(bytes: &[u8]) -> Result<Vec<Triangle3D>, String> {
    let text = std::str::from_utf8(bytes).map_err(|_| "Invalid UTF-8 in ASCII STL")?;
    let mut tris = Vec::new();
    let mut verts: Vec<[f32; 3]> = Vec::with_capacity(3);
    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("vertex") {
            let mut parts = rest.split_whitespace();
            if let (Some(sx), Some(sy), Some(sz)) = (parts.next(), parts.next(), parts.next()) {
                if let (Ok(x), Ok(y), Ok(z)) = (sx.parse::<f32>(), sy.parse::<f32>(), sz.parse::<f32>())
                {
                    verts.push([x, y, z]);
                    if verts.len() == 3 {
                        tris.push(Triangle3D {
                            v0: verts[0],
                            v1: verts[1],
                            v2: verts[2],
                        });
                        verts.clear();
                    }
                }
            }
        }
    }
    Ok(tris)
}

fn fill_nan_cells(buf: &mut [f32], w: usize, h: usize, fallback: f32) {
    if !buf.iter().any(|v| v.is_nan()) {
        return;
    }
    for _pass in 0..8 {
        let mut any_nan = false;
        let snapshot = buf.to_vec();
        for y in 0..h {
            for x in 0..w {
                let idx = y * w + x;
                if snapshot[idx].is_nan() {
                    let mut sum = 0.0f32;
                    let mut cnt = 0usize;
                    for dy in -1i32..=1 {
                        for dx in -1i32..=1 {
                            if dx == 0 && dy == 0 {
                                continue;
                            }
                            let nx = x as i32 + dx;
                            let ny = y as i32 + dy;
                            if nx >= 0 && nx < w as i32 && ny >= 0 && ny < h as i32 {
                                let v = snapshot[(ny as usize) * w + (nx as usize)];
                                if !v.is_nan() {
                                    sum += v;
                                    cnt += 1;
                                }
                            }
                        }
                    }
                    if cnt > 0 {
                        buf[idx] = sum / (cnt as f32);
                    } else {
                        any_nan = true;
                    }
                }
            }
        }
        if !any_nan {
            break;
        }
    }
    for v in buf.iter_mut() {
        if v.is_nan() {
            *v = fallback;
        }
    }
}
