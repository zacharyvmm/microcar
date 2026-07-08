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
use microcar_dogfood::toml_zoo::{self, DEFAULT_CORPUS_DIR};
use microcar_dogfood::{
    check_solo_vs_repeat, run_churn, run_panic_isolation, run_simfarm, write_summary,
};

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
        "simfarm" => cmd_simfarm(rest),
        "toml-zoo" | "toml_zoo" => cmd_toml_zoo(rest),
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
    // Show failing invariants so problems are visible; keep the happy path terse.
    for inv in &s.invariants {
        if inv.status == CheckStatus::Fail {
            println!("        - {} {}: {}", inv.status.as_str(), inv.name, inv.message);
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

/// `ok  ` / `FAIL` marker for the human summary.
fn okmark(ok: bool) -> &'static str {
    if ok {
        "ok  "
    } else {
        "FAIL"
    }
}

/// Print an argument error for a subcommand and return the usage exit code.
fn argerr(sub: &str, msg: &str) -> ExitCode {
    eprintln!("harness {sub}: {msg}");
    ExitCode::from(2)
}

/// Write a JSON string to `path`, reporting success/failure like the other lanes.
fn write_json(path: &Path, json_str: &str) {
    match std::fs::write(path, json_str) {
        Ok(()) => println!("wrote JSON summary: {}", path.display()),
        Err(e) => eprintln!("harness: failed to write {}: {e}", path.display()),
    }
}

/// `harness simfarm <scenario.toml> [-n N] [--churn M] [--bad PATH] [--timeout-secs S] [--json OUT]`
///
/// Runs the simfarm lane: concurrent-determinism (N sessions == solo), churn
/// (M create/run/destroy iterations stay stable), and panic-isolation (a
/// malformed sibling fails cleanly while the healthy run is unaffected).
fn cmd_simfarm(args: &[String]) -> ExitCode {
    let mut n = 4usize;
    let mut churn = 5usize;
    let mut timeout_secs = DEFAULT_TIMEOUT_SECS;
    let mut json: Option<PathBuf> = None;
    let mut bad: Option<PathBuf> = None;
    let mut scenario: Option<PathBuf> = None;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-n" | "--sessions" => {
                i += 1;
                match args.get(i).and_then(|v| v.parse().ok()) {
                    Some(v) => n = v,
                    None => return argerr("simfarm", "-n/--sessions needs a number"),
                }
            }
            "--churn" => {
                i += 1;
                match args.get(i).and_then(|v| v.parse().ok()) {
                    Some(v) => churn = v,
                    None => return argerr("simfarm", "--churn needs a number"),
                }
            }
            "--timeout-secs" => {
                i += 1;
                match args.get(i).and_then(|v| v.parse().ok()) {
                    Some(v) => timeout_secs = v,
                    None => return argerr("simfarm", "--timeout-secs needs a number"),
                }
            }
            "--json" => {
                i += 1;
                match args.get(i) {
                    Some(v) => json = Some(PathBuf::from(v)),
                    None => return argerr("simfarm", "--json needs a path"),
                }
            }
            "--bad" => {
                i += 1;
                match args.get(i) {
                    Some(v) => bad = Some(PathBuf::from(v)),
                    None => return argerr("simfarm", "--bad needs a path"),
                }
            }
            other if other.starts_with('-') => {
                return argerr("simfarm", &format!("unknown flag: {other}"));
            }
            other => scenario = Some(PathBuf::from(other)),
        }
        i += 1;
    }

    let scenario = match scenario {
        Some(s) => s,
        None => return argerr("simfarm", "expected <scenario.toml>"),
    };
    let bin = match locate_microcar() {
        Ok(b) => b,
        Err(e) => {
            eprintln!("harness: {e}");
            return ExitCode::from(2);
        }
    };
    let timeout = Duration::from_secs(timeout_secs);
    let bad = bad.unwrap_or_else(|| PathBuf::from(format!("{DEFAULT_CORPUS_DIR}/missing_gateway.toml")));

    println!("microcar dogfood harness v{}", microcar_dogfood::HARNESS_VERSION);
    println!("microcar binary: {}", bin.display());
    println!("simfarm: {}\n", scenario.display());

    let sim = run_simfarm(&bin, &scenario, n, timeout);
    println!(
        "[{}] concurrent x{:<3}  solo={}  all_match={}  all_clean={}",
        okmark(sim.passed()),
        sim.n,
        short_hash(&sim.solo_hash),
        sim.all_match,
        sim.all_clean
    );
    for (idx, rh) in sim.concurrent.iter().enumerate() {
        if rh.hash != sim.solo_hash || !rh.status.is_clean() {
            println!(
                "        - run #{idx}: status={} hash={}",
                rh.status.as_str(),
                short_hash(&rh.hash)
            );
        }
    }

    let mut all_ok = sim.passed();

    let churn_report = if churn > 0 {
        let c = run_churn(&bin, &scenario, churn, timeout);
        println!(
            "[{}] churn x{:<7}  distinct_hashes={}  clean={}/{}",
            okmark(c.passed()),
            c.iterations,
            c.distinct_hashes,
            c.clean,
            c.iterations
        );
        all_ok &= c.passed();
        Some(c)
    } else {
        None
    };

    let panic_report = if bad.is_file() {
        let p = run_panic_isolation(&bin, &scenario, &bad, timeout);
        println!(
            "[{}] panic-isolation  bad={}  healthy={}  bad_status={}(exit={:?})  isolated={}",
            okmark(p.passed()),
            bad.display(),
            p.healthy_status.as_str(),
            p.bad_status.as_str(),
            p.bad_exit_code,
            p.isolated
        );
        all_ok &= p.passed();
        Some(p)
    } else {
        println!(
            "[skip] panic-isolation: no bad scenario at {} (pass --bad PATH)",
            bad.display()
        );
        None
    };

    if let Some(path) = &json {
        let mut fields = vec![("simfarm".to_string(), sim.to_json())];
        if let Some(c) = &churn_report {
            fields.push(("churn".to_string(), c.to_json()));
        }
        if let Some(p) = &panic_report {
            fields.push(("panic_isolation".to_string(), p.to_json()));
        }
        let obj = microcar_dogfood::json::Json::Obj(fields);
        write_json(path, &obj.to_pretty());
    }

    println!("──────────────────────────────────────────");
    println!("simfarm lane: {}", if all_ok { "PASS" } else { "FAIL" });
    if all_ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

/// `harness toml-zoo [--dir DIR] [--healthy SCENARIO] [--no-sibling] [--timeout-secs S] [--json OUT]`
///
/// Runs the toml_zoo lane: every malformed scenario in the corpus must produce
/// a structured error with the expected kind, exit code 2, and no panic; plus a
/// sibling-isolation check (a malformed scenario run concurrently with a healthy
/// one does not disturb it).
fn cmd_toml_zoo(args: &[String]) -> ExitCode {
    let mut dir = PathBuf::from(DEFAULT_CORPUS_DIR);
    let mut healthy: Option<PathBuf> = Some(PathBuf::from("scenarios/boot_and_heartbeat.toml"));
    let mut timeout_secs = DEFAULT_TIMEOUT_SECS;
    let mut json: Option<PathBuf> = None;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--dir" => {
                i += 1;
                match args.get(i) {
                    Some(v) => dir = PathBuf::from(v),
                    None => return argerr("toml-zoo", "--dir needs a path"),
                }
            }
            "--healthy" => {
                i += 1;
                match args.get(i) {
                    Some(v) => healthy = Some(PathBuf::from(v)),
                    None => return argerr("toml-zoo", "--healthy needs a path"),
                }
            }
            "--no-sibling" => healthy = None,
            "--timeout-secs" => {
                i += 1;
                match args.get(i).and_then(|v| v.parse().ok()) {
                    Some(v) => timeout_secs = v,
                    None => return argerr("toml-zoo", "--timeout-secs needs a number"),
                }
            }
            "--json" => {
                i += 1;
                match args.get(i) {
                    Some(v) => json = Some(PathBuf::from(v)),
                    None => return argerr("toml-zoo", "--json needs a path"),
                }
            }
            other if other.starts_with('-') => {
                return argerr("toml-zoo", &format!("unknown flag: {other}"));
            }
            other => dir = PathBuf::from(other),
        }
        i += 1;
    }

    let bin = match locate_microcar() {
        Ok(b) => b,
        Err(e) => {
            eprintln!("harness: {e}");
            return ExitCode::from(2);
        }
    };
    let timeout = Duration::from_secs(timeout_secs);
    // Only run sibling isolation if the healthy scenario actually exists.
    let healthy = healthy.filter(|h| h.is_file());

    println!("microcar dogfood harness v{}", microcar_dogfood::HARNESS_VERSION);
    println!("microcar binary: {}", bin.display());
    println!("toml_zoo corpus: {}\n", dir.display());

    let report = match toml_zoo::run_toml_zoo(&bin, &dir, timeout, healthy.as_deref()) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("harness toml-zoo: reading corpus {}: {e}", dir.display());
            return ExitCode::from(2);
        }
    };

    for c in &report.cases {
        println!("[{}] {:<30} {}", okmark(c.passed), c.name, c.detail);
    }
    if let Some(s) = &report.sibling {
        println!(
            "[{}] sibling-isolation  healthy={}  bad={}  isolated={}",
            okmark(s.isolated),
            s.healthy_status.as_str(),
            s.bad_status.as_str(),
            s.isolated
        );
    }

    let (pass, fail) = report.totals();
    println!("──────────────────────────────────────────");
    println!(
        "toml_zoo: {pass} passed, {fail} failed (of {} cases)",
        report.cases.len()
    );

    if let Some(path) = &json {
        write_json(path, &report.to_json().to_pretty());
    }

    if report.passed() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

fn usage() {
    eprintln!(
        "microcar dogfood harness\n\
         \n\
         USAGE:\n\
         \x20 harness run      <scenario.toml> [--timeout-secs N] [--repeats N] [--json OUT]\n\
         \x20 harness run-all  [--scenario-dir DIR] [--timeout-secs N] [--repeats N] [--json OUT]\n\
         \x20 harness simfarm  <scenario.toml> [-n N] [--churn M] [--bad PATH] [--timeout-secs N] [--json OUT]\n\
         \x20 harness toml-zoo [--dir DIR] [--healthy SCENARIO] [--no-sibling] [--timeout-secs N] [--json OUT]\n\
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
