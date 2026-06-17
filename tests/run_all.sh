#!/bin/bash
# run_all.sh — run all microcar scenario tests.
#
# Usage: ./tests/run_all.sh
#
# Iterates over all scenarios/*.toml files, runs each through
# check_assertions.py, and reports pass/fail/skip counts.
#
# CI-ready: exit code 0 if all pass, 1 if any fail.
#
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"

cd "$PROJECT_DIR"

PASS=0
FAIL=0
SKIP=0
TOTAL=0

echo "=== microcar scenario tests ==="
echo ""

for scenario in scenarios/*.toml; do
    name=$(basename "$scenario" .toml)
    TOTAL=$((TOTAL + 1))
    echo -n "  $name ... "

    # Skip long_drive_10min by default (stress test, ~13s to simulate).
    if [ "$name" = "long_drive_10min" ]; then
        if [ "${RUN_LONG:-0}" = "1" ]; then
            echo -n "[running] "
        else
            echo "SKIP (set RUN_LONG=1 to run)"
            SKIP=$((SKIP + 1))
            continue
        fi
    fi

    # Run check_assertions.py (generates trace + validates assertions).
    if python3 tests/check_assertions.py "$scenario" >/dev/null 2>/dev/null; then
        echo "PASS"
        PASS=$((PASS + 1))
    else
        echo "FAIL"
        # Show details on failure.
        python3 tests/check_assertions.py "$scenario" 2>/dev/null || true
        FAIL=$((FAIL + 1))
    fi
done

echo ""
echo "Results: $PASS passed, $FAIL failed, $SKIP skipped (of $TOTAL scenarios)"

if [ $FAIL -gt 0 ]; then
    exit 1
fi
exit 0
