#!/usr/bin/env python3
"""check_trace.py — validate microcar scenario trace output.

Placeholder: will parse JSONL traces and check assertions.
See docs/scenario_format.md for the assertion syntax.
"""
import sys

def main():
    scenario = sys.argv[1] if len(sys.argv) > 1 else None
    if scenario is None:
        print("Usage: check_trace.py <scenario.toml>")
        sys.exit(1)
    print(f"check_trace: {scenario} (not yet implemented)")

if __name__ == "__main__":
    main()
