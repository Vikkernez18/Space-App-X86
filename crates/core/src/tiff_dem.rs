// AI-assisted
//! In-memory TIFF / GeoTIFF elevation raster (DEM) parser and mesh generator.
//!
//! Reads elevation rasters (`u16`, `i16`, `f32`, etc.) from `std::io::Cursor<&[u8]>`
//! without `std::fs` or native C libraries so `analog_core` compiles cleanly to
//! `wasm32-unknown-unknown`.
//!
//! Enforces `AGENTS.md` coordinate, UV, and mesh budget laws:
//! - Automatically decimates high-resolution rasters so triangle count never exceeds
//!   [`MAX_TRIANGLES`] (`50,000` triangles, i.e., `<= 158x158` grid cells for a square DEM).
//! - Converts Z-up `(X_stl, Y_stl, Z_stl)` to Bevy Y-up `[X_stl, Z_stl, -Y_stl]`.
//! - Computes planar UVs strictly from [`DemExtent`]:
//!   `u = (X_stl - min_x) / (max_x - min_x)`, `v = (max_y - Y_stl) / (max_y - min_y)`.
//! - Returns the exact [`WeldedMesh`] structure produced by [`crate::stl::parse_and_weld_stl`].

use std::fmt;
use std::io::Cursor;

use tiff::decoder::{Decoder, DecodingResult};

use crate::models::DemExtent;
use crate::stl::{MAX_TRIANGLES, WeldedMesh, stl_to_bevy_position};

/// Maximum grid cells per side for a square DEM (`158 * 158 * 2 = 49,928 <= 50,000` triangles).
pub const MAX_SQUARE_GRID_CELLS: u32 = 158;

/// Errors returned by [`parse_dem_tiff`].
#[derive(Debug)]
pub enum DemTiffError {
    /// The provided [`DemExtent`] has non-finite or non-positive bounds.
    InvalidDemExtent,
    /// `vertical_exaggeration` is non-finite or `<= 0.0`.
    InvalidVerticalExaggeration(f32),
    /// Underlying TIFF decoding error.
    TiffDecode(tiff::TiffError),
    /// Raster dimensions are smaller than `2x2` pixels (cannot form triangles).
    DimensionsTooSmall { width: u32, height: u32 },
    /// Decoded pixel buffer is shorter than `width * height`.
    EmptyOrTruncatedBand { expected_pixels: usize, actual_samples: usize },
}

impl fmt::Display for DemTiffError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidDemExtent => write!(
                f,
                "invalid DemExtent: bounds must be finite with max_x > min_x and max_y > min_y"
            ),
            Self::InvalidVerticalExaggeration(v) => write!(
                f,
                "invalid vertical_exaggeration {v}: must be finite and > 0.0"
            ),
            Self::TiffDecode(err) => write!(f, "failed to decode DEM TIFF: {err}"),
            Self::DimensionsTooSmall { width, height } => write!(
                f,
                "DEM TIFF dimensions {width}x{height} are too small; need at least 2x2 pixels"
            ),
            Self::EmptyOrTruncatedBand {
                expected_pixels,
                actual_samples,
            } => write!(
                f,
                "DEM TIFF band truncated: expected at least {expected_pixels} samples, got {actual_samples}"
            ),
        }
    }
}

impl std::error::Error for DemTiffError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::TiffDecode(err) => Some(err),
            _ => None,
        }
    }
}

impl From<tiff::TiffError> for DemTiffError {
    fn from(value: tiff::TiffError) -> Self {
        Self::TiffDecode(value)
    }
}

/// Computes the minimum integer decimation stride `step >= 1` so that a raster of
/// `width x height` pixels produces at most [`MAX_TRIANGLES`] (`50,000`) triangles
/// (`<= 158x158` cells for a square raster).
#[inline]
pub fn compute_decimation_step(width: u32, height: u32) -> u32 {
    if width < 2 || height < 2 {
        return 1;
    }
    let raw_cols = width - 1;
    let raw_rows = height - 1;
    let mut step = 1u32;
    loop {
        let cells_x = u64::from(raw_cols.div_ceil(step));
        let cells_y = u64::from(raw_rows.div_ceil(step));
        if 2 * cells_x * cells_y <= u64::from(MAX_TRIANGLES) {
            return step;
        }
        step += 1;
    }
}

/// Extracts single-band elevation values (in `f32` meters) from a decoded TIFF buffer.
/// Supports `u16`, `i16`, `f32` (as well as `f64`, `u8`, `i8`, `u32`, `i32`).
fn extract_elevation_samples(
    decoded: DecodingResult,
    expected_pixels: usize,
) -> Result<Vec<f32>, DemTiffError> {
    macro_rules! convert_band {
        ($vec:expr, $conv:expr) => {{
            let actual = $vec.len();
            if actual < expected_pixels {
                return Err(DemTiffError::EmptyOrTruncatedBand {
                    expected_pixels,
                    actual_samples: actual,
                });
            }
            let spp = actual / expected_pixels;
            let mut out = Vec::with_capacity(expected_pixels);
            for i in 0..expected_pixels {
                let raw = $vec[i * spp];
                let val: f32 = $conv(raw);
                out.push(if val.is_finite() && val > -1.0e20 && val < 1.0e20 {
                    val
                } else {
                    0.0
                });
            }
            Ok(out)
        }};
    }

    match decoded {
        DecodingResult::F32(v) => convert_band!(v, |x: f32| x),
        DecodingResult::I16(v) => convert_band!(v, |x: i16| f32::from(x)),
        DecodingResult::U16(v) => convert_band!(v, |x: u16| f32::from(x)),
        DecodingResult::F64(v) => convert_band!(v, |x: f64| x as f32),
        DecodingResult::I32(v) => convert_band!(v, |x: i32| x as f32),
        DecodingResult::U32(v) => convert_band!(v, |x: u32| x as f32),
        DecodingResult::U8(v) => convert_band!(v, |x: u8| f32::from(x)),
        DecodingResult::I8(v) => convert_band!(v, |x: i8| f32::from(x)),
        DecodingResult::U64(v) => convert_band!(v, |x: u64| x as f32),
        DecodingResult::I64(v) => convert_band!(v, |x: i64| x as f32),
    }
}

/// Parses an in-memory TIFF/GeoTIFF DEM byte slice (`u16`, `i16`, or `f32`), applies automatic
/// grid decimation so the mesh never exceeds `50,000` triangles (`<= 158x158` cells), converts
/// vertices to Bevy Y-up space, and computes planar UVs strictly against `extent`.
pub fn parse_dem_tiff(
    bytes: &[u8],
    extent: &DemExtent,
    vertical_exaggeration: f32,
) -> Result<WeldedMesh, DemTiffError> {
    if !extent.is_valid() {
        return Err(DemTiffError::InvalidDemExtent);
    }
    if !vertical_exaggeration.is_finite() || vertical_exaggeration <= 0.0 {
        return Err(DemTiffError::InvalidVerticalExaggeration(
            vertical_exaggeration,
        ));
    }

    let cursor = Cursor::new(bytes);
    let mut decoder = Decoder::new(cursor)?;
    let (width, height) = decoder.dimensions()?;
    if width < 2 || height < 2 {
        return Err(DemTiffError::DimensionsTooSmall { width, height });
    }

    let expected_pixels = (width as usize).saturating_mul(height as usize);
    let decoded = decoder.read_image()?;
    let elevations = extract_elevation_samples(decoded, expected_pixels)?;

    let step = compute_decimation_step(width, height);
    let cells_x = (width - 1).div_ceil(step);
    let cells_y = (height - 1).div_ceil(step);

    let verts_x = (cells_x + 1) as usize;
    let verts_y = (cells_y + 1) as usize;
    let total_vertices = verts_x * verts_y;
    let triangle_count = 2 * cells_x * cells_y;
    debug_assert!(triangle_count <= MAX_TRIANGLES);

    let mut positions: Vec<[f32; 3]> = Vec::with_capacity(total_vertices);
    let mut normals: Vec<[f32; 3]> = vec![[0.0, 0.0, 0.0]; total_vertices];
    let mut uvs: Vec<[f32; 2]> = Vec::with_capacity(total_vertices);
    let mut indices: Vec<u32> = Vec::with_capacity((triangle_count as usize) * 3);

    let max_col = width - 1;
    let max_row = height - 1;
    let span_x = extent.width();
    let span_y = extent.height();

    // Generate welded grid vertices from North (row = 0 -> max_y) to South (row = max_row -> min_y),
    // and West (col = 0 -> min_x) to East (col = max_col -> max_x).
    for gy in 0..=cells_y {
        let py = (gy * step).min(max_row);
        let y_stl = if py == 0 {
            extent.max_y
        } else if py == max_row {
            extent.min_y
        } else {
            extent.max_y - (py as f32 / max_row as f32) * span_y
        };

        for gx in 0..=cells_x {
            let px = (gx * step).min(max_col);
            let x_stl = if px == 0 {
                extent.min_x
            } else if px == max_col {
                extent.max_x
            } else {
                extent.min_x + (px as f32 / max_col as f32) * span_x
            };

            let raw_z = elevations[(py as usize) * (width as usize) + (px as usize)];
            let z_stl = raw_z * vertical_exaggeration;

            positions.push(stl_to_bevy_position(x_stl, y_stl, z_stl));
            uvs.push(extent.compute_uv(x_stl, y_stl));
        }
    }

    // Build counter-clockwise triangles in Bevy Y-up space (+X East, +Y Up, +Z South)
    // and accumulate area-weighted smooth normals.
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
        triangle_count,
    })
}

/// Encodes an uncompressed baseline single-band `f32` grayscale TIFF in memory.
/// Primarily used for unit testing [`parse_dem_tiff`].
pub fn encode_uncompressed_f32_tiff(width: u32, height: u32, pixels: &[f32]) -> Vec<u8> {
    assert_eq!(pixels.len(), (width as usize) * (height as usize));
    let mut raw_bytes = Vec::with_capacity(pixels.len() * 4);
    for &p in pixels {
        raw_bytes.extend_from_slice(&p.to_le_bytes());
    }
    encode_baseline_grayscale_tiff(width, height, 32, 3, &raw_bytes)
}

/// Encodes an uncompressed baseline single-band `i16` grayscale TIFF in memory.
pub fn encode_uncompressed_i16_tiff(width: u32, height: u32, pixels: &[i16]) -> Vec<u8> {
    assert_eq!(pixels.len(), (width as usize) * (height as usize));
    let mut raw_bytes = Vec::with_capacity(pixels.len() * 2);
    for &p in pixels {
        raw_bytes.extend_from_slice(&p.to_le_bytes());
    }
    encode_baseline_grayscale_tiff(width, height, 16, 2, &raw_bytes)
}

/// Encodes an uncompressed baseline single-band `u16` grayscale TIFF in memory.
pub fn encode_uncompressed_u16_tiff(width: u32, height: u32, pixels: &[u16]) -> Vec<u8> {
    assert_eq!(pixels.len(), (width as usize) * (height as usize));
    let mut raw_bytes = Vec::with_capacity(pixels.len() * 2);
    for &p in pixels {
        raw_bytes.extend_from_slice(&p.to_le_bytes());
    }
    encode_baseline_grayscale_tiff(width, height, 16, 1, &raw_bytes)
}

fn encode_baseline_grayscale_tiff(
    width: u32,
    height: u32,
    bits_per_sample: u16,
    sample_format: u16,
    pixel_bytes: &[u8],
) -> Vec<u8> {
    // Layout:
    // 0..8: Header ("II", 42, offset=8)
    // 8..10: Entry count (10 entries)
    // 10..130: 10 * 12-byte IFD entries
    // 130..134: Next IFD offset (0)
    // 134..: Pixel data strip
    let entry_count: u16 = 10;
    let pixel_offset: u32 = 8 + 2 + u32::from(entry_count) * 12 + 4;
    let strip_byte_count = pixel_bytes.len() as u32;

    let mut buf = Vec::with_capacity((pixel_offset as usize) + pixel_bytes.len());
    buf.extend_from_slice(b"II");
    buf.extend_from_slice(&42u16.to_le_bytes());
    buf.extend_from_slice(&8u32.to_le_bytes());
    buf.extend_from_slice(&entry_count.to_le_bytes());

    let push_entry = |buf: &mut Vec<u8>, tag: u16, field_type: u16, count: u32, val: u32| {
        buf.extend_from_slice(&tag.to_le_bytes());
        buf.extend_from_slice(&field_type.to_le_bytes());
        buf.extend_from_slice(&count.to_le_bytes());
        buf.extend_from_slice(&val.to_le_bytes());
    };

    // Tags MUST be in ascending numerical order per the TIFF 6.0 specification.
    push_entry(&mut buf, 256, 4, 1, width); // ImageWidth (LONG)
    push_entry(&mut buf, 257, 4, 1, height); // ImageLength (LONG)
    push_entry(&mut buf, 258, 3, 1, u32::from(bits_per_sample)); // BitsPerSample (SHORT)
    push_entry(&mut buf, 259, 3, 1, 1); // Compression = 1 (None)
    push_entry(&mut buf, 262, 3, 1, 1); // PhotometricInterpretation = 1 (BlackIsZero)
    push_entry(&mut buf, 273, 4, 1, pixel_offset); // StripOffsets (LONG)
    push_entry(&mut buf, 277, 3, 1, 1); // SamplesPerPixel = 1 (SHORT)
    push_entry(&mut buf, 278, 4, 1, height); // RowsPerStrip (LONG)
    push_entry(&mut buf, 279, 4, 1, strip_byte_count); // StripByteCounts (LONG)
    push_entry(&mut buf, 339, 3, 1, u32::from(sample_format)); // SampleFormat (SHORT)

    buf.extend_from_slice(&0u32.to_le_bytes()); // Next IFD offset = 0
    buf.extend_from_slice(pixel_bytes);
    buf
}
