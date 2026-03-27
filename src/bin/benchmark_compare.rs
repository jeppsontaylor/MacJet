//! Idle benchmark: sample **MacJet** and **Activity Monitor** CPU% + RSS on a fixed interval.
//!
//! Uses the same `sysinfo` primitives as the main app (`process.cpu_usage()`, RSS in MB).
//! Intended for apples-to-apples comparison with historical docs (300 samples, 1 s spacing).

use std::io::{stderr, IsTerminal};
use std::thread;
use std::time::{Duration, Instant};

use clap::Parser;
use indicatif::{ProgressBar, ProgressDrawTarget, ProgressStyle};
use macjet::benchmark_fingerprint::{
    collect_system_fingerprint, default_benchmark_json_path, merge_report_shell,
    resolve_default_benchmark_path, tool_meta, utc_now_rfc3339_millis, RunMeta, SCHEMA_VERSION,
};
use serde::Serialize;
use serde_json::json;
use std::path::{Path, PathBuf};
use sysinfo::{Pid, ProcessesToUpdate, System};

#[derive(Parser, Debug)]
#[command(name = "benchmark_compare")]
#[command(
    about = "Sample MacJet vs Activity Monitor CPU and RSS (sysinfo, same as in-app telemetry)"
)]
struct Args {
    /// Stop after this many samples (then exit). Alias: `--samples`
    #[arg(
        long = "max-samples",
        visible_alias = "samples",
        short = 's',
        default_value_t = 300
    )]
    max_samples: usize,

    /// Seconds between samples. Alias: `--refresh` (CPU usage needs ~1 s for stable readings)
    #[arg(
        long = "interval-secs",
        visible_alias = "refresh",
        short = 'r',
        default_value_t = 1.0,
        value_name = "SECS"
    )]
    interval_secs: f64,

    /// JSON output path (default: `benchmarks/results/benchmark_compare_<unix_ts>.json` under the
    /// MacJet repo root when found, else `./benchmarks/results/` under the current directory)
    #[arg(long)]
    output: Option<std::path::PathBuf>,

    /// Same flag as `macjet --no-ml` for convenience (this binary has no ML). When set, prints a
    /// one-line hint: start the MacJet UI with `--no-ml` so the sampled process matches.
    #[arg(long = "no-ml", visible_alias = "noML", default_value_t = false)]
    no_ml: bool,
}

#[derive(Serialize)]
struct SampleRow {
    index: usize,
    macjet_cpu_percent: f64,
    macjet_rss_mb: f64,
    activity_monitor_cpu_percent: Option<f64>,
    activity_monitor_rss_mb: Option<f64>,
}

#[derive(Serialize)]
struct Summary {
    samples: usize,
    interval_secs: f64,
    macjet_cpu_avg: f64,
    macjet_cpu_p95: f64,
    macjet_cpu_max: f64,
    macjet_rss_avg_mb: f64,
    macjet_rss_p95_mb: f64,
    macjet_rss_max_mb: f64,
    activity_monitor_cpu_avg: Option<f64>,
    activity_monitor_cpu_p95: Option<f64>,
    activity_monitor_cpu_max: Option<f64>,
    activity_monitor_rss_avg_mb: Option<f64>,
    activity_monitor_rss_p95_mb: Option<f64>,
    activity_monitor_rss_max_mb: Option<f64>,
}

fn find_macjet_pid(sys: &System) -> Option<Pid> {
    sys.processes()
        .iter()
        .filter(|(_, p)| p.name().eq_ignore_ascii_case("macjet"))
        .max_by_key(|(_, p)| p.memory())
        .map(|(pid, _)| *pid)
}

fn find_activity_monitor_pid(sys: &System) -> Option<Pid> {
    sys.processes()
        .iter()
        .find(|(_, p)| p.name() == "Activity Monitor")
        .map(|(pid, _)| *pid)
}

fn percentile(sorted: &mut [f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let idx = ((sorted.len() - 1) as f64 * (p / 100.0)).round() as usize;
    sorted[idx.min(sorted.len() - 1)]
}

fn mean(xs: &[f64]) -> f64 {
    if xs.is_empty() {
        return 0.0;
    }
    xs.iter().sum::<f64>() / xs.len() as f64
}

/// Sidecar updated after each sample (`benchmark_compare_<ts>.progress.json` next to final JSON).
fn progress_path(final_json: &Path) -> PathBuf {
    final_json.with_extension("progress.json")
}

#[derive(Serialize)]
struct ProgressSnapshot {
    schema_version: u32,
    samples_completed: usize,
    max_samples: usize,
    interval_secs: f64,
    /// Approximate seconds left (mostly sleep between remaining samples).
    eta_seconds_remaining: f64,
    wall_seconds_so_far: f64,
    started_at_utc: String,
    final_json_path: String,
    pid: u32,
}

fn write_progress_file(path: &Path, snap: &ProgressSnapshot) {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            let _ = std::fs::create_dir_all(parent);
        }
    }
    let tmp = path.with_extension("progress.json.partial");
    let Ok(json) = serde_json::to_string_pretty(snap) else {
        return;
    };
    if std::fs::write(&tmp, json).is_ok() {
        let _ = std::fs::rename(&tmp, path);
    }
}

fn tqdm_style_bar(total: u64) -> ProgressBar {
    let pb = ProgressBar::with_draw_target(Some(total), ProgressDrawTarget::stderr());
    pb.set_style(
        ProgressStyle::with_template(
            "{spinner:.green.bold} [{elapsed_precise}] \
             [{wide_bar:.cyan/blue}] {percent:>3}%  \
             {human_pos:>4}/{human_len:4}  \
             {msg:.dim}  \
             ETA {eta_precise}",
        )
        .unwrap()
        .progress_chars("█▓▒░  "),
    );
    pb.set_message("benchmark_compare");
    // Updates the clock/elapsed display during long `sleep` gaps between samples.
    pb.enable_steady_tick(Duration::from_secs(1));
    pb
}

fn main() {
    let args = Args::parse();
    if args.max_samples == 0 {
        eprintln!("--max-samples must be > 0");
        std::process::exit(1);
    }

    if args.no_ml {
        eprintln!(
            "benchmark_compare: --no-ml is accepted for convenience (this tool does not run ML). \
             Start MacJet in another terminal with ML disabled, e.g.: \
             cargo run --release -- --no-ml --refresh {}  (or: ./macjet.sh --no-ml --refresh {})",
            args.interval_secs, args.interval_secs
        );
    }

    let mut sys = System::new();
    let interval = Duration::from_secs_f64(args.interval_secs);
    let started_at_utc = utc_now_rfc3339_millis();
    let argv: Vec<String> = std::env::args().collect();

    let run_unix_ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let resolved_final_path = match &args.output {
        Some(p) => {
            let p = p.clone();
            if let Some(parent) = p.parent() {
                if !parent.as_os_str().is_empty() {
                    if let Err(e) = std::fs::create_dir_all(parent) {
                        eprintln!(
                            "benchmark_compare: failed to create {} for --output: {}",
                            parent.display(),
                            e
                        );
                        std::process::exit(1);
                    }
                }
            }
            p
        }
        None => resolve_default_benchmark_path(default_benchmark_json_path(run_unix_ts)),
    };
    let progress_file = progress_path(&resolved_final_path);

    println!(
        "benchmark_compare: {} samples every {:.1}s (~{:.0}s of sampling sleeps + overhead)",
        args.max_samples,
        args.interval_secs,
        (args.max_samples.saturating_sub(1)) as f64 * args.interval_secs
    );
    println!("Looking for processes named `macjet` and `Activity Monitor`…");

    let mut rows: Vec<SampleRow> = Vec::with_capacity(args.max_samples);
    let mut mj_cpu: Vec<f64> = Vec::new();
    let mut mj_rss: Vec<f64> = Vec::new();
    let mut am_cpu: Vec<f64> = Vec::new();
    let mut am_rss: Vec<f64> = Vec::new();

    let started = Instant::now();
    let use_tty_bar = stderr().is_terminal()
        || std::env::var_os("MACJET_BENCHMARK_PROGRESS").is_some_and(|v| v != "0");
    let mut progress_bar: Option<ProgressBar> = None;

    for i in 0..args.max_samples {
        // Sleep *between* samples, not before the first — otherwise long `--refresh` values look
        // like a hang right after "Looking for processes…".
        if i > 0 {
            thread::sleep(interval);
        }

        sys.refresh_cpu_usage();
        sys.refresh_processes(ProcessesToUpdate::All, true);

        let mj_pid = find_macjet_pid(&sys);
        let am_pid = find_activity_monitor_pid(&sys);

        let (mj_c, mj_m) = mj_pid
            .and_then(|pid| sys.process(pid))
            .map(|p| (p.cpu_usage() as f64, p.memory() as f64 / (1024.0 * 1024.0)))
            .unwrap_or((f64::NAN, f64::NAN));

        let (am_c, am_m) = am_pid
            .and_then(|pid| sys.process(pid))
            .map(|p| {
                (
                    Some(p.cpu_usage() as f64),
                    Some(p.memory() as f64 / (1024.0 * 1024.0)),
                )
            })
            .unwrap_or((None, None));

        if i == 0 {
            match mj_pid {
                Some(p) => println!("MacJet PID: {} (cpu% / RSS MB will be sampled)", p),
                None => eprintln!(
                    "Warning: no process named `macjet` found — values will be NaN. Run MacJet first."
                ),
            }
            match am_pid {
                Some(p) => println!("Activity Monitor PID: {}", p),
                None => eprintln!(
                    "Warning: Activity Monitor not running — AM columns will be null in JSON."
                ),
            }
            if use_tty_bar {
                progress_bar = Some(tqdm_style_bar(args.max_samples as u64));
                eprintln!(
                    "benchmark_compare: tqdm-style progress on stderr (1s refresh); live JSON → {}",
                    progress_file.display()
                );
            }
        }

        if mj_c.is_finite() {
            mj_cpu.push(mj_c);
            mj_rss.push(mj_m);
        }
        if let (Some(c), Some(m)) = (am_c, am_m) {
            if c.is_finite() {
                am_cpu.push(c);
                am_rss.push(m);
            }
        }

        rows.push(SampleRow {
            index: i,
            macjet_cpu_percent: mj_c,
            macjet_rss_mb: mj_m,
            activity_monitor_cpu_percent: am_c,
            activity_monitor_rss_mb: am_m,
        });

        let done = i + 1;
        let eta = (args.max_samples.saturating_sub(done)) as f64 * args.interval_secs;
        write_progress_file(
            &progress_file,
            &ProgressSnapshot {
                schema_version: SCHEMA_VERSION,
                samples_completed: done,
                max_samples: args.max_samples,
                interval_secs: args.interval_secs,
                eta_seconds_remaining: eta,
                wall_seconds_so_far: started.elapsed().as_secs_f64(),
                started_at_utc: started_at_utc.clone(),
                final_json_path: resolved_final_path.display().to_string(),
                pid: std::process::id(),
            },
        );

        if let Some(ref pb) = progress_bar {
            pb.set_position(done as u64);
        } else if i == 0 && args.interval_secs >= 5.0 && args.max_samples > 1 {
            eprintln!(
                "benchmark_compare: {:.0}s between samples ({} samples remaining after this one).",
                args.interval_secs,
                args.max_samples - 1
            );
            eprintln!(
                "benchmark_compare: stderr is not a TTY — logging every 30 samples instead of a live bar."
            );
        }

        if !use_tty_bar && args.interval_secs >= 5.0 && args.max_samples > 1 {
            let milestone = done % 30 == 0 || done == args.max_samples;
            if milestone {
                eprintln!(
                    "benchmark_compare: {}/{} samples | {:.0}s elapsed | ~{:.0}s remaining…",
                    done,
                    args.max_samples,
                    started.elapsed().as_secs_f64(),
                    eta
                );
            }
        }
    }

    if let Some(pb) = progress_bar {
        pb.finish_and_clear();
    }

    let elapsed = started.elapsed();

    let summary = Summary {
        samples: args.max_samples,
        interval_secs: args.interval_secs,
        macjet_cpu_avg: mean(&mj_cpu),
        macjet_cpu_p95: percentile(&mut mj_cpu.clone(), 95.0),
        macjet_cpu_max: mj_cpu.iter().copied().fold(0.0, f64::max),
        macjet_rss_avg_mb: mean(&mj_rss),
        macjet_rss_p95_mb: percentile(&mut mj_rss.clone(), 95.0),
        macjet_rss_max_mb: mj_rss.iter().copied().fold(0.0, f64::max),
        activity_monitor_cpu_avg: if am_cpu.is_empty() {
            None
        } else {
            Some(mean(&am_cpu))
        },
        activity_monitor_cpu_p95: if am_cpu.is_empty() {
            None
        } else {
            Some(percentile(&mut am_cpu.clone(), 95.0))
        },
        activity_monitor_cpu_max: if am_cpu.is_empty() {
            None
        } else {
            Some(am_cpu.iter().copied().fold(0.0, f64::max))
        },
        activity_monitor_rss_avg_mb: if am_rss.is_empty() {
            None
        } else {
            Some(mean(&am_rss))
        },
        activity_monitor_rss_p95_mb: if am_rss.is_empty() {
            None
        } else {
            Some(percentile(&mut am_rss.clone(), 95.0))
        },
        activity_monitor_rss_max_mb: if am_rss.is_empty() {
            None
        } else {
            Some(am_rss.iter().copied().fold(0.0, f64::max))
        },
    };

    println!("\n=== Summary (sysinfo, % of one core for CPU) ===");
    println!("Wall time: {:.1}s", elapsed.as_secs_f64());
    println!(
        "MacJet           avg CPU {:>6.2}%  P95 {:>6.2}%  max {:>6.2}%",
        summary.macjet_cpu_avg, summary.macjet_cpu_p95, summary.macjet_cpu_max
    );
    println!(
        "MacJet           avg RSS {:>8.1} MB  P95 {:>8.1} MB  max {:>8.1} MB",
        summary.macjet_rss_avg_mb, summary.macjet_rss_p95_mb, summary.macjet_rss_max_mb
    );
    if let Some(a) = summary.activity_monitor_cpu_avg {
        println!(
            "Activity Monitor avg CPU {:>6.2}%  P95 {:>6.2}%  max {:>6.2}%",
            a,
            summary.activity_monitor_cpu_p95.unwrap_or(0.0),
            summary.activity_monitor_cpu_max.unwrap_or(0.0)
        );
        println!(
            "Activity Monitor avg RSS {:>8.1} MB  P95 {:>8.1} MB  max {:>8.1} MB",
            summary.activity_monitor_rss_avg_mb.unwrap_or(0.0),
            summary.activity_monitor_rss_p95_mb.unwrap_or(0.0),
            summary.activity_monitor_rss_max_mb.unwrap_or(0.0)
        );
    } else {
        println!("Activity Monitor: (no samples — app not running?)");
    }

    let finished_at_utc = utc_now_rfc3339_millis();
    let run = RunMeta {
        started_at_utc,
        finished_at_utc,
        wall_seconds: elapsed.as_secs_f64(),
        argv,
        max_samples: args.max_samples,
        interval_secs: args.interval_secs,
        no_ml_flag: args.no_ml,
    };

    let system = collect_system_fingerprint(&mut sys);

    let mut out = json!({
        "summary": summary,
        "samples": rows,
    });
    out = merge_report_shell(out, tool_meta(), run, system);

    let path = resolved_final_path;

    match std::fs::write(&path, serde_json::to_string_pretty(&out).unwrap()) {
        Ok(()) => {
            println!("\nWrote {}", path.display());
            let _ = std::fs::remove_file(&progress_file);
        }
        Err(e) => eprintln!("Failed to write {}: {}", path.display(), e),
    }
}
