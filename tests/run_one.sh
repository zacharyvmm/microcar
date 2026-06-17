#!/bin/bash
# run_one.sh — run a single microcar scenario through the assertion engine.
#
# Usage: ./tests/run_one.sh <scenario.toml>
#
# Runs check_assertions.py which:
#   1. Generates a trace via check_trace.py (Python ECU simulation)
#   2. Validates [[expect.event]] and [[assert]] stanzas
#   3. Checks safety rules S1-S8
#   4. Reports PASS/FAIL
#
# For golden trace comparison, pass the trace file as second arg:
#   ./tests/run_one.sh scenarios/bms_overtemp_limp_mode.toml expected/traces/bms_overtemp_limp_mode.trace
#
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"

cd "$PROJECT_DIR"

SCENARIO="$1"
if [ ! -f "$SCENARIO" ]; then
    echo "error: scenario file not found: $SCENARIO"
    exit 1
fi

name=$(basename "$SCENARIO" .toml)
echo -n "  $name ... "

if [ $# -ge 2 ]; then
    # Compare against a golden trace file.
    if python3 tests/check_assertions.py "$SCENARIO" "$2" >/dev/null 2>/dev/null; then
        echo "PASS"
    else
        echo "FAIL"
        python3 tests/check_assertions.py "$SCENARIO" "$2" 2>/dev/null || true
        exit 1
    fi
else
    # Generate trace and validate.
    if python3 tests/check_assertions.py "$SCENARIO" >/dev/null 2>/dev/null; then
        echo "PASS"
    else
        echo "FAIL"
        python3 tests/check_assertions.py "$SCENARIO" 2>/dev/null || true
        exit 1
    fi
fi
