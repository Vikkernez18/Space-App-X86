// AI-assisted
//! `analog_core` — WASM-compatible shared library for the Space-App-X86 platform.
//!
//! Provides:
//! - [`models`]: Shared data structures (`Catalog`, `SiteMetadata`, `DemExtent`, `Body`, `MeshFormat`, `WeightsConfig`).
//! - [`stl`]: Custom binary STL parser with vertex welding, automatic deterministic mesh simplification
//!   (`<= 50,000` triangles), Z-up to Bevy Y-up conversion, and `DemExtent`-driven planar UV mapping.
//! - [`tiff_dem`]: In-memory TIFF/GeoTIFF DEM elevation raster parser (`u16`, `i16`, `f32`)
//!   with automatic decimation (`<= 50,000` triangles) returning [`stl::WeldedMesh`].
//! - [`scoring`]: Dynamic weighted-sum recalculation over pre-normalized `[0.0, 1.0]` feature scores.
//! - [`terrain_analysis`]: Morphometric, differential-geometric, and 0D Persistent Homology (TDA)
//!   engine with `<= 1024x1024` DEM subsampling and RFC 7578 `multipart/form-data` binary transport.

pub mod models;
pub mod scoring;
pub mod stl;
pub mod terrain_analysis;
pub mod tiff_dem;

pub use models::{
    Body, Catalog, DemExtent, MeshFormat, RecalculateRequest, RecalculateResponse, ScoredSite,
    SiteMetadata, WeightsConfig,
};
pub use stl::{
    WeldedMesh, parse_and_weld_stl, parse_and_weld_stl_with_stats, read_stl_triangle_count,
};
pub use terrain_analysis::{
    AnalogProposal, MAX_TDA_GRID_DIM, MorphometryMetrics, ProcessedSiteBundle,
    TerrainAnalysisReport, TopologyMetrics, UploadSiteRequest, UploadSiteResponse,
    analyze_and_build_site_bundle, base64_decode, base64_encode, decode_multipart_upload_request,
    encode_multipart_upload_request, subsample_grid_max_dim,
};
pub use tiff_dem::parse_dem_tiff;

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::models::{Body, Catalog, DemExtent, MeshFormat, SiteMetadata};
    use crate::scoring::{compute_weighted_score, recalculate_catalog};
    use crate::stl::{
        MAX_TRIANGLES, StlError, encode_binary_stl, parse_and_weld_stl,
        parse_and_weld_stl_with_stats, stl_to_bevy_position,
    };
    use crate::tiff_dem::{
        compute_decimation_step, encode_uncompressed_f32_tiff, encode_uncompressed_i16_tiff,
        encode_uncompressed_u16_tiff, parse_dem_tiff,
    };

    #[test]
    fn test_z_up_to_bevy_y_up_conversion() {
        // STL Z-up: +X = East (100), +Y = North (250), +Z = Elevation (42)
        // Bevy Y-up: X_bevy = 100, Y_bevy = 42 (Up), Z_bevy = -250 (North is -Z)
        let bevy_pos = stl_to_bevy_position(100.0, 250.0, 42.0);
        assert_eq!(bevy_pos, [100.0, 42.0, -250.0]);
    }

    #[test]
    fn test_planar_uv_checkerboard_corners_and_non_aabb_dependence() {
        let extent = DemExtent::new(-1000.0, 1000.0, -500.0, 1500.0);
        assert!(extent.is_valid());

        // North-West corner: (min_x, max_y) -> (u = 0.0, v = 0.0)
        let nw = extent.compute_uv(-1000.0, 1500.0);
        assert!((nw[0] - 0.0).abs() < 1e-6);
        assert!((nw[1] - 0.0).abs() < 1e-6);

        // South-East corner: (max_x, min_y) -> (u = 1.0, v = 1.0)
        let se = extent.compute_uv(1000.0, -500.0);
        assert!((se[0] - 1.0).abs() < 1e-6);
        assert!((se[1] - 1.0).abs() < 1e-6);

        // Center point: (0.0, 500.0) -> (u = 0.5, v = 0.5)
        let center = extent.compute_uv(0.0, 500.0);
        assert!((center[0] - 0.5).abs() < 1e-6);
        assert!((center[1] - 0.5).abs() < 1e-6);

        // Sub-region triangle covering only the NE quadrant of DemExtent:
        // UVs must still be computed from DemExtent, NOT stretched to [0, 1] by mesh AABB.
        let triangles = [[
            [0.0, 500.0, 10.0],     // center -> u=0.5, v=0.5
            [1000.0, 500.0, 20.0],  // east-mid -> u=1.0, v=0.5
            [1000.0, 1500.0, 30.0], // NE corner -> u=1.0, v=0.0
        ]];
        let bytes = encode_binary_stl(&triangles);
        let mesh = parse_and_weld_stl(&bytes, &extent).expect("valid STL should parse");
        assert_eq!(mesh.positions.len(), 3);
        assert!((mesh.uvs[0][0] - 0.5).abs() < 1e-6 && (mesh.uvs[0][1] - 0.5).abs() < 1e-6);
        assert!((mesh.uvs[1][0] - 1.0).abs() < 1e-6 && (mesh.uvs[1][1] - 0.5).abs() < 1e-6);
        assert!((mesh.uvs[2][0] - 1.0).abs() < 1e-6 && (mesh.uvs[2][1] - 0.0).abs() < 1e-6);
    }

    #[test]
    fn test_binary_stl_vertex_welding_and_normals() {
        let extent = DemExtent::new(0.0, 100.0, 0.0, 100.0);
        // Two triangles forming a flat horizontal quad at Z_stl = 15.0 m
        let triangles = [
            [[0.0, 0.0, 15.0], [100.0, 0.0, 15.0], [100.0, 100.0, 15.0]],
            [[0.0, 0.0, 15.0], [100.0, 100.0, 15.0], [0.0, 100.0, 15.0]],
        ];
        let bytes = encode_binary_stl(&triangles);
        let welded = parse_and_weld_stl(&bytes, &extent).expect("quad STL should parse");

        // 6 raw STL vertices weld down to 4 unique corner vertices
        assert_eq!(welded.triangle_count, 2);
        assert_eq!(welded.positions.len(), 4);
        assert_eq!(welded.normals.len(), 4);
        assert_eq!(welded.uvs.len(), 4);
        assert_eq!(welded.indices.len(), 6);

        // Flat horizontal terrain in Z-up must have +Y normal [0, 1, 0] in Bevy Y-up space
        for n in &welded.normals {
            assert!((n[0] - 0.0).abs() < 1e-5);
            assert!((n[1] - 1.0).abs() < 1e-5);
            assert!((n[2] - 0.0).abs() < 1e-5);
        }
    }

    #[test]
    fn test_large_stl_automatic_simplification_and_truncated_buffer_rejection() {
        let extent = DemExtent::new(0.0, 1000.0, 0.0, 1000.0);

        // 1. A truncated 84-byte buffer claiming 50,001 triangles is rejected with UnexpectedBufferLength.
        let mut fake_header = vec![0u8; 80];
        let over_budget: u32 = MAX_TRIANGLES + 1;
        fake_header.extend_from_slice(&over_budget.to_le_bytes());

        let err = parse_and_weld_stl(&fake_header, &extent).unwrap_err();
        assert_eq!(
            err,
            StlError::UnexpectedBufferLength {
                expected: 84 + ((MAX_TRIANGLES as usize + 1) * 50),
                actual: 84,
            }
        );

        // 2. A valid binary STL exceeding 50,000 triangles (165 x 165 cells * 2 = 54,450 triangles)
        // is automatically simplified to <= 50,000 triangles while preserving corners and relief.
        let grid_cells = 165usize;
        let dx = 1000.0 / (grid_cells as f32);
        let dy = 1000.0 / (grid_cells as f32);
        let mut tris = Vec::with_capacity(grid_cells * grid_cells * 2);
        for iy in 0..grid_cells {
            let y0 = (iy as f32) * dy;
            let y1 = ((iy + 1) as f32) * dy;
            for ix in 0..grid_cells {
                let x0 = (ix as f32) * dx;
                let x1 = ((ix + 1) as f32) * dx;
                let z = ((ix + iy) as f32) * 0.5;
                tris.push([[x0, y0, z], [x1, y0, z], [x1, y1, z]]);
                tris.push([[x0, y0, z], [x1, y1, z], [x0, y1, z]]);
            }
        }
        assert_eq!(tris.len(), 54_450);
        let large_stl_bytes = encode_binary_stl(&tris);

        let (welded, orig_tris) = parse_and_weld_stl_with_stats(&large_stl_bytes, &extent)
            .expect("large valid STL should automatically simplify and weld");
        assert_eq!(orig_tris, 54_450);
        assert!(welded.triangle_count <= MAX_TRIANGLES);
        assert_eq!(welded.triangle_count, 49_928);

        // Verify NW corner (0, 0) and SE corner (1, 1) UVs are preserved after simplification
        let nw_uv = welded.uvs.first().copied().unwrap();
        let se_uv = welded.uvs.last().copied().unwrap();
        assert!((nw_uv[0] - 0.0).abs() < 1e-6 && (nw_uv[1] - 0.0).abs() < 1e-6);
        assert!((se_uv[0] - 1.0).abs() < 1e-6 && (se_uv[1] - 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_mesh_format_autodetection_and_override() {
        assert_eq!(
            MeshFormat::from_path("sites/jezero/terrain.stl"),
            Some(MeshFormat::Stl)
        );
        assert_eq!(
            MeshFormat::from_path("sites/shackleton/dem.tif"),
            Some(MeshFormat::Tiff)
        );
        assert_eq!(
            MeshFormat::from_path("sites/shackleton/dem.TIFF?v=1"),
            Some(MeshFormat::Tiff)
        );

        let extent = DemExtent::new(0.0, 100.0, 0.0, 100.0);
        let mut meta = SiteMetadata {
            id: "s1".to_string(),
            name: "S1".to_string(),
            body: Body::Moon,
            description: String::new(),
            center_lat_lon: None,
            dem_extent: extent,
            elevation_range_m: None,
            triangle_count: 10,
            mesh_path: "sites/s1/elevation.tif".to_string(),
            mesh_format: None,
            texture_path: "sites/s1/tex.png".to_string(),
            feature_scores: BTreeMap::new(),
            canonical_score: 0.5,
            generated_by: None,
        };
        assert_eq!(meta.resolved_mesh_format(), MeshFormat::Tiff);

        meta.mesh_format = Some(MeshFormat::Stl);
        assert_eq!(meta.resolved_mesh_format(), MeshFormat::Stl);
    }

    #[test]
    fn test_parse_dem_tiff_f32_i16_u16_and_planar_uvs() {
        let extent = DemExtent::new(-500.0, 500.0, -1000.0, 1000.0);
        // 3x3 raster -> 2x2 cells -> 8 triangles, 9 vertices
        let f32_pixels = [
            10.0_f32, 10.0, 10.0, // North row (row=0 -> y_stl = max_y = 1000)
            10.0, 20.0, 10.0, // Center row (row=1 -> y_stl = 0)
            10.0, 10.0, 10.0, // South row (row=2 -> y_stl = min_y = -1000)
        ];
        let tiff_bytes = encode_uncompressed_f32_tiff(3, 3, &f32_pixels);
        let mesh = parse_dem_tiff(&tiff_bytes, &extent, 2.0).expect("f32 DEM TIFF should parse");

        assert_eq!(mesh.triangle_count, 8);
        assert_eq!(mesh.positions.len(), 9);
        assert_eq!(mesh.uvs.len(), 9);
        assert_eq!(mesh.indices.len(), 24);

        // Vertex 0 is North-West (x_stl = -500, y_stl = 1000, z_stl = 10 * 2 = 20)
        // Bevy Y-up: [-500.0, 20.0, -1000.0], UV: [0.0, 0.0]
        assert_eq!(mesh.positions[0], [-500.0, 20.0, -1000.0]);
        assert!((mesh.uvs[0][0] - 0.0).abs() < 1e-6 && (mesh.uvs[0][1] - 0.0).abs() < 1e-6);

        // Vertex 4 is Center (x_stl = 0, y_stl = 0, z_stl = 20 * 2 = 40)
        // Bevy Y-up: [0.0, 40.0, 0.0], UV: [0.5, 0.5]
        assert_eq!(mesh.positions[4], [0.0, 40.0, 0.0]);
        assert!((mesh.uvs[4][0] - 0.5).abs() < 1e-6 && (mesh.uvs[4][1] - 0.5).abs() < 1e-6);

        // Vertex 8 is South-East (x_stl = 500, y_stl = -1000, z_stl = 10 * 2 = 20)
        // Bevy Y-up: [500.0, 20.0, 1000.0], UV: [1.0, 1.0]
        assert_eq!(mesh.positions[8], [500.0, 20.0, 1000.0]);
        assert!((mesh.uvs[8][0] - 1.0).abs() < 1e-6 && (mesh.uvs[8][1] - 1.0).abs() < 1e-6);

        // Also verify i16 and u16 TIFF elevation bands
        let i16_bytes = encode_uncompressed_i16_tiff(2, 2, &[-50_i16, 0, 25, 100]);
        let i16_mesh = parse_dem_tiff(&i16_bytes, &extent, 1.0).expect("i16 DEM TIFF should parse");
        assert_eq!(i16_mesh.triangle_count, 2);
        assert_eq!(i16_mesh.positions[0][1], -50.0);
        assert_eq!(i16_mesh.positions[3][1], 100.0);

        let u16_bytes = encode_uncompressed_u16_tiff(2, 2, &[100_u16, 200, 300, 400]);
        let u16_mesh = parse_dem_tiff(&u16_bytes, &extent, 1.5).expect("u16 DEM TIFF should parse");
        assert_eq!(u16_mesh.triangle_count, 2);
        assert_eq!(u16_mesh.positions[0][1], 150.0);
        assert_eq!(u16_mesh.positions[3][1], 600.0);
    }

    #[test]
    fn test_parse_dem_tiff_decimation_respects_50k_triangle_budget() {
        let extent = DemExtent::new(0.0, 2000.0, 0.0, 2000.0);
        // 159x159 pixels = 158x158 cells = 49,928 triangles (step = 1)
        assert_eq!(compute_decimation_step(159, 159), 1);
        // 160x160 pixels = 159x159 cells = 50,562 triangles > 50,000 -> requires step = 2
        assert_eq!(compute_decimation_step(160, 160), 2);

        // Create an in-memory 317x317 DEM TIFF (raw = 316*316*2 = 199,712 triangles > 50,000).
        // With step = 2, cells_x = 158, cells_y = 158 -> 49,928 triangles <= 50,000.
        let width = 317_u32;
        let height = 317_u32;
        let samples = vec![42.0_f32; (width * height) as usize];
        let tiff_bytes = encode_uncompressed_f32_tiff(width, height, &samples);

        let mesh = parse_dem_tiff(&tiff_bytes, &extent, 1.0)
            .expect("high-res DEM TIFF should decimate and parse");
        assert!(mesh.triangle_count <= MAX_TRIANGLES);
        assert_eq!(mesh.triangle_count, 49_928);

        // Verify NW and SE corners still align with (0, 0) and (1, 1) after decimation
        let nw_uv = mesh.uvs.first().copied().unwrap();
        let se_uv = mesh.uvs.last().copied().unwrap();
        assert!((nw_uv[0] - 0.0).abs() < 1e-6 && (nw_uv[1] - 0.0).abs() < 1e-6);
        assert!((se_uv[0] - 1.0).abs() < 1e-6 && (se_uv[1] - 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_dynamic_weighted_scoring() {
        let mut s1_features = BTreeMap::new();
        s1_features.insert("slope_safety".to_string(), 0.9);
        s1_features.insert("roughness_similarity".to_string(), 0.5);

        let mut s2_features = BTreeMap::new();
        s2_features.insert("slope_safety".to_string(), 0.4);
        s2_features.insert("roughness_similarity".to_string(), 0.95);

        let mut weights = BTreeMap::new();
        weights.insert("slope_safety".to_string(), 3.0);
        weights.insert("roughness_similarity".to_string(), 1.0);

        // Normalized weights: slope_safety = 0.75, roughness_similarity = 0.25
        // s1 = 0.9 * 0.75 + 0.5 * 0.25 = 0.675 + 0.125 = 0.80
        let score1 = compute_weighted_score(&s1_features, &weights);
        assert!((score1 - 0.80).abs() < 1e-9);

        let extent = DemExtent::new(0.0, 100.0, 0.0, 100.0);
        let catalog = Catalog {
            version: "1.0.0".to_string(),
            default_weights: weights.clone(),
            sites: vec![
                SiteMetadata {
                    id: "site_b".to_string(),
                    name: "Site B".to_string(),
                    body: Body::Earth,
                    description: String::new(),
                    center_lat_lon: None,
                    dem_extent: extent,
                    elevation_range_m: None,
                    triangle_count: 100,
                    mesh_path: "sites/site_b/terrain.stl".to_string(),
                    mesh_format: None,
                    texture_path: "sites/site_b/texture.png".to_string(),
                    feature_scores: s2_features,
                    canonical_score: 0.5,
                    generated_by: None,
                },
                SiteMetadata {
                    id: "site_a".to_string(),
                    name: "Site A".to_string(),
                    body: Body::Moon,
                    description: String::new(),
                    center_lat_lon: None,
                    dem_extent: extent,
                    elevation_range_m: None,
                    triangle_count: 100,
                    mesh_path: "sites/site_a/terrain.stl".to_string(),
                    mesh_format: None,
                    texture_path: "sites/site_a/texture.png".to_string(),
                    feature_scores: s1_features,
                    canonical_score: 0.8,
                    generated_by: None,
                },
            ],
            generated_by: None,
        };

        let response = recalculate_catalog(&catalog, &weights);
        assert_eq!(response.results.len(), 2);
        assert_eq!(response.results[0].site_id, "site_a");
        assert!((response.results[0].score - 0.80).abs() < 1e-9);
    }

    #[test]
    fn test_terrain_analysis_morphometry_persistent_homology_and_euler_on_test_site() {
        use crate::terrain_analysis::{
            UploadSiteRequest, analyze_and_build_site_bundle, decode_multipart_upload_request,
            encode_multipart_upload_request, subsample_grid_max_dim,
        };

        let stl_bytes = include_bytes!("../../../data/processed/test_site/terrain.stl");
        let extent = DemExtent::new(-1000.0, 1000.0, -1000.0, 1000.0);

        let mut default_weights = BTreeMap::new();
        default_weights.insert("slope_safety".to_string(), 0.35);
        default_weights.insert("roughness_similarity".to_string(), 0.25);
        default_weights.insert("elevation_relief".to_string(), 0.20);
        default_weights.insert("geomorphology_match".to_string(), 0.20);

        let mut ref_features = BTreeMap::new();
        ref_features.insert("slope_safety".to_string(), 0.85);
        ref_features.insert("roughness_similarity".to_string(), 0.90);
        ref_features.insert("elevation_relief".to_string(), 0.75);
        ref_features.insert("geomorphology_match".to_string(), 0.88);

        let catalog = Catalog {
            version: "1.0.0".to_string(),
            default_weights,
            sites: vec![SiteMetadata {
                id: "test_site".to_string(),
                name: "Synthetic Crater & Ridge UV Verification Site".to_string(),
                body: Body::Moon,
                description: "Reference lunar crater site".to_string(),
                center_lat_lon: Some([-89.5, 0.0]),
                dem_extent: extent,
                elevation_range_m: Some([-45.0, 120.0]),
                triangle_count: 2048,
                mesh_path: "test_site/terrain.stl".to_string(),
                mesh_format: Some(MeshFormat::Stl),
                texture_path: "test_site/checkerboard.png".to_string(),
                feature_scores: ref_features,
                canonical_score: 0.8485,
                generated_by: None,
            }],
            generated_by: None,
        };

        // Verify binary multipart/form-data round-trip without Base64
        let raw_req = UploadSiteRequest {
            site_id: "test_site_tda".to_string(),
            site_name: "Synthetic Crater TDA Test".to_string(),
            body: None,
            description: "Topological & morphometric verification".to_string(),
            dem_extent: Some(extent),
            stl_base64: None,
            tiff_base64: None,
            png_base64: None,
            stl_bytes: Some(stl_bytes.to_vec()),
            tiff_bytes: None,
            png_bytes: None,
        };

        let (content_type, multipart_body) = encode_multipart_upload_request(&raw_req);
        let decoded_req = decode_multipart_upload_request(&content_type, &multipart_body)
            .expect("multipart/form-data payload should decode cleanly");
        assert_eq!(decoded_req.site_id, "test_site_tda");
        assert_eq!(decoded_req.stl_bytes.as_deref(), Some(stl_bytes.as_slice()));

        let bundle = analyze_and_build_site_bundle(&decoded_req, &catalog)
            .expect("analyze_and_build_site_bundle should succeed on test_site STL");

        // Verify triangle budget & STL/TIFF/PNG generation
        assert!(bundle.report.generated_site_metadata.triangle_count <= MAX_TRIANGLES);
        assert_eq!(bundle.report.generated_site_metadata.triangle_count, 2048);
        assert_eq!(bundle.report.original_triangle_count, 2048);
        assert_eq!(bundle.report.simplified_triangle_count, 2048);
        assert!(!bundle.stl_bytes.is_empty());
        assert!(!bundle.tiff_bytes.is_empty());
        assert!(bundle.png_bytes.starts_with(&[137, 80, 78, 71, 13, 10, 26, 10]));

        // Verify dense grid subsampling helper caps to <= 1024x1024
        let dense = vec![10.0_f32; 1500 * 1200];
        let (sw, sh, sgrid) = subsample_grid_max_dim(1500, 1200, &dense, 1024);
        assert!(sw <= 1024 && sh <= 1024);
        assert_eq!(sgrid.len(), sw * sh);

        // Verify morphometry metrics (slope, TRI, hypsometric integral, tortuosity)
        let morph = &bundle.report.morphometry;
        assert!(morph.elevation_range_m > 100.0);
        assert!(morph.slope_mean_deg > 1.0 && morph.slope_mean_deg < 45.0);
        assert!(morph.slope_p90_deg >= morph.slope_mean_deg);
        assert!(morph.tri_mean_m > 0.0);
        assert!(morph.roughness_rms_m > 0.0);
        assert!(morph.hypsometric_integral > 0.05 && morph.hypsometric_integral < 0.95);
        assert!(morph.surface_to_planar_ratio > 1.0);

        // Verify 0D Persistent Homology (beta_0 sublevel basins & superlevel peaks) and Euler characteristic
        let topo = &bundle.report.topology;
        assert!(
            topo.persistent_basins_b0 >= 1,
            "Crater terrain must have at least 1 persistent sublevel basin"
        );
        assert!(
            topo.persistent_peaks_b0 >= 1,
            "Crater rim/central peak must have at least 1 persistent superlevel peak"
        );
        assert!(topo.max_persistence_m > 5.0);
        // Simply connected 32x32 grid sheet: V = 1089, E = 3136, F = 2048 -> V - E + F = 1
        assert_eq!(topo.euler_characteristic, 1);
        assert_eq!(topo.connected_components, 1);
        assert!(topo.is_manifold_with_boundary);

        // Verify Analog Proposal
        let proposal = &bundle.report.proposal;
        assert!(proposal.analog_score > 0.0 && proposal.analog_score <= 1.0);
        assert_eq!(proposal.closest_site_id, "test_site");
        assert!(!proposal.justification.is_empty());
    }
}
