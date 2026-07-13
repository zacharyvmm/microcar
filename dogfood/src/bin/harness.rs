//! `harness` — CLI entry point for the microcar dogfood harness.
//!
//! Subcommands:
//!
//! ```text
//! harness run     <scenario.toml> [--timeout-secs N] [--repeats N] [--json OUT]
//! harness run-all [--scenario-dir DIR] [--timeout-secs N] [--repeats N] [--json OUT]
//! ```
//!
//! For every scenario it runs the `microcar` binary `--repeats` times (default 2),
//! verifies solo-vs-repeat trace-hash determinism, checks the default invariant
//! set, prints a human summary, optionally writes a JSON summary for CI, and
//! exits non-zero if any scenario failed.
//!
//! The `microcar` binary is located via (in order): `$MICROCAR_BIN`, then the
//! first existing of `target/debug/microcar`, `target/release/microcar`,
//! `./microcar`. Run the harness from the microcar repo root so relative paths
//! inside scenario TOML (firmware, expected traces) resolve correctly.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

use microcar_dogfood::determinism::DeterminismReport;
use microcar_dogfood::invariants::CheckStatus;
use microcar_dogfood::summary::{build_summary, ScenarioSummary};
use microcar_dogfood::{check_solo_vs_repeat, write_summary};
use std::process::Command as StdCommand;

const DEFAULT_TIMEOUT_SECS: u64 = 60;
const DEFAULT_REPEATS: usize = 2;
const DEFAULT_SCENARIO_DIR: &str = "scenarios";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        usage();
        return ExitCode::from(2);
    }

    let cmd = args[0].as_str();
    let rest = &args[1..];

    match cmd {
        "run" => cmd_run(rest),
        "run-all" => cmd_run_all(rest),
        "cockpit" => cmd_cockpit(rest),
        "telematics" => cmd_telematics(rest),
        "-h" | "--help" | "help" => {
            usage();
            ExitCode::SUCCESS
        }
        other => {
            eprintln!("harness: unknown subcommand '{other}'\n");
            usage();
            ExitCode::from(2)
        }
    }
}

/// Parsed common flags.
struct Opts {
    timeout: Duration,
    repeats: usize,
    json: Option<PathBuf>,
    scenario_dir: PathBuf,
    positionals: Vec<String>,
}

fn parse_opts(args: &[String]) -> Result<Opts, String> {
    let mut timeout_secs = DEFAULT_TIMEOUT_SECS;
    let mut repeats = DEFAULT_REPEATS;
    let mut json = None;
    let mut scenario_dir = PathBuf::from(DEFAULT_SCENARIO_DIR);
    let mut positionals = Vec::new();

    let mut i = 0;
    while i < args.len() {
        let a = args[i].as_str();
        match a {
            "--timeout-secs" => {
                i += 1;
                let v = args.get(i).ok_or("--timeout-secs needs a value")?;
                timeout_secs = v.parse().map_err(|_| format!("bad --timeout-secs: {v}"))?;
            }
            "--repeats" => {
                i += 1;
                let v = args.get(i).ok_or("--repeats needs a value")?;
                repeats = v.parse().map_err(|_| format!("bad --repeats: {v}"))?;
            }
            "--json" => {
                i += 1;
                let v = args.get(i).ok_or("--json needs a path")?;
                json = Some(PathBuf::from(v));
            }
            "--scenario-dir" => {
                i += 1;
                let v = args.get(i).ok_or("--scenario-dir needs a path")?;
                scenario_dir = PathBuf::from(v);
            }
            other if other.starts_with("--") => {
                return Err(format!("unknown flag: {other}"));
            }
            other => positionals.push(other.to_string()),
        }
        i += 1;
    }

    Ok(Opts {
        timeout: Duration::from_secs(timeout_secs),
        repeats,
        json,
        scenario_dir,
        positionals,
    })
}

fn cmd_run(args: &[String]) -> ExitCode {
    let opts = match parse_opts(args) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("harness run: {e}");
            return ExitCode::from(2);
        }
    };
    if opts.positionals.len() != 1 {
        eprintln!("harness run: expected exactly one <scenario.toml>");
        return ExitCode::from(2);
    }
    let bin = match locate_microcar() {
        Ok(b) => b,
        Err(e) => {
            eprintln!("harness: {e}");
            return ExitCode::from(2);
        }
    };
    let scenario = PathBuf::from(&opts.positionals[0]);
    let report = check_solo_vs_repeat(&bin, &scenario, opts.repeats, opts.timeout);
    finish(&bin, vec![report], &opts.json)
}

fn cmd_run_all(args: &[String]) -> ExitCode {
    let opts = match parse_opts(args) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("harness run-all: {e}");
            return ExitCode::from(2);
        }
    };
    let bin = match locate_microcar() {
        Ok(b) => b,
        Err(e) => {
            eprintln!("harness: {e}");
            return ExitCode::from(2);
        }
    };
    let scenarios = match collect_scenarios(&opts.scenario_dir) {
        Ok(s) if !s.is_empty() => s,
        Ok(_) => {
            eprintln!(
                "harness run-all: no .toml scenarios in {}",
                opts.scenario_dir.display()
            );
            return ExitCode::from(2);
        }
        Err(e) => {
            eprintln!("harness run-all: {e}");
            return ExitCode::from(2);
        }
    };
    println!(
        "Running {} scenario(s) from {} ({} repeat(s) each, {:?} timeout)\n",
        scenarios.len(),
        opts.scenario_dir.display(),
        opts.repeats,
        opts.timeout,
    );
    let reports: Vec<DeterminismReport> = scenarios
        .iter()
        .map(|s| check_solo_vs_repeat(&bin, s, opts.repeats, opts.timeout))
        .collect();
    finish(&bin, reports, &opts.json)
}

/// Print the human summary, optionally write JSON, and choose the exit code.
fn finish(bin: &Path, reports: Vec<DeterminismReport>, json: &Option<PathBuf>) -> ExitCode {
    let summary = build_summary(&reports);

    println!("microcar dogfood harness v{}", summary.harness_version);
    println!("microcar binary: {}", bin.display());
    println!();

    for s in &summary.scenarios {
        print_scenario(s);
    }

    let t = &summary.totals;
    println!("──────────────────────────────────────────");
    println!(
        "scenarios: {} passed, {} failed (of {})",
        t.passed, t.failed, t.scenarios
    );
    println!(
        "invariants: {} pass, {} fail, {} skip",
        t.invariants_passed, t.invariants_failed, t.invariants_skipped
    );

    if let Some(path) = json {
        match write_summary(&reports, path) {
            Ok(()) => println!("wrote JSON summary: {}", path.display()),
            Err(e) => eprintln!("harness: failed to write {}: {e}", path.display()),
        }
    }

    if summary.all_passed() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

fn print_scenario(s: &ScenarioSummary) {
    let mark = if s.passed() { "ok  " } else { "FAIL" };
    let det = if s.deterministic {
        "deterministic"
    } else {
        "NON-DETERMINISTIC"
    };
    println!(
        "[{mark}] {name:<32} {status:<7} {wall:>6}ms  hash={hash}  {det} (x{reps})",
        name = s.name,
        status = s.status.as_str(),
        wall = s.wall_ms,
        hash = short_hash(&s.trace_hash),
        reps = s.repeats,
    );
    // Show failing / non-skipped invariants so problems are visible; keep the
    // happy path terse.
    for inv in &s.invariants {
        if inv.status == CheckStatus::Fail {
            println!(
                "        - {} {}: {}",
                inv.status.as_str(),
                inv.name,
                inv.message
            );
        }
    }
    if !s.deterministic {
        println!("        - repeat hashes: {}", s.repeat_hashes.join(", "));
    }
}

fn short_hash(h: &str) -> String {
    if h.len() > 16 {
        format!("{}…", &h[..16])
    } else {
        h.to_string()
    }
}

/// Locate the microcar binary via env override then conventional target paths.
fn locate_microcar() -> Result<PathBuf, String> {
    if let Ok(p) = std::env::var("MICROCAR_BIN") {
        let pb = PathBuf::from(&p);
        if pb.is_file() {
            return Ok(pb);
        }
        return Err(format!("MICROCAR_BIN={p} does not point to a file"));
    }
    let candidates = [
        "target/debug/microcar",
        "target/release/microcar",
        "./microcar",
        "../target/debug/microcar",
    ];
    for c in candidates {
        let pb = PathBuf::from(c);
        if pb.is_file() {
            return Ok(pb);
        }
    }
    Err(format!(
        "could not find the microcar binary. Build it with `cargo build` and run \
         the harness from the microcar repo root, or set MICROCAR_BIN. Looked in: {}",
        candidates.join(", ")
    ))
}

/// Collect `*.toml` files from `dir`, sorted for deterministic ordering.
fn collect_scenarios(dir: &Path) -> Result<Vec<PathBuf>, String> {
    let mut out = Vec::new();
    let entries = std::fs::read_dir(dir).map_err(|e| format!("read_dir {}: {e}", dir.display()))?;
    for entry in entries {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("toml") {
            out.push(path);
        }
    }
    out.sort();
    Ok(out)
}

fn usage() {
    eprintln!(
        "microcar dogfood harness\n\
         \n\
         USAGE:\n\
         \x20 harness run         <scenario.toml> [--timeout-secs N] [--repeats N] [--json OUT]\n\
         \x20 harness run-all     [--scenario-dir DIR] [--timeout-secs N] [--repeats N] [--json OUT]\n\
         \x20 harness cockpit     Run gRPC cockpit integration test\n\
         \x20 harness telematics  Run telematics integration tests\n\
         \n\
         FLAGS:\n\
         \x20 --timeout-secs N   Wall-clock timeout per run (default {DEFAULT_TIMEOUT_SECS})\n\
         \x20 --repeats N        Runs per scenario for determinism check (default {DEFAULT_REPEATS})\n\
         \x20 --json OUT         Write a JSON summary to OUT\n\
         \x20 --scenario-dir DIR Directory of .toml scenarios for run-all (default {DEFAULT_SCENARIO_DIR})\n\
         \n\
         The microcar binary is found via $MICROCAR_BIN or target/debug/microcar.\n\
         Exit code is non-zero if any scenario failed."
    );
}

/// Run the gRPC cockpit integration test from the costar sim-grpc crate.
fn cmd_cockpit(_args: &[String]) -> ExitCode {
    let costar_root = PathBuf::from("../costar");
    if !costar_root.join("Cargo.toml").is_file() {
        eprintln!(
            "harness cockpit: costar repo not found at {}",
            costar_root.display()
        );
        eprintln!("Run this from the microcar repo root with costar at ../costar");
        return ExitCode::from(2);
    }

    println!("Running gRPC cockpit integration test (sim-grpc)...");
    let status = StdCommand::new("cargo")
        .args(["test", "-p", "sim-grpc", "--", "cockpit", "--nocapture"])
        .current_dir(&costar_root)
        .status();

    match status {
        Ok(s) if s.success() => {
            println!("cockpit: PASS");
            ExitCode::SUCCESS
        }
        Ok(s) => {
            let code = s.code().unwrap_or(-1);
            eprintln!("cockpit: FAIL (exit code: {code})");
            ExitCode::FAILURE
        }
        Err(e) => {
            eprintln!("harness cockpit: failed to run cargo test: {e}");
            ExitCode::from(2)
        }
    }
}

/// Run telematics integration tests: runs the microcar binary with a
/// telematics scenario and verifies the JSON summary output contains
/// expected record counts, request IDs, and payloads.
fn cmd_telematics(args: &[String]) -> ExitCode {
    let opts = match parse_opts(args) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("harness telematics: {e}");
            return ExitCode::from(2);
        }
    };
    let bin = match locate_microcar() {
        Ok(b) => b,
        Err(e) => {
            eprintln!("harness: {e}");
            return ExitCode::from(2);
        }
    };

    // Run the fixed telematics debug_gym scenario.
    let scenario = PathBuf::from("dogfood/debug_gym/telematics_partial_write_bug/fixed.toml");
    if !scenario.is_file() {
        eprintln!(
            "harness telematics: scenario not found at {}",
            scenario.display()
        );
        eprintln!("Run this from the microcar repo root.");
        return ExitCode::from(2);
    }

    println!(
        "Running telematics integration test: {}",
        scenario.display()
    );
    let report = check_solo_vs_repeat(&bin, &scenario, opts.repeats, opts.timeout);
    finish(&bin, vec![report], &opts.json)
}
