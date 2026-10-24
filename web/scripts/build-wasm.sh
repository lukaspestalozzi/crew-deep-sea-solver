#!/bin/sh
# Build the solver as WebAssembly and generate the JS bindings into src/wasm.
set -e
cd "$(dirname "$0")/../.."
cargo build --profile web --target wasm32-unknown-unknown -p crew-wasm
wasm-bindgen --target web --out-dir web/src/wasm target/wasm32-unknown-unknown/web/crew_wasm.wasm
echo "wasm bindings written to web/src/wasm"
