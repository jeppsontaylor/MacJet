//! Add or refresh `schema_version`, `tool`, `run`, and `system` on benchmark JSON.

use std::fs;

use clap::Parser;
use macjet::benchmark_fingerprint::{
    collect_system_fingerprint, merge_report_shell, tool_meta, utc_now_rfc3339_millis, RunMeta,
};
use serde_json::Value;
use sysinfo::System;

#[derive(Parser, Debug)]
#[command(name = "benchmark_enrich")]
#[command(about = "Add or refresh benchmark JSON metadata (system fingerprint, run, tool)")]
struct Args {
    /// Path to benchmark_compare_*.json
    path: std::path::PathBuf,

    /// Overwrite the input file instead of writing `<name>.enriched.json`
    #[arg(long)]
    in_place: bool,

    /// Recompute `system` even if already present
    #[arg(long)]
    force: bool,
}

fn main() {
    let args = Args::parse();
    let raw = fs::read_to_string(&args.path).unwrap_or_else(|e| {
        eprintln!("benchmark_enrich: read {}: {}", args.path.display(), e);
        std::process::exit(1);
    });

    let v: Value = serde_json::from_str(&raw).unwrap_or_else(|e| {
        eprintln!("benchmark_enrich: parse JSON: {}", e);
        std::process::exit(1);
    });

    if v.get("summary").is_none() || v.get("samples").is_none() {
        eprintln!("benchmark_enrich: JSON must contain `summary` and `samples`");
        std::process::exit(1);
    }

    if v.get("system").is_some() && !args.force {
        eprintln!(
            "benchmark_enrich: `system` already present (use --force to refresh fingerprint)"
        );
        std::process::exit(0);
    }

    let mut sys = System::new();
    let system = collect_system_fingerprint(&mut sys);
    let now = utc_now_rfc3339_millis();

    let argv = std::env::args().collect::<Vec<_>>();

    let (run, post_hoc) = match v.get("run") {
        Some(r) => (
            serde_json::from_value(r.clone()).unwrap_or_else(|_| synthetic_run(&now, &argv, &v)),
            false,
        ),
        None => (synthetic_run(&now, &argv, &v), true),
    };

    let mut out = serde_json::json!({
        "summary": v.get("summary").cloned().unwrap(),
        "samples": v.get("samples").cloned().unwrap(),
    });
    out = merge_report_shell(out, tool_meta(), run, system);

    if post_hoc {
        if let Some(obj) = out.as_object_mut() {
            obj.insert(
                "enrichment".to_string(),
                serde_json::json!({
                    "post_hoc": true,
                    "enriched_at_utc": now,
                    "note": "Original file lacked `run` / full metadata; `run.started_at` / `wall_seconds` may be approximate.",
                }),
            );
        }
    }

    let out_path = if args.in_place {
        args.path.clone()
    } else {
        args.path.with_extension("enriched.json")
    };

    if let Some(parent) = out_path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let pretty = serde_json::to_string_pretty(&out).unwrap();
    fs::write(&out_path, pretty).unwrap_or_else(|e| {
        eprintln!("benchmark_enrich: write {}: {}", out_path.display(), e);
        std::process::exit(1);
    });
    eprintln!("benchmark_enrich: wrote {}", out_path.display());
}

fn synthetic_run(now: &str, argv: &[String], v: &Value) -> RunMeta {
    let summary = v.get("summary").and_then(|s| s.as_object());
    let max_samples = summary
        .and_then(|o| o.get("samples"))
        .and_then(|x| x.as_u64())
        .unwrap_or(0) as usize;
    let interval_secs = summary
        .and_then(|o| o.get("interval_secs"))
        .and_then(|x| x.as_f64())
        .unwrap_or(0.0);
    let wall = max_samples as f64 * interval_secs;

    RunMeta {
        started_at_utc: now.to_string(),
        finished_at_utc: now.to_string(),
        wall_seconds: wall,
        argv: argv.to_vec(),
        max_samples,
        interval_secs,
        no_ml_flag: false,
    }
}
