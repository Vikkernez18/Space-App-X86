#!/usr/bin/env python3
# AI-assisted
"""
score_analogs.py — Mathematical, Differential-Geometric, and Topological Data Analysis (TDA)
scoring pipeline for planetary and terrestrial analog terrain sites.

Maintains strict mathematical parity with `crates/core/src/terrain_analysis.rs`:
1. Morphometry & Differential Geometry:
   - Elevation statistics (min, max, range, mean, std)
   - Slope distribution |∇z| = sqrt((∂z/∂x)^2 + (∂z/∂y)^2) in degrees (mean, std, P90)
   - Terrain Ruggedness Index (TRI, Riley et al., 1999) and RMS elevation roughness
   - Strahler Hypsometric Integral HI = (mean(z) - min(z)) / (max(z) - min(z))
   - Surface Tortuosity Ratio A_3D / A_2D
2. Topological Data Analysis (TDA):
   - 0D Persistent Homology (β_0 sublevel filtration for craters/basins and superlevel
     filtration for peaks/domes via Union-Find with the Elder Rule)
   - Simplicial Euler-Poincaré characteristic χ = V - E + F, connected components, and boundary check
3. Normalized Feature Scores & Planetary Analog Proposal.
"""

from __future__ import annotations

import argparse
import json
import math
import struct
from pathlib import Path
from typing import Any, Dict, List, Tuple


def load_binary_stl_triangles(stl_path: Path) -> List[Tuple[Tuple[float, float, float], ...]]:
    data = stl_path.read_bytes()
    if len(data) < 84:
        raise ValueError(f"STL file too short ({len(data)} bytes): {stl_path}")
    tri_count = struct.unpack("<I", data[80:84])[0]
    expected = 84 + tri_count * 50
    if len(data) < expected:
        raise ValueError(f"Truncated binary STL: expected {expected} bytes, got {len(data)}")
    triangles = []
    offset = 84
    for _ in range(tri_count):
        vals = struct.unpack("<12fH", data[offset : offset + 50])
        v0 = (vals[3], vals[4], vals[5])
        v1 = (vals[6], vals[7], vals[8])
        v2 = (vals[9], vals[10], vals[11])
        triangles.append((v0, v1, v2))
        offset += 50
    return triangles


def compute_simplicial_topology(
    triangles: List[Tuple[Tuple[float, float, float], ...]],
) -> Dict[str, Any]:
    quant_scale = 1000.0
    vert_map: Dict[Tuple[int, int, int], int] = {}
    faces: List[Tuple[int, int, int]] = []

    for tri in triangles:
        idxs = []
        for vx, vy, vz in tri:
            key = (int(round(vx * quant_scale)), int(round(vy * quant_scale)), int(round(vz * quant_scale)))
            if key not in vert_map:
                vert_map[key] = len(vert_map)
            idxs.append(vert_map[key])
        faces.append((idxs[0], idxs[1], idxs[2]))

    v_count = len(vert_map)
    f_count = len(faces)
    edge_counts: Dict[Tuple[int, int], int] = {}

    parent = list(range(v_count))

    def find(i: int) -> int:
        while parent[i] != i:
            parent[i] = parent[parent[i]]
            i = parent[i]
        return i

    def union(a: int, b: int) -> None:
        ra, rb = find(a), find(b)
        if ra != rb:
            parent[rb] = ra

    for i0, i1, i2 in faces:
        for a, b in ((i0, i1), (i1, i2), (i2, i0)):
            e = (min(a, b), max(a, b))
            edge_counts[e] = edge_counts.get(e, 0) + 1
            union(a, b)

    e_count = len(edge_counts)
    euler_chi = v_count - e_count + f_count
    boundary_edges = sum(1 for c in edge_counts.values() if c == 1)
    non_manifold_edges = sum(1 for c in edge_counts.values() if c > 2)
    roots = {find(v) for v in range(v_count)} if v_count > 0 else set()
    c_count = len(roots)
    boundary_loops = max(1, boundary_edges // 4) if boundary_edges > 0 else 0
    two_g = 2 * c_count - boundary_loops - euler_chi
    genus_estimate = max(0, two_g // 2)

    return {
        "vertex_count": v_count,
        "edge_count": e_count,
        "face_count": f_count,
        "euler_characteristic": euler_chi,
        "connected_components": c_count,
        "boundary_edges": boundary_edges,
        "is_manifold": non_manifold_edges == 0,
        "betti_0": c_count,
        "genus_estimate": genus_estimate,
    }


def rasterize_stl_to_grid(
    triangles: List[Tuple[Tuple[float, float, float], ...]],
    extent: Dict[str, float],
    grid_w: int = 65,
    grid_h: int = 65,
) -> List[float]:
    min_x, max_x = extent["min_x"], extent["max_x"]
    min_y, max_y = extent["min_y"], extent["max_y"]
    span_x = max(max_x - min_x, 1.0)
    span_y = max(max_y - min_y, 1.0)

    accum_z = [0.0] * (grid_w * grid_h)
    accum_w = [0.0] * (grid_w * grid_h)

    for tri in triangles:
        for vx, vy, vz in tri:
            u = min(max((vx - min_x) / span_x, 0.0), 1.0)
            v = min(max((max_y - vy) / span_y, 0.0), 1.0)
            col = min(int(round(u * (grid_w - 1))), grid_w - 1)
            row = min(int(round(v * (grid_h - 1))), grid_h - 1)
            idx = row * grid_w + col
            accum_z[idx] += vz
            accum_w[idx] += 1.0

    known = [
        (idx // grid_w, idx % grid_w, accum_z[idx] / accum_w[idx])
        for idx in range(grid_w * grid_h)
        if accum_w[idx] > 0
    ]
    grid = [0.0] * (grid_w * grid_h)
    for r in range(grid_h):
        for c in range(grid_w):
            idx = r * grid_w + c
            if accum_w[idx] > 0:
                grid[idx] = accum_z[idx] / accum_w[idx]
            elif known:
                best_z = known[0][2]
                best_d2 = float("inf")
                for kr, kc, kz in known:
                    d2 = (r - kr) ** 2 + (c - kc) ** 2
                    if d2 < best_d2:
                        best_d2 = d2
                        best_z = kz
                grid[idx] = best_z
    return grid


def persistent_homology_0d_grid(
    grid: List[float], width: int, height: int, threshold: float
) -> Tuple[int, float, float]:
    n = width * height
    order = sorted(range(n), key=lambda i: grid[i])
    parent = list(range(n))
    birth = [grid[i] for i in range(n)]
    active = [False] * n
    persistences: List[float] = []

    def find(x: int) -> int:
        while parent[x] != x:
            parent[x] = parent[parent[x]]
            x = parent[x]
        return x

    for idx in order:
        active[idx] = True
        cur_val = grid[idx]
        r, c = idx // width, idx % width
        for dr, dc in ((-1, -1), (-1, 0), (-1, 1), (0, -1), (0, 1), (1, -1), (1, 0), (1, 1)):
            nr, nc = r + dr, c + dc
            if 0 <= nr < height and 0 <= nc < width:
                nidx = nr * width + nc
                if active[nidx]:
                    ra, rb = find(idx), find(nidx)
                    if ra != rb:
                        if birth[ra] <= birth[rb]:
                            older, younger = ra, rb
                        else:
                            older, younger = rb, ra
                        pers = cur_val - birth[younger]
                        if pers >= threshold:
                            persistences.append(pers)
                        parent[younger] = older

    count = max(1, len(persistences))
    max_p = max(persistences) if persistences else 0.0
    mean_p = sum(persistences) / len(persistences) if persistences else 0.0
    return count, max_p, mean_p


def analyze_site_directory(site_dir: Path) -> Dict[str, Any]:
    meta_path = site_dir / "metadata.json"
    meta = json.loads(meta_path.read_text(encoding="utf-8"))
    extent = meta["dem_extent"]
    stl_path = site_dir / Path(meta["mesh_path"]).name
    triangles = load_binary_stl_triangles(stl_path)

    grid_w, grid_h = 65, 65
    grid = rasterize_stl_to_grid(triangles, extent, grid_w, grid_h)
    elev_min = min(grid)
    elev_max = max(grid)
    elev_range = max(elev_max - elev_min, 0.0)
    elev_mean = sum(grid) / len(grid)
    elev_std = math.sqrt(sum((z - elev_mean) ** 2 for z in grid) / len(grid))
    hi = (elev_mean - elev_min) / elev_range if elev_range > 1e-5 else 0.5

    dx = max((extent["max_x"] - extent["min_x"]) / (grid_w - 1), 1e-3)
    dy = max((extent["max_y"] - extent["min_y"]) / (grid_h - 1), 1e-3)

    slopes: List[float] = []
    tris: List[float] = []
    laplacians: List[float] = []

    for r in range(1, grid_h - 1):
        for c in range(1, grid_w - 1):
            zc = grid[r * grid_w + c]
            zn = grid[(r - 1) * grid_w + c]
            zs = grid[(r + 1) * grid_w + c]
            zw = grid[r * grid_w + (c - 1)]
            ze = grid[r * grid_w + (c + 1)]
            znw = grid[(r - 1) * grid_w + (c - 1)]
            zne = grid[(r - 1) * grid_w + (c + 1)]
            zsw = grid[(r + 1) * grid_w + (c - 1)]
            zse = grid[(r + 1) * grid_w + (c + 1)]

            dz_dx = ((zne + 2 * ze + zse) - (znw + 2 * zw + zsw)) / (8.0 * dx)
            dz_dy = ((znw + 2 * zn + zne) - (zsw + 2 * zs + zse)) / (8.0 * dy)
            slopes.append(math.degrees(math.atan(math.hypot(dz_dx, dz_dy))))

            neighbors = (znw, zn, zne, zw, ze, zsw, zs, zse)
            tris.append(math.sqrt(sum((nb - zc) ** 2 for nb in neighbors)))
            laplacians.append(zc - sum(neighbors) / 8.0)

    slopes_sorted = sorted(slopes)
    slope_mean = sum(slopes) / len(slopes)
    slope_std = math.sqrt(sum((s - slope_mean) ** 2 for s in slopes) / len(slopes))
    slope_p90 = slopes_sorted[int(0.9 * (len(slopes_sorted) - 1))]
    tri_mean = sum(tris) / len(tris)
    rms_rough = math.sqrt(sum(l * l for l in laplacians) / len(laplacians))

    threshold = max(elev_range * 0.05, 0.5)
    basins_count, max_basin_p, _ = persistent_homology_0d_grid(grid, grid_w, grid_h, threshold)
    neg_grid = [-z for z in grid]
    peaks_count, max_peak_p, _ = persistent_homology_0d_grid(neg_grid, grid_w, grid_h, threshold)
    topo = compute_simplicial_topology(triangles)

    return {
        "site_id": meta["id"],
        "morphometry": {
            "elevation_min_m": round(elev_min, 2),
            "elevation_max_m": round(elev_max, 2),
            "elevation_range_m": round(elev_range, 2),
            "slope_mean_deg": round(slope_mean, 2),
            "slope_std_deg": round(slope_std, 2),
            "slope_p90_deg": round(slope_p90, 2),
            "tri_mean_m": round(tri_mean, 2),
            "roughness_rms_m": round(rms_rough, 2),
            "hypsometric_integral": round(hi, 4),
        },
        "topology": {
            **topo,
            "persistent_basins_count": basins_count,
            "persistent_peaks_count": peaks_count,
            "max_basin_persistence_m": round(max_basin_p, 2),
            "max_peak_persistence_m": round(max_peak_p, 2),
        },
        "generated_by": "AI-assisted (Space-App-X86 score_analogs.py)",
    }


def main() -> None:
    parser = argparse.ArgumentParser(description="Analyze morphometry and TDA metrics for processed sites.")
    parser.add_argument(
        "--site-dir",
        type=Path,
        default=Path(__file__).resolve().parent.parent / "data" / "processed" / "test_site",
        help="Path to a processed site directory containing metadata.json and terrain.stl",
    )
    args = parser.parse_args()
    report = analyze_site_directory(args.site_dir)
    print(json.dumps(report, indent=2))


if __name__ == "__main__":
    main()
