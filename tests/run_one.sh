#!/bin/bash
# run_one.sh — run a single microcar scenario
set -euo pipefail
SCENARIO="$1"
cd "$(dirname "$0")/.."

if [ ! -f "$SCENARIO" ]; then
    echo "error: scenario file not found: $SCENARIO"
    exit 1
fi

name=$(basename "$SCENARIO" .toml)

# Try costar runner first (Rust-based, fast).
COSTAR_BIN="../costar/target/debug/costar"
if [ -x "$COSTAR_BIN" ] && [ "$name" != "long_drive_10min" ]; then
    # costar runner with golden trace comparison.
    if "$COSTAR_BIN" test "$SCENARIO" --verbose 2>/dev/null; then
        exit 0
    fi
    # Fall through to Python runner if costar fails.
fi

# Python-based simulation (always works, good for CI).
python3 tests/check_trace.py "$SCENARIO"
