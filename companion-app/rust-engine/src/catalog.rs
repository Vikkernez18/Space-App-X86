use std::fs;
use std::path::Path;

use crate::earth_craters::export_earth_craters_json;
use crate::model::{GeoBounds, PlanetaryBody, RecommendedSite, TerrainGrid};
use crate::stl::{write_ascii_stl, write_binary_stl};
use crate::tiff::write_geotiff_f32;

pub fn default_recommended_sites() -> Vec<RecommendedSite> {
    vec![
        // MARS RECOMMENDED SETTLEMENT ZONES
        RecommendedSite {
            id: "M-1".to_string(),
            name: "Arcadia-Jezero Smooth Terrace".to_string(),
            body: PlanetaryBody::Mars,
            norm_x: 0.27,
            norm_y: 0.28,
            norm_radius: 0.095,
            lat_deg: 18.676,
            lon_deg: 77.416,
            radius_km: 2.8,
            rationale: "Low-elevation smooth volcanic plain (-2540 m MOLA) offering thick atmospheric column for heavy lander aerobraking and <2 deg baseline slope.".to_string(),
            primary_resources: "Subsurface water-ice sheet, basaltic regolith for radiation shielding, unobstructed solar horizon".to_string(),
        },
        RecommendedSite {
            id: "M-2".to_string(),
            name: "Jezero Western Delta Apron".to_string(),
            body: PlanetaryBody::Mars,
            norm_x: 0.52,
            norm_y: 0.54,
            norm_radius: 0.088,
            lat_deg: 18.432,
            lon_deg: 77.616,
            radius_km: 2.4,
            rationale: "Ancient lacustrine delta front with hydrated phyllosilicate clays and flat alluvial bench in coarse MOLA baseline imagery.".to_string(),
            primary_resources: "Hydrated clay minerals, sedimentary aggregates, proximity to ancient lakebed deposits".to_string(),
        },
        RecommendedSite {
            id: "M-3".to_string(),
            name: "Nili Fossae Regolith Bench".to_string(),
            body: PlanetaryBody::Mars,
            norm_x: 0.76,
            norm_y: 0.72,
            norm_radius: 0.092,
            lat_deg: 18.288,
            lon_deg: 77.808,
            radius_km: 2.6,
            rationale: "Consolidated basaltic plateau sheltered from regional dust storm corridors with stable load-bearing bedrock.".to_string(),
            primary_resources: "Olivine/carbonate outcrops, high-strength basalt bedrock, high solar insolation".to_string(),
        },
        // MOON RECOMMENDED SETTLEMENT ZONES
        RecommendedSite {
            id: "L-1".to_string(),
            name: "Shackleton Sunlit Rim Alpha".to_string(),
            body: PlanetaryBody::Moon,
            norm_x: 0.32,
            norm_y: 0.30,
            norm_radius: 0.090,
            lat_deg: -89.540,
            lon_deg: 2.400,
            radius_km: 1.8,
            rationale: "Elevated South Pole crater rim crest (+1720 m LOLA) with quasi-continuous solar illumination (>85% of lunar year) adjacent to cold-trap volatiles.".to_string(),
            primary_resources: "Permanently shadowed crater water-ice (<2 km), near-continuous solar power, Earth RF visibility".to_string(),
        },
        RecommendedSite {
            id: "L-2".to_string(),
            name: "Connecting Ridge Basecamp".to_string(),
            body: PlanetaryBody::Moon,
            norm_x: 0.58,
            norm_y: 0.48,
            norm_radius: 0.095,
            lat_deg: -89.180,
            lon_deg: -4.100,
            radius_km: 2.1,
            rationale: "Broad topographic saddle linking Shackleton and de Gerlache rims; combines flat landing terrain with dual cold-trap access.".to_string(),
            primary_resources: "Anorthositic regolith (oxygen extraction), solar ridge exposure, low-grade rover traverse routes".to_string(),
        },
        RecommendedSite {
            id: "L-3".to_string(),
            name: "Malapert Massif Southern Bench".to_string(),
            body: PlanetaryBody::Moon,
            norm_x: 0.74,
            norm_y: 0.73,
            norm_radius: 0.092,
            lat_deg: -88.620,
            lon_deg: 4.800,
            radius_km: 2.2,
            rationale: "High-altitude highland terrace maintaining uninterrupted direct line-of-sight to Earth for autonomous telemetry and crewed comms.".to_string(),
            primary_resources: "Uninterrupted Earth communications relay vantage, ilmenite/anorthosite regolith, thermal stability".to_string(),
        },
    ]
}

/// Generate synthetic Mars DEM (either coarse baseline or high-resolution scan with micro-features)
pub fn synthesize_mars_grid(width: usize, height: usize, high_res_detail: bool) -> TerrainGrid {
    let bounds = GeoBounds::default_for(PlanetaryBody::Mars, width, height);
    let mut elev = vec![0.0f32; width * height];

    // Craters: (u, v, radius_norm, depth_m, rim_height_m)
    let craters: &[(f64, f64, f64, f64, f64)] = &[
        (0.14, 0.72, 0.12, 340.0, 85.0),
        (0.84, 0.22, 0.14, 410.0, 105.0),
        (0.42, 0.16, 0.08, 220.0, 55.0),
        (0.65, 0.85, 0.09, 250.0, 62.0),
        (0.18, 0.14, 0.06, 160.0, 38.0),
    ];

    for y in 0..height {
        let v = (y as f64) / ((height - 1).max(1) as f64);
        for x in 0..width {
            let u = (x as f64) / ((width - 1).max(1) as f64);

            // Regional MOLA dichotomy slope + broad basin modulation (-2650 m to -2100 m)
            let mut z = -2520.0
                + 180.0 * (u * 2.2 + v * 1.4).sin()
                + 95.0 * ((u * 4.5).cos() * (v * 3.8).sin());

            // Add major craters with raised ejecta rims
            for &(cu, cv, r, depth, rim) in craters {
                let d = ((u - cu).powi(2) + (v - cv).powi(2)).sqrt() / r;
                if d < 1.0 {
                    // Bowl-shaped parabolic crater interior
                    z -= depth * (1.0 - d * d);
                } else if d < 1.45 {
                    // Raised crater rim tapering outward
                    let t = (d - 1.0) / 0.45;
                    z += rim * (1.0 - t).powi(2);
                }
            }

            // Smooth landing pads at recommended sites M-1 (0.27, 0.28), M-2 (0.52, 0.54), M-3 (0.76, 0.72)
            for &(su, sv, sr, pad_z) in &[
                (0.27f64, 0.28f64, 0.130f64, -2545.0f64),
                (0.52f64, 0.54f64, 0.125f64, -2430.0f64),
                (0.76f64, 0.72f64, 0.130f64, -2385.0f64),
            ] {
                let d = ((u - su).powi(2) + (v - sv).powi(2)).sqrt() / sr;
                if d < 1.25 {
                    let blend = (1.0 - (d / 1.25).min(1.0)).powi(2);
                    z = z * (1.0 - 0.94 * blend) + pad_z * (0.94 * blend);
                }
            }

            // In the coarse baseline map, M-3 has a slight artificial quantization ripple;
            // in the high-res scan, M-1 & M-3 are confirmed ultra-smooth, while M-2 (0.52, 0.54)
            // reveals a previously unresolved sub-pixel fault scarp & boulder field!
            if !high_res_detail {
                let d_m3 = ((u - 0.76).powi(2) + (v - 0.72).powi(2)).sqrt() / 0.10;
                if d_m3 < 1.0 {
                    z += 16.0 * (u * 55.0).sin() * (v * 55.0).cos() * (1.0 - d_m3);
                }
            } else {
                // Fine-scale HiRISE topographic texture outside safe pads
                let fine_tex = 2.8 * ((u * 95.0).sin() * (v * 95.0).cos());
                let d_m1 = ((u - 0.27).powi(2) + (v - 0.28).powi(2)).sqrt() / 0.11;
                let d_m3 = ((u - 0.76).powi(2) + (v - 0.72).powi(2)).sqrt() / 0.11;
                if d_m1 > 1.0 && d_m3 > 1.0 {
                    z += fine_tex;
                }

                // Sub-resolution hazard inside M-2 (Jezero Western Delta Apron):
                // Steep eroded scarp + coarse boulder clusters revealed only at high resolution!
                let d_m2 = ((u - 0.52).powi(2) + (v - 0.54).powi(2)).sqrt() / 0.095;
                if d_m2 < 1.0 {
                    let env = (1.0 - d_m2 * 0.65).max(0.0);
                    let scarp = 95.0 * ((u - 0.52) * 180.0).tanh();
                    let boulders =
                        58.0 * ((u * 190.0).sin() * (v * 190.0).cos()) + 38.0 * (u * 290.0 + v * 260.0).sin();
                    z += (scarp + boulders) * env;
                }
            }

            elev[y * width + x] = z as f32;
        }
    }

    TerrainGrid {
        width,
        height,
        elevation: elev,
        bounds,
        source_format: "GeoTIFF (.TIF)".to_string(),
        has_geotiff_tags: true,
        nodata_value: None,
    }
}

/// Generate synthetic Moon South Pole DEM (Shackleton Rim & Connecting Ridge)
pub fn synthesize_moon_grid(width: usize, height: usize, high_res_detail: bool) -> TerrainGrid {
    let bounds = GeoBounds::default_for(PlanetaryBody::Moon, width, height);
    let mut elev = vec![0.0f32; width * height];

    // Secondary polar craters: (u, v, radius_norm, depth_m, rim_m)
    let craters: &[(f64, f64, f64, f64, f64)] = &[
        (0.16, 0.18, 0.13, 980.0, 240.0), // Shackleton interior edge
        (0.82, 0.25, 0.11, 640.0, 150.0),
        (0.22, 0.78, 0.12, 720.0, 165.0),
        (0.45, 0.82, 0.08, 410.0, 95.0),
    ];

    for y in 0..height {
        let v = (y as f64) / ((height - 1).max(1) as f64);
        for x in 0..width {
            let u = (x as f64) / ((width - 1).max(1) as f64);

            // Broad highland ridge spine running diagonally across (0.32, 0.30) -> (0.58, 0.48) -> (0.74, 0.73)
            let dist_to_spine = ((v - (0.98 * u - 0.03)).abs()) / 1.4;
            let mut z = 1750.0 * (-dist_to_spine * 3.2).exp() - 320.0
                + 120.0 * (u * 3.1).sin() * (v * 2.7).cos();

            for &(cu, cv, r, depth, rim) in craters {
                let d = ((u - cu).powi(2) + (v - cv).powi(2)).sqrt() / r;
                if d < 1.0 {
                    z -= depth * (1.0 - d * d);
                } else if d < 1.4 {
                    let t = (d - 1.0) / 0.4;
                    z += rim * (1.0 - t).powi(2);
                }
            }

            // Smooth high-altitude benches at L-1 (0.32, 0.30), L-2 (0.58, 0.48), L-3 (0.74, 0.73)
            for &(su, sv, sr, pad_z) in &[
                (0.32f64, 0.30f64, 0.135f64, 1715.0f64),
                (0.58f64, 0.48f64, 0.135f64, 1540.0f64),
                (0.74f64, 0.73f64, 0.135f64, 1620.0f64),
            ] {
                let d = ((u - su).powi(2) + (v - sv).powi(2)).sqrt() / sr;
                if d < 1.30 {
                    let blend = if d < 0.78 {
                        0.97
                    } else {
                        0.97 * (1.0 - ((d - 0.78) / 0.52).min(1.0)).powi(2)
                    };
                    z = z * (1.0 - blend) + pad_z * blend;
                }
            }

            if high_res_detail {
                // High-res LOLA laser altimetry adds micro-cratered regolith texture outside L-1 and L-2,
                // while revealing a secondary impact crater swarm inside L-3 (0.74, 0.73)
                let d_l1 = ((u - 0.32).powi(2) + (v - 0.30).powi(2)).sqrt() / 0.11;
                let d_l2 = ((u - 0.58).powi(2) + (v - 0.48).powi(2)).sqrt() / 0.11;
                if d_l1 > 1.0 && d_l2 > 1.0 {
                    z += 2.5 * ((u * 90.0).sin() * (v * 90.0).cos());
                }

                let d_l3 = ((u - 0.74).powi(2) + (v - 0.73).powi(2)).sqrt() / 0.095;
                if d_l3 < 1.0 {
                    let env = (1.0 - d_l3 * 0.6).max(0.0);
                    let micro_craters = 64.0 * ((u * 195.0).sin() * (v * 195.0).cos())
                        + 85.0 * ((u - 0.74) * 180.0).tanh();
                    z += micro_craters * env;
                }
            }

            elev[y * width + x] = z as f32;
        }
    }

    TerrainGrid {
        width,
        height,
        elevation: elev,
        bounds,
        source_format: "GeoTIFF (.TIF)".to_string(),
        has_geotiff_tags: true,
        nodata_value: None,
    }
}

pub fn baseline_tif_path(data_dir: &Path, body: PlanetaryBody) -> std::path::PathBuf {
    let base = data_dir.join("baseline");
    match body {
        PlanetaryBody::Mars => base.join("mars_jezero_arcadia_baseline.tif"),
        PlanetaryBody::Moon => base.join("moon_shackleton_southpole_baseline.tif"),
    }
}

/// Create all baseline maps (.tif, .tiff, .stl) and high-res sample upload maps in `data_dir`
pub fn seed_all_placeholders(data_dir: &Path) -> Result<Vec<String>, String> {
    let base_dir = data_dir.join("baseline");
    let up_dir = data_dir.join("sample_uploads");
    let rep_dir = data_dir.join("reports");
    fs::create_dir_all(&base_dir).map_err(|e| e.to_string())?;
    fs::create_dir_all(&up_dir).map_err(|e| e.to_string())?;
    fs::create_dir_all(&rep_dir).map_err(|e| e.to_string())?;

    let mut created = Vec::new();

    // 1. Mars Baseline (128x128) in .tif, .tiff, and binary .stl
    let mars_base = synthesize_mars_grid(128, 128, false);
    let p_mars_tif = base_dir.join("mars_jezero_arcadia_baseline.tif");
    let p_mars_tiff = base_dir.join("mars_jezero_arcadia_baseline.tiff");
    let p_mars_stl = base_dir.join("mars_jezero_arcadia_baseline.stl");
    write_geotiff_f32(&p_mars_tif, &mars_base)?;
    write_geotiff_f32(&p_mars_tiff, &mars_base)?;
    write_binary_stl(&p_mars_stl, &mars_base, 2)?;
    created.push(p_mars_tif.display().to_string());
    created.push(p_mars_tiff.display().to_string());
    created.push(p_mars_stl.display().to_string());

    // 2. Moon Baseline (128x128) in .tif, .tiff, and binary .stl
    let moon_base = synthesize_moon_grid(128, 128, false);
    let p_moon_tif = base_dir.join("moon_shackleton_southpole_baseline.tif");
    let p_moon_tiff = base_dir.join("moon_shackleton_southpole_baseline.tiff");
    let p_moon_stl = base_dir.join("moon_shackleton_southpole_baseline.stl");
    write_geotiff_f32(&p_moon_tif, &moon_base)?;
    write_geotiff_f32(&p_moon_tiff, &moon_base)?;
    write_binary_stl(&p_moon_stl, &moon_base, 2)?;
    created.push(p_moon_tif.display().to_string());
    created.push(p_moon_tiff.display().to_string());
    created.push(p_moon_stl.display().to_string());

    // 3. High-Resolution Sample User Uploads (256x256 for GeoTIFFs, 128x128 / 64x64 for STL meshes)
    let mars_hires = synthesize_mars_grid(256, 256, true);
    let p_up_mars_tif = up_dir.join("mars_hires_hirise_scan.tif");
    let p_up_mars_tiff = up_dir.join("mars_hires_hirise_scan.tiff");
    let p_up_mars_stl = up_dir.join("mars_hires_custom_mesh.stl");
    write_geotiff_f32(&p_up_mars_tif, &mars_hires)?;
    write_geotiff_f32(&p_up_mars_tiff, &mars_hires)?;
    write_binary_stl(&p_up_mars_stl, &mars_hires, 2)?;
    created.push(p_up_mars_tif.display().to_string());
    created.push(p_up_mars_tiff.display().to_string());
    created.push(p_up_mars_stl.display().to_string());

    let moon_hires = synthesize_moon_grid(256, 256, true);
    let p_up_moon_tiff = up_dir.join("moon_hires_lola_dem.tiff");
    let p_up_moon_tif = up_dir.join("moon_hires_lola_dem.tif");
    let p_up_moon_stl = up_dir.join("moon_hires_crater_rim_ascii.stl");
    write_geotiff_f32(&p_up_moon_tiff, &moon_hires)?;
    write_geotiff_f32(&p_up_moon_tif, &moon_hires)?;
    write_ascii_stl(&p_up_moon_stl, &moon_hires, 4)?;
    created.push(p_up_moon_tiff.display().to_string());
    created.push(p_up_moon_tif.display().to_string());
    created.push(p_up_moon_stl.display().to_string());

    // 4. Write recommended_sites.json
    let sites = default_recommended_sites();
    let mut sites_json = String::from("[\n");
    for (i, s) in sites.iter().enumerate() {
        sites_json.push_str(&format!(
            "  {{\"id\": \"{}\", \"name\": \"{}\", \"body\": \"{}\", \"lat_deg\": {:.4}, \"lon_deg\": {:.4}, \"radius_km\": {:.2}, \"norm_x\": {:.3}, \"norm_y\": {:.3}, \"norm_radius\": {:.3}, \"rationale\": \"{}\", \"primary_resources\": \"{}\"}}{}\n",
            s.id,
            s.name,
            s.body.as_str(),
            s.lat_deg,
            s.lon_deg,
            s.radius_km,
            s.norm_x,
            s.norm_y,
            s.norm_radius,
            s.rationale,
            s.primary_resources,
            if i + 1 < sites.len() { "," } else { "" }
        ));
    }
    sites_json.push_str("]\n");
    let p_sites = base_dir.join("recommended_sites.json");
    fs::write(&p_sites, sites_json).map_err(|e| e.to_string())?;
    created.push(p_sites.display().to_string());

    // 5. Export Earth Crater Reference Database JSON
    let p_earth_craters = data_dir.join("earth_craters").join("earth_craters.json");
    export_earth_craters_json(&p_earth_craters)?;
    created.push(p_earth_craters.display().to_string());

    Ok(created)
}
