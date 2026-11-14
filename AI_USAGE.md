# AI Usage Log

- `crates/core/Cargo.toml`, `Cargo.lock`: `serde` dependency added. Tool: Gemini 3.8 Flash (High) via agy
- `crates/core/Cargo.toml`: Added pure-Rust `tiff = "0.9"` dependency for WASM-compatible in-memory DEM TIFF parsing. Tool: Gemini 4 Argon (Medium) via agy
- `crates/core/src/models.rs`: Shared WASM-compatible data structures (`Body`, `MeshFormat`, `DemExtent`, `SiteMetadata` with `mesh_format` and extension auto-detection, `Catalog`, `WeightsConfig`, `RecalculateRequest`, `RecalculateResponse`, `ScoredSite`). Tool: Gemini 4 Argon (Medium) via agy
- `crates/core/src/stl.rs`: Custom binary STL parser (`parse_and_weld_stl`) with 50,000-triangle budget enforcement, spatial vertex welding, Z-up to Bevy Y-up coordinate conversion, and `DemExtent` planar UV mapping. Tool: Gemini 4 Argon (Medium) via agy
- `crates/core/src/tiff_dem.rs`: In-memory DEM TIFF/GeoTIFF parser (`parse_dem_tiff`) reading `u16`, `i16`, and `f32` bands via `std::io::Cursor<&[u8]>`, automatic grid decimation (`<= 50,000` triangles / `<= 158x158` cells), Z-up to Bevy Y-up conversion, and `DemExtent` planar UVs returning `WeldedMesh`. Tool: Gemini 4 Argon (Medium) via agy
- `crates/core/src/scoring.rs`: Dynamic weighted-sum recalculation over pre-normalized `[0.0, 1.0]` feature scores (`compute_weighted_score`, `recalculate_catalog`). Tool: Gemini 4 Argon (Medium) via agy
- `crates/core/src/lib.rs`: Module exports, public re-exports, and unit tests for binary STL parsing, in-memory DEM TIFF parsing (`f32`, `i16`, `u16`), 50k triangle decimation, `MeshFormat` detection, planar UV mapping, and weighted scoring. Tool: Gemini 4 Argon (Medium) via agy
- `pipeline/weights.json`: Default feature weights configuration for canonical catalog scoring. Tool: Gemini 4 Argon (Medium) via agy
- `pipeline/build_catalog.py`: Canonical score computation script reading `pipeline/weights.json` and site `metadata.json` files to generate `data/processed/catalog.json`. Tool: Gemini 4 Argon (Medium) via agy
- `pipeline/validate_uv_test_site.py`: Synthetic Z-up binary STL (`terrain.stl`) and 8x8 PNG checkerboard (`checkerboard.png`) generator and planar UV alignment validator for `data/processed/test_site/` and JMARS exports. Tool: Gemini 4 Argon (Medium) via agy
- `data/processed/test_site/metadata.json`, `data/processed/catalog.json`: Metadata and catalog entry for the synthetic UV verification site (`test_site`). Tool: Gemini 4 Argon (Medium) via agy
