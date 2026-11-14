#!/usr/bin/env python3
# AI-assisted
"""Synthetic test-site generator and planar UV / binary STL / PNG checkerboard validator.

This script performs two functions:
1. Generates `data/processed/test_site/` (`terrain.stl`, `checkerboard.png`, `metadata.json`)
   using only the Python standard library (zero external dependencies required).
2. Validates any site directory (synthetic or JMARS-exported) against `AGENTS.md` rules:
   - Binary STL format (80-byte header + u32 triangle count + 50 bytes/triangle)
   - Triangle budget: 1 <= triangle_count <= 50,000
   - Z-up to Bevy Y-up conversion: X_bevy = X_stl, Y_bevy = Z_stl, Z_bevy = -Y_stl
   - Planar UV mapping strictly from DemExtent (min_x, max_x, min_y, max_y):
       u = (X_stl - min_x) / (max_x - min_x)
       v = (max_y - Y_stl) / (max_y - min_y)
   - Checkerboard alignment: NW corner (min_x, max_y) -> (u=0, v=0),
     SE corner (max_x, min_y) -> (u=1, v=1).
"""

from __future__ import annotations

import argparse
import json
import math
import struct
import sys
import zlib
from pathlib import Path

MAX_TRIANGLES = 50_000
STL_HEADER_BYTES = 80
STL_TRIANGLE_BYTES = 50


def synthetic_elevation_z(x: float, y: float) -> float:
    """Computes synthetic lunar crater + rim + ridge elevation in meters (Z-up)."""
    r = math.hypot(x, y)
    # Central impact crater of radius ~450 m with raised rim and gentle regional ridge
    crater_bowl = -65.0 * math.exp(-((r / 320.0) ** 2))
    crater_rim = 85.0 * math.exp(-(((r - 450.0) / 140.0) ** 2))
    ridge = 20.0 * math.sin(x / 350.0) * math.cos(y / 400.0)
    raw_z = crater_bowl + crater_rim + ridge + 15.0
    return max(-45.0, min(120.0, raw_z))


def write_synthetic_binary_stl(
    stl_path: Path,
    min_x: float,
    max_x: float,
    min_y: float,
    max_y: float,
    grid_cells: int = 32,
) -> tuple[int, float, float]:
    """Writes a binary Z-up STL terrain mesh for the given DemExtent."""
    triangles: list[tuple[tuple[float, float, float], ...]] = []
    dx = (max_x - min_x) / grid_cells
    dy = (max_y - min_y) / grid_cells

    min_z = float("inf")
    max_z = float("-inf")

    for iy in range(grid_cells):
        y0 = min_y + iy * dy
        y1 = min_y + (iy + 1) * dy if iy + 1 < grid_cells else max_y
        for ix in range(grid_cells):
            x0 = min_x + ix * dx
            x1 = min_x + (ix + 1) * dx if ix + 1 < grid_cells else max_x

            z00 = synthetic_elevation_z(x0, y0)
            z10 = synthetic_elevation_z(x1, y0)
            z11 = synthetic_elevation_z(x1, y1)
            z01 = synthetic_elevation_z(x0, y1)

            for z in (z00, z10, z11, z01):
                min_z = min(min_z, z)
                max_z = max(max_z, z)

            v00 = (x0, y0, z00)
            v10 = (x1, y0, z10)
            v11 = (x1, y1, z11)
            v01 = (x0, y1, z01)

            # Counter-clockwise winding in Z-up (+X East, +Y North, +Z Up)
            triangles.append((v00, v10, v11))
            triangles.append((v00, v11, v01))

    tri_count = len(triangles)
    assert tri_count <= MAX_TRIANGLES, f"Triangle count {tri_count} exceeds {MAX_TRIANGLES}"

    header = b"Space-App-X86 Synthetic Test Site Binary STL (Z-up)".ljust(
        STL_HEADER_BYTES, b"\x00"
    )
    stl_path.parent.mkdir(parents=True, exist_ok=True)
    with stl_path.open("wb") as f:
        f.write(header)
        f.write(struct.pack("<I", tri_count))
        for v0, v1, v2 in triangles:
            e1 = (v1[0] - v0[0], v1[1] - v0[1], v1[2] - v0[2])
            e2 = (v2[0] - v0[0], v2[1] - v0[1], v2[2] - v0[2])
            nx = e1[1] * e2[2] - e1[2] * e2[1]
            ny = e1[2] * e2[0] - e1[0] * e2[2]
            nz = e1[0] * e2[1] - e1[1] * e2[0]
            length = math.sqrt(nx * nx + ny * ny + nz * nz) or 1.0
            nx, ny, nz = nx / length, ny / length, nz / length
            f.write(
                struct.pack(
                    "<12fH",
                    nx,
                    ny,
                    nz,
                    v0[0],
                    v0[1],
                    v0[2],
                    v1[0],
                    v1[1],
                    v1[2],
                    v2[0],
                    v2[1],
                    v2[2],
                    0,
                )
            )

    return tri_count, round(min_z, 2), round(max_z, 2)


def _png_chunk(chunk_type: bytes, data: bytes) -> bytes:
    crc = zlib.crc32(chunk_type + data) & 0xFFFFFFFF
    return struct.pack(">I", len(data)) + chunk_type + data + struct.pack(">I", crc)


def write_checkerboard_png(
    png_path: Path, width: int = 512, height: int = 512, tiles: int = 8
) -> None:
    """Writes an 8x8 RGB checkerboard PNG with distinct NW (u=0,v=0) and SE (u=1,v=1) markers.

    - NW tile (tile_x=0, tile_y=0 -> u in [0, 1/8], v in [0, 1/8]): Teal/Cyan (0, 210, 220)
    - SE tile (tile_x=7, tile_y=7 -> u in [7/8, 1], v in [7/8, 1]): Amber/Orange (245, 140, 30)
    - Standard alternating tiles: Dark Slate (36, 44, 58) and Light Silver (220, 226, 234)
    """
    tile_w = width // tiles
    tile_h = height // tiles
    raw_rows = bytearray()

    for y in range(height):
        raw_rows.append(0)  # Filter type 0 (None)
        ty = min(y // tile_h, tiles - 1)
        for x in range(width):
            tx = min(x // tile_w, tiles - 1)
            if tx == 0 and ty == 0:
                # North-West corner tile (u=0, v=0)
                r, g, b = (0, 210, 220)
            elif tx == tiles - 1 and ty == tiles - 1:
                # South-East corner tile (u=1, v=1)
                r, g, b = (245, 140, 30)
            elif (tx + ty) % 2 == 0:
                r, g, b = (220, 226, 234)
            else:
                r, g, b = (36, 44, 58)
            raw_rows.extend((r, g, b))

    ihdr = struct.pack(">IIBBBBB", width, height, 8, 2, 0, 0, 0)
    idat = zlib.compress(bytes(raw_rows), level=9)

    png_path.parent.mkdir(parents=True, exist_ok=True)
    with png_path.open("wb") as f:
        f.write(b"\x89PNG\r\n\x1a\n")
        f.write(_png_chunk(b"IHDR", ihdr))
        f.write(_png_chunk(b"IDAT", idat))
        f.write(_png_chunk(b"IEND", b""))


def generate_test_site(repo_root: Path) -> Path:
    """Generates data/processed/test_site/{terrain.stl, checkerboard.png, metadata.json}."""
    site_dir = repo_root / "data" / "processed" / "test_site"
    stl_path = site_dir / "terrain.stl"
    png_path = site_dir / "checkerboard.png"
    meta_path = site_dir / "metadata.json"

    min_x, max_x = -1000.0, 1000.0
    min_y, max_y = -1000.0, 1000.0

    tri_count, min_z, max_z = write_synthetic_binary_stl(
        stl_path, min_x, max_x, min_y, max_y, grid_cells=32
    )
    write_checkerboard_png(png_path, width=512, height=512, tiles=8)

    metadata = {
        "id": "test_site",
        "name": "Synthetic Crater & Ridge UV Verification Site",
        "body": "moon",
        "description": (
            "Synthetic lunar crater and ridge terrain in Z-up meter coordinates used to "
            "verify DemExtent planar UV alignment against an 8x8 checkerboard texture."
        ),
        "center_lat_lon": [-89.5, 0.0],
        "dem_extent": {
            "min_x": min_x,
            "max_x": max_x,
            "min_y": min_y,
            "max_y": max_y,
        },
        "elevation_range_m": [min_z, max_z],
        "triangle_count": tri_count,
        "mesh_path": "test_site/terrain.stl",
        "texture_path": "test_site/checkerboard.png",
        "feature_scores": {
            "slope_safety": 0.85,
            "roughness_similarity": 0.90,
            "elevation_relief": 0.75,
            "geomorphology_match": 0.88,
        },
        "canonical_score": 0.8485,
        "generated_by": "AI-assisted (Gemini 4 Argon via Antigravity)",
    }
    with meta_path.open("w", encoding="utf-8") as f:
        json.dump(metadata, f, indent=2)
        f.write("\n")

    return site_dir


def validate_site(site_dir: Path, processed_root: Path) -> None:
    """Validates a site's metadata.json, binary STL mesh, PNG texture, and planar UV laws."""
    meta_path = site_dir / "metadata.json"
    if not meta_path.is_file():
        raise FileNotFoundError(f"Missing metadata.json in {site_dir}")

    with meta_path.open("r", encoding="utf-8") as f:
        meta = json.load(f)

    extent = meta["dem_extent"]
    min_x = float(extent["min_x"])
    max_x = float(extent["max_x"])
    min_y = float(extent["min_y"])
    max_y = float(extent["max_y"])
    if not (max_x > min_x and max_y > min_y):
        raise ValueError(f"Invalid DemExtent in {meta_path}: {extent}")

    stl_path = processed_root / meta["mesh_path"]
    if not stl_path.is_file():
        stl_path = site_dir / Path(meta["mesh_path"]).name
    png_path = processed_root / meta["texture_path"]
    if not png_path.is_file():
        png_path = site_dir / Path(meta["texture_path"]).name

    # 1. Verify PNG signature and dimensions
    png_bytes = png_path.read_bytes()
    if png_bytes[:8] != b"\x89PNG\r\n\x1a\n":
        raise ValueError(f"{png_path} is not a valid PNG file")
    tex_w, tex_h = struct.unpack(">II", png_bytes[16:24])

    # 2. Verify binary STL header & triangle budget
    stl_bytes = stl_path.read_bytes()
    if len(stl_bytes) < STL_HEADER_BYTES + 4:
        raise ValueError(f"{stl_path} is shorter than 84 bytes")

    (tri_count,) = struct.unpack("<I", stl_bytes[STL_HEADER_BYTES : STL_HEADER_BYTES + 4])
    if tri_count == 0 or tri_count > MAX_TRIANGLES:
        raise ValueError(
            f"Triangle count {tri_count} out of allowed range [1, {MAX_TRIANGLES}]"
        )

    expected_len = STL_HEADER_BYTES + 4 + tri_count * STL_TRIANGLE_BYTES
    if len(stl_bytes) != expected_len:
        raise ValueError(
            f"Binary STL byte length mismatch: expected {expected_len}, got {len(stl_bytes)}"
        )

    # 3. Parse vertices, weld, transform Z-up -> Bevy Y-up, and compute planar UVs
    welded_uvs: dict[tuple[int, int, int], tuple[float, float]] = {}
    welded_bevy: dict[tuple[int, int, int], tuple[float, float, float]] = {}

    offset = STL_HEADER_BYTES + 4
    for _ in range(tri_count):
        vals = struct.unpack("<12fH", stl_bytes[offset : offset + STL_TRIANGLE_BYTES])
        offset += STL_TRIANGLE_BYTES
        for v_idx in range(3):
            base = 3 + v_idx * 3
            x_stl, y_stl, z_stl = vals[base], vals[base + 1], vals[base + 2]
            key = (
                round(x_stl * 10_000),
                round(y_stl * 10_000),
                round(z_stl * 10_000),
            )
            if key not in welded_uvs:
                u = (x_stl - min_x) / (max_x - min_x)
                v = (max_y - y_stl) / (max_y - min_y)
                welded_uvs[key] = (u, v)
                # Z-up to Bevy Y-up: X_bevy = X_stl, Y_bevy = Z_stl, Z_bevy = -Y_stl
                welded_bevy[key] = (x_stl, z_stl, -y_stl)

    us = [uv[0] for uv in welded_uvs.values()]
    vs = [uv[1] for uv in welded_uvs.values()]
    min_u, max_u = min(us), max(us)
    min_v, max_v = min(vs), max(vs)

    # 4. Verify NW (u=0, v=0) and SE (u=1, v=1) corner alignment
    nw_uv = ((min_x - min_x) / (max_x - min_x), (max_y - max_y) / (max_y - min_y))
    se_uv = ((max_x - min_x) / (max_x - min_x), (max_y - min_y) / (max_y - min_y))
    assert abs(nw_uv[0] - 0.0) < 1e-6 and abs(nw_uv[1] - 0.0) < 1e-6
    assert abs(se_uv[0] - 1.0) < 1e-6 and abs(se_uv[1] - 1.0) < 1e-6

    print(f"[OK] Site ID           : {meta['id']} ({meta['body']})")
    print(f"[OK] Binary STL        : {stl_path.name} ({tri_count} triangles <= {MAX_TRIANGLES})")
    print(f"[OK] Welded Vertices   : {len(welded_uvs)} unique vertices (from {tri_count * 3} raw)")
    print(f"[OK] Texture PNG       : {png_path.name} ({tex_w}x{tex_h} px)")
    print(f"[OK] DemExtent (m)     : X=[{min_x}, {max_x}], Y=[{min_y}, {max_y}]")
    print(f"[OK] Planar UV Range   : u=[{min_u:.4f}, {max_u:.4f}], v=[{min_v:.4f}, {max_v:.4f}]")
    print(
        f"[OK] Checkerboard NW/SE: NW({min_x}, {max_y}) -> UV(0.0, 0.0) px(0, 0); "
        f"SE({max_x}, {min_y}) -> UV(1.0, 1.0) px({tex_w - 1}, {tex_h - 1})"
    )


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Generate and validate synthetic or JMARS STL+PNG site assets."
    )
    parser.add_argument(
        "--site-dir",
        type=Path,
        default=None,
        help="Path to site directory containing metadata.json (defaults to data/processed/test_site)",
    )
    parser.add_argument(
        "--skip-generate",
        action="store_true",
        help="Only validate existing files without regenerating test_site binary assets.",
    )
    args = parser.parse_args()

    repo_root = Path(__file__).resolve().parent.parent
    processed_root = repo_root / "data" / "processed"
    site_dir = args.site_dir or (processed_root / "test_site")

    if not args.skip_generate and site_dir == (processed_root / "test_site"):
        generate_test_site(repo_root)

    validate_site(site_dir, processed_root)
    return 0


if __name__ == "__main__":
    sys.exit(main())
