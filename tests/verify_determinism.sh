#!/bin/bash
# verify_determinism.sh — Run normal_drive_cycle.toml 10 times and
# assert all SHA-256 hashes of trace output are identical.
#
# Usage: bash tests/verify_determinism.sh
#
# Prerequisites:
#   - costar binary built (cargo build in costar workspace)
#   - microcar firmware compiled (build.rs already ran)
#
# Exit code: 0 if all 10 runs produce identical traces, 1 otherwise.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
MICROCAR_DIR="$(dirname "$SCRIPT_DIR")"
COSTAR_DIR="$MICROCAR_DIR/../costar"
SCENARIO="$MICROCAR_DIR/scenarios/normal_drive_cycle.toml"
RUNS=10
TMPDIR="${TMPDIR:-/tmp}/microcar_determinism_$$"
HASHES=()

cleanup() {
    rm -rf "$TMPDIR"
}
trap cleanup EXIT

mkdir -p "$TMPDIR"

echo "=== Determinism Verification ==="
echo "Scenario: $SCENARIO"
echo "Runs: $RUNS"
echo "Temp dir: $TMPDIR"
echo ""

# Find SHA-256 tool
if command -v shasum &>/dev/null; then
    SHA_CMD="shasum -a 256"
elif command -v sha256sum &>/dev/null; then
    SHA_CMD="sha256sum"
else
    echo "ERROR: No SHA-256 tool found (shasum or sha256sum)."
    echo "  On macOS: brew install coreutils (for sha256sum) or use built-in shasum -a 256"
    exit 1
fi

# ── Build once ───────────────────────────────────────────────
echo "Building costar (one-time)..."
(cd "$COSTAR_DIR" && cargo build 2>&1) > "$TMPDIR/build.log" || {
    echo "FAIL: Build failed"
    cat "$TMPDIR/build.log"
    exit 1
}
echo "Build complete."
echo ""

# ── Run simulation N times ───────────────────────────────────
echo "Running $RUNS iterations..."
SIM_RUNNER="$COSTAR_DIR/target/debug/sim-runner"

for i in $(seq 1 $RUNS); do
    # Run the sim-runner binary directly (no cargo run)
    # Use --microcar --no-golden to skip golden comparison and just verify simulation ran
    "$SIM_RUNNER" test --microcar --no-golden "$SCENARIO" \
        > "$TMPDIR/run_${i}.log" 2>&1 || {
        echo "FAIL: Run $i exited with error"
        cat "$TMPDIR/run_${i}.log"
        exit 1
    }
    
    # Filter out build-like warnings and timestamps for hashing
    # Only keep lines that look like trace events (start with digit or contain specific patterns)
    grep -v '^warning:' "$TMPDIR/run_${i}.log" | \
        grep -v '^[[:space:]]*|' | \
        grep -v '^[[:space:]]*$' \
        > "$TMPDIR/run_${i}.trace" 2>/dev/null || true
    
    if [ ! -s "$TMPDIR/run_${i}.trace" ]; then
        # If the filtered trace is empty, hash the full log minus warnings
        grep -v '^warning:' "$TMPDIR/run_${i}.log" > "$TMPDIR/run_${i}.trace" 2>/dev/null || true
    fi
    
    HASH=$(cat "$TMPDIR/run_${i}.trace" | $SHA_CMD | awk '{print $1}')
    HASHES+=("$HASH")
    
    echo "  Run $i: $HASH ($(wc -l < "$TMPDIR/run_${i}.trace" 2>/dev/null || echo 0) lines)"
done

echo ""

# Check all hashes are identical
UNIQUE_HASHES=$(printf '%s\n' "${HASHES[@]}" | sort -u | wc -l)
if [ "$UNIQUE_HASHES" -eq 1 ]; then
    echo "=== PASS ==="
    echo "All $RUNS runs produced identical traces."
    echo "SHA-256: ${HASHES[0]}"
    exit 0
else
    echo "=== FAIL ==="
    echo "Found $UNIQUE_HASHES unique hashes across $RUNS runs."
    echo "Hashes:"
    for i in $(seq 0 $((${#HASHES[@]} - 1))); do
        echo "  Run $((i+1)): ${HASHES[$i]}"
    done
    
    # Show diff between first two differing runs
    for i in $(seq 1 $((${#HASHES[@]} - 1))); do
        if [ "${HASHES[$i]}" != "${HASHES[0]}" ]; then
            echo ""
            echo "Diff between run 1 and run $((i+1)):"
            diff "$TMPDIR/run_1.trace" "$TMPDIR/run_$((i+1)).trace" 2>/dev/null || true
            break
        fi
    done
    
    echo ""
    echo "NOTE: If the only differences are in timing or build output,"
    echo "the simulation logic itself may be deterministic but the test"
    echo "harness includes non-deterministic output. Check the diff above."
    exit 1
fi
