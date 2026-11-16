#!/usr/bin/env bash
set -euo pipefail

PROJECT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
export PATH="$HOME/.cargo/bin:$PATH"

JAVA_BIN="${JAVA_BIN:-/home/Raven-62/JMARS_5_public/jre/bin/java}"
if ! [ -x "$JAVA_BIN" ]; then
    JAVA_BIN="$(command -v java)"
fi
ECJ_JAR="$PROJECT_DIR/tools/ecj.jar"

echo "[1/3] Building Rust Geospatial, 3D Mesh & Mathematical Viability Engine..."
cd "$PROJECT_DIR/rust-engine"
cargo build --release

echo "[2/3] Seeding JMARS Mars & Moon Placeholder Maps (.tif, .tiff, .stl)..."
"$PROJECT_DIR/rust-engine/target/release/planetary-engine" seed "$PROJECT_DIR/data"

echo "[3/3] Compiling Java Desktop Application (Java 17)..."
mkdir -p "$PROJECT_DIR/java-app/build"
"$JAVA_BIN" -jar "$ECJ_JAR" -17 \
    -d "$PROJECT_DIR/java-app/build" \
    -sourcepath "$PROJECT_DIR/java-app/src" \
    "$PROJECT_DIR/java-app/src/planetary/model/ViabilityModels.java" \
    "$PROJECT_DIR/java-app/src/planetary/bridge/RustEngineBridge.java" \
    "$PROJECT_DIR/java-app/src/planetary/ui/MapCanvasPanel.java" \
    "$PROJECT_DIR/java-app/src/planetary/ui/Mesh3DPreviewPanel.java" \
    "$PROJECT_DIR/java-app/src/planetary/ui/CraterComparisonPanel.java" \
    "$PROJECT_DIR/java-app/src/planetary/ui/PlanetaryWorkbenchFrame.java" \
    "$PROJECT_DIR/java-app/src/planetary/Main.java"

echo "Build complete! Run './run.sh' to launch the Desktop GUI or './run.sh --verify' to run verification."
