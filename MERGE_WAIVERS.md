# Merge Waivers — microcar#2

This PR is recommended for merge with the following explicit waivers.

## 1. `dogfood/b3_gateway_reboot_downtime.toml` is not a passing harness acceptance scenario

```
Fiber::drop id=1 state=Created | Fiber::drop leaking coroutine id=1
microcar: error [check]: trace mismatch
```

- The failure is caused by a pre-existing `sim-fiber` / FreeRTOS coroutine leak,
  not by the scenario comments or formatting in this PR.
- Reproduced at the pre-cleanup commit `949c0fe` — identical hash (`2d4d4446a68f4d79`),
  identical error.
- The scenario exercises the correct restart infrastructure (pending_boots,
  deferred reconstruction, firmware factory, delivery boundary); it just
  cannot run to completion due to the coroutine leak in the C firmware stack.
- Waived for Stage A foundation merge.

## 2. `harness run-all` has 26 known pre-existing failures

26 of 31 scenarios fail with `ProcessTerminatedCleanly` + `TraceNonEmpty`.
All 26 produce the same deterministic hash (`cbf29ce484222325`), confirming
they are empty-output crashes, not regressions from this PR.

## 3. Full workspace clippy (`-D warnings`) not clean

Fails with pre-existing issues in upstream costar dependencies:
- `sim-net`: `manual_c_str_literals` clippy lint (Rust 1.97)
- `sim-grpc`: requires `PROTOC` env var for build

The microcar crates themselves pass clippy cleanly.

## 4. Scoped-out for follow-up

- Plant-backed restart scenario: blocked by pre-existing C firmware / plant-model segfault
- Explicit RTOS/config identity assertion: TODO
- Explicit firmware-factory invocation assertion: TODO
- Explicit powertrain heartbeat-continuity assertion: TODO
- NetworkBank / Ethernet isolation: Stage B3
- gRPC-specific restart/session-failure tests: follow-up
- JSON-RPC `sim.stop` semantics: follow-up
