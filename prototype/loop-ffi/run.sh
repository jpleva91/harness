#!/usr/bin/env bash
# PROTOTYPE. Build both bindings and run both demos.
set -euo pipefail
cd "$(dirname "$0")"
PYO3_PYTHON=python3 cargo build --release
cp target/release/libproto_node.so node/harness.node
cp target/release/lib_native.so py/python/harness_proto/_native.so
node --expose-gc node/demo.mjs
python3 py/demo.py
