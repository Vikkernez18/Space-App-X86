# AGENTS.md — Multi-Agent Concurrency & Engineering Rules

## 1. Directory Ownership Boundaries
Each developer or coding agent MUST only edit files inside its assigned boundary:

- **Agent CORE (`crates/core/**`)**: Shared data models (`Catalog`, `SiteMetadata`, `DemExtent`), custom binary STL parser with vertex welding, and researcher weighted-score recalculation.
- **Agent PIPELINE (`pipeline/**`, `data/processed/**`)**: Real NASA data scripts (`gdalwarp`, `gdaldem`, headless Blender), `pipeline/weights.json`, and canonical score computation in `pipeline/build_catalog.py`.
- **Agent SERVER (`crates/server/**`, `.env.example`)**: Full-mode Axum server, Argon2 auth, session cookies, rate limiting, and authenticated weight-recalculation endpoint.
- **Agent VIEWER (`crates/viewer/**`, `Trunk.toml`)**: Bevy 3D viewer (native + static GitHub Pages WASM demo), Z-up to Y-up mesh loader, checkerboard UV test toggle, split-screen comparison, and `bevy_egui` UI.
- **Agent DOCS (`README.md`, `DATA_SOURCES.md`, `AI_USAGE.md`, `docs/**`)**: Compliance logs, dataset citations, and the 30-second video / 7-slide presentation deck.

## 2. Coordinate System, Mesh & UV Laws (MANDATORY)
1. **Axis Convention**:
   - Blender exports binary STL meshes in **Z-up** (`+X` = East, `+Y` = North, `+Z` = Elevation in meters).
   - On load, `analog_core::stl::parse_and_weld_stl` converts vertices to Bevy's right-handed **Y-up** space:
     `X_bevy = X_stl`, `Y_bevy = Z_stl` (Up), `Z_bevy = -Y_stl` (South, so North is `-Z`).
2. **Planar UV Mapping**:
   - Compute UVs **only** from the horizontal STL coordinates (`X_stl`, `Y_stl`) against the `DemExtent` (`min_x`, `max_x`, `min_y`, `max_y`) stored in `metadata.json`—NEVER from the mesh AABB:
     - `u = (X_stl - min_x) / (max_x - min_x)`
     - `v = (max_y - Y_stl) / (max_y - min_y)` (North `max_y` maps to top row `v = 0.0`).
3. **Vertical Exaggeration**:
   - Scales **only** Bevy's up axis (`Transform.scale = Vec3::new(1.0, exaggeration, 1.0)`).
4. **Checkerboard Alignment Verification**:
   - Every terrain mesh must pass the procedural checkerboard texture alignment test (North-West corner at `u=0, v=0`, South-East at `u=1, v=1`).
5. **Mesh Budget**:
   - Maximum **50,000 triangles** per site (~2.5 MB binary STL). Always use `u32` vertex indices (`Indices::U32`).
   - Do not use external STL parser crates; use `analog_core::stl`.

## 3. Scoring Single Source of Truth & Planetary GIS Rules
1. **Scoring**: `pipeline/build_catalog.py` reads `pipeline/weights.json` and writes canonical scores into `data/processed/catalog.json`. Rust never runs raw raster math; `analog_core::scoring` only recalculates weighted sums over pre-normalized `[0.0, 1.0]` feature scores for logged-in researcher mode.
2. **Planetary Projections**:
   - Warp every ROI GeoTIFF to a local meter-based projection (`+units=m`) with explicit planetary radii (Moon `R=1737400`, Mars `R=3396190`, Earth `WGS84`) before running `gdaldem`, or pass explicit `-s` horizontal-to-vertical scale factors if operating on degree rasters.

## 4. Deployment & Compliance
1. **Public Demo Mode**: Static Trunk/WASM build (`--profile wasm-release`) deployed to GitHub Pages requires zero backend and loads `data/processed/*` statically.
2. **Full Mode**: Axum backend (`crates/server`) adds Argon2 login, session cookies, rate limiting, and researcher weight customization.
3. **Compliance**:
   - Add `// AI-assisted` (or `# AI-assisted`) to the top of all AI-generated files and log them in `AI_USAGE.md`.
   - Credit all datasets in `DATA_SOURCES.md`. Zero NASA logos/insignia anywhere. English only.
   - Official demo deliverable is at most a **30-second video OR 7 slides**.

## 5. Engineering Rules (added)
1. Do not guess crate APIs or versions: use `cargo add` and `cargo check`
   before writing implementation code. Pin compatible Bevy and bevy_egui
   versions.
2. `crates/core` must stay WASM-compatible: no tokio, no gdal, no
   filesystem calls.
3. Never commit `.env`, plain-text passwords or hashes. Read secrets only
   through environment variables.
4. Do NOT hardcode scientific target sites or scoring formulas. They come
   from the human team's decisions via config/catalog files.
5. JSON outputs include a `"generated_by"` field when AI-assisted.
6. Each person works on their own branch and merges to `main` through a pull
   request. Small commits that pass `cargo check --workspace`, prefixed
   `feat(core):`, `feat(server):`, `feat(viewer):`, `feat(pipeline):`,
   `docs:`.
7. Changes to root files (Cargo.toml, .gitignore) need the team's agreement.
8. Trunk deployment to GitHub Pages uses `--public-url /Space-App-X86/`.
