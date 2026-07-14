#!/bin/bash
# run_all.sh — run all microcar scenario tests.
#
# Usage: ./tests/run_all.sh
#
# Environment:
#   RUN_LONG=1          include long_drive_10min.toml
#   TEST_TIMEOUT_SECS=N wall-clock timeout per scenario (default: 60)
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
TEST_TIMEOUT_SECS="${TEST_TIMEOUT_SECS:-60}"

case "$TEST_TIMEOUT_SECS" in
    ''|*[!0-9]*|0)
        echo "error: TEST_TIMEOUT_SECS must be a positive integer" >&2
        exit 2
        ;;
esac

if ! command -v timeout >/dev/null 2>&1; then
    echo "error: tests/run_all.sh requires the 'timeout' command" >&2
    exit 2
fi

OUTPUT_FILE=$(mktemp "${TMPDIR:-/tmp}/microcar-run-all.XXXXXX")
trap 'rm -f "$OUTPUT_FILE"' EXIT

run_scenario() {
    timeout --foreground "${TEST_TIMEOUT_SECS}s" \
        python3 tests/check_assertions.py "$1"
}

echo "=== microcar scenario tests ==="
echo ""

for scenario in scenarios/*.toml; do
    name=$(basename "$scenario" .toml)
    TOTAL=$((TOTAL + 1))
    echo -n "  $name ... "

    # Long and soak scenarios are excluded from the fast suite.  In particular,
    # the legacy Python simulator buffers its complete trace and is not safe for
    # the 1-hour / 8-hour cases. Those are always excluded here and will move to
    # the bounded-memory Rust soak lane.
    if [ "$name" = "long_drive_10min" ]; then
        if [ "${RUN_LONG:-0}" = "1" ]; then
            echo -n "[running] "
        else
            echo "SKIP (set RUN_LONG=1 to run)"
            SKIP=$((SKIP + 1))
            continue
        fi
    fi

    if [ "$name" = "soak_1hour" ] || [ "$name" = "overnight_8hour" ]; then
        echo "SKIP (unsafe in Python runner; use the planned Rust soak lane)"
        SKIP=$((SKIP + 1))
        continue
    fi

    # Run once, with a wall-clock timeout. Retain output for diagnostics instead
    # of re-running an expensive or wedged scenario after failure.
    : >"$OUTPUT_FILE"
    if run_scenario "$scenario" >"$OUTPUT_FILE" 2>&1; then
        echo "PASS"
        PASS=$((PASS + 1))
    else
        status=$?
        if [ "$status" -eq 124 ]; then
            echo "TIMEOUT (${TEST_TIMEOUT_SECS}s)"
        else
            echo "FAIL (exit $status)"
        fi
        sed -n '1,200p' "$OUTPUT_FILE"
        FAIL=$((FAIL + 1))
    fi
done

echo ""
echo "Results: $PASS passed, $FAIL failed, $SKIP skipped (of $TOTAL scenarios)"

if [ $FAIL -gt 0 ]; then
    exit 1
fi
exit 0
