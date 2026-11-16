use std::fs;
use std::path::Path;

use crate::model::{GeoBounds, PlanetaryBody, TerrainGrid};

#[derive(Clone, Copy)]
enum Endian {
    Little,
    Big,
}

impl Endian {
    fn u16(self, b: &[u8], off: usize) -> u16 {
        match self {
            Endian::Little => u16::from_le_bytes([b[off], b[off + 1]]),
            Endian::Big => u16::from_be_bytes([b[off], b[off + 1]]),
        }
    }
    fn u32(self, b: &[u8], off: usize) -> u32 {
        match self {
            Endian::Little => u32::from_le_bytes([b[off], b[off + 1], b[off + 2], b[off + 3]]),
            Endian::Big => u32::from_be_bytes([b[off], b[off + 1], b[off + 2], b[off + 3]]),
        }
    }
    fn f32(self, b: &[u8], off: usize) -> f32 {
        f32::from_bits(self.u32(b, off))
    }
    fn f64(self, b: &[u8], off: usize) -> f64 {
        let mut arr = [0u8; 8];
        arr.copy_from_slice(&b[off..off + 8]);
        match self {
            Endian::Little => f64::from_le_bytes(arr),
            Endian::Big => f64::from_be_bytes(arr),
        }
    }
}

/// Write a 32-bit float GeoTIFF (.tif or .tiff) with embedded ModelPixelScaleTag (33550)
/// and ModelTiepointTag (33922) compatible with JMARS / GDAL / GIS tools.
pub fn write_geotiff_f32(path: &Path, grid: &TerrainGrid) -> Result<(), String> {
    let w = grid.width as u32;
    let h = grid.height as u32;
    let num_pixels = (w as usize) * (h as usize);
    if grid.elevation.len() != num_pixels {
        return Err("Grid elevation buffer size mismatch".to_string());
    }

    let pixel_bytes_len = (num_pixels * 4) as u32;
    // Header (8 bytes) + Pixel data + IFD + GeoTIFF double arrays
    let pixel_offset: u32 = 8;
    let ifd_offset: u32 = pixel_offset + pixel_bytes_len;

    // 12 IFD entries:
    // 256 ImageWidth
    // 257 ImageLength
    // 258 BitsPerSample (32)
    // 259 Compression (1 = Uncompressed)
    // 262 PhotometricInterpretation (1 = BlackIsZero)
    // 273 StripOffsets
    // 277 SamplesPerPixel (1)
    // 278 RowsPerStrip (h)
    // 279 StripByteCounts
    // 339 SampleFormat (3 = IEEE floating point)
    // 33550 ModelPixelScaleTag (3 doubles = 24 bytes)
    // 33922 ModelTiepointTag (6 doubles = 48 bytes)
    let num_entries: u16 = 12;
    let ifd_size: u32 = 2 + (num_entries as u32) * 12 + 4;
    let scale_tag_offset: u32 = ifd_offset + ifd_size;
    let tiepoint_tag_offset: u32 = scale_tag_offset + 24;

    let mut buf = Vec::with_capacity((tiepoint_tag_offset + 48) as usize);

    // Little-endian TIFF header ("II", 42, ifd_offset)
    buf.extend_from_slice(b"II");
    buf.extend_from_slice(&42u16.to_le_bytes());
    buf.extend_from_slice(&ifd_offset.to_le_bytes());

    // Pixel data (32-bit float)
    for &v in &grid.elevation {
        buf.extend_from_slice(&v.to_le_bytes());
    }

    // IFD entry count
    buf.extend_from_slice(&num_entries.to_le_bytes());

    let mut write_entry = |tag: u16, field_type: u16, count: u32, val_or_offset: u32| {
        buf.extend_from_slice(&tag.to_le_bytes());
        buf.extend_from_slice(&field_type.to_le_bytes());
        buf.extend_from_slice(&count.to_le_bytes());
        buf.extend_from_slice(&val_or_offset.to_le_bytes());
    };

    // TIFF Types: 3 = SHORT (u16), 4 = LONG (u32), 12 = DOUBLE (f64)
    write_entry(256, 4, 1, w);
    write_entry(257, 4, 1, h);
    write_entry(258, 3, 1, 32);
    write_entry(259, 3, 1, 1);
    write_entry(262, 3, 1, 1);
    write_entry(273, 4, 1, pixel_offset);
    write_entry(277, 3, 1, 1);
    write_entry(278, 4, 1, h);
    write_entry(279, 4, 1, pixel_bytes_len);
    write_entry(339, 3, 1, 3);
    write_entry(33550, 12, 3, scale_tag_offset);
    write_entry(33922, 12, 6, tiepoint_tag_offset);

    // Next IFD offset = 0
    buf.extend_from_slice(&0u32.to_le_bytes());

    // GeoTIFF ModelPixelScaleTag (33550): [ScaleX (deg/px), ScaleY (deg/px), ScaleZ (m/px)]
    // We encode deg/px in X,Y and meters_per_pixel_x in ScaleZ so both degree & meter scales are preserved!
    let deg_scale_x = (grid.bounds.max_lon - grid.bounds.min_lon) / (w.max(1) as f64);
    let deg_scale_y = (grid.bounds.max_lat - grid.bounds.min_lat) / (h.max(1) as f64);
    buf.extend_from_slice(&deg_scale_x.to_le_bytes());
    buf.extend_from_slice(&deg_scale_y.to_le_bytes());
    buf.extend_from_slice(&grid.bounds.meters_per_pixel_x.to_le_bytes());

    // GeoTIFF ModelTiepointTag (33922): [I=0, J=0, K=0, X=min_lon, Y=max_lat, Z=0]
    for d in [
        0.0f64,
        0.0f64,
        0.0f64,
        grid.bounds.min_lon,
        grid.bounds.max_lat,
        0.0f64,
    ] {
        buf.extend_from_slice(&d.to_le_bytes());
    }

    fs::write(path, &buf).map_err(|e| format!("Failed to write GeoTIFF {:?}: {}", path, e))
}

/// Read a .tif or .tiff file (supports 32-bit float GeoTIFFs, 16-bit int DEMs, and 8-bit RGB/grayscale JMARS renders)
pub fn read_tiff_or_geotiff(path: &Path, body: PlanetaryBody) -> Result<TerrainGrid, String> {
    let bytes = fs::read(path).map_err(|e| format!("Failed to read {:?}: {}", path, e))?;
    if bytes.len() < 8 {
        return Err("File too small to be a valid TIFF/GeoTIFF".to_string());
    }

    let endian = match &bytes[0..2] {
        b"II" => Endian::Little,
        b"MM" => Endian::Big,
        _ => return Err("Invalid TIFF byte-order header (expected II or MM)".to_string()),
    };

    let magic = endian.u16(&bytes, 2);
    if magic != 42 {
        return Err(format!("Unsupported TIFF magic number: {} (expected 42)", magic));
    }

    let ifd_offset = endian.u32(&bytes, 4) as usize;
    if ifd_offset + 2 > bytes.len() {
        return Err("Corrupt TIFF IFD offset".to_string());
    }

    let num_entries = endian.u16(&bytes, ifd_offset) as usize;
    let mut width: usize = 0;
    let mut height: usize = 0;
    let mut bits_per_sample: u16 = 8;
    let mut compression: u16 = 1;
    let mut samples_per_pixel: usize = 1;
    let mut sample_format: u16 = 1; // 1=uint, 2=sint, 3=float
    let mut strip_offsets: Vec<usize> = Vec::new();
    let mut strip_byte_counts: Vec<usize> = Vec::new();
    let mut tile_width: usize = 0;
    let mut tile_length: usize = 0;
    let mut tile_offsets: Vec<usize> = Vec::new();
    let mut tile_byte_counts: Vec<usize> = Vec::new();

    let mut pixel_scale: Option<[f64; 3]> = None;
    let mut tiepoint: Option<[f64; 6]> = None;

    for i in 0..num_entries {
        let entry_off = ifd_offset + 2 + i * 12;
        if entry_off + 12 > bytes.len() {
            break;
        }
        let tag = endian.u16(&bytes, entry_off);
        let field_type = endian.u16(&bytes, entry_off + 2);
        let count = endian.u32(&bytes, entry_off + 4) as usize;
        let val_off_raw = endian.u32(&bytes, entry_off + 8) as usize;

        let read_scalar_usize = || -> usize {
            match field_type {
                3 => endian.u16(&bytes, entry_off + 8) as usize,
                4 => val_off_raw,
                1 => bytes[entry_off + 8] as usize,
                _ => val_off_raw,
            }
        };

        let read_usize_list = || -> Vec<usize> {
            let elem_size = match field_type {
                3 => 2,
                4 => 4,
                _ => 4,
            };
            if count * elem_size <= 4 {
                let mut v = Vec::with_capacity(count);
                for k in 0..count {
                    let p = entry_off + 8 + k * elem_size;
                    let val = if elem_size == 2 {
                        endian.u16(&bytes, p) as usize
                    } else {
                        endian.u32(&bytes, p) as usize
                    };
                    v.push(val);
                }
                v
            } else {
                let mut v = Vec::with_capacity(count);
                for k in 0..count {
                    let p = val_off_raw + k * elem_size;
                    if p + elem_size <= bytes.len() {
                        let val = if elem_size == 2 {
                            endian.u16(&bytes, p) as usize
                        } else {
                            endian.u32(&bytes, p) as usize
                        };
                        v.push(val);
                    }
                }
                v
            }
        };

        match tag {
            256 => width = read_scalar_usize(),
            257 => height = read_scalar_usize(),
            258 => {
                let list = read_usize_list();
                if let Some(&first) = list.first() {
                    bits_per_sample = first as u16;
                }
            }
            259 => compression = read_scalar_usize() as u16,
            273 => strip_offsets = read_usize_list(),
            277 => samples_per_pixel = read_scalar_usize().max(1),
            279 => strip_byte_counts = read_usize_list(),
            322 => tile_width = read_scalar_usize(),
            323 => tile_length = read_scalar_usize(),
            324 => tile_offsets = read_usize_list(),
            325 => tile_byte_counts = read_usize_list(),
            339 => {
                let list = read_usize_list();
                if let Some(&first) = list.first() {
                    sample_format = first as u16;
                }
            }
            33550 => {
                // ModelPixelScaleTag (3 doubles)
                if count >= 3 && val_off_raw + 24 <= bytes.len() {
                    pixel_scale = Some([
                        endian.f64(&bytes, val_off_raw),
                        endian.f64(&bytes, val_off_raw + 8),
                        endian.f64(&bytes, val_off_raw + 16),
                    ]);
                }
            }
            33922 => {
                // ModelTiepointTag (6 doubles)
                if count >= 6 && val_off_raw + 48 <= bytes.len() {
                    tiepoint = Some([
                        endian.f64(&bytes, val_off_raw),
                        endian.f64(&bytes, val_off_raw + 8),
                        endian.f64(&bytes, val_off_raw + 16),
                        endian.f64(&bytes, val_off_raw + 24),
                        endian.f64(&bytes, val_off_raw + 32),
                        endian.f64(&bytes, val_off_raw + 40),
                    ]);
                }
            }
            _ => {}
        }
    }

    if width == 0 || height == 0 {
        return Err("Invalid TIFF dimensions (width or height is 0)".to_string());
    }

    // Reconstruct uncompressed raw byte buffer from strips or tiles
    let bytes_per_sample = ((bits_per_sample as usize) + 7) / 8;
    let bytes_per_pixel = bytes_per_sample * samples_per_pixel;
    let expected_raw_len = width * height * bytes_per_pixel;
    let mut raw_pixels = Vec::with_capacity(expected_raw_len);

    if !strip_offsets.is_empty() {
        for (idx, &off) in strip_offsets.iter().enumerate() {
            let count = strip_byte_counts
                .get(idx)
                .copied()
                .unwrap_or_else(|| bytes.len().saturating_sub(off));
            if off + count <= bytes.len() {
                let chunk = &bytes[off..off + count];
                if compression == 1 {
                    raw_pixels.extend_from_slice(chunk);
                } else if compression == 32773 {
                    // PackBits decompression
                    decompress_packbits(chunk, &mut raw_pixels);
                } else {
                    return Err(format!(
                        "Unsupported TIFF compression {} (use uncompressed or PackBits)",
                        compression
                    ));
                }
            }
        }
    } else if !tile_offsets.is_empty() && tile_width > 0 && tile_length > 0 {
        raw_pixels.resize(expected_raw_len, 0u8);
        let tiles_across = (width + tile_width - 1) / tile_width;
        for (t_idx, &off) in tile_offsets.iter().enumerate() {
            let count = tile_byte_counts.get(t_idx).copied().unwrap_or(0);
            if off + count <= bytes.len() {
                let tx = (t_idx % tiles_across) * tile_width;
                let ty = (t_idx / tiles_across) * tile_length;
                let tile_data = &bytes[off..off + count];
                let tile_row_bytes = tile_width * bytes_per_pixel;
                for row in 0..tile_length {
                    let dst_y = ty + row;
                    if dst_y >= height {
                        break;
                    }
                    let copy_w = (width - tx).min(tile_width);
                    let src_start = row * tile_row_bytes;
                    let src_end = src_start + copy_w * bytes_per_pixel;
                    let dst_start = (dst_y * width + tx) * bytes_per_pixel;
                    if src_end <= tile_data.len() && dst_start + (src_end - src_start) <= raw_pixels.len() {
                        raw_pixels[dst_start..dst_start + (src_end - src_start)]
                            .copy_from_slice(&tile_data[src_start..src_end]);
                    }
                }
            }
        }
    } else {
        return Err("TIFF file contains neither StripOffsets nor TileOffsets".to_string());
    }

    let num_pixels = width * height;
    let mut elevation = Vec::with_capacity(num_pixels);
    let mut is_visual_render = false;

    for i in 0..num_pixels {
        let p = i * bytes_per_pixel;
        if p + bytes_per_sample > raw_pixels.len() {
            elevation.push(0.0);
            continue;
        }
        let val: f32 = match (bits_per_sample, sample_format) {
            (32, 3) | (32, 1) if samples_per_pixel == 1 => endian.f32(&raw_pixels, p),
            (16, 2) => (endian.u16(&raw_pixels, p) as i16) as f32,
            (16, _) => endian.u16(&raw_pixels, p) as f32,
            (8, _) => {
                is_visual_render = true;
                if samples_per_pixel >= 3 && p + 3 <= raw_pixels.len() {
                    let r = raw_pixels[p] as f32;
                    let g = raw_pixels[p + 1] as f32;
                    let b = raw_pixels[p + 2] as f32;
                    0.299 * r + 0.587 * g + 0.114 * b
                } else {
                    raw_pixels[p] as f32
                }
            }
            _ => endian.f32(&raw_pixels, p),
        };
        elevation.push(val);
    }

    // If the uploaded TIFF is an 8-bit visual JMARS render (0..255 luminance),
    // map its luminance values into the planetary sector's physical elevation range!
    if is_visual_render {
        let (target_min, target_max) = match body {
            PlanetaryBody::Mars => (-2850.0f32, -1950.0f32),
            PlanetaryBody::Moon => (-1600.0f32, 2150.0f32),
        };
        for v in &mut elevation {
            let norm = (*v / 255.0).clamp(0.0, 1.0);
            *v = target_min + norm * (target_max - target_min);
        }
    }

    let has_geotiff_tags = pixel_scale.is_some() && tiepoint.is_some();
    let mut bounds = GeoBounds::default_for(body, width, height);
    if let (Some(scale), Some(tp)) = (pixel_scale, tiepoint) {
        if scale[0] > 0.0 && scale[1] > 0.0 {
            bounds.min_lon = tp[3];
            bounds.max_lon = tp[3] + scale[0] * (width as f64);
            bounds.max_lat = tp[4];
            bounds.min_lat = tp[4] - scale[1] * (height as f64);
        }
        if scale[2] > 0.1 {
            bounds.meters_per_pixel_x = scale[2];
            bounds.meters_per_pixel_y = scale[2];
        }
    }

    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("tif")
        .to_ascii_uppercase();
    let source_format = if has_geotiff_tags {
        format!("GeoTIFF (.{})", ext)
    } else if is_visual_render {
        format!("TIFF Visual Render (.{})", ext)
    } else {
        format!("TIFF Raster (.{})", ext)
    };

    Ok(TerrainGrid {
        width,
        height,
        elevation,
        bounds,
        source_format,
        has_geotiff_tags,
        nodata_value: None,
    })
}

fn decompress_packbits(src: &[u8], dst: &mut Vec<u8>) {
    let mut i = 0;
    while i < src.len() {
        let n = src[i] as i8;
        i += 1;
        if n >= 0 {
            let count = (n as usize) + 1;
            let end = (i + count).min(src.len());
            dst.extend_from_slice(&src[i..end]);
            i = end;
        } else if n != -128 {
            let count = ((-n) as usize) + 1;
            if i < src.len() {
                let val = src[i];
                i += 1;
                for _ in 0..count {
                    dst.push(val);
                }
            }
        }
    }
}
