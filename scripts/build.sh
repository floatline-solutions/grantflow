#!/usr/bin/env bash
# Build and verify everything locally: contract tests, wasm, embedded specs,
# app tests. No network needed beyond npm install.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

echo "== cargo test (both contracts, Soroban host)"
cargo test

echo "== stellar contract build (wasm32v1-none)"
stellar contract build
ls -la target/wasm32v1-none/release/*.wasm

echo "== embed contract specs into the app"
"$ROOT/scripts/gen-specs.sh"

echo "== app: install, compile, test"
cd "$ROOT/app"
if [ ! -d node_modules ]; then npm install --no-audit --no-fund; fi
npm test
