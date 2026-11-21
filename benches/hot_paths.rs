//! Criterion benches for the three hot paths that carried real
//! performance regressions: whole-analyze over a large transcript (the
//! main loop), hostile glob matching against many targets (PBI
//! 2026-11-21-21 — 258 s on the old matcher), and the kilocode billing
//! window attribution over skewed task files (PBI 2026-11-21-23 —
//! quadratic on the old scan). Run with `cargo bench`.
//!
//! Fixtures are synthetic (privacy rule) and built once per bench via
//! `OnceLock`, with per-iteration work isolated by `iter_batched` so
//! setup (fresh store paths, file writes) is not measured.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use criterion::{BatchSize, Criterion, criterion_group, criterion_main};

/// One synthetic transcript large enough to exercise the ingest loop:
/// 100k user/assistant alternating lines (~10 MB), the shape of a
/// long-lived session, all timestamps well inside the calendar.
fn big_transcript() -> &'static str {
    static TEXT: OnceLock<String> = OnceLock::new();
    TEXT.get_or_init(|| {
        let mut text = String::with_capacity(10 << 20);
        for i in 0..50_000u64 {
            let (day, minute, second) = (1 + (i / 1440) % 28, i / 60 % 60, i % 60);
            text.push_str(&format!(
                r#"{{"type":"user","cwd":"/tmp/example/proj","timestamp":"2026-01-{day:02}T00:{minute:02}:{second:02}.000Z","message":{{"content":"<command-name>/run</command-name>"}}}}"#
            ));
            text.push('\n');
            text.push_str(&format!(
                r#"{{"type":"assistant","timestamp":"2026-01-{day:02}T00:{minute:02}:{second:02}.500Z","message":{{"model":"claude-opus-4-7","usage":{{"input_tokens":10,"cache_read_input_tokens":90,"cache_creation_input_tokens":0,"output_tokens":40}}}}}}"#
            ));
            text.push('\n');
        }
        text
    })
}

fn claude_fixture(root: &Path) -> PathBuf {
    let projects = root.join("projects/-tmp-example-proj");
    std::fs::create_dir_all(&projects).unwrap();
    let transcript = projects.join("aaaaaaaa-0000-0000-0000-000000000001.jsonl");
    std::fs::write(&transcript, big_transcript()).unwrap();
    transcript
}

fn bench_analyze_claude(c: &mut Criterion) {
    c.bench_function("analyze_claude_100k_lines", |b| {
        b.iter_batched(
            || {
                let root = bench_root("analyze");
                let transcript = claude_fixture(&root);
                let db = root.join("store.db");
                (root, transcript, db)
            },
            |(root, transcript, db)| {
                let projects = root.join("projects");
                let stats = saniwalens::adapter::claude::analyze(Some(projects), &db).unwrap();
                drop(transcript);
                std::fs::remove_dir_all(&root).ok();
                stats
            },
            BatchSize::LargeInput,
        );
    });
}

/// The hostile shape from PBI 2026-11-21-21: `**` chains against a deeply
/// segmented target, past the point the matcher's predecessors went
/// exponential.
fn bench_glob_hostile(c: &mut Criterion) {
    c.bench_function("glob_hostile_star_chains", |b| {
        let pattern = vec!["src/**/x/**/y/**/z/**/**/a.md"];
        let path = "/repo/".to_string() + &vec!["s"; 26].join("/");
        b.iter(|| saniwalens::core::glob_match::matches_any(&pattern, &path));
    });
}

/// The skewed shape from PBI 2026-11-21-23: 100k api_req_started rows all
/// stamped after the single assistant message — every window scan walks the
/// whole assistant list, the worst case for the pre-binary-search scan.
fn bench_kilocode_window(c: &mut Criterion) {
    c.bench_function("kilocode_window_skewed_100k", |b| {
        b.iter_batched(
            || {
                let root = bench_root("kilocode");
                let tasks = root.join("tasks/task-1");
                std::fs::create_dir_all(&tasks).unwrap();
                std::fs::write(
                    tasks.join("api_conversation_history.json"),
                    r#"[{"role":"user","ts":1700000000000,"content":[{"type":"text","text":"fix it"}]},{"role":"assistant","ts":1700000001000,"content":[{"type":"text","text":"ok"}]}]"#,
                )
                .unwrap();
                let mut ui = String::with_capacity(16 << 20);
                ui.push('[');
                for i in 0..100_000u64 {
                    if i > 0 {
                        ui.push(',');
                    }
                    ui.push_str(&format!(
                        r#"{{"ts":{},"type":"say","say":"api_req_started","text":"{{\"tokensIn\":10,\"tokensOut\":4}}"}}"#,
                        1_700_000_002_000 + i,
                    ));
                }
                ui.push(']');
                std::fs::write(tasks.join("ui_messages.json"), ui).unwrap();
                let db = root.join("store.db");
                (root, tasks, db)
            },
            |(root, tasks, db)| {
                let stats = saniwalens::adapter::kilocode::analyze(Some(tasks), &db).unwrap();
                std::fs::remove_dir_all(&root).ok();
                stats
            },
            BatchSize::LargeInput,
        );
    });
}
/// A unique per-call temp root under the system temp dir: benches and tests
/// may run side by side, so the pid keeps the paths apart. Created
/// owner-only (0700) so the store inside stays private and PBI 25's
/// already-existing-directory warning never fires per iteration.
fn bench_root(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("saniwalens-bench-{name}-{}", std::process::id()));
    if !path.exists() {
        std::fs::create_dir_all(&path).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
    }
    path
}

criterion_group!(
    benches,
    bench_analyze_claude,
    bench_glob_hostile,
    bench_kilocode_window
);
criterion_main!(benches);
