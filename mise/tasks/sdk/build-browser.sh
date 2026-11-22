#!/usr/bin/env bash
#MISE description="Build the JavaScript SDK's browser Wasm module"
set -euo pipefail

rustup target add wasm32-unknown-unknown
if ! command -v wasm-bindgen >/dev/null || [[ "$(wasm-bindgen --version)" != "wasm-bindgen 0.2.130" ]]; then
  cargo install wasm-bindgen-cli --version 0.2.130 --locked
fi
cargo build --locked --release -p struo-cache-core --features browser --target wasm32-unknown-unknown
wasm-bindgen --target web --out-dir packages/js/lib/wasm \
  "${CARGO_TARGET_DIR:-target}/wasm32-unknown-unknown/release/struo_cache_core.wasm"
printf '{"type":"module"}\n' > packages/js/lib/wasm/package.json
