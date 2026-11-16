use std::fs;
use std::path::Path;

use crate::earth_craters::{default_earth_craters, synthesize_earth_crater_grid, EarthCrater};
use crate::math::compute_slope_deg;
use crate::model::{PlanetaryBody, TerrainGrid};

#[derive(Debug, Clone)]
pub struct DetectedCrater {
    pub id: String,
    pub norm_x: f64,
    pub norm_y: f64,
    pub norm_radius: f64,
    pub diameter_px: f64,
    pub depth_val: f64,
    pub rim_height_val: f64,
    pub depth_to_diameter: f64,
    pub rim_to_depth: f64,
    pub circularity: f64,
    pub floor_flatness: f64,
    pub central_peak: f64,
    pub wall_slope_deg: f64,
    pub confidence: f64,
    pub radial_profile: [f32; 16],
    pub ice_potential_score: f64,
    pub ice_potential_label: String,
    pub ice_rationale: String,
}

#[derive(Debug, Clone)]
pub struct EarthCraterMatch {
    pub crater: EarthCrater,
    pub similarity_score: f64,
    pub profile_correlation: f64,
    pub feature_rmse: f64,
    pub match_summary: String,
}

#[derive(Debug, Clone)]
pub struct CraterAnalysisResult {
    pub detected_craters: Vec<DetectedCrater>,
    pub earth_matches: Vec<EarthCraterMatch>,
}

struct SmallGrid {
    w: usize,
    h: usize,
    data: Vec<f32>,
    std_dev: f32,
}

fn build_analysis_grid(grid: &TerrainGrid, max_side: usize) -> SmallGrid {
    let aspect = (grid.width as f64) / (grid.height.max(1) as f64);
    let (w, h) = if grid.width >= grid.height {
        let nw = grid.width.min(max_side).max(32);
        let nh = ((nw as f64) / aspect).round().max(24.0) as usize;
        (nw, nh)
    } else {
        let nh = grid.height.min(max_side).max(32);
        let nw = ((nh as f64) * aspect).round().max(24.0) as usize;
        (nw, nh)
    };

    let mut data = vec![0.0f32; w * h];
    let mut sum = 0.0f64;
    for y in 0..h {
        let sy = ((y as f64) / ((h - 1).max(1) as f64) * ((grid.height - 1) as f64))
            .round()
            .clamp(0.0, (grid.height - 1) as f64) as usize;
        for x in 0..w {
            let sx = ((x as f64) / ((w - 1).max(1) as f64) * ((grid.width - 1) as f64))
                .round()
                .clamp(0.0, (grid.width - 1) as f64) as usize;
            let v = grid.get(sx, sy);
            data[y * w + x] = v;
            sum += v as f64;
        }
    }
    let n = (w * h).max(1) as f64;
    let mean = sum / n;
    let mut var = 0.0f64;
    for &v in &data {
        let d = (v as f64) - mean;
        var += d * d;
    }
    let std_dev = ((var / n).sqrt() as f32).max(1e-4);
    SmallGrid { w, h, data, std_dev }
}

impl SmallGrid {
    fn sample_bilinear(&self, fx: f32, fy: f32) -> f32 {
        let x0 = (fx.floor() as isize).clamp(0, (self.w as isize) - 1) as usize;
        let y0 = (fy.floor() as isize).clamp(0, (self.h as isize) - 1) as usize;
        let x1 = (x0 + 1).min(self.w - 1);
        let y1 = (y0 + 1).min(self.h - 1);
        let wx = (fx - (x0 as f32)).clamp(0.0, 1.0);
        let wy = (fy - (y0 as f32)).clamp(0.0, 1.0);
        let v00 = self.data[y0 * self.w + x0];
        let v10 = self.data[y0 * self.w + x1];
        let v01 = self.data[y1 * self.w + x0];
        let v11 = self.data[y1 * self.w + x1];
        (v00 * (1.0 - wx) + v10 * wx) * (1.0 - wy) + (v01 * (1.0 - wx) + v11 * wx) * wy
    }
}

#[derive(Clone, Copy)]
struct Candidate {
    cx: f32,
    cy: f32,
    r: f32,
    score: f32,
}

fn score_crater_candidate(sg: &SmallGrid, cx: f32, cy: f32, r: f32) -> f32 {
    if r < 3.5 {
        return -10.0;
    }
    // Check bounds so at least rim fits largely inside map
    if cx - r * 0.85 < 0.0
        || cx + r * 0.85 >= sg.w as f32
        || cy - r * 0.85 < 0.0
        || cy + r * 0.85 >= sg.h as f32
    {
        return -10.0;
    }

    let mut inner_sum = 0.0f32;
    let mut inner_cnt = 0usize;
    let mut mid_sum = 0.0f32;
    let mut mid_cnt = 0usize;
    let mut rim_vals = [0.0f32; 16];
    let mut outer_sum = 0.0f32;
    let mut outer_cnt = 0usize;

    // Sample center + annular ring at 0.35R
    inner_sum += sg.sample_bilinear(cx, cy);
    inner_cnt += 1;
    for k in 0..8 {
        let ang = (k as f32) * (std::f32::consts::TAU / 8.0);
        let px = cx + r * 0.35 * ang.cos();
        let py = cy + r * 0.35 * ang.sin();
        if px >= 0.0 && px < (sg.w - 1) as f32 && py >= 0.0 && py < (sg.h - 1) as f32 {
            inner_sum += sg.sample_bilinear(px, py);
            inner_cnt += 1;
        }
    }

    // Mid-bowl ring at 0.55R (catches flat floors & annular troughs around central peaks)
    for k in 0..12 {
        let ang = (k as f32) * (std::f32::consts::TAU / 12.0);
        let px = cx + r * 0.55 * ang.cos();
        let py = cy + r * 0.55 * ang.sin();
        if px >= 0.0 && px < (sg.w - 1) as f32 && py >= 0.0 && py < (sg.h - 1) as f32 {
            mid_sum += sg.sample_bilinear(px, py);
            mid_cnt += 1;
        }
    }

    // Rim ring at 1.0R (16 sectors)
    let mut rim_sum = 0.0f32;
    let mut rim_cnt = 0usize;
    for k in 0..16 {
        let ang = (k as f32) * (std::f32::consts::TAU / 16.0);
        let px = cx + r * ang.cos();
        let py = cy + r * ang.sin();
        if px >= 0.0 && px < (sg.w - 1) as f32 && py >= 0.0 && py < (sg.h - 1) as f32 {
            let v = sg.sample_bilinear(px, py);
            rim_vals[k] = v;
            rim_sum += v;
            rim_cnt += 1;
        } else {
            rim_vals[k] = f32::NAN;
        }
    }
    if rim_cnt < 12 || inner_cnt == 0 || mid_cnt == 0 {
        return -10.0;
    }

    // Outer annulus at 1.38R
    for k in 0..16 {
        let ang = (k as f32) * (std::f32::consts::TAU / 16.0);
        let px = cx + r * 1.38 * ang.cos();
        let py = cy + r * 1.38 * ang.sin();
        if px >= 0.0 && px < (sg.w - 1) as f32 && py >= 0.0 && py < (sg.h - 1) as f32 {
            outer_sum += sg.sample_bilinear(px, py);
            outer_cnt += 1;
        }
    }

    let mean_inner = inner_sum / (inner_cnt as f32);
    let mean_mid = mid_sum / (mid_cnt as f32);
    let floor_ref = mean_inner.min(mean_mid);
    let mean_rim = rim_sum / (rim_cnt as f32);
    let mean_outer = if outer_cnt > 0 {
        outer_sum / (outer_cnt as f32)
    } else {
        mean_rim
    };

    let depth = mean_rim - floor_ref;
    if depth <= sg.std_dev * 0.12 {
        return -5.0;
    }

    // Count how many rim sectors are meaningfully higher than the crater interior
    let mut above_cnt = 0usize;
    let mut rim_var = 0.0f32;
    for &rv in &rim_vals {
        if !rv.is_nan() {
            if rv > floor_ref + 0.25 * depth {
                above_cnt += 1;
            }
            let d = rv - mean_rim;
            rim_var += d * d;
        }
    }
    let consistency = (above_cnt as f32) / (rim_cnt as f32);
    if consistency < 0.55 {
        return -3.0;
    }

    let rim_std = (rim_var / (rim_cnt as f32)).sqrt();
    let circularity_factor = (1.0 - (rim_std / depth.max(1e-4)) * 0.45).clamp(0.1, 1.0);
    let rim_prom = ((mean_rim - mean_outer) / sg.std_dev).clamp(-0.5, 2.5);
    let depth_norm = (depth / sg.std_dev).clamp(0.0, 5.0);

    // Slight preference for well-resolved craters over tiny 4px noise blobs
    let size_bonus = (r / (sg.w.min(sg.h) as f32 * 0.18)).clamp(0.4, 1.25);

    (depth_norm * 1.3 + rim_prom * 0.85) * consistency * circularity_factor * size_bonus
}

pub fn analyze_craters(grid: &TerrainGrid, body: PlanetaryBody) -> CraterAnalysisResult {
    let sg = build_analysis_grid(grid, 144);
    let min_dim = sg.w.min(sg.h) as f32;

    let min_r = (min_dim * 0.06).max(4.5);
    let max_r = (min_dim * 0.42).max(min_r + 2.0);
    let r_steps = 11usize;

    let stride = ((min_dim / 34.0).round() as usize).clamp(2, 5);
    let mut raw_candidates: Vec<Candidate> = Vec::new();

    for ri in 0..r_steps {
        let t = (ri as f32) / ((r_steps - 1).max(1) as f32);
        let r = min_r + t * (max_r - min_r);
        let margin = (r * 0.85) as usize;
        let x_start = margin.min(sg.w.saturating_sub(2));
        let x_end = sg.w.saturating_sub(margin).max(x_start + 1);
        let y_start = margin.min(sg.h.saturating_sub(2));
        let y_end = sg.h.saturating_sub(margin).max(y_start + 1);

        let mut y = y_start;
        while y < y_end {
            let mut x = x_start;
            while x < x_end {
                let sc = score_crater_candidate(&sg, x as f32, y as f32, r);
                if sc > 0.35 {
                    raw_candidates.push(Candidate {
                        cx: x as f32,
                        cy: y as f32,
                        r,
                        score: sc,
                    });
                }
                x += stride;
            }
            y += stride;
        }
    }

    raw_candidates.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));

    // Non-maximum suppression to get up to 4 distinct crater candidates
    let mut picked: Vec<Candidate> = Vec::new();
    for cand in raw_candidates {
        let overlaps = picked.iter().any(|p| {
            let dist = ((p.cx - cand.cx).powi(2) + (p.cy - cand.cy).powi(2)).sqrt();
            dist < (p.r.max(cand.r) * 0.80)
        });
        if !overlaps {
            // Local coordinate refinement
            let refined = refine_candidate(&sg, cand);
            picked.push(refined);
            if picked.len() >= 4 {
                break;
            }
        }
    }

    // Fallback if terrain is extremely flat or subtle: pick global lowest basin
    if picked.is_empty() {
        let mut best_x = sg.w / 2;
        let mut best_y = sg.h / 2;
        let mut min_z = f32::INFINITY;
        for y in (sg.h / 6)..(sg.h * 5 / 6) {
            for x in (sg.w / 6)..(sg.w * 5 / 6) {
                let v = sg.data[y * sg.w + x];
                if v < min_z {
                    min_z = v;
                    best_x = x;
                    best_y = y;
                }
            }
        }
        picked.push(Candidate {
            cx: best_x as f32,
            cy: best_y as f32,
            r: (min_dim * 0.18).max(6.0),
            score: 0.25,
        });
    }

    // Compute full-resolution morphological parameters for each detected crater
    let slope_grid = compute_slope_deg(grid);
    let mut detected_craters = Vec::with_capacity(picked.len());
    for (idx, c) in picked.iter().enumerate() {
        let norm_x = (c.cx as f64) / ((sg.w - 1).max(1) as f64);
        let norm_y = (c.cy as f64) / ((sg.h - 1).max(1) as f64);
        let norm_radius = (c.r as f64) / (sg.w.min(sg.h).max(1) as f64);
        let dc = extract_crater_features(
            &format!("C-{}", idx + 1),
            grid,
            &slope_grid,
            norm_x,
            norm_y,
            norm_radius,
            c.score as f64,
            body,
        );
        detected_craters.push(dc);
    }

    let db = default_earth_craters();
    let earth_matches = if let Some(primary) = detected_craters.first() {
        match_against_earth_db(primary, &db)
    } else {
        Vec::new()
    };

    CraterAnalysisResult {
        detected_craters,
        earth_matches,
    }
}

fn refine_candidate(sg: &SmallGrid, init: Candidate) -> Candidate {
    let mut best = init;
    for step in [2.0f32, 1.0f32] {
        let mut improved = true;
        let mut iter = 0;
        while improved && iter < 4 {
            improved = false;
            iter += 1;
            for dy in [-step, 0.0, step] {
                for dx in [-step, 0.0, step] {
                    for dr in [-step, 0.0, step] {
                        let nx = best.cx + dx;
                        let ny = best.cy + dy;
                        let nr = (best.r + dr).max(4.0);
                        let sc = score_crater_candidate(sg, nx, ny, nr);
                        if sc > best.score {
                            best = Candidate {
                                cx: nx,
                                cy: ny,
                                r: nr,
                                score: sc,
                            };
                            improved = true;
                        }
                    }
                }
            }
        }
    }
    best
}

fn extract_crater_features(
    id: &str,
    grid: &TerrainGrid,
    slope_grid: &[f32],
    norm_x: f64,
    norm_y: f64,
    norm_radius: f64,
    raw_score: f64,
    body: PlanetaryBody,
) -> DetectedCrater {
    let w = grid.width;
    let h = grid.height;
    let cx = norm_x * ((w - 1).max(1) as f64);
    let cy = norm_y * ((h - 1).max(1) as f64);
    let rad_px = (norm_radius * (w.min(h) as f64)).max(4.0);
    let diameter_px = rad_px * 2.0;

    // Bin samples into 16 radial rings (r/R from 0.0 to 1.5 in steps of 0.1)
    let mut bin_sum = [0.0f64; 16];
    let mut bin_cnt = [0usize; 16];

    let max_r = rad_px * 1.55;
    let x_min = ((cx - max_r).floor().max(0.0) as usize).min(w - 1);
    let x_max = ((cx + max_r).ceil().max(0.0) as usize).min(w - 1);
    let y_min = ((cy - max_r).floor().max(0.0) as usize).min(h - 1);
    let y_max = ((cy + max_r).ceil().max(0.0) as usize).min(h - 1);

    // Subsample step if crater bounding box is huge so feature extraction stays <5ms
    let span = (x_max - x_min).max(y_max - y_min);
    let step = (span / 120).max(1);

    let mut wall_slope_sum = 0.0f64;
    let mut wall_slope_cnt = 0usize;

    let mut y = y_min;
    while y <= y_max {
        let mut x = x_min;
        while x <= x_max {
            let dx = (x as f64) - cx;
            let dy = (y as f64) - cy;
            let r_ratio = (dx * dx + dy * dy).sqrt() / rad_px;
            if r_ratio <= 1.55 {
                let b = ((r_ratio / 0.1).round() as usize).min(15);
                let z = grid.get(x, y) as f64;
                bin_sum[b] += z;
                bin_cnt[b] += 1;

                if r_ratio >= 0.55 && r_ratio <= 0.95 {
                    wall_slope_sum += slope_grid[y * w + x] as f64;
                    wall_slope_cnt += 1;
                }
            }
            x += step;
        }
        y += step;
    }

    let mut raw_prof = [0.0f64; 16];
    let mut last_valid = grid.get((cx.round() as usize).min(w - 1), (cy.round() as usize).min(h - 1)) as f64;
    for i in 0..16 {
        if bin_cnt[i] > 0 {
            raw_prof[i] = bin_sum[i] / (bin_cnt[i] as f64);
            last_valid = raw_prof[i];
        } else {
            raw_prof[i] = last_valid;
        }
    }

    // Reference outer background elevation (bins 13..=15 -> r/R in 1.3..1.5)
    let outer_bg = (raw_prof[13] + raw_prof[14] + raw_prof[15]) / 3.0;
    // Rim peak around bins 9..=11 -> r/R in 0.9..1.1
    let rim_peak = raw_prof[9].max(raw_prof[10]).max(raw_prof[11]);
    // Deepest interior floor across bins 0..=6
    let mut floor_min = raw_prof[0];
    for i in 1..=6 {
        if raw_prof[i] < floor_min {
            floor_min = raw_prof[i];
        }
    }

    let depth_val = (rim_peak - floor_min).max(1e-3);
    let rim_height_val = (rim_peak - outer_bg).max(0.0);

    // Normalized radial profile: outer_bg = 0.0, floor_min = -1.0, rim_peak = +rim_to_depth
    let mut radial_profile = [0.0f32; 16];
    let floor_to_bg = (outer_bg - floor_min).max(depth_val * 0.5);
    for i in 0..16 {
        radial_profile[i] = ((raw_prof[i] - outer_bg) / floor_to_bg).clamp(-1.35, 0.85) as f32;
    }

    let m_per_px = grid.bounds.meters_per_pixel_x.max(0.1);
    let diameter_m = diameter_px * m_per_px;
    let depth_to_diameter = (depth_val / diameter_m.max(1.0)).clamp(0.002, 0.30);
    let rim_to_depth = (rim_height_val / depth_val).clamp(0.0, 0.60);

    // Floor flatness: how close bins 0..=4 are to floor_min relative to depth
    let floor_mean_inner = (raw_prof[0] + raw_prof[1] + raw_prof[2] + raw_prof[3]) / 4.0;
    let mid_bowl = raw_prof[4].min(raw_prof[5]);
    let central_rise = ((raw_prof[0].max(raw_prof[1]) - mid_bowl) / depth_val).clamp(0.0, 1.0);
    let floor_flatness = (1.0 - ((floor_mean_inner - floor_min) / (depth_val * 0.55))).clamp(0.10, 0.95);

    // Azimuthal circularity at rim r = 1.0R across 16 directions
    let mut rim_sectors = Vec::with_capacity(16);
    for k in 0..16 {
        let ang = (k as f64) * (std::f64::consts::TAU / 16.0);
        let sx = (cx + rad_px * ang.cos()).round() as isize;
        let sy = (cy + rad_px * ang.sin()).round() as isize;
        if sx >= 0 && sx < w as isize && sy >= 0 && sy < h as isize {
            rim_sectors.push(grid.get(sx as usize, sy as usize) as f64);
        }
    }
    let circularity = if rim_sectors.len() >= 8 {
        let rm = rim_sectors.iter().sum::<f64>() / (rim_sectors.len() as f64);
        let rvar = rim_sectors.iter().map(|v| (v - rm).powi(2)).sum::<f64>() / (rim_sectors.len() as f64);
        (1.0 - (rvar.sqrt() / depth_val) * 0.55).clamp(0.35, 0.99)
    } else {
        0.75
    };

    let wall_slope_deg = if wall_slope_cnt > 0 {
        wall_slope_sum / (wall_slope_cnt as f64)
    } else {
        15.0
    };

    let confidence = ((raw_score / 2.2) * 100.0).clamp(25.0, 99.0);

    // Ice Deposit Potential assessment:
    // Deep craters (high d/D), steep inner walls (>15 deg), well-preserved circular rims,
    // and polar/Lunar South Pole or Mars cold-trap geometry strongly indicate water-ice retention!
    let dd_factor = (depth_to_diameter / 0.10).clamp(0.0, 1.0);
    let slope_factor = (wall_slope_deg / 22.0).clamp(0.0, 1.0);
    let body_bonus = match body {
        PlanetaryBody::Moon => 0.15,
        PlanetaryBody::Mars => 0.08,
    };
    let ice_potential_score =
        ((0.40 * dd_factor + 0.30 * slope_factor + 0.15 * circularity + body_bonus) * 100.0)
            .clamp(12.0, 98.0);

    let (ice_potential_label, ice_rationale) = if ice_potential_score >= 68.0 {
        (
            "HIGH".to_string(),
            format!(
                "Deep shadowed bowl (d/D={:.3}, inner wall {:.1}°, circ {:.2}) creates strong cold-trap shielding for subsurface/surface water-ice deposits.",
                depth_to_diameter, wall_slope_deg, circularity
            ),
        )
    } else if ice_potential_score >= 42.0 {
        (
            "MODERATE".to_string(),
            format!(
                "Moderate crater enclosure (d/D={:.3}, wall {:.1}°) supports localized polar/equatorial-shadowed regolith ice pockets.",
                depth_to_diameter, wall_slope_deg
            ),
        )
    } else {
        (
            "LOW".to_string(),
            format!(
                "Shallow/eroded relief (d/D={:.3}, wall {:.1}°) receives higher solar exposure; only buried ice beneath regolith expected.",
                depth_to_diameter, wall_slope_deg
            ),
        )
    };

    DetectedCrater {
        id: id.to_string(),
        norm_x,
        norm_y,
        norm_radius,
        diameter_px,
        depth_val,
        rim_height_val,
        depth_to_diameter,
        rim_to_depth,
        circularity,
        floor_flatness,
        central_peak: central_rise,
        wall_slope_deg,
        confidence,
        radial_profile,
        ice_potential_score,
        ice_potential_label,
        ice_rationale,
    }
}

fn match_against_earth_db(det: &DetectedCrater, db: &[EarthCrater]) -> Vec<EarthCraterMatch> {
    let mut matches = Vec::with_capacity(db.len());

    for ec in db {
        // 1. Radial profile Pearson correlation & RMSE
        let (prof_corr, prof_rmse) = profile_similarity(&det.radial_profile, &ec.radial_profile);
        let prof_sim = (0.5 * (prof_corr + 1.0)) * 0.65 + (1.0 - (prof_rmse / 0.8).min(1.0)) * 0.35;

        // 2. Scalar morphological feature distances
        let dd_diff = ((det.depth_to_diameter + 0.005).ln() - (ec.depth_to_diameter + 0.005).ln()).abs() / 2.5;
        let dd_sim = (1.0 - dd_diff.min(1.0)).max(0.0);

        let rim_diff = ((det.rim_to_depth - ec.rim_to_depth).abs() / 0.35).min(1.0);
        let rim_sim = 1.0 - rim_diff;

        let flat_diff = ((det.floor_flatness - ec.floor_flatness).abs() / 0.6).min(1.0);
        let flat_sim = 1.0 - flat_diff;

        let peak_diff = ((det.central_peak - ec.central_peak).abs() / 0.7).min(1.0);
        let peak_sim = 1.0 - peak_diff;

        let circ_diff = ((det.circularity - ec.circularity).abs() / 0.3).min(1.0);
        let circ_sim = 1.0 - circ_diff;

        let combined = 0.45 * prof_sim
            + 0.18 * dd_sim
            + 0.12 * rim_sim
            + 0.10 * flat_sim
            + 0.10 * peak_sim
            + 0.05 * circ_sim;

        let similarity_score = (combined * 100.0).clamp(5.0, 99.5);
        let feature_rmse = ((dd_diff * dd_diff + rim_diff * rim_diff + flat_diff * flat_diff + peak_diff * peak_diff) / 4.0).sqrt();

        let match_summary = format!(
            "{} ({}, {}): {:.1}% match | {} | Radial r={:.2} | d/D={:.3} vs {:.3} | {}",
            ec.name,
            ec.location,
            ec.country,
            similarity_score,
            ec.morphology,
            prof_corr,
            det.depth_to_diameter,
            ec.depth_to_diameter,
            ec.ice_analog_notes
        );

        matches.push(EarthCraterMatch {
            crater: ec.clone(),
            similarity_score,
            profile_correlation: prof_corr,
            feature_rmse,
            match_summary,
        });
    }

    matches.sort_by(|a, b| {
        b.similarity_score
            .partial_cmp(&a.similarity_score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    matches.truncate(3);
    matches
}

fn profile_similarity(a: &[f32; 16], b: &[f32; 16]) -> (f64, f64) {
    let n = 16.0f64;
    let mut sum_a = 0.0f64;
    let mut sum_b = 0.0f64;
    let mut sq_err = 0.0f64;
    for i in 0..16 {
        let va = a[i] as f64;
        let vb = b[i] as f64;
        sum_a += va;
        sum_b += vb;
        sq_err += (va - vb) * (va - vb);
    }
    let ma = sum_a / n;
    let mb = sum_b / n;
    let mut cov = 0.0f64;
    let mut va_s = 0.0f64;
    let mut vb_s = 0.0f64;
    for i in 0..16 {
        let da = (a[i] as f64) - ma;
        let db = (b[i] as f64) - mb;
        cov += da * db;
        va_s += da * da;
        vb_s += db * db;
    }
    let r = if va_s > 1e-9 && vb_s > 1e-9 {
        (cov / (va_s.sqrt() * vb_s.sqrt())).clamp(-1.0, 1.0)
    } else {
        0.0
    };
    let rmse = (sq_err / n).sqrt();
    (r, rmse)
}

/// Write binary DEM grids for the top matched Earth craters:
/// [u32 count, u32 width, u32 height, then `count` blocks of width*height f32 elevations]
pub fn write_matched_earth_crater_grids(
    path: &Path,
    matches: &[EarthCraterMatch],
    size: usize,
) -> Result<(), String> {
    let size = size.max(64);
    let count = matches.len() as u32;
    let mut buf = Vec::with_capacity(12 + matches.len() * size * size * 4);
    buf.extend_from_slice(&count.to_le_bytes());
    buf.extend_from_slice(&(size as u32).to_le_bytes());
    buf.extend_from_slice(&(size as u32).to_le_bytes());

    for m in matches {
        let grid = synthesize_earth_crater_grid(&m.crater, size);
        for &v in &grid.elevation {
            buf.extend_from_slice(&v.to_le_bytes());
        }
    }

    fs::write(path, &buf).map_err(|e| format!("Failed to write Earth crater grids {:?}: {}", path, e))
}
