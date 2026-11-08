//! Sessions view, end to end (PBI 13): per-session tokens, duration, and
//! friction with the default tokens-desc sort, the `--sort` cuts, friction-0
//! rows that stay listed, and the JSON machine contract. Synthetic store only.

use std::path::{Path, PathBuf};

use saniwalens::core::span::SessionStart;
use saniwalens::core::usage::UsageEvent;
use saniwalens::store::{IngestBatch, OpenMode, SessionMeta, Store};

mod common;
use common::ScratchDir;

fn scratch_dir(name: &str) -> ScratchDir {
    let dir = ScratchDir::new("saniwalens-sessions", name);
    std::fs::create_dir_all(dir.join(".claude").join("projects")).unwrap();
    dir
}

fn session(id: &str, sub_tokens: i64) -> SessionMeta {
    SessionMeta {
        id: id.to_string(),
        project: "demo".to_string(),
        slug: format!("demo-{id}"),
        root: String::new(),
        source_path: format!("/tmp/{id}.jsonl"),
        sub_tokens,
        sub_agent_count: 0,
        start: Some(SessionStart {
            timestamp_ms: 1_700_000_000_000,
            ctx: 9_000,
        }),
    }
}

fn span_out(skill: &str, ms: i64, out: u64, dur: f64) -> saniwalens::core::span::Span {
    saniwalens::core::span::Span {
        skill: skill.to_string(),
        source: saniwalens::core::span::Source::Slash,
        started_epoch_ms: ms,
        duration_sec: dur,
        out_tokens: out,
        ctx_growth: 0,
        ctx_start: 0,
        ctx_peak: 0,
        model: None,
        is_trailing: false,
        spawns: vec![],
        sub_tokens: 0,
        sub_agent_count: 0,
        sub_tokens_estimated: false,
    }
}

/// s1: 100 span tokens + 2 tool errors. s2: quiet (50 subagent tokens, no
/// events, no friction — must still list with zeros).
fn write_store(dir: &Path) -> PathBuf {
    let db = dir.join("store.db");
    {
        let mut store = Store::open(&db, OpenMode::ReadWrite).unwrap();
        let usage = |id: &str| UsageEvent {
            surface_kind: "skill".to_string(),
            surface_id: id.to_string(),
            started_epoch_ms: 1_700_000_100_000,
            tool_use_id: None,
        };
        store
            .ingest(
                IngestBatch::new(
                    &session("s1", 30),
                    &[span_out("a", 1_700_000_100_000, 100, 60.0)],
                    &[usage("a")],
                )
                .tool_errors(&[
                    (1_700_000_100_000, "timeout", "t", "Bash", "sleep 9"),
                    (1_700_000_200_000, "timeout", "t", "Bash", "sleep 9"),
                ]),
            )
            .unwrap();
        store
            .ingest(IngestBatch::new(&session("s2", 50), &[], &[]))
            .unwrap();
    }
    db
}

fn run(dir: &Path, db: &Path, args: &[&str]) -> std::process::Output {
    std::process::Command::new(env!("CARGO_BIN_EXE_saniwalens"))
        .args(args)
        .arg("--db")
        .arg(db)
        .env("HOME", dir)
        .env("CLAUDE_CONFIG_DIR", dir.join(".claude"))
        .env("XDG_STATE_HOME", dir.join("state"))
        .env_remove("XDG_CONFIG_HOME")
        .env_remove("XDG_DATA_HOME")
        .output()
        .expect("the binary runs")
}

#[test]
fn sessions_sort_tokens_descending_by_default() {
    // s1: 100 span + 30 sub = 130 tokens, 2 friction. s2: 50 tokens, 0 friction.
    let dir = scratch_dir("default-sort");
    let db = write_store(&dir);
    let out = run(&dir, &db, &["sessions", "--format", "json", "--frozen"]);

    assert!(out.status.success(), "sessions exits 0: {out:?}");
    let json: serde_json::Value = String::from_utf8_lossy(&out.stdout)
        .parse()
        .expect("valid JSON");
    let sessions = json["sessions"].as_array().expect("sessions array");
    assert_eq!(sessions.len(), 2);
    assert_eq!(sessions[0]["session"], "s1");
    assert_eq!(sessions[0]["out_tokens"], 130);
    assert_eq!(sessions[0]["friction"], 2);
    assert_eq!(sessions[1]["session"], "s2");
    assert_eq!(sessions[1]["out_tokens"], 50);
    assert_eq!(
        sessions[1]["friction"], 0,
        "quiet rows read 0, never vanish"
    );
}

#[test]
fn sort_cuts_reorder_and_stay_stable() {
    let dir = scratch_dir("sort-cuts");
    let db = write_store(&dir);
    // By friction s2 (0) still lists, last.
    let out = run(
        &dir,
        &db,
        &[
            "sessions", "--format", "json", "--sort", "friction", "--frozen",
        ],
    );
    let json: serde_json::Value = String::from_utf8_lossy(&out.stdout)
        .parse()
        .expect("valid JSON");
    let ids: Vec<&str> = json["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["session"].as_str().unwrap())
        .collect();
    assert_eq!(ids, vec!["s1", "s2"]);
    assert!(out.status.success());
    // Unknown cut fails with the list, like --by.
    let out = run(&dir, &db, &["sessions", "--sort", "bogus", "--frozen"]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("tokens|duration|friction"));
}

#[test]
fn the_table_names_sessions_and_their_shape() {
    let dir = scratch_dir("table");
    let db = write_store(&dir);
    let out = run(&dir, &db, &["sessions", "--frozen"]);

    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("session"), "header: {stdout}");
    assert!(
        stdout.contains("s1") && stdout.contains("s2"),
        "both rows: {stdout}"
    );
}
