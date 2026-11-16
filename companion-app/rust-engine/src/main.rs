pub mod bmp;
pub mod catalog;
pub mod crater;
pub mod earth_craters;
pub mod math;
pub mod model;
pub mod render;
pub mod stl;
pub mod tiff;

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use crate::bmp::read_bmp_to_grid;
use crate::catalog::{baseline_tif_path, default_recommended_sites, seed_all_placeholders};
use crate::crater::write_matched_earth_crater_grids;
use crate::math::analyze_map;
use crate::model::{PlanetaryBody, SuitabilityCriteria, TerrainGrid};
use crate::render::{export_report_bmps, write_telemetry_bin};
use crate::stl::read_stl_to_grid;
use crate::tiff::read_tiff_or_geotiff;

pub fn load_any_map(path: &Path, body: PlanetaryBody) -> Result<TerrainGrid, String> {
    load_any_map_with_format(path, body, None)
}

pub fn load_any_map_with_format(
    path: &Path,
    body: PlanetaryBody,
    source_format_override: Option<&str>,
) -> Result<TerrainGrid, String> {
    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match ext.as_str() {
        "tif" | "tiff" => read_tiff_or_geotiff(path, body),
        "stl" => read_stl_to_grid(path, body, 0, 0),
        "bmp" => read_bmp_to_grid(path, body, source_format_override),
        _ => Err(format!(
            "Unsupported file extension '.{}' (expected .tif, .tiff, .stl, or .bmp)",
            ext
        )),
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        print_usage();
        return;
    }

    match args[1].as_str() {
        "seed" => {
            let data_dir = args
                .get(2)
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("../data"));
            match seed_all_placeholders(&data_dir) {
                Ok(files) => {
                    println!("SEED_STATUS=OK");
                    for f in files {
                        println!("CREATED={}", f);
                    }
                }
                Err(e) => {
                    eprintln!("ERROR: {}", e);
                    std::process::exit(1);
                }
            }
        }
        "analyze" => {
            let mut body = PlanetaryBody::Mars;
            let mut data_dir = PathBuf::from("../data");
            let mut upload_path: Option<PathBuf> = None;
            let mut display_name_override: Option<String> = None;
            let mut source_format_override: Option<String> = None;
            let mut telemetry_out: Option<PathBuf> = None;
            let mut crater_grid_out: Option<PathBuf> = None;
            let mut json_out: Option<PathBuf> = None;
            let mut output_format = "kv".to_string();
            let mut criteria = SuitabilityCriteria::default();

            let mut i = 2;
            while i < args.len() {
                match args[i].as_str() {
                    "--body" if i + 1 < args.len() => {
                        if let Some(b) = PlanetaryBody::from_str(&args[i + 1]) {
                            body = b;
                        }
                        i += 2;
                    }
                    "--data-dir" if i + 1 < args.len() => {
                        data_dir = PathBuf::from(&args[i + 1]);
                        i += 2;
                    }
                    "--upload" if i + 1 < args.len() => {
                        let val = args[i + 1].trim();
                        if !val.is_empty() && !val.eq_ignore_ascii_case("baseline") {
                            upload_path = Some(PathBuf::from(val));
                        }
                        i += 2;
                    }
                    "--display-name" if i + 1 < args.len() => {
                        let val = args[i + 1].trim();
                        if !val.is_empty() {
                            display_name_override = Some(val.to_string());
                        }
                        i += 2;
                    }
                    "--source-format" if i + 1 < args.len() => {
                        let val = args[i + 1].trim();
                        if !val.is_empty() {
                            source_format_override = Some(val.to_string());
                        }
                        i += 2;
                    }
                    "--telemetry-out" if i + 1 < args.len() => {
                        telemetry_out = Some(PathBuf::from(&args[i + 1]));
                        i += 2;
                    }
                    "--crater-grid-out" if i + 1 < args.len() => {
                        crater_grid_out = Some(PathBuf::from(&args[i + 1]));
                        i += 2;
                    }
                    "--json-out" if i + 1 < args.len() => {
                        json_out = Some(PathBuf::from(&args[i + 1]));
                        i += 2;
                    }
                    "--format" if i + 1 < args.len() => {
                        output_format = args[i + 1].to_ascii_lowercase();
                        i += 2;
                    }
                    "--max-slope" if i + 1 < args.len() => {
                        if let Ok(v) = args[i + 1].parse::<f64>() {
                            criteria.max_slope_deg = v;
                        }
                        i += 2;
                    }
                    "--max-rough" if i + 1 < args.len() => {
                        if let Ok(v) = args[i + 1].parse::<f64>() {
                            criteria.max_roughness_m = v;
                        }
                        i += 2;
                    }
                    "--min-score" if i + 1 < args.len() => {
                        if let Ok(v) = args[i + 1].parse::<f64>() {
                            criteria.min_viability_score = v;
                        }
                        i += 2;
                    }
                    "--w-slope" if i + 1 < args.len() => {
                        if let Ok(v) = args[i + 1].parse::<f64>() {
                            criteria.weight_slope = v;
                        }
                        i += 2;
                    }
                    "--w-rough" if i + 1 < args.len() => {
                        if let Ok(v) = args[i + 1].parse::<f64>() {
                            criteria.weight_roughness = v;
                        }
                        i += 2;
                    }
                    "--w-elev" if i + 1 < args.len() => {
                        if let Ok(v) = args[i + 1].parse::<f64>() {
                            criteria.weight_elevation = v;
                        }
                        i += 2;
                    }
                    "--w-solar" if i + 1 < args.len() => {
                        if let Ok(v) = args[i + 1].parse::<f64>() {
                            criteria.weight_solar = v;
                        }
                        i += 2;
                    }
                    _ => {
                        i += 1;
                    }
                }
            }

            let base_file = baseline_tif_path(&data_dir, body);
            if !base_file.exists() {
                if let Err(e) = seed_all_placeholders(&data_dir) {
                    eprintln!("ERROR seeding placeholders: {}", e);
                    std::process::exit(1);
                }
            }

            let baseline_grid = match load_any_map(&base_file, body) {
                Ok(g) => g,
                Err(e) => {
                    eprintln!("ERROR loading baseline {:?}: {}", base_file, e);
                    std::process::exit(1);
                }
            };

            let has_upload = upload_path.is_some();
            let target_upload = upload_path.unwrap_or_else(|| base_file.clone());
            let uploaded_grid = match load_any_map_with_format(
                &target_upload,
                body,
                source_format_override.as_deref(),
            ) {
                Ok(g) => g,
                Err(e) => {
                    eprintln!("ERROR loading uploaded map {:?}: {}", target_upload, e);
                    std::process::exit(1);
                }
            };

            let sites = default_recommended_sites();
            let base_name = base_file
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("baseline.tif");
            let up_name_owned = display_name_override.unwrap_or_else(|| {
                target_upload
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("uploaded")
                    .to_string()
            });

            let (mut report, layers) = analyze_map(
                body,
                base_name,
                &up_name_owned,
                &baseline_grid,
                &uploaded_grid,
                &sites,
                criteria,
            );
            report.is_imported_map = has_upload;

            let reports_dir = data_dir.join("reports");
            let _ = fs::create_dir_all(&reports_dir);

            let bin_out = telemetry_out.unwrap_or_else(|| reports_dir.join("grid_telemetry.bin"));
            if let Err(e) = write_telemetry_bin(&bin_out, &uploaded_grid, &layers) {
                eprintln!("WARN: {}", e);
            }

            let cg_out = crater_grid_out.unwrap_or_else(|| reports_dir.join("earth_crater_grid.bin"));
            if let Err(e) = write_matched_earth_crater_grids(&cg_out, &report.earth_matches, 128) {
                eprintln!("WARN: {}", e);
            }

            let j_out = json_out.unwrap_or_else(|| reports_dir.join("latest_report.json"));
            let _ = fs::write(&j_out, report.to_json());
            let _ = export_report_bmps(&reports_dir, body, &uploaded_grid, &layers);

            if output_format == "json" {
                print!("{}", report.to_json());
            } else {
                print!("{}", report.to_kv_lines());
            }
        }
        _ => print_usage(),
    }
}

fn print_usage() {
    eprintln!("Usage:");
    eprintln!("  planetary-engine seed <data_dir>");
    eprintln!("  planetary-engine analyze --body <mars|moon> --data-dir <dir> [--upload <file.tif|.tiff|.stl>] [--format kv|json]");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::{synthesize_mars_grid, synthesize_moon_grid};
    use crate::stl::{write_ascii_stl, write_binary_stl};
    use crate::tiff::write_geotiff_f32;

    #[test]
    fn test_geotiff_and_stl_roundtrip_and_viability() {
        let tmp_dir = std::env::temp_dir().join("planetary_settler_rust_test");
        let _ = fs::create_dir_all(&tmp_dir);

        let mars_base = synthesize_mars_grid(64, 64, false);
        let mars_hires = synthesize_mars_grid(128, 128, true);

        let tif_path = tmp_dir.join("test_mars.tif");
        let tiff_path = tmp_dir.join("test_mars_hires.tiff");
        let stl_bin_path = tmp_dir.join("test_mars_mesh.stl");
        let stl_ascii_path = tmp_dir.join("test_moon_ascii.stl");

        write_geotiff_f32(&tif_path, &mars_base).expect("write .tif");
        write_geotiff_f32(&tiff_path, &mars_hires).expect("write .tiff");
        write_binary_stl(&stl_bin_path, &mars_hires, 2).expect("write binary .stl");

        let moon_base = synthesize_moon_grid(32, 32, false);
        write_ascii_stl(&stl_ascii_path, &moon_base, 2).expect("write ascii .stl");

        let loaded_base = load_any_map(&tif_path, PlanetaryBody::Mars).expect("load .tif");
        let loaded_hires = load_any_map(&tiff_path, PlanetaryBody::Mars).expect("load .tiff");
        let loaded_stl = load_any_map(&stl_bin_path, PlanetaryBody::Mars).expect("load binary .stl");
        let loaded_ascii = load_any_map(&stl_ascii_path, PlanetaryBody::Moon).expect("load ascii .stl");

        assert_eq!(loaded_base.width, 64);
        assert!(loaded_base.has_geotiff_tags);
        assert_eq!(loaded_hires.width, 128);
        assert!(loaded_stl.elevation.len() > 0);
        assert!(loaded_ascii.source_format.contains("ASCII"));

        let sites = default_recommended_sites();
        let (report, _) = analyze_map(
            PlanetaryBody::Mars,
            "test_mars.tif",
            "test_mars_hires.tiff",
            &loaded_base,
            &loaded_hires,
            &sites,
            SuitabilityCriteria::default(),
        );

        assert!(report.comparison.pearson_r > 0.85);
        // Verify M-1 is confirmed viable while M-2 catches the high-res scarp/boulder hazard!
        let m1 = report
            .site_evaluations
            .iter()
            .find(|s| s.site.id == "M-1")
            .unwrap();
        let m2 = report
            .site_evaluations
            .iter()
            .find(|s| s.site.id == "M-2")
            .unwrap();
        assert_eq!(m1.status, "CONFIRMED_VIABLE");
        assert_eq!(m2.status, "DOWNGRADED_HAZARD");
    }
}
