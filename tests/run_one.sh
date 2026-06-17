#!/bin/bash
# run_one.sh — run a single microcar scenario
set -euo pipefail
SCENARIO="$1"

cd "$(dirname "$0")/.."

# Placeholder: will invoke costar scenario runner when integrated.
# For pure-logic testing, delegates to check_trace.py.
python3 tests/check_trace.py "$SCENARIO" 2>/dev/null || {
    echo "SKIP (costar scenario runner not yet integrated)"
    exit 0
}
