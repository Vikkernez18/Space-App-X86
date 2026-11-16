#!/usr/bin/env bash
set -euo pipefail

PROJECT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
export PATH="$HOME/.cargo/bin:$PATH"

JAVA_BIN="${JAVA_BIN:-/home/Raven-62/JMARS_5_public/jre/bin/java}"
if ! [ -x "$JAVA_BIN" ]; then
    JAVA_BIN="$(command -v java)"
fi

if ! [ -f "$PROJECT_DIR/java-app/build/planetary/Main.class" ] || ! [ -x "$PROJECT_DIR/rust-engine/target/release/planetary-engine" ]; then
    "$PROJECT_DIR/build.sh"
fi

cd "$PROJECT_DIR"
exec "$JAVA_BIN" -cp "$PROJECT_DIR/java-app/build" planetary.Main "$@"
