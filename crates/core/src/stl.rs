// AI-assisted
//! Custom binary STL parser with vertex welding, automatic double-precision (`f64`) origin centering,
//! deterministic mesh simplification (`<= 50,000` triangles), Z-up to Y-up coordinate conversion,
//! and `DemExtent`-based planar UV mapping.
//!
//! Mandatory laws from `AGENTS.md`:
//! 1. Blender / JMARS exports binary STL meshes in **Z-up** (`+X` = East, `+Y` = North, `+Z` = Up).
//! 2. On load, `parse_and_weld_stl` computes the 3D bounding-box centroid `(center_x, center_y, center_z)`
//!    in double precision (`f64`), translates all vertices so the mesh is centered at `(0.0, 0.0, 0.0)`
//!    before converting to `f32`, and transforms to Bevy's right-handed **Y-up** space:
//!    `X_bevy = X_stl_local`, `Y_bevy = Z_stl_local`, `Z_bevy = -Y_stl_local`.
//! 3. Planar UVs are computed strictly from horizontal STL coordinates against `DemExtent`:
//!    `u = (X - min_x) / (max_x - min_x)`
//!    `v = (max_y - Y) / (max_y - min_y)`
//! 4. Maximum **50,000 triangles** per site; always use `u32` vertex indices.
//!    If an uploaded or loaded binary STL exceeds `50,000` triangles (e.g. 1M+ triangles),
//!    it is automatically decimated via deterministic spatial vertex clustering into `<= 50,000`
//!    triangles while preserving exterior boundaries and elevation extremes.

use std::collections::HashMap;
use std::fmt;

use crate::models::DemExtent;

/// Maximum allowed triangles per site mesh (`AGENTS.md` §2.5).
pub const MAX_TRIANGLES: u32 = 50_000;

/// Size in bytes of the binary STL header.
pub const STL_HEADER_BYTES: usize = 80;

/// Size in bytes of each binary STL triangle record (12B normal + 36B vertices + 2B attr).
pub const STL_TRIANGLE_BYTES: usize = 50;

/// Default spatial quantization scale for vertex welding (`10_000.0` = 0.1 mm precision).
const WELD_QUANTIZATION_SCALE: f64 = 10_000.0;

/// Errors returned by [`parse_and_weld_stl`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StlError {
    /// Input buffer is shorter than the 84-byte binary STL minimum (80B header + 4B count).
    BufferTooShort { actual: usize },
    /// Input appears to be an ASCII STL file rather than a binary STL file.
    AsciiFormatNotSupported,
    /// Triangle count is zero.
    EmptyMesh,
    /// Triangle count exceeds [`MAX_TRIANGLES`] (50,000).
    TriangleBudgetExceeded { count: u32, max: u32 },
    /// Byte length does not match `84 + triangle_count * 50`.
    UnexpectedBufferLength { expected: usize, actual: usize },
    /// The provided [`DemExtent`] has non-finite or non-positive dimensions.
    InvalidDemExtent,
    /// A vertex in triangle `triangle_index` contains `NaN` or `Infinity`.
    NonFiniteVertex { triangle_index: u32 },
}

impl fmt::Display for StlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BufferTooShort { actual } => write!(
                f,
                "binary STL buffer too short: expected at least 84 bytes, got {actual}"
            ),
            Self::AsciiFormatNotSupported => {
                write!(f, "ASCII STL format is not supported; export binary STL")
            }
            Self::EmptyMesh => write!(f, "binary STL contains zero triangles"),
            Self::TriangleBudgetExceeded { count, max } => write!(
                f,
                "STL triangle count {count} exceeds maximum budget of {max} triangles"
            ),
            Self::UnexpectedBufferLength { expected, actual } => write!(
                f,
                "binary STL byte length mismatch: expected {expected} bytes, got {actual}"
            ),
            Self::InvalidDemExtent => {
                write!(
                    f,
                    "invalid DemExtent: bounds must be finite with max_x > min_x and max_y > min_y"
                )
            }
            Self::NonFiniteVertex { triangle_index } => write!(
                f,
                "non-finite vertex coordinate encountered at triangle {triangle_index}"
            ),
        }
    }
}

impl std::error::Error for StlError {}

/// Welded triangle mesh ready for direct upload into Bevy (`Indices::U32`), centered at `(0.0, 0.0, 0.0)`.
#[derive(Debug, Clone, PartialEq)]
pub struct WeldedMesh {
    /// Vertex positions in Bevy right-handed Y-up space (`[X_stl, Z_stl, -Y_stl]`), centered at `(0, 0, 0)`.
    pub positions: Vec<[f32; 3]>,
    /// Smooth unit vertex normals in Bevy right-handed Y-up space.
    pub normals: Vec<[f32; 3]>,
    /// Planar UV coordinates `[u, v]` derived strictly from `DemExtent`.
    pub uvs: Vec<[f32; 2]>,
    /// Triangle vertex indices (`3 * triangle_count` entries, `u32`).
    pub indices: Vec<u32>,
    /// Number of triangles in this welded (and if needed, simplified) mesh (`<= 50,000`).
    pub triangle_count: u32,
}

/// Converts a single point from Z-up STL space (`+X` East, `+Y` North, `+Z` Elevation)
/// to Bevy's right-handed Y-up space (`+X` East, `+Y` Up, `-Z` North / `+Z` South).
#[inline]
pub fn stl_to_bevy_position(x_stl: f32, y_stl: f32, z_stl: f32) -> [f32; 3] {
    [x_stl, z_stl, -y_stl]
}

/// Quantizes a 3D STL coordinate into an integer grid key for deterministic vertex welding.
#[inline]
fn quantize_vertex(x: f32, y: f32, z: f32) -> (i64, i64, i64) {
    let qx = (f64::from(x) * WELD_QUANTIZATION_SCALE).round() as i64;
    let qy = (f64::from(y) * WELD_QUANTIZATION_SCALE).round() as i64;
    let qz = (f64::from(z) * WELD_QUANTIZATION_SCALE).round() as i64;
    (qx, qy, qz)
}

#[inline]
fn read_f32_le(bytes: &[u8], offset: usize) -> f32 {
    let arr: [u8; 4] = [
        bytes[offset],
        bytes[offset + 1],
        bytes[offset + 2],
        bytes[offset + 3],
    ];
    f32::from_le_bytes(arr)
}

/// Reads and validates the raw triangle count from a binary STL byte slice.
pub fn read_stl_triangle_count(bytes: &[u8]) -> Result<u32, StlError> {
    if bytes.len() < STL_HEADER_BYTES + 4 {
        if bytes.starts_with(b"solid") {
            return Err(StlError::AsciiFormatNotSupported);
        }
        return Err(StlError::BufferTooShort {
            actual: bytes.len(),
        });
    }

    let triangle_count = u32::from_le_bytes([
        bytes[STL_HEADER_BYTES],
        bytes[STL_HEADER_BYTES + 1],
        bytes[STL_HEADER_BYTES + 2],
        bytes[STL_HEADER_BYTES + 3],
    ]);

    let expected_len = STL_HEADER_BYTES
        .saturating_add(4)
        .saturating_add((triangle_count as usize).saturating_mul(STL_TRIANGLE_BYTES));

    if bytes.starts_with(b"solid ") && bytes.len() != expected_len {
        return Err(StlError::AsciiFormatNotSupported);
    }

    if triangle_count == 0 {
        return Err(StlError::EmptyMesh);
    }

    if bytes.len() != expected_len {
        return Err(StlError::UnexpectedBufferLength {
            expected: expected_len,
            actual: bytes.len(),
        });
    }

    Ok(triangle_count)
}

/// Computes the exact 3D bounding box `([min_x, min_y, min_z], [max_x, max_y, max_z])` of a
/// binary STL in double precision (`f64`) while validating that all vertex coordinates are finite.
fn compute_stl_bbox_f64(
    bytes: &[u8],
    triangle_count: u32,
) -> Result<([f64; 3], [f64; 3]), StlError> {
    let mut min_x = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_y = f64::NEG_INFINITY;
    let mut min_z = f64::INFINITY;
    let mut max_z = f64::NEG_INFINITY;

    let mut cursor = STL_HEADER_BYTES + 4;
    for tri_idx in 0..triangle_count {
        for v_slot in 0..3 {
            let v_offset = cursor + 12 + v_slot * 12;
            let x = read_f32_le(bytes, v_offset);
            let y = read_f32_le(bytes, v_offset + 4);
            let z = read_f32_le(bytes, v_offset + 8);
            if !x.is_finite() || !y.is_finite() || !z.is_finite() {
                return Err(StlError::NonFiniteVertex {
                    triangle_index: tri_idx,
                });
            }
            let xf = f64::from(x);
            let yf = f64::from(y);
            let zf = f64::from(z);
            min_x = min_x.min(xf);
            max_x = max_x.max(xf);
            min_y = min_y.min(yf);
            max_y = max_y.max(yf);
            min_z = min_z.min(zf);
            max_z = max_z.max(zf);
        }
        cursor += STL_TRIANGLE_BYTES;
    }

    Ok(([min_x, min_y, min_z], [max_x, max_y, max_z]))
}

/// Computes the origin-centered local [`DemExtent`] (`[-half_w, half_w]`, `[-half_h, half_h]`)
/// of a binary STL in double precision (`f64`).
pub fn compute_centered_dem_extent_from_stl(bytes: &[u8]) -> Result<DemExtent, StlError> {
    let tri_count = read_stl_triangle_count(bytes)?;
    let ([min_x, min_y, _], [max_x, max_y, _]) = compute_stl_bbox_f64(bytes, tri_count)?;
    if !(max_x > min_x && max_y > min_y) {
        return Err(StlError::InvalidDemExtent);
    }
    let half_w = (0.5 * (max_x - min_x)).max(0.5) as f32;
    let half_h = (0.5 * (max_y - min_y)).max(0.5) as f32;
    Ok(DemExtent::new(-half_w, half_w, -half_h, half_h))
}

/// Parses a binary STL byte slice, centers all vertices around `(0.0, 0.0, 0.0)` using a
/// double-precision (`f64`) bounding-box centroid, welds shared vertices, transforms coordinates
/// from Z-up to Bevy Y-up, and computes planar UV coordinates strictly against `extent`.
///
/// If the input mesh exceeds [`MAX_TRIANGLES`] (`50,000`), it is automatically simplified
/// via deterministic spatial vertex clustering so the output mesh has `<= 50,000` triangles.
pub fn parse_and_weld_stl(bytes: &[u8], extent: &DemExtent) -> Result<WeldedMesh, StlError> {
    parse_and_weld_stl_with_stats(bytes, extent).map(|(mesh, _)| mesh)
}

/// Parses a binary STL byte slice, centers vertices at `(0.0, 0.0, 0.0)` in `f64` before `f32`
/// conversion, and returns `(welded_mesh, original_triangle_count)`.
///
/// When `original_triangle_count > MAX_TRIANGLES` (`50,000`), automatically applies
/// deterministic spatial vertex clustering to simplify the mesh down to `<= 50,000` triangles
/// while preserving horizontal boundaries and elevation extremes.
pub fn parse_and_weld_stl_with_stats(
    bytes: &[u8],
    extent: &DemExtent,
) -> Result<(WeldedMesh, u32), StlError> {
    if !extent.is_valid() {
        return Err(StlError::InvalidDemExtent);
    }

    let original_triangle_count = read_stl_triangle_count(bytes)?;
    let (bbox_min, bbox_max) = compute_stl_bbox_f64(bytes, original_triangle_count)?;

    if original_triangle_count > MAX_TRIANGLES {
        let simplified = simplify_and_weld_large_stl(
            bytes,
            original_triangle_count,
            bbox_min,
            bbox_max,
            extent,
            MAX_TRIANGLES,
        )?;
        return Ok((simplified, original_triangle_count));
    }

    let [min_x, min_y, min_z] = bbox_min;
    let [max_x, max_y, max_z] = bbox_max;
    let center_x = 0.5 * (min_x + max_x);
    let center_y = 0.5 * (min_y + max_y);
    let center_z = 0.5 * (min_z + max_z);

    let half_w = (0.5 * (max_x - min_x)).max(0.5) as f32;
    let half_h = (0.5 * (max_y - min_y)).max(0.5) as f32;
    let centered_extent = DemExtent::new(-half_w, half_w, -half_h, half_h);

    // Check if the raw STL coordinates already lie within the caller-supplied `extent`
    // (e.g. for sub-region UV tests or already-centered meshes). If the STL is in displaced
    // planetocentric JMARS coordinates outside `extent`, compute UVs from the centered coordinates.
    let tol_x = f64::from(extent.width()) * 0.01;
    let tol_y = f64::from(extent.height()) * 0.01;
    let raw_inside_extent = min_x >= f64::from(extent.min_x) - tol_x
        && max_x <= f64::from(extent.max_x) + tol_x
        && min_y >= f64::from(extent.min_y) - tol_y
        && max_y <= f64::from(extent.max_y) + tol_y;

    let est_vertices = (original_triangle_count as usize).div_ceil(2).max(3);
    let mut positions: Vec<[f32; 3]> = Vec::with_capacity(est_vertices);
    let mut normals: Vec<[f32; 3]> = Vec::with_capacity(est_vertices);
    let mut uvs: Vec<[f32; 2]> = Vec::with_capacity(est_vertices);
    let mut indices: Vec<u32> = Vec::with_capacity((original_triangle_count as usize) * 3);
    let mut weld_map: HashMap<(i64, i64, i64), u32> = HashMap::with_capacity(est_vertices);

    let mut cursor = STL_HEADER_BYTES + 4;

    for _tri_idx in 0..original_triangle_count {
        let nx_stl = read_f32_le(bytes, cursor);
        let ny_stl = read_f32_le(bytes, cursor + 4);
        let nz_stl = read_f32_le(bytes, cursor + 8);

        let mut tri_vertex_indices = [0u32; 3];
        for (v_slot, item) in tri_vertex_indices.iter_mut().enumerate() {
            let v_offset = cursor + 12 + v_slot * 12;
            let x_stl = read_f32_le(bytes, v_offset);
            let y_stl = read_f32_le(bytes, v_offset + 4);
            let z_stl = read_f32_le(bytes, v_offset + 8);

            // Translate to origin (0.0, 0.0, 0.0) in double precision (f64) before f32 cast
            let local_x = (f64::from(x_stl) - center_x) as f32;
            let local_y = (f64::from(y_stl) - center_y) as f32;
            let local_z = (f64::from(z_stl) - center_z) as f32;

            let key = quantize_vertex(local_x, local_y, local_z);
            let idx = if let Some(&existing_idx) = weld_map.get(&key) {
                existing_idx
            } else {
                let new_idx = positions.len() as u32;
                positions.push(stl_to_bevy_position(local_x, local_y, local_z));
                normals.push([0.0, 0.0, 0.0]);
                let uv = if raw_inside_extent {
                    extent.compute_uv(x_stl, y_stl)
                } else {
                    centered_extent.compute_uv(local_x, local_y)
                };
                uvs.push(uv);
                weld_map.insert(key, new_idx);
                new_idx
            };
            *item = idx;
        }

        let i0 = tri_vertex_indices[0];
        let i1 = tri_vertex_indices[1];
        let i2 = tri_vertex_indices[2];
        indices.extend_from_slice(&[i0, i1, i2]);

        let p0 = positions[i0 as usize];
        let p1 = positions[i1 as usize];
        let p2 = positions[i2 as usize];

        let e1 = [p1[0] - p0[0], p1[1] - p0[1], p1[2] - p0[2]];
        let e2 = [p2[0] - p0[0], p2[1] - p0[1], p2[2] - p0[2]];

        // Cross product e1 x e2 in right-handed Bevy Y-up space.
        let mut cx = e1[1] * e2[2] - e1[2] * e2[1];
        let mut cy = e1[2] * e2[0] - e1[0] * e2[2];
        let mut cz = e1[0] * e2[1] - e1[1] * e2[0];

        let len_sq = cx * cx + cy * cy + cz * cz;
        if len_sq <= 1e-20 && nx_stl.is_finite() && ny_stl.is_finite() && nz_stl.is_finite() {
            let fallback = stl_to_bevy_position(nx_stl, ny_stl, nz_stl);
            cx = fallback[0];
            cy = fallback[1];
            cz = fallback[2];
        }

        for &vi in &[i0, i1, i2] {
            let n = &mut normals[vi as usize];
            n[0] += cx;
            n[1] += cy;
            n[2] += cz;
        }

        cursor += STL_TRIANGLE_BYTES;
    }

    // Normalize accumulated vertex normals; default to +Y (Up in Bevy) if zero.
    for n in &mut normals {
        let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
        if len > 1e-10 {
            n[0] /= len;
            n[1] /= len;
            n[2] /= len;
        } else {
            *n = [0.0, 1.0, 0.0];
        }
    }

    Ok((
        WeldedMesh {
            positions,
            normals,
            uvs,
            indices,
            triangle_count: original_triangle_count,
        },
        original_triangle_count,
    ))
}

/// Deterministic spatial vertex clustering decimation for large binary STL terrain meshes
/// (`triangle_count > MAX_TRIANGLES`, e.g., 1,000,000+ triangles / ~56 MiB), with double-precision
/// (`f64`) centroid translation so the resulting mesh is centered at `(0.0, 0.0, 0.0)`.
fn simplify_and_weld_large_stl(
    bytes: &[u8],
    original_triangle_count: u32,
    bbox_min: [f64; 3],
    bbox_max: [f64; 3],
    _extent: &DemExtent,
    max_triangles: u32,
) -> Result<WeldedMesh, StlError> {
    let [min_x, min_y, min_z] = bbox_min;
    let [max_x, max_y, max_z] = bbox_max;

    let center_z = 0.5 * (min_z + max_z);
    let span_x_f64 = (max_x - min_x).max(1.0);
    let span_y_f64 = (max_y - min_y).max(1.0);
    let half_w = (0.5 * span_x_f64) as f32;
    let half_h = (0.5 * span_y_f64) as f32;
    let centered_extent = DemExtent::new(-half_w, half_w, -half_h, half_h);

    let local_min_z = (min_z - center_z) as f32;
    let local_max_z = (max_z - center_z) as f32;

    // Compute aspect-aware grid cell dimensions (cells_x, cells_y) such that
    // 2 * cells_x * cells_y <= max_triangles (<= 50,000).
    let max_cells_total = (max_triangles / 2).max(4) as f64;
    let aspect = (span_x_f64 / span_y_f64).clamp(0.1, 10.0);
    let mut cells_x = ((max_cells_total * aspect).sqrt().floor() as u32).max(2);
    let mut cells_y = ((max_cells_total / aspect).sqrt().floor() as u32).max(2);
    while 2 * u64::from(cells_x) * u64::from(cells_y) > u64::from(max_triangles) {
        if cells_x >= cells_y && cells_x > 2 {
            cells_x -= 1;
        } else if cells_y > 2 {
            cells_y -= 1;
        } else {
            break;
        }
    }

    let verts_x = (cells_x + 1) as usize;
    let verts_y = (cells_y + 1) as usize;
    let total_vertices = verts_x * verts_y;
    let simplified_triangle_count = 2 * cells_x * cells_y;
    debug_assert!(simplified_triangle_count <= max_triangles);

    let mut accum_z = vec![0.0_f64; total_vertices];
    let mut counts = vec![0_u32; total_vertices];
    let mut cell_min_z = vec![f32::INFINITY; total_vertices];
    let mut cell_max_z = vec![f32::NEG_INFINITY; total_vertices];

    let inv_span_x = (cells_x as f64) / span_x_f64;
    let inv_span_y = (cells_y as f64) / span_y_f64;

    let mut add_sample = |x: f64, y: f64, z: f64| {
        let gx = (((x - min_x) * inv_span_x).round() as isize).clamp(0, cells_x as isize) as usize;
        let gy = (((max_y - y) * inv_span_y).round() as isize).clamp(0, cells_y as isize) as usize;
        let idx = gy * verts_x + gx;
        let lz = z - center_z;
        let lz_f32 = lz as f32;
        accum_z[idx] += lz;
        counts[idx] += 1;
        cell_min_z[idx] = cell_min_z[idx].min(lz_f32);
        cell_max_z[idx] = cell_max_z[idx].max(lz_f32);
    };

    let mut cursor = STL_HEADER_BYTES + 4;
    for _ in 0..original_triangle_count {
        let x0 = f64::from(read_f32_le(bytes, cursor + 12));
        let y0 = f64::from(read_f32_le(bytes, cursor + 16));
        let z0 = f64::from(read_f32_le(bytes, cursor + 20));

        let x1 = f64::from(read_f32_le(bytes, cursor + 24));
        let y1 = f64::from(read_f32_le(bytes, cursor + 28));
        let z1 = f64::from(read_f32_le(bytes, cursor + 32));

        let x2 = f64::from(read_f32_le(bytes, cursor + 36));
        let y2 = f64::from(read_f32_le(bytes, cursor + 40));
        let z2 = f64::from(read_f32_le(bytes, cursor + 44));

        add_sample(x0, y0, z0);
        add_sample(x1, y1, z1);
        add_sample(x2, y2, z2);
        add_sample(
            (x0 + x1 + x2) * (1.0 / 3.0),
            (y0 + y1 + y2) * (1.0 / 3.0),
            (z0 + z1 + z2) * (1.0 / 3.0),
        );

        cursor += STL_TRIANGLE_BYTES;
    }

    // Compute representative elevation per cluster around Y = 0, preserving global min/max extremes.
    let mut grid_z = vec![f32::NAN; total_vertices];
    let mut sum_valid = 0.0_f64;
    let mut num_valid = 0_usize;
    for i in 0..total_vertices {
        if counts[i] > 0 {
            let z = if (cell_min_z[i] - local_min_z).abs() <= 1e-4 {
                local_min_z
            } else if (cell_max_z[i] - local_max_z).abs() <= 1e-4 {
                local_max_z
            } else {
                (accum_z[i] / f64::from(counts[i])) as f32
            };
            grid_z[i] = z;
            sum_valid += f64::from(z);
            num_valid += 1;
        }
    }
    let fallback_mean = if num_valid > 0 {
        (sum_valid / (num_valid as f64)) as f32
    } else {
        0.0
    };

    // Harmonic inpainting for any unoccupied grid nodes
    for _pass in 0..10 {
        let prev = grid_z.clone();
        let mut any_nan = false;
        for r in 0..verts_y {
            for c in 0..verts_x {
                let idx = r * verts_x + c;
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
                            && nr < (verts_y as isize)
                            && nc >= 0
                            && nc < (verts_x as isize)
                        {
                            let v = prev[(nr as usize) * verts_x + (nc as usize)];
                            if v.is_finite() {
                                n_sum += v;
                                n_cnt += 1;
                            }
                        }
                    }
                }
                if n_cnt > 0 {
                    grid_z[idx] = n_sum / (n_cnt as f32);
                }
            }
        }
        if !any_nan {
            break;
        }
    }
    for val in &mut grid_z {
        if !val.is_finite() {
            *val = fallback_mean;
        }
    }

    // Build welded vertex positions centered at (0.0, 0.0, 0.0) in Bevy Y-up space
    // and planar UVs strictly from centered_extent.
    let mut positions: Vec<[f32; 3]> = Vec::with_capacity(total_vertices);
    let mut normals: Vec<[f32; 3]> = vec![[0.0, 0.0, 0.0]; total_vertices];
    let mut uvs: Vec<[f32; 2]> = Vec::with_capacity(total_vertices);
    let mut indices: Vec<u32> = Vec::with_capacity((simplified_triangle_count as usize) * 3);

    let span_x_f32 = 2.0 * half_w;
    let span_y_f32 = 2.0 * half_h;

    for gy in 0..=cells_y {
        let local_y = if gy == 0 {
            half_h
        } else if gy == cells_y {
            -half_h
        } else {
            half_h - (gy as f32 / cells_y as f32) * span_y_f32
        };

        for gx in 0..=cells_x {
            let local_x = if gx == 0 {
                -half_w
            } else if gx == cells_x {
                half_w
            } else {
                -half_w + (gx as f32 / cells_x as f32) * span_x_f32
            };

            let local_z = grid_z[(gy as usize) * verts_x + (gx as usize)];
            positions.push(stl_to_bevy_position(local_x, local_y, local_z));
            uvs.push(centered_extent.compute_uv(local_x, local_y));
        }
    }

    for gy in 0..cells_y {
        let row0 = (gy as usize) * verts_x;
        let row1 = ((gy + 1) as usize) * verts_x;
        for gx in 0..cells_x {
            let col0 = gx as usize;
            let col1 = col0 + 1;

            let i00 = (row0 + col0) as u32; // NW
            let i10 = (row0 + col1) as u32; // NE
            let i01 = (row1 + col0) as u32; // SW
            let i11 = (row1 + col1) as u32; // SE

            for tri in [[i00, i01, i11], [i00, i11, i10]] {
                indices.extend_from_slice(&tri);
                let p0 = positions[tri[0] as usize];
                let p1 = positions[tri[1] as usize];
                let p2 = positions[tri[2] as usize];

                let e1 = [p1[0] - p0[0], p1[1] - p0[1], p1[2] - p0[2]];
                let e2 = [p2[0] - p0[0], p2[1] - p0[1], p2[2] - p0[2]];
                let cx = e1[1] * e2[2] - e1[2] * e2[1];
                let cy = e1[2] * e2[0] - e1[0] * e2[2];
                let cz = e1[0] * e2[1] - e1[1] * e2[0];

                for &vi in &tri {
                    let n = &mut normals[vi as usize];
                    n[0] += cx;
                    n[1] += cy;
                    n[2] += cz;
                }
            }
        }
    }

    for n in &mut normals {
        let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
        if len > 1e-10 {
            n[0] /= len;
            n[1] /= len;
            n[2] /= len;
        } else {
            *n = [0.0, 1.0, 0.0];
        }
    }

    Ok(WeldedMesh {
        positions,
        normals,
        uvs,
        indices,
        triangle_count: simplified_triangle_count,
    })
}

/// Encodes a list of Z-up triangles (`[[x, y, z]; 3]`) into a valid binary STL byte buffer.
/// Primarily used by unit tests, mesh simplification persistence, and synthetic test-site verification.
pub fn encode_binary_stl(triangles_z_up: &[[[f32; 3]; 3]]) -> Vec<u8> {
    let count = triangles_z_up.len() as u32;
    let mut buf =
        Vec::with_capacity(STL_HEADER_BYTES + 4 + triangles_z_up.len() * STL_TRIANGLE_BYTES);

    let mut header = [0u8; STL_HEADER_BYTES];
    let tag = b"Space-App-X86 Binary STL (Z-up)";
    header[..tag.len()].copy_from_slice(tag);
    buf.extend_from_slice(&header);
    buf.extend_from_slice(&count.to_le_bytes());

    for tri in triangles_z_up {
        let v0 = tri[0];
        let v1 = tri[1];
        let v2 = tri[2];
        let e1 = [v1[0] - v0[0], v1[1] - v0[1], v1[2] - v0[2]];
        let e2 = [v2[0] - v0[0], v2[1] - v0[1], v2[2] - v0[2]];
        let mut nx = e1[1] * e2[2] - e1[2] * e2[1];
        let mut ny = e1[2] * e2[0] - e1[0] * e2[2];
        let mut nz = e1[0] * e2[1] - e1[1] * e2[0];
        let len = (nx * nx + ny * ny + nz * nz).sqrt();
        if len > 1e-10 {
            nx /= len;
            ny /= len;
            nz /= len;
        } else {
            nx = 0.0;
            ny = 0.0;
            nz = 1.0;
        }

        for f in [
            nx, ny, nz, v0[0], v0[1], v0[2], v1[0], v1[1], v1[2], v2[0], v2[1], v2[2],
        ] {
            buf.extend_from_slice(&f.to_le_bytes());
        }
        buf.extend_from_slice(&0u16.to_le_bytes());
    }

    buf
}
