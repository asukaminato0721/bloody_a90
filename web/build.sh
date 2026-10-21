#!/bin/sh
# Build the static site into web/dist. Needs the wasm32-unknown-unknown
# target and the wasm-bindgen CLI of the version pinned in web/Cargo.toml.
set -eu
cd "$(dirname "$0")/.."
cargo build -p bloody_a90_web --target wasm32-unknown-unknown --release
rm -rf web/dist
mkdir -p web/dist
cp -r web/www/. web/dist/
wasm-bindgen --target web --no-typescript --out-dir web/dist/pkg \
    target/wasm32-unknown-unknown/release/bloody_a90_web.wasm
if command -v wasm-opt >/dev/null; then
    wasm-opt -Oz --enable-bulk-memory --enable-nontrapping-float-to-int \
        -o web/dist/pkg/bloody_a90_web_bg.wasm web/dist/pkg/bloody_a90_web_bg.wasm
fi
