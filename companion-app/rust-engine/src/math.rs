use std::f64::consts::PI;

use crate::crater::analyze_craters;
use crate::model::{
    AnalysisReport, ComparisonMetrics, PlanetaryBody, RecommendedSite, SiteEvaluation,
    SuitabilityCriteria, TerrainGrid,
};

pub struct DerivedLayers {
    pub width: usize,
    pub height: usize,
    pub slope_deg: Vec<f32>,
    pub roughness_m: Vec<f32>,
    pub solar_score: Vec<f32>,
    pub viability_score: Vec<f32>,
    /// Residual difference (upload - baseline resampled to upload dims) in meters
    pub residual_diff_m: Vec<f32>,
}

/// Bilinear resampling of a TerrainGrid to target dimensions (dst_w, dst_h)
pub fn resample_elevation(grid: &TerrainGrid, dst_w: usize, dst_h: usize) -> Vec<f32> {
    if grid.width == dst_w && grid.height == dst_h {
        return grid.elevation.clone();
    }
    let mut out = vec![0.0f32; dst_w * dst_h];
    let sx = ((grid.width - 1).max(1) as f32) / ((dst_w - 1).max(1) as f32);
    let sy = ((grid.height - 1).max(1) as f32) / ((dst_h - 1).max(1) as f32);

    for y in 0..dst_h {
        let fy = (y as f32) * sy;
        let y0 = (fy.floor() as usize).min(grid.height - 1);
        let y1 = (y0 + 1).min(grid.height - 1);
        let wy = fy - (y0 as f32);

        for x in 0..dst_w {
            let fx = (x as f32) * sx;
            let x0 = (fx.floor() as usize).min(grid.width - 1);
            let x1 = (x0 + 1).min(grid.width - 1);
            let wx = fx - (x0 as f32);

            let v00 = grid.get(x0, y0);
            let v10 = grid.get(x1, y0);
            let v01 = grid.get(x0, y1);
            let v11 = grid.get(x1, y1);

            let top = v00 * (1.0 - wx) + v10 * wx;
            let bot = v01 * (1.0 - wx) + v11 * wx;
            out[y * dst_w + x] = top * (1.0 - wy) + bot * wy;
        }
    }
    out
}

/// Horn's 3x3 finite-difference slope angle (in degrees)
pub fn compute_slope_deg(grid: &TerrainGrid) -> Vec<f32> {
    let w = grid.width;
    let h = grid.height;
    let dx = grid.bounds.meters_per_pixel_x.max(0.1) as f32;
    let dy = grid.bounds.meters_per_pixel_y.max(0.1) as f32;
    let mut slope = vec![0.0f32; w * h];

    let sample = |x: isize, y: isize| -> f32 {
        let cx = x.clamp(0, (w as isize) - 1) as usize;
        let cy = y.clamp(0, (h as isize) - 1) as usize;
        grid.get(cx, cy)
    };

    for y in 0..h {
        let iy = y as isize;
        for x in 0..w {
            let ix = x as isize;
            let z_nw = sample(ix - 1, iy - 1);
            let z_n = sample(ix, iy - 1);
            let z_ne = sample(ix + 1, iy - 1);
            let z_w = sample(ix - 1, iy);
            let z_e = sample(ix + 1, iy);
            let z_sw = sample(ix - 1, iy + 1);
            let z_s = sample(ix, iy + 1);
            let z_se = sample(ix + 1, iy + 1);

            let dz_dx = ((z_ne + 2.0 * z_e + z_se) - (z_nw + 2.0 * z_w + z_sw)) / (8.0 * dx);
            let dz_dy = ((z_sw + 2.0 * z_s + z_se) - (z_nw + 2.0 * z_n + z_ne)) / (8.0 * dy);
            let grad_mag = (dz_dx * dz_dx + dz_dy * dz_dy).sqrt();
            let deg = (grad_mag as f64).atan() * (180.0 / PI);
            slope[y * w + x] = deg as f32;
        }
    }
    slope
}

/// Terrain Ruggedness Index (TRI): root-mean-square elevation delta normalized to a 100m horizontal reference span
pub fn compute_roughness_tri(grid: &TerrainGrid) -> Vec<f32> {
    let w = grid.width;
    let h = grid.height;
    let dx = grid.bounds.meters_per_pixel_x.max(1.0) as f32;
    let scale_norm = (250.0 / dx).clamp(0.25, 4.0);
    let mut tri = vec![0.0f32; w * h];

    for y in 0..h {
        for x in 0..w {
            let c = grid.get(x, y);
            let mut sq_sum = 0.0f32;
            let mut cnt = 0usize;
            for dy in -1isize..=1 {
                for dx_i in -1isize..=1 {
                    if dx_i == 0 && dy == 0 {
                        continue;
                    }
                    let nx = (x as isize + dx_i).clamp(0, (w as isize) - 1) as usize;
                    let ny = (y as isize + dy).clamp(0, (h as isize) - 1) as usize;
                    let d = grid.get(nx, ny) - c;
                    sq_sum += d * d;
                    cnt += 1;
                }
            }
            tri[y * w + x] = (sq_sum / (cnt.max(1) as f32)).sqrt() * scale_norm;
        }
    }
    tri
}

/// Solar illumination & horizon exposure score in [0.0, 1.0]
pub fn compute_solar_score(grid: &TerrainGrid, body: PlanetaryBody) -> Vec<f32> {
    let w = grid.width;
    let h = grid.height;
    let dx = grid.bounds.meters_per_pixel_x.max(0.1) as f32;
    let dy = grid.bounds.meters_per_pixel_y.max(0.1) as f32;
    let (min_z, max_z, _) = grid.min_max_mean();
    let z_span = (max_z - min_z).max(1.0);

    let mut out = vec![0.0f32; w * h];
    for y in 0..h {
        for x in 0..w {
            let x0 = x.saturating_sub(1);
            let x1 = (x + 1).min(w - 1);
            let y0 = y.saturating_sub(1);
            let y1 = (y + 1).min(h - 1);

            let dz_dx = (grid.get(x1, y) - grid.get(x0, y)) / (2.0 * dx);
            let dz_dy = (grid.get(x, y1) - grid.get(x, y0)) / (2.0 * dy);
            let rel_z = ((grid.get(x, y) - min_z) / z_span).clamp(0.0, 1.0);

            // On Mars (~18.5N), gentle south-facing slopes & unshadowed plains receive optimal solar flux.
            // On Moon South Pole (~89S), high sunlit topographic ridges receive quasi-continuous illumination.
            let score = match body {
                PlanetaryBody::Mars => {
                    let south_aspect = (-dz_dy * 4.0).tanh() * 0.15 + 0.80;
                    let flat_bonus = (-((dz_dx * dz_dx + dz_dy * dz_dy).sqrt()) * 2.5).exp() * 0.15;
                    (south_aspect + flat_bonus).clamp(0.1, 1.0)
                }
                PlanetaryBody::Moon => {
                    // South Pole rim: altitude above deep crater floor strongly drives illumination percentage
                    let altitude_factor = 0.25 + 0.70 * rel_z;
                    let north_aspect = (dz_dy * 3.0).tanh() * 0.10;
                    (altitude_factor + north_aspect).clamp(0.05, 1.0)
                }
            };
            out[y * w + x] = score;
        }
    }
    out
}

/// Compute cell-by-cell Multi-Criteria Viability Score in [0.0, 100.0]
pub fn compute_derived_layers(
    baseline: &TerrainGrid,
    uploaded: &TerrainGrid,
    body: PlanetaryBody,
    criteria: &SuitabilityCriteria,
) -> DerivedLayers {
    let w = uploaded.width;
    let h = uploaded.height;
    let slope_deg = compute_slope_deg(uploaded);
    let roughness_m = compute_roughness_tri(uploaded);
    let solar_score = compute_solar_score(uploaded, body);
    let baseline_on_upload = resample_elevation(baseline, w, h);

    let (min_z, max_z, _) = uploaded.min_max_mean();
    let z_span = (max_z - min_z).max(1.0);
    let (w_slope, w_rough, w_elev, w_solar) = criteria.normalized_weights();

    let mut viability_score = vec![0.0f32; w * h];
    let mut residual_diff_m = vec![0.0f32; w * h];

    let max_slope = criteria.max_slope_deg.max(0.5) as f32;
    let max_rough = criteria.max_roughness_m.max(0.5) as f32;

    for i in 0..(w * h) {
        let z = uploaded.elevation[i];
        let s = slope_deg[i];
        let r = roughness_m[i];
        let sol = solar_score[i];
        residual_diff_m[i] = z - baseline_on_upload[i];

        // 1. Slope sub-score [0..1]: smooth near-1 for flat terrain, drops steeply near max_slope
        let s_ratio = s / max_slope;
        let f_slope = if s <= max_slope {
            (1.0 - 0.55 * s_ratio * s_ratio).clamp(0.0, 1.0)
        } else {
            (0.45 * (-(s - max_slope) / (0.5 * max_slope)).exp()).clamp(0.0, 0.45)
        };

        // 2. Roughness (TRI) sub-score [0..1]
        let r_ratio = r / max_rough;
        let f_rough = if r <= max_rough {
            (1.0 - 0.50 * r_ratio * r_ratio).clamp(0.0, 1.0)
        } else {
            (0.50 * (-(r - max_rough) / (0.6 * max_rough)).exp()).clamp(0.0, 0.50)
        };

        // 3. Elevation sub-score [0..1]
        // Mars: lower elevation (< -2200 m) provides higher atmospheric column density for EDL & radiation shielding,
        // while avoiding deep crater traps.
        // Moon South Pole: higher rim elevation provides earth communication line-of-sight & solar access.
        let rel_z = ((z - min_z) / z_span).clamp(0.0, 1.0);
        let f_elev = match body {
            PlanetaryBody::Mars => (1.0 - (rel_z - 0.35).abs() * 1.1).clamp(0.2, 1.0),
            PlanetaryBody::Moon => (0.25 + 0.75 * rel_z).clamp(0.1, 1.0),
        };

        let mut combined = (w_slope as f32) * f_slope
            + (w_rough as f32) * f_rough
            + (w_elev as f32) * f_elev
            + (w_solar as f32) * sol;

        // Hard engineering safety penalty if slope or roughness exceeds critical landing threshold
        if s > max_slope * 1.25 || r > max_rough * 1.35 {
            combined *= 0.48;
        } else if s > max_slope || r > max_rough {
            combined *= 0.72;
        }

        viability_score[i] = (combined * 100.0).clamp(0.0, 100.0);
    }

    DerivedLayers {
        width: w,
        height: h,
        slope_deg,
        roughness_m,
        solar_score,
        viability_score,
        residual_diff_m,
    }
}

/// Mathematical comparison between the uploaded map and the baseline JMARS reference map
pub fn compare_maps(
    baseline: &TerrainGrid,
    uploaded: &TerrainGrid,
    min_corr_threshold: f64,
) -> ComparisonMetrics {
    let w = uploaded.width;
    let h = uploaded.height;
    let n = (w * h).max(1) as f64;
    let base_resampled = resample_elevation(baseline, w, h);

    let mut sum_u = 0.0f64;
    let mut sum_b = 0.0f64;
    let mut sq_err_sum = 0.0f64;
    let mut abs_err_sum = 0.0f64;
    let mut max_abs_diff = 0.0f64;

    for i in 0..(w * h) {
        let u = uploaded.elevation[i] as f64;
        let b = base_resampled[i] as f64;
        let diff = (u - b).abs();
        sum_u += u;
        sum_b += b;
        sq_err_sum += (u - b) * (u - b);
        abs_err_sum += diff;
        if diff > max_abs_diff {
            max_abs_diff = diff;
        }
    }

    let mean_u = sum_u / n;
    let mean_b = sum_b / n;

    let mut cov = 0.0f64;
    let mut var_u = 0.0f64;
    let mut var_b = 0.0f64;
    for i in 0..(w * h) {
        let du = (uploaded.elevation[i] as f64) - mean_u;
        let db = (base_resampled[i] as f64) - mean_b;
        cov += du * db;
        var_u += du * du;
        var_b += db * db;
    }

    let pearson_r = if var_u > 1e-9 && var_b > 1e-9 {
        (cov / (var_u.sqrt() * var_b.sqrt())).clamp(-1.0, 1.0)
    } else {
        1.0
    };

    let sigma_u_sq = var_u / n;
    let sigma_b_sq = var_b / n;
    let sigma_ub = cov / n;
    let dynamic_range = 2500.0f64;
    let c1 = (0.01 * dynamic_range).powi(2);
    let c2 = (0.03 * dynamic_range).powi(2);
    let ssim = (((2.0 * mean_u * mean_b + c1) * (2.0 * sigma_ub + c2))
        / (((mean_u * mean_u + mean_b * mean_b + c1) * (sigma_u_sq + sigma_b_sq + c2)).max(1e-9)))
    .clamp(-1.0, 1.0);

    let rmse_m = (sq_err_sum / n).sqrt();
    let mae_m = abs_err_sum / n;

    let resolution_ratio =
        (baseline.bounds.meters_per_pixel_x / uploaded.bounds.meters_per_pixel_x.max(0.01)).max(0.01);

    // High-frequency detail energy ratio (mean absolute Laplacian on uploaded vs resampled baseline)
    let hf_upload = mean_laplacian_energy(&uploaded.elevation, w, h);
    let hf_base = mean_laplacian_energy(&base_resampled, w, h).max(1e-4);
    let high_freq_detail_ratio = (hf_upload / hf_base).clamp(0.1, 50.0);

    let alignment_status = if pearson_r >= 0.85 {
        "EXCELLENT_ALIGNMENT (High Regional Fidelity)".to_string()
    } else if pearson_r >= min_corr_threshold {
        "VALID_ALIGNMENT (Compatible Planetary Sector)".to_string()
    } else {
        "MISMATCH_WARNING (Low Spatial Correlation with Baseline)".to_string()
    };

    ComparisonMetrics {
        pearson_r,
        ssim,
        rmse_m,
        mae_m,
        max_abs_diff_m: max_abs_diff,
        resolution_ratio,
        high_freq_detail_ratio,
        alignment_status,
    }
}

fn mean_laplacian_energy(buf: &[f32], w: usize, h: usize) -> f64 {
    if w < 3 || h < 3 {
        return 1.0;
    }
    let mut sum = 0.0f64;
    let mut cnt = 0usize;
    for y in 1..(h - 1) {
        for x in 1..(w - 1) {
            let c = buf[y * w + x];
            let lap = buf[(y - 1) * w + x]
                + buf[(y + 1) * w + x]
                + buf[y * w + (x - 1)]
                + buf[y * w + (x + 1)]
                - 4.0 * c;
            sum += (lap.abs()) as f64;
            cnt += 1;
        }
    }
    sum / (cnt.max(1) as f64)
}

/// Run full mathematical comparison and evaluate all recommended settlement zones
pub fn analyze_map(
    body: PlanetaryBody,
    baseline_file: &str,
    uploaded_file: &str,
    baseline: &TerrainGrid,
    uploaded: &TerrainGrid,
    sites: &[RecommendedSite],
    criteria: SuitabilityCriteria,
) -> (AnalysisReport, DerivedLayers) {
    let base_layers = compute_derived_layers(baseline, baseline, body, &criteria);
    let up_layers = compute_derived_layers(baseline, uploaded, body, &criteria);
    let comparison = compare_maps(baseline, uploaded, criteria.min_correlation_r);

    let (elev_min, elev_max, elev_mean) = uploaded.min_max_mean();
    let n_up = (uploaded.width * uploaded.height).max(1) as f64;
    let global_mean_slope_deg =
        up_layers.slope_deg.iter().map(|&v| v as f64).sum::<f64>() / n_up;
    let global_mean_roughness_m =
        up_layers.roughness_m.iter().map(|&v| v as f64).sum::<f64>() / n_up;
    let global_viability_score =
        up_layers.viability_score.iter().map(|&v| v as f64).sum::<f64>() / n_up;

    let mut site_evaluations = Vec::with_capacity(sites.len());

    for site in sites {
        if site.body != body {
            continue;
        }
        let base_stats = sample_zone_stats(baseline, &base_layers, site, &criteria);
        let up_stats = sample_zone_stats(uploaded, &up_layers, site, &criteria);
        let score_delta = up_stats.mean_score - base_stats.mean_score;

        let (status, finding_summary) = if comparison.pearson_r < criteria.min_correlation_r {
            (
                "MISMATCH_OR_INVALID".to_string(),
                format!(
                    "Uploaded map has low spatial correlation (r={:.2}) with baseline {}; verify coordinates/projection.",
                    comparison.pearson_r,
                    body.as_str()
                ),
            )
        } else if up_stats.mean_score >= criteria.min_viability_score
            && up_stats.safe_fraction >= 0.80
        {
            if base_stats.mean_score < criteria.min_viability_score + 8.0 && score_delta >= 4.0 {
                (
                    "UPGRADED_VIABLE".to_string(),
                    format!(
                        "High-res data upgraded borderline site (+{:.1} pts to {:.1}/100); smooth pad confirmed ({:.1}% safe area, mean slope {:.2}°).",
                        score_delta,
                        up_stats.mean_score,
                        up_stats.safe_fraction * 100.0,
                        up_stats.mean_slope_deg
                    ),
                )
            } else {
                (
                    "CONFIRMED_VIABLE".to_string(),
                    format!(
                        "Viability confirmed ({:.1}/100 vs baseline {:.1}); {:.1}% of zone meets slope (<{:.1}°) & roughness (<{:.1}m) limits.",
                        up_stats.mean_score,
                        base_stats.mean_score,
                        up_stats.safe_fraction * 100.0,
                        criteria.max_slope_deg,
                        criteria.max_roughness_m
                    ),
                )
            }
        } else if base_stats.mean_score >= criteria.min_viability_score {
            (
                "DOWNGRADED_HAZARD".to_string(),
                format!(
                    "HAZARD DETECTED at higher resolution: score dropped {:.1} -> {:.1}/100 (max local slope {:.1}°, roughness {:.1}m, safe area {:.0}%).",
                    base_stats.mean_score,
                    up_stats.mean_score,
                    up_stats.max_slope_deg,
                    up_stats.mean_roughness_m,
                    up_stats.safe_fraction * 100.0
                ),
            )
        } else {
            (
                "BELOW_THRESHOLD".to_string(),
                format!(
                    "Zone score {:.1}/100 is below current cutoff ({:.1}); mean slope {:.2}°, roughness {:.2}m.",
                    up_stats.mean_score,
                    criteria.min_viability_score,
                    up_stats.mean_slope_deg,
                    up_stats.mean_roughness_m
                ),
            )
        };

        site_evaluations.push(SiteEvaluation {
            site: site.clone(),
            baseline_score: base_stats.mean_score,
            upload_score: up_stats.mean_score,
            score_delta,
            baseline_mean_slope_deg: base_stats.mean_slope_deg,
            upload_mean_slope_deg: up_stats.mean_slope_deg,
            upload_max_slope_deg: up_stats.max_slope_deg,
            baseline_roughness_m: base_stats.mean_roughness_m,
            upload_roughness_m: up_stats.mean_roughness_m,
            mean_elevation_m: up_stats.mean_elevation_m,
            solar_score: up_stats.mean_solar * 100.0,
            local_rmse_m: up_stats.local_rmse_m,
            safe_area_fraction: up_stats.safe_fraction,
            status,
            finding_summary,
        });
    }

    let crater_res = analyze_craters(uploaded, body);

    let report = AnalysisReport {
        body,
        baseline_file: baseline_file.to_string(),
        uploaded_file: uploaded_file.to_string(),
        uploaded_format: uploaded.source_format.clone(),
        is_imported_map: baseline_file != uploaded_file,
        baseline_dims: (baseline.width, baseline.height),
        uploaded_dims: (uploaded.width, uploaded.height),
        baseline_res_m: baseline.bounds.meters_per_pixel_x,
        uploaded_res_m: uploaded.bounds.meters_per_pixel_x,
        has_geotiff_tags: uploaded.has_geotiff_tags,
        elevation_min_m: elev_min,
        elevation_max_m: elev_max,
        elevation_mean_m: elev_mean,
        global_mean_slope_deg,
        global_mean_roughness_m,
        global_viability_score,
        comparison,
        criteria,
        site_evaluations,
        detected_craters: crater_res.detected_craters,
        earth_matches: crater_res.earth_matches,
    };

    (report, up_layers)
}

struct ZoneStats {
    mean_score: f64,
    mean_slope_deg: f64,
    max_slope_deg: f64,
    mean_roughness_m: f64,
    mean_elevation_m: f64,
    mean_solar: f64,
    local_rmse_m: f64,
    safe_fraction: f64,
}

fn sample_zone_stats(
    grid: &TerrainGrid,
    layers: &DerivedLayers,
    site: &RecommendedSite,
    criteria: &SuitabilityCriteria,
) -> ZoneStats {
    let w = grid.width;
    let h = grid.height;
    let cx = site.norm_x * ((w - 1) as f64);
    let cy = site.norm_y * ((h - 1) as f64);
    let rad_px = (site.norm_radius * ((w.min(h)) as f64)).max(2.0);
    let rad_sq = rad_px * rad_px;

    let x_min = ((cx - rad_px).floor().max(0.0) as usize).min(w - 1);
    let x_max = ((cx + rad_px).ceil().max(0.0) as usize).min(w - 1);
    let y_min = ((cy - rad_px).floor().max(0.0) as usize).min(h - 1);
    let y_max = ((cy + rad_px).ceil().max(0.0) as usize).min(h - 1);

    let mut sum_score = 0.0f64;
    let mut sum_slope = 0.0f64;
    let mut max_slope = 0.0f64;
    let mut sum_rough = 0.0f64;
    let mut sum_elev = 0.0f64;
    let mut sum_solar = 0.0f64;
    let mut sum_sq_diff = 0.0f64;
    let mut safe_cnt = 0usize;
    let mut cnt = 0usize;

    for y in y_min..=y_max {
        for x in x_min..=x_max {
            let dx = (x as f64) - cx;
            let dy = (y as f64) - cy;
            if dx * dx + dy * dy <= rad_sq {
                let idx = y * w + x;
                let sc = layers.viability_score[idx] as f64;
                let sl = layers.slope_deg[idx] as f64;
                let ro = layers.roughness_m[idx] as f64;
                let el = grid.elevation[idx] as f64;
                let so = layers.solar_score[idx] as f64;
                let df = layers.residual_diff_m[idx] as f64;

                sum_score += sc;
                sum_slope += sl;
                if sl > max_slope {
                    max_slope = sl;
                }
                sum_rough += ro;
                sum_elev += el;
                sum_solar += so;
                sum_sq_diff += df * df;
                if sl <= criteria.max_slope_deg && ro <= criteria.max_roughness_m {
                    safe_cnt += 1;
                }
                cnt += 1;
            }
        }
    }

    let n = cnt.max(1) as f64;
    ZoneStats {
        mean_score: sum_score / n,
        mean_slope_deg: sum_slope / n,
        max_slope_deg: max_slope,
        mean_roughness_m: sum_rough / n,
        mean_elevation_m: sum_elev / n,
        mean_solar: sum_solar / n,
        local_rmse_m: (sum_sq_diff / n).sqrt(),
        safe_fraction: (safe_cnt as f64) / n,
    }
}
