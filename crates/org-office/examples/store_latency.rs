//! Office store latency benchmark and CI regression guard (`TASK-33` / B1c, ADR-0014).
//!
//! Uses only `org-office`'s public engine-neutral `OfficeStore` API with `EngineChoice::Sqlite`
//! (zero SQLite driver references, complying with ADR-0018 ENG-1).
//!
//! Usage:
//!   cargo run --release -p org-office --features testkit --example store_latency -- \
//!     --iterations 500 --check-regression

use std::env;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use org_office::office_local::testkit::ca_material;
use org_office::organization_family::features::CreateTeam;
use org_office::organization_family::testkit::{creation_command, CREATION_AT};
use org_office::{
    open_store, CreateOrganizationOp, CreateTeamOp, DirectorySnapshot, EngineChoice, OfficeEngine,
    OfficeStore, StoreConfig, StoreOp,
};
use org_store::testkit::{id, identity, FakeCustody, FixedClock, Probe};
use org_store::{Ack, Actor, AnchorFacts, Opening, Outcome, Reply, Store, StoreError};
use serde::Serialize;

const CREATOR: u64 = 0x12;
const TOP_TEAM: u64 = 0x11;

struct Args {
    iterations: usize,
    durable_iterations: usize,
    flush_ms: u64,
    check_regression: bool,
    max_read_p95_us: u64,
    max_committed_p50_us: u64,
    max_committed_p95_us: u64,
    json_path: Option<PathBuf>,
    markdown_path: Option<PathBuf>,
}

impl Default for Args {
    fn default() -> Self {
        Self {
            iterations: 500,
            durable_iterations: 40,
            flush_ms: 50,
            check_regression: false,
            // Generous CI ceilings catching real regressions without flaking on shared runners.
            max_read_p95_us: 5_000,
            max_committed_p50_us: 5_000,
            max_committed_p95_us: 25_000,
            json_path: None,
            markdown_path: None,
        }
    }
}

#[derive(Serialize)]
struct Stats {
    samples: usize,
    min_us: u64,
    p50_us: u64,
    p95_us: u64,
    p99_us: u64,
    max_us: u64,
}

#[derive(Serialize)]
struct Report {
    os: String,
    arch: String,
    flush_interval_ms: u64,
    snapshot_read: Stats,
    committed_write: Stats,
    durable_org_write: Stats,
    continuous_load_flushed: bool,
}

struct CommittedProbeOp<'a> {
    probe: &'a Probe,
    name: String,
    body: Vec<u8>,
}

impl StoreOp for CommittedProbeOp<'_> {
    type Out = Result<Reply, StoreError>;

    fn run<E: OfficeEngine>(self, store: &Store<E>) -> Self::Out {
        let decide = self.probe.put::<E>(&self.name, &self.body, Ack::Committed);
        store.submit(None, false, decide)
    }
}

fn parse_args() -> Result<Args, String> {
    let mut args = Args::default();
    let mut it = env::args().skip(1);
    while let Some(flag) = it.next() {
        apply_flag(&mut args, &flag, &mut it)?;
    }
    if args.iterations == 0 || args.durable_iterations == 0 || args.flush_ms == 0 {
        return Err("--iterations, --durable-iterations, and --flush-ms must be >= 1".into());
    }
    Ok(args)
}

fn apply_flag(
    args: &mut Args,
    flag: &str,
    it: &mut impl Iterator<Item = String>,
) -> Result<(), String> {
    match flag {
        "--iterations" => args.iterations = parse_next(it, flag)?,
        "--durable-iterations" => args.durable_iterations = parse_next(it, flag)?,
        "--flush-ms" => args.flush_ms = parse_next(it, flag)?,
        "--check-regression" => args.check_regression = true,
        "--json" => args.json_path = Some(PathBuf::from(next_val(it, flag)?)),
        "--markdown" => args.markdown_path = Some(PathBuf::from(next_val(it, flag)?)),
        other => apply_ceiling_flag(args, other, it)?,
    }
    Ok(())
}

fn apply_ceiling_flag(
    args: &mut Args,
    flag: &str,
    it: &mut impl Iterator<Item = String>,
) -> Result<(), String> {
    match flag {
        "--max-read-p95-us" => args.max_read_p95_us = parse_next(it, flag)?,
        "--max-committed-p50-us" => args.max_committed_p50_us = parse_next(it, flag)?,
        "--max-committed-p95-us" => args.max_committed_p95_us = parse_next(it, flag)?,
        other => return Err(format!("unknown argument: {other}")),
    }
    Ok(())
}

fn next_val(it: &mut impl Iterator<Item = String>, flag: &str) -> Result<String, String> {
    it.next().ok_or_else(|| format!("missing value for {flag}"))
}

fn parse_next<T: std::str::FromStr>(
    it: &mut impl Iterator<Item = String>,
    flag: &str,
) -> Result<T, String> {
    next_val(it, flag)?
        .parse::<T>()
        .map_err(|_| format!("invalid integer for {flag}"))
}

fn summarize(mut samples: Vec<u64>) -> Stats {
    if samples.is_empty() {
        return Stats {
            samples: 0,
            min_us: 0,
            p50_us: 0,
            p95_us: 0,
            p99_us: 0,
            max_us: 0,
        };
    }
    samples.sort_unstable();
    let n = samples.len();
    let pct = |p: usize| samples[(n.saturating_sub(1) * p) / 100];
    Stats {
        samples: n,
        min_us: samples[0],
        p50_us: pct(50),
        p95_us: pct(95),
        p99_us: pct(99),
        max_us: samples[n - 1],
    }
}

fn open_bench_store(root: &std::path::Path, flush_ms: u64, probe: &Probe) -> OfficeStore {
    let custody = Arc::new(FakeCustody::new());
    let clock = Arc::new(FixedClock(CREATION_AT));
    let mut cfg = StoreConfig::new(
        EngineChoice::Sqlite,
        root.join("durable"),
        root.join("runtime"),
        identity(),
        custody,
        clock,
    );
    cfg.opening = Opening::Create(AnchorFacts::default());
    cfg.flush_interval = Duration::from_millis(flush_ms);
    cfg.testing.probe = Some(probe.clone());
    let store = open_store(&cfg).expect("open bench store");
    let ca = ca_material(&identity().organization_id, 1);
    store
        .run(CreateOrganizationOp {
            operation_id: id(1),
            here: identity(),
            command: creation_command(),
            ca,
        })
        .expect("seed organization");
    store.sync().expect("sync baseline");
    store
}

fn bench_reads(store: &OfficeStore, n: usize) -> Stats {
    let mut samples = Vec::with_capacity(n);
    for _ in 0..n {
        let t0 = Instant::now();
        let _ = store
            .read(DirectorySnapshot { owner: id(CREATOR) })
            .expect("snapshot read");
        samples.push(t0.elapsed().as_micros() as u64);
    }
    summarize(samples)
}

fn bench_committed_writes(store: &OfficeStore, probe: &Probe, n: usize) -> Stats {
    let mut samples = Vec::with_capacity(n);
    for i in 0..n {
        let t0 = Instant::now();
        let reply = store
            .run(CommittedProbeOp {
                probe,
                name: format!("committed-{i}"),
                body: format!("bench-payload-{i}").into_bytes(),
            })
            .expect("committed write");
        assert_eq!(reply.outcome, Outcome::Completed);
        samples.push(t0.elapsed().as_micros() as u64);
    }
    summarize(samples)
}

fn bench_durable_writes(store: &OfficeStore, n: usize) -> Stats {
    let mut samples = Vec::with_capacity(n);
    for i in 0..n {
        let seq = 0x5000 + i as u64;
        let t0 = Instant::now();
        let reply = store
            .run(CreateTeamOp {
                operation_id: id(0x9000 + i as u64),
                here: identity(),
                owner: id(CREATOR),
                by: Actor::Employee(id(CREATOR)),
                command: CreateTeam {
                    id: id(seq),
                    name: format!("Bench Team {i}"),
                    parent: Some(id(TOP_TEAM)),
                },
            })
            .expect("durable team create");
        assert_eq!(reply.outcome, Outcome::Completed);
        samples.push(t0.elapsed().as_micros() as u64);
    }
    summarize(samples)
}

/// Verifies `ACK-5`: `status().flushed` advances *during* a continuous committed-write stream
/// without waiting for an idle pause, using a 5 s outer ceiling to tolerate slow macOS fsyncs.
fn verify_continuous_flush_bound(store: &OfficeStore, probe: &Probe, flush_ms: u64) -> bool {
    store.sync().expect("sync before flush check");
    let before = store.status().flushed;
    let deadline =
        Instant::now() + Duration::from_secs(5).max(Duration::from_millis(flush_ms * 10));
    let mut i = 0u64;
    while Instant::now() < deadline {
        i += 1;
        let _ = store
            .run(CommittedProbeOp {
                probe,
                name: format!("flush-load-{i}"),
                body: b"tick".to_vec(),
            })
            .expect("flush load write");
        if store.status().flushed > before {
            return true;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    false
}

fn render_markdown(report: &Report) -> String {
    format!(
        "### Office store latency ({}/{}, flush_interval={} ms)\n\n\
         | Operation | Samples | p50 (ms) | p95 (ms) | p99 (ms) | max (ms) |\n\
         | --- | ---: | ---: | ---: | ---: | ---: |\n\
         | Snapshot read (`DirectorySnapshot`) | {} | {:.3} | {:.3} | {:.3} | {:.3} |\n\
         | Committed write (`Ack::Committed`) | {} | {:.3} | {:.3} | {:.3} | {:.3} |\n\
         | Durable org write (`CreateTeamOp`) | {} | {:.3} | {:.3} | {:.3} | {:.3} |\n\n\
         Continuous committed load flush-bound check (`ACK-5`): **{}**\n",
        report.os,
        report.arch,
        report.flush_interval_ms,
        report.snapshot_read.samples,
        report.snapshot_read.p50_us as f64 / 1000.0,
        report.snapshot_read.p95_us as f64 / 1000.0,
        report.snapshot_read.p99_us as f64 / 1000.0,
        report.snapshot_read.max_us as f64 / 1000.0,
        report.committed_write.samples,
        report.committed_write.p50_us as f64 / 1000.0,
        report.committed_write.p95_us as f64 / 1000.0,
        report.committed_write.p99_us as f64 / 1000.0,
        report.committed_write.max_us as f64 / 1000.0,
        report.durable_org_write.samples,
        report.durable_org_write.p50_us as f64 / 1000.0,
        report.durable_org_write.p95_us as f64 / 1000.0,
        report.durable_org_write.p99_us as f64 / 1000.0,
        report.durable_org_write.max_us as f64 / 1000.0,
        if report.continuous_load_flushed {
            "PASS"
        } else {
            "FAIL"
        },
    )
}

fn check_thresholds(args: &Args, report: &Report) -> Result<(), String> {
    if report.snapshot_read.p95_us > args.max_read_p95_us {
        return Err(format!(
            "snapshot read p95 {} us exceeded ceiling {} us",
            report.snapshot_read.p95_us, args.max_read_p95_us
        ));
    }
    if report.committed_write.p50_us > args.max_committed_p50_us {
        return Err(format!(
            "committed write p50 {} us exceeded ceiling {} us",
            report.committed_write.p50_us, args.max_committed_p50_us
        ));
    }
    if report.committed_write.p95_us > args.max_committed_p95_us {
        return Err(format!(
            "committed write p95 {} us exceeded ceiling {} us",
            report.committed_write.p95_us, args.max_committed_p95_us
        ));
    }
    if !report.continuous_load_flushed {
        return Err(
            "continuous committed load did not advance flushed ordinal within bound".into(),
        );
    }
    Ok(())
}

fn main() {
    org_office_sqlite::install();
    let args = parse_args().unwrap_or_else(|e| {
        eprintln!("store_latency: {e}");
        std::process::exit(2);
    });
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path().canonicalize().expect("canonical tempdir");
    let probe = Probe::new(&identity().office_id);
    let store = open_bench_store(&root, args.flush_ms, &probe);

    let snapshot_read = bench_reads(&store, args.iterations);
    let committed_write = bench_committed_writes(&store, &probe, args.iterations);
    let durable_org_write = bench_durable_writes(&store, args.durable_iterations);
    let continuous_load_flushed = verify_continuous_flush_bound(&store, &probe, args.flush_ms);
    store.close().expect("clean close");

    let report = Report {
        os: env::consts::OS.to_owned(),
        arch: env::consts::ARCH.to_owned(),
        flush_interval_ms: args.flush_ms,
        snapshot_read,
        committed_write,
        durable_org_write,
        continuous_load_flushed,
    };
    let md = render_markdown(&report);
    println!("{md}");

    if let Some(path) = &args.json_path {
        let json = serde_json::to_string_pretty(&report).expect("json report");
        fs::write(path, json).expect("write json report");
    }
    if let Some(path) = &args.markdown_path {
        fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .and_then(|mut f| std::io::Write::write_all(&mut f, md.as_bytes()))
            .expect("append markdown report");
    }
    if args.check_regression {
        if let Err(reason) = check_thresholds(&args, &report) {
            eprintln!("store_latency REGRESSION: {reason}");
            std::process::exit(1);
        }
    }
}
