#!/usr/bin/env bash
set -e
[ -f .env ] || cp .env.example .env
[ -f dist/index.html ] || trunk build --release
echo "Open http://localhost:8080 in your browser and click Enter as Guest"
cargo run --release -p analog_server
