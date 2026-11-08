//! Deterministic analyze-pipeline benchmark (PBI 27): parse + span
//! extraction + usage attribution + in-memory ingest over a synthetic
//! transcript, timed over N iterations with the median reported.
//!
//! Why an example, not criterion: criterion is not vendored in this offline
//! environment, so the PBI's letter (a `criterion` dev-dependency under
//! `benches/`) is not implementable here without network. This keeps the
//! measurable part — a fixed fixture, a repeatable driver, a single number
//! per run — with the standard library only. Like criterion benches it never
//! runs in CI (`cargo test` only builds examples) and never touches real
//! transcripts, real config, or real databases.
//!
//! Usage: `cargo run --release --example bench_analyze [SESSIONS] [ITERS]`
//! (defaults 200 sessions, 5 iterations).

use std::time::{Duration, Instant};

use saniwalens::adapter::transcript;
use saniwalens::core::span;
use saniwalens::core::usage;
use saniwalens::store::{IngestBatch, SessionMeta, Store};

/// One synthetic session: a slash invocation, two assistant cost records,
/// and a closing human turn. Fixed timestamps and text, so every run
/// measures the same work.
fn session_jsonl(id: usize) -> String {
    let base = 1_700_000_000_000i64 + id as i64 * 60_000;
    let ts = |off: i64| {
        // RFC3339 millis the parser accepts; arithmetic stays in range.
        let ms = base + off;
        format!(
            "2023-11-14T22:{:02}:{:02}.{:03}Z",
            (ms / 60_000 % 60) as u32,
            (ms / 1_000 % 60) as u32,
            (ms % 1_000) as u32
        )
    };
    [
        format!(
            "{{\"type\":\"user\",\"timestamp\":\"{}\",\"message\":{{\"content\":\"<command-name>/git-commit</command-name>\"}}}}",
            ts(0)
        ),
        format!(
            "{{\"type\":\"assistant\",\"timestamp\":\"{}\",\"message\":{{\"model\":\"m\",\"usage\":{{\"input_tokens\":10,\"cache_read_input_tokens\":0,\"cache_creation_input_tokens\":0,\"output_tokens\":40}}}}}}",
            ts(1_000)
        ),
        format!(
            "{{\"type\":\"user\",\"timestamp\":\"{}\",\"message\":{{\"content\":\"thanks\"}}}}",
            ts(2_000)
        ),
    ]
    .join("\n")
}

fn session_meta(id: usize) -> SessionMeta {
    SessionMeta {
        id: format!("bench-{id}"),
        project: "bench".to_string(),
        slug: "bench".to_string(),
        root: String::new(),
        source_path: format!("/tmp/bench-{id}.jsonl"),
        sub_tokens: 0,
        sub_agent_count: 0,
        start: None,
    }
}

fn one_pass(sessions: &[String]) -> Duration {
    let start = Instant::now();
    let mut store = Store::in_memory().unwrap();
    for (i, jsonl) in sessions.iter().enumerate() {
        let records = transcript::parse_session(jsonl);
        let spans = span::extract_spans(&records, span::DEFAULT_IDLE_GAP_MS);
        let usage = usage::extract_usage_events(&records);
        let meta = session_meta(i);
        store
            .ingest(IngestBatch::new(&meta, &spans, &usage))
            .unwrap();
    }
    // Read back, like a report would: the pipeline is parse-to-query.
    let _ = store.query("SELECT COUNT(*) FROM events").unwrap();
    start.elapsed()
}

fn median(mut xs: Vec<Duration>) -> Duration {
    xs.sort();
    xs[xs.len() / 2]
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let n_sessions: usize = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(200);
    let iters: usize = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(5);
    let sessions: Vec<String> = (0..n_sessions).map(session_jsonl).collect();
    // Warm up once so the timed runs do not include first-touch costs.
    one_pass(&sessions);
    let mut times = Vec::with_capacity(iters);
    for _ in 0..iters {
        times.push(one_pass(&sessions));
    }
    let med = median(times.clone());
    println!(
        "bench_analyze: {n_sessions} sessions x {iters} iters, median {:?} ({:.2} ms/session)",
        med,
        med.as_secs_f64() * 1000.0 / n_sessions as f64
    );
}
