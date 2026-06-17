#!/bin/bash
# run_all.sh — run all microcar scenarios
set -euo pipefail
cd "$(dirname "$0")/.."

PASS=0
FAIL=0
SKIP=0

echo "=== microcar scenario tests ==="
echo ""

for scenario in scenarios/*.toml; do
    name=$(basename "$scenario" .toml)
    echo -n "  $name ... "

    # Skip scenarios that are too long for CI.
    if [ "$name" = "long_drive_10min" ]; then
        echo "SKIP (stress test, run separately)"
        ((SKIP++))
        continue
    fi

    if python3 tests/check_trace.py "$scenario" > /dev/null 2>&1; then
        echo "PASS"
        ((PASS++))
    else
        echo "FAIL"
        python3 tests/check_trace.py "$scenario" 2>&1 | tail -5
        ((FAIL++))
    fi
done

echo ""
echo "Results: $PASS passed, $FAIL failed, $SKIP skipped"

if [ $FAIL -gt 0 ]; then
    exit 1
fi
exit 0
