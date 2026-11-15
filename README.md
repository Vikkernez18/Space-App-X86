# Space-App-X86 — Lunar & Martian Terrestrial Analogs 3D Platform

<!-- AI-assisted -->

**Space-App-X86** is an interactive 3D planetary terrain exploration, morphometric analysis, and **Topological Data Analysis (TDA)** platform built in Rust (`Bevy 0.14` + `bevy_egui 0.28` + `Axum 0.8`). It enables researchers and mission planners to evaluate and compare **Earth terrestrial analogs** against target landing and habitat zones on the **Moon** (Artemis Lunar South Pole) and **Mars**.

---

## Key Capabilities

1. **Startup Access Gate & Dual Operating Modes**:
   - **Guest / Judge Public Demo Mode**: Passwordless entry to inspect 3D planetary terrains, toggle the 8x8 NW/SE planar UV verification checkerboard, compare sites in synchronized Split-Screen 3D, view real-time Planetary Telemetry HUD cards, and simulate custom morphometric weightings.
   - **Authenticated Researcher Mode (Argon2id)**: Full-featured workspace supporting raw binary `multipart/form-data` terrain uploads up to **512 MiB** (`.stl`, `.tif`/`.tiff`, `.png` with zero Base64 overhead), automated mesh simplification (`<= 50,000` triangles), live **0D Persistent Homology ($\beta_0$)** and **Euler Characteristic ($\chi$)** computation, canonical weight persistence, and one-click Workspace Catalog Reset (`POST /api/sites/reset`).
2. **Automatic Planetocentric Centering & 50,000-Triangle Mesh Budget**:
   - Binary `.stl` meshes exported from JMARS or Blender in displaced planetocentric/projected coordinates are automatically centered to `(0.0, 0.0, 0.0)` in `f64` precision before converting from Z-up (`+X` East, `+Y` North, `+Z` Elevation) to Bevy's right-handed Y-up space (`X_bevy = X_stl`, `Y_bevy = Z_stl`, `Z_bevy = -Y_stl`).
   - Meshes exceeding `50,000` triangles are deterministically simplified via 3D spatial vertex clustering while preserving boundary edges and elevation extrema.
3. **Planar `DemExtent` UV Mapping & Checkerboard Alignment Test**:
   - Planar UVs are computed strictly from the horizontal coordinates against `DemExtent` (`min_x`, `max_x`, `min_y`, `max_y`)—never from the mesh AABB:
     $$u = \frac{X_{\text{stl}} - \min_x}{\max_x - \min_x}, \quad v = \frac{\max_y - Y_{\text{stl}}}{\max_y - \min_y}$$
4. **Live Planetary Telemetry HUD (`MOON`, `MARS`, `EARTH`)**:
   - Real-time astronomical clock and environmental station computing Lunar Local Solar Time (LST) & synodic illumination %, Mars Sol Date (MSD), Coordinated Mars Time (MTC) & Local Mean Solar Time (LMST), and surface thermal/barometric/gravity telemetry.
5. **Mathematical Foundation & Lunar/Martian Settlement Engineering Guide**:
   - Built-in interactive multi-bar similarity charts (`egui::Painter`), collapsible mathematical formulas for morphometry and 0D Persistent Homology, and an engineering reference mapping the 5 core habitat requirements (slope stability, polar solar/volatile access, rover trafficability, radiation shielding via $\beta_0$ basins, and terrestrial analog validation) to platform modules.

---

## Repository Structure

```text
Space-App-MiniPC/
├── crates/
│   ├── core/       # WASM-compatible data models, binary STL parser/simplifier, TIFF DEM loader,
│   │               # weighted scoring engine, and 0D Persistent Homology + Euler characteristic TDA
│   ├── server/     # Axum HTTP server, Argon2id auth, rate limiting, 512 MiB multipart upload,
│   │               # catalog persistence, and POST /api/sites/reset
│   └── viewer/     # Bevy + bevy_egui 3D WebAssembly & native desktop application
├── pipeline/       # Python/GDAL/Blender offline planetary GIS processing & canonical scoring scripts
├── data/processed/ # Processed catalog.json, 3D binary .stl meshes, .tif DEMs, and .png textures
├── docs/           # Presentation deck & compliance documentation
├── AGENTS.md       # Multi-agent engineering rules, coordinate/UV laws, and ownership boundaries
├── DATA_SOURCES.md # Planetary dataset citations (LOLA, MOLA, SRTM/ALOS)
└── AI_USAGE.md     # AI assistance compliance log
```

---

## Prerequisites

1. **Rust Toolchain** (stable `1.80+` with `rustup`):
   ```bash
   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
   rustup target add wasm32-unknown-unknown
   ```
2. **Trunk** (for building the WebAssembly frontend):
   ```bash
   cargo install --locked trunk
   ```
3. **Linux System Dependencies** (only required when compiling the native desktop binary `analog_viewer` on Ubuntu/Debian):
   ```bash
   sudo apt-get update && sudo apt-get install -y \
     pkg-config libx11-dev libasound2-dev libudev-dev libxkbcommon-x11-0 \
     libwayland-dev libxrandr-dev libxi-dev libgl1-mesa-dev
   ```

---

## Quick Start — Full-Mode Web Application (Axum Backend + WASM 3D Viewer)

Full Mode runs the `analog_server` backend (serving the REST API, Argon2id authentication, 512 MiB binary terrain upload & TDA engine, and `data/processed/*` assets) junto with the compiled WebAssembly 3D viewer in `dist/`.

### 1. Clone the Repository
```bash
git clone <REPOSITORY_URL> Space-App-MiniPC
cd Space-App-MiniPC
```

### 2. Configure Environment Variables (`.env`)
Copy `.env.example` to `.env` and set your session secret and researcher Argon2id password hash:
```bash
cp .env.example .env
```
Key variables in `.env`:
- `BIND_ADDR=0.0.0.0:8080`
- `DATA_DIR=data/processed`
- `STATIC_DIR=dist`
- `RESEARCHER_USERNAME=researcher`
- `RESEARCHER_PASSWORD_HASH=$argon2id$...` (Argon2id PHC string; never store plain-text passwords)
- `SESSION_SECRET=<at-least-32-character-random-secret>`

### 3. Build the WebAssembly 3D Viewer with Trunk
```bash
trunk build --release
```
This compiles `crates/viewer` for `wasm32-unknown-unknown` (using the `wasm-release` profile) and writes the static bundle into `dist/`.

### 4. Start the Full-Mode Axum Server
```bash
cargo run --release -p analog_server
```
Open **`http://127.0.0.1:8080`** in your browser:
- Click **`Enter as Guest (Public Demo Mode)`** on the **Space-App-X86 Access Gate** for passwordless exploration, or
- Enter your **`Username`** and **`Password`** and click **`Log in as Researcher (Argon2id)`** to upload `.stl` / `.tif` / `.png` files, run TDA, persist weights, or reset the workspace catalog.

---

## Running the Native Desktop 3D Application (`analog_viewer`)

You can also run the 3D viewer natively as a desktop application using hardware Vulkan/OpenGL/Metal acceleration:

1. Start `analog_server` in one terminal (optional if using Public Demo fallback, required for Researcher login/upload):
   ```bash
   cargo run --release -p analog_server
   ```
2. In a second terminal, launch the native viewer:
   ```bash
   SERVER_URL=http://127.0.0.1:8080 cargo run --release -p analog_viewer
   ```

---

## Static GitHub Pages Deployment (Zero Backend Demo Mode)

To build a standalone static bundle for GitHub Pages (`https://<org>.github.io/Space-App-X86/`) that loads `data/processed/*` directly without a backend server:

```bash
trunk build --profile wasm-release --public-url /Space-App-X86/
```

---

## Controls & Keyboard Shortcuts

- **Mouse / Touchpad**:
  - **Left-Click + Drag**: Orbit 3D camera around terrain center `(0, 0, 0)`
  - **Right-Click + Drag** (or **Shift + Left-Click + Drag**): Pan camera focus
  - **Scroll Wheel / Two-Finger Touchpad Scroll**: Smooth zoom in/out
- **Keyboard Shortcuts**:
  - **`H`**: Show / Hide the left control panel (or restore UI from Full 3D mode)
  - **`A`**: Toggle smooth orbital **Auto-Rotate** (automatically pauses while dragging)
  - **`C`**: Toggle synchronized **Split-Screen 3D Comparison** with the best cross-body analog
  - **`R`**: Reset & center the 3D camera on `(0, 0, 0)` with dynamic `near`/`far` clip planes
  - **`ArrowUp` / `ArrowDown`**: Switch to the previous / next terrain site in the catalog
  - **`+` / `-`**: Zoom in / out without a mouse wheel

---

## Running Verification Tests

Run the full workspace unit and integration test suite (`analog_core`, `analog_server`, and `analog_viewer`):

```bash
cargo test --workspace
```

---

## Compliance & Data Attribution

- All planetary datasets (Lunar LRO LOLA, Mars MGS MOLA, and Terrestrial SRTM/ALOS analogs) are credited in [DATA_SOURCES.md](DATA_SOURCES.md).
- Zero NASA logos or insignia are used anywhere in the application or documentation.
- All AI-assisted code and documentation changes are logged in [AI_USAGE.md](AI_USAGE.md).
