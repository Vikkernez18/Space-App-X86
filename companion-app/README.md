# JMARS Planetary Settlement Site Viability Workbench (Rust + Java)

A desktop geospatial & 3D mesh analysis application built in **Rust** (core raster/mesh ingestion & mathematical comparison engine) and **Java** (Swing/Java2D interactive desktop workbench) for recommending and validating permanent human settlement zones on **Mars** and the **Moon**.

---

## Quick Start

```bash
cd /home/Raven-62/planetary-settler

# 1. Build the Rust engine, seed placeholder maps (.tif, .tiff, .stl), and compile the Java Desktop App
./build.sh

# 2. Launch the Interactive Desktop GUI
./run.sh

# 3. Or run the automated CLI/Headless verification across all sample .tif, .tiff, and .stl files
./run.sh --verify
```

---

## Swapping in Your Real JMARS Mars `.tif` Render

Once your JMARS Mars `.tif` export finishes rendering:
1. **In the Desktop GUI**: Click **`Upload Map (.tif / .tiff / .stl)...`** in the top toolbar and select your exported `.tif` file from disk.
2. **Or via Command Line**:
   ```bash
   ./run.sh --body mars --upload /path/to/your/jmars_mars_export.tif
   ```
3. **Or to replace the default Mars baseline permanently**:
   Copy your file to `data/baseline/mars_jezero_arcadia_baseline.tif`.
   *(The Rust engine supports both 32-bit float GeoTIFF DEMs and 8-bit/16-bit RGB/grayscale visual `.tif`/`.tiff` renders exported from JMARS, as well as Binary and ASCII `.stl` 3D meshes).*

---

## Mathematical Comparison & Viability Model (`rust-engine/src/math.rs`)

Because settlement suitability criteria evolve during mission design, all parameters are **tunable in real time** via the left-hand sliders in the Java Desktop UI:

1. **Spatial Registration & Fidelity vs. Baseline (`compare_maps`)**:
   - **Pearson Correlation ($r \in [-1, 1]$)** & **SSIM**: Verifies that the uploaded map represents the same planetary region as the baseline JMARS reference map.
   - **Elevation RMSE & MAE (meters)**: Measures vertical deviation $\Delta z = z_{\text{upload}} - z_{\text{baseline}}$.
   - **High-Frequency Detail Gain**: Ratio of Laplacian high-frequency terrain energy between the uploaded map and the baseline.
2. **Physical Terrain Derivatives**:
   - **Slope Angle ($\theta^\circ$)**: Computed via Horn's $3 \times 3$ finite-difference gradient kernel.
   - **Terrain Ruggedness Index ($\text{TRI}$, meters)**: Riley's 8-neighbor root-mean-square elevation delta normalized to a horizontal reference span (detects boulders, fault scarps, and micro-craters).
   - **Solar Illumination & Elevation Sub-Scores**: Evaluates atmospheric column depth on Mars and sunlit ridge horizon visibility on the Lunar South Pole.
3. **Zone Classification**:
   - `CONFIRMED_VIABLE`: High-resolution map confirms the recommended zone meets slope and roughness limits.
   - `DOWNGRADED_HAZARD`: Coarse baseline looked flat, but the higher-resolution upload revealed steep scarps, craters, or boulder fields inside the zone.
   - `UPGRADED_VIABLE`: High-resolution upload resolves a borderline zone into a verified smooth landing pad.
   - `MISMATCH_OR_INVALID`: Uploaded map fails spatial correlation with the baseline sector.
