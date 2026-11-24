#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

if ! rustup target list --installed | grep -q '^wasm32-wasip2$'; then
  echo "Installing wasm32-wasip2 target..."
  rustup target add wasm32-wasip2
fi

echo "Building openidea.dart and openidea.flutter for wasm32-wasip2..."
cargo build --release --target wasm32-wasip2 -p openidea-plugin-dart -p openidea-plugin-flutter

mkdir -p plugins/dart/dist plugins/flutter/dist
cp target/wasm32-wasip2/release/openidea_plugin_dart.wasm plugins/dart/dist/plugin.wasm
cp target/wasm32-wasip2/release/openidea_plugin_flutter.wasm plugins/flutter/dist/plugin.wasm

echo "Built:"
ls -lh plugins/dart/dist/plugin.wasm plugins/flutter/dist/plugin.wasm

if command -v openidea-plugin >/dev/null 2>&1; then
  echo "Validating plugin packages with openidea-plugin check..."
  openidea-plugin check plugins/dart
  openidea-plugin check plugins/flutter
else
  echo "Note: 'openidea-plugin' CLI not found in PATH; skipping 'openidea-plugin check'."
fi
