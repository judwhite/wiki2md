#!/bin/bash
set -euo pipefail

# configuration
BIN="wiki2md"

cargo build --release
rm -rf docs/json
cargo flamegraph --bin "$BIN" -- -r

xdg-open flamegraph.svg  # Opens in browser
