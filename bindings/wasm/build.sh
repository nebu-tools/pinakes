#!/bin/sh
# Build the WebAssembly module into pkg/. Needs the wasm32-unknown-unknown target and
# wasm-bindgen-cli at the version Cargo.toml pins (0.2.130).
set -eu
cd "$(dirname "$0")"
cargo build -p pinakes-wasm --target wasm32-unknown-unknown --profile wasm
target_dir=$(cargo metadata --format-version 1 --no-deps | sed -n 's/.*"target_directory":"\([^"]*\)".*/\1/p')
wasm-bindgen --target web --out-dir pkg --no-typescript --remove-name-section --remove-producers-section \
  "$target_dir/wasm32-unknown-unknown/wasm/pinakes_wasm.wasm"
if command -v wasm-opt >/dev/null 2>&1; then
  wasm-opt -Oz --enable-bulk-memory --enable-nontrapping-float-to-int \
    -o pkg/pinakes_wasm_bg.wasm pkg/pinakes_wasm_bg.wasm
fi
ls -l pkg/pinakes_wasm_bg.wasm
