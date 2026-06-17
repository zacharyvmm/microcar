# Zephyr Dashboard ECU

Zephyr-based dashboard ECU firmware for the microcar demo. Functionally
equivalent to the FreeRTOS dashboard ECU but uses Zephyr threading APIs
(`k_thread_create`, `k_sleep`) and compiles via costar's `sim-zephyr-port`
cc crate.

## Files

- `src/main.c` — Zephyr app entry point (`zephyr_app_main()`), spawns the
  dashboard thread, enters infinite sleep loop.

Shared from FreeRTOS ECU (`firmware/dashboard_ecu/src/`):
- `dashboard_state.c/h` — Dashboard state management (vehicle mode,
  speed, battery SOC/temp/voltage, warning tracking).
- `warning_display.c/h` — Priority-based warning display (highest
  severity wins).

## How It Works

The dashboard thread:
1. Initializes dashboard state and warning display
2. Sends initial heartbeat
3. Loops every 10ms: checks top warning, sends heartbeat every 100ms
4. Uses `sim_trace_u32` for diagnostic trace events when compiled with
   the costar ABI

## Building (standalone mode, no Zephyr SDK needed)

```bash
cd /path/to/costar
cargo run -- --rtos zephyr --golden
```

## Building (full Zephyr kernel, via cc crate)

Requires Zephyr v4.1.0 source tree. The dashboard shares business logic
with the FreeRTOS ECU, so include paths must point to both directories:

```bash
cd /path/to/costar
ZEPHYR_BASE=/path/to/zephyr-workspace/zephyr \
ZEPHYR_APP_SOURCES=/path/to/microcar/firmware/dashboard_ecu_zephyr/src/main.c \
ZEPHYR_EXTRA_SOURCES="/path/to/microcar/firmware/dashboard_ecu/src/dashboard_state.c \
                     /path/to/microcar/firmware/dashboard_ecu/src/warning_display.c" \
ZEPHYR_APP_INCLUDES="/path/to/microcar/common/include \
                    /path/to/microcar/firmware/dashboard_ecu_zephyr/src \
                    /path/to/microcar/firmware/dashboard_ecu/src" \
cargo build --features zephyr_real
```

## Scenario Testing

The mixed-RTOS boot scenario validates the dashboard alongside FreeRTOS ECUs:

```bash
cd /path/to/microcar
python3 tests/check_assertions.py scenarios/mixed_rtos_boot.toml
```
