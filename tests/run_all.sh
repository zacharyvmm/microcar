#!/bin/bash
# run_all.sh — run all implemented microcar scenarios
set -euo pipefail
cd "$(dirname "$0")/.."

PASS=0
FAIL=0

echo "=== microcar scenario tests ==="
echo ""

for scenario in scenarios/*.toml; do
    if [ ! -f "$scenario" ]; then
        echo "No scenario files found."
        exit 0
    fi
    name=$(basename "$scenario" .toml)
    echo -n "  $name ... "

    if ./tests/run_one.sh "$scenario" > /dev/null 2>&1; then
        echo "PASS"
        ((PASS++))
    else
        echo "FAIL"
        ((FAIL++))
    fi
done

echo ""
echo "Results: $PASS passed, $FAIL failed"

if [ $FAIL -gt 0 ]; then
    exit 1
fi
exit 0
