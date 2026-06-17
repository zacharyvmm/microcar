# Zephyr Dashboard ECU

Zephyr-based dashboard ECU firmware for the microcar demo. Functionally
equivalent to the FreeRTOS dashboard ECU but uses Zephyr threading APIs
and compiles via costar's sim-zephyr-port cc crate.

## Files

- `src/main.c` — Zephyr app entry point (`c_zephyr_main`), spawns the
  dashboard thread, runs the Zephyr scheduler loop.
- Reuses `dashboard_state.c/h` and `warning_display.c/h` from
  `firmware/dashboard_ecu/src/` (shared between FreeRTOS and Zephyr ECUs).

## How It Works

The dashboard thread:
1. Initializes dashboard state and warning display
2. Simulates CAN frame reception at predetermined times (vehicle mode,
   wheel speed, BMS status, warnings)
3. Publishes heartbeats every 100 ms
4. Emits trace events via `sim_trace_u32` for all state changes
5. Exits after 600 ms of simulated time

## Building (standalone mode, no Zephyr SDK needed)

```bash
cd /path/to/costar
ZEPHYR_APP_SOURCES=/path/to/microcar/firmware/dashboard_ecu_zephyr/src/main.c \
ZEPHYR_EXTRA_SOURCES="/path/to/microcar/firmware/dashboard_ecu/src/dashboard_state.c \
                     /path/to/microcar/firmware/dashboard_ecu/src/warning_display.c" \
ZEPHYR_APP_INCLUDES="/path/to/microcar/common/include:/path/to/microcar/firmware/dashboard_ecu/src" \
cargo build
```

## Running

```bash
cd /path/to/costar
ZEPHYR_APP_SOURCES=... \
ZEPHYR_EXTRA_SOURCES=... \
ZEPHYR_APP_INCLUDES=... \
cargo run -- --rtos zephyr --golden
```

The `--golden` flag produces machine-readable trace output suitable for
golden-trace comparison.

## Golden Trace

The expected trace output is stored in `../../expected/traces/zephyr_dashboard.trace`.
Regenerate with:

```bash
ZEPHYR_APP_SOURCES=... cargo run -- --rtos zephyr --golden \
  > ../../expected/traces/zephyr_dashboard.trace
```
