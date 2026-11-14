// AI-assisted
//! Custom binary STL parser with vertex welding, Z-up to Y-up coordinate conversion,
//! and `DemExtent`-based planar UV mapping.
//!
//! Mandatory laws from `AGENTS.md`:
//! 1. Blender / JMARS exports binary STL meshes in **Z-up** (`+X` = East, `+Y` = North, `+Z` = Up).
//! 2. On load, `parse_and_weld_stl` converts vertices to Bevy's right-handed **Y-up** space:
//!    `X_bevy = X_stl`, `Y_bevy = Z_stl`, `Z_bevy = -Y_stl`.
//! 3. Planar UVs are computed **only** from `(X_stl, Y_stl)` against `DemExtent`:
//!    `u = (X_stl - min_x) / (max_x - min_x)`
//!    `v = (max_y - Y_stl) / (max_y - min_y)`
//! 4. Maximum **50,000 triangles** per site; always use `u32` vertex indices.

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

/// Welded triangle mesh ready for direct upload into Bevy (`Indices::U32`).
#[derive(Debug, Clone, PartialEq)]
pub struct WeldedMesh {
    /// Vertex positions in Bevy right-handed Y-up space (`[X_stl, Z_stl, -Y_stl]`).
    pub positions: Vec<[f32; 3]>,
    /// Smooth unit vertex normals in Bevy right-handed Y-up space.
    pub normals: Vec<[f32; 3]>,
    /// Planar UV coordinates `[u, v]` derived strictly from `DemExtent`.
    pub uvs: Vec<[f32; 2]>,
    /// Triangle vertex indices (`3 * triangle_count` entries, `u32`).
    pub indices: Vec<u32>,
    /// Original number of triangles parsed from the binary STL.
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

/// Parses a binary STL byte slice, welds shared vertices, transforms coordinates from
/// Z-up to Bevy Y-up, and computes planar UV coordinates strictly against `extent`.
pub fn parse_and_weld_stl(bytes: &[u8], extent: &DemExtent) -> Result<WeldedMesh, StlError> {
    if !extent.is_valid() {
        return Err(StlError::InvalidDemExtent);
    }

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

    // If the file starts with "solid " and its length does not match the binary STL formula,
    // report an explicit ASCII STL error before checking triangle count bounds.
    if bytes.starts_with(b"solid ") && bytes.len() != expected_len {
        return Err(StlError::AsciiFormatNotSupported);
    }

    if triangle_count == 0 {
        return Err(StlError::EmptyMesh);
    }

    if triangle_count > MAX_TRIANGLES {
        return Err(StlError::TriangleBudgetExceeded {
            count: triangle_count,
            max: MAX_TRIANGLES,
        });
    }

    if bytes.len() != expected_len {
        return Err(StlError::UnexpectedBufferLength {
            expected: expected_len,
            actual: bytes.len(),
        });
    }

    let est_vertices = (triangle_count as usize).div_ceil(2).max(3);
    let mut positions: Vec<[f32; 3]> = Vec::with_capacity(est_vertices);
    let mut normals: Vec<[f32; 3]> = Vec::with_capacity(est_vertices);
    let mut uvs: Vec<[f32; 2]> = Vec::with_capacity(est_vertices);
    let mut indices: Vec<u32> = Vec::with_capacity((triangle_count as usize) * 3);
    let mut weld_map: HashMap<(i64, i64, i64), u32> = HashMap::with_capacity(est_vertices);

    let mut cursor = STL_HEADER_BYTES + 4;

    for tri_idx in 0..triangle_count {
        // Bytes 0..12 of the triangle record are the facet normal in Z-up STL space.
        // We compute area-weighted smooth normals directly from the welded Bevy Y-up vertices,
        // using the STL facet normal as a fallback if a triangle is degenerate.
        let nx_stl = read_f32_le(bytes, cursor);
        let ny_stl = read_f32_le(bytes, cursor + 4);
        let nz_stl = read_f32_le(bytes, cursor + 8);

        let mut tri_vertex_indices = [0u32; 3];
        for (v_slot, item) in tri_vertex_indices.iter_mut().enumerate() {
            let v_offset = cursor + 12 + v_slot * 12;
            let x_stl = read_f32_le(bytes, v_offset);
            let y_stl = read_f32_le(bytes, v_offset + 4);
            let z_stl = read_f32_le(bytes, v_offset + 8);

            if !x_stl.is_finite() || !y_stl.is_finite() || !z_stl.is_finite() {
                return Err(StlError::NonFiniteVertex {
                    triangle_index: tri_idx,
                });
            }

            let key = quantize_vertex(x_stl, y_stl, z_stl);
            let idx = if let Some(&existing_idx) = weld_map.get(&key) {
                existing_idx
            } else {
                let new_idx = positions.len() as u32;
                positions.push(stl_to_bevy_position(x_stl, y_stl, z_stl));
                normals.push([0.0, 0.0, 0.0]);
                uvs.push(extent.compute_uv(x_stl, y_stl));
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

    Ok(WeldedMesh {
        positions,
        normals,
        uvs,
        indices,
        triangle_count,
    })
}

/// Encodes a list of Z-up triangles (`[[x, y, z]; 3]`) into a valid binary STL byte buffer.
/// Primarily used by unit tests and synthetic test-site verification.
pub fn encode_binary_stl(triangles_z_up: &[[[f32; 3]; 3]]) -> Vec<u8> {
    let count = triangles_z_up.len() as u32;
    let mut buf = Vec::with_capacity(STL_HEADER_BYTES + 4 + triangles_z_up.len() * STL_TRIANGLE_BYTES);

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

        for f in [nx, ny, nz, v0[0], v0[1], v0[2], v1[0], v1[1], v1[2], v2[0], v2[1], v2[2]] {
            buf.extend_from_slice(&f.to_le_bytes());
        }
        buf.extend_from_slice(&0u16.to_le_bytes());
    }

    buf
}
