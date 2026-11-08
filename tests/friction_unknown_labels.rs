//! Unknown friction labels stay visible: a stored category the classifier
//! does not know (typo, schema drift) must warn and count in the `failures`
//! view instead of silently joining `Other`. End to end through the real
//! binary on a synthetic store: every path touched is synthetic
//! (`tests/windowed_flag_vocabulary.rs` contract).

use std::path::{Path, PathBuf};

use saniwalens::core::span::SessionStart;
use saniwalens::store::{IngestBatch, OpenMode, SessionMeta, Store};

mod common;
use common::ScratchDir;

fn scratch_dir(name: &str) -> ScratchDir {
    let dir = ScratchDir::new("saniwalens-unknown-label", name);
    std::fs::create_dir_all(dir.join(".claude").join("projects")).unwrap();
    dir
}

fn session(id: &str, project: &str, epoch_secs: i64) -> SessionMeta {
    SessionMeta {
        id: id.to_string(),
        project: project.to_string(),
        slug: project.to_string(),
        root: String::new(),
        source_path: format!("/tmp/{id}.jsonl"),
        sub_tokens: 0,
        sub_agent_count: 0,
        start: Some(SessionStart {
            timestamp_ms: epoch_secs * 1000,
            ctx: 9_000,
        }),
    }
}

/// One known and one unknown stored category in the same project.
fn write_store(dir: &Path) -> PathBuf {
    let db = dir.join("store.db");
    {
        let mut store = Store::open(&db, OpenMode::ReadWrite).unwrap();
        let meta = session("demo-session", "demo", 100_000);
        store
            .ingest(IngestBatch::new(&meta, &[], &[]).tool_errors(&[
                (
                    100_000_000,
                    "edit-precondition",
                    "String to replace not found",
                    "Edit",
                    "main.rs",
                ),
                (
                    100_001_000,
                    "edit-preconditon",
                    "a typo of a real category",
                    "Edit",
                    "other.rs",
                ),
            ]))
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
fn an_unknown_stored_category_warns_on_stderr_but_stays_listed() {
    let dir = scratch_dir("warns-but-listed");
    let db = write_store(&dir);
    let out = run(&dir, &db, &["failures"]);

    assert!(out.status.success(), "failures exits 0: {out:?}");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    // The known row renders as usual.
    assert!(
        stdout.contains("edit-precondition"),
        "the known category is listed: {stdout:?}"
    );
    // The typo row is still listed (lenient: nothing is dropped) ...
    assert!(
        stdout.contains("edit-preconditon"),
        "the unknown category stays listed: {stdout:?}"
    );
    // ... and its unknown-ness is warned about, not silent.
    assert!(
        stderr.contains("edit-preconditon"),
        "the unknown label is warned on stderr: {stderr:?}"
    );
}

#[test]
fn an_unknown_stored_category_is_counted_in_json() {
    let dir = scratch_dir("counted-in-json");
    let db = write_store(&dir);
    let out = run(&dir, &db, &["failures", "--format", "json"]);

    assert!(
        out.status.success(),
        "failures --format json exits 0: {out:?}"
    );
    let json: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&out.stdout)).expect("the contract is JSON");
    assert_eq!(json["unknown_labels"]["count"], 1);
    assert_eq!(
        json["unknown_labels"]["labels"],
        serde_json::json!(["edit-preconditon"])
    );
    // Known routing is unchanged: the real category still aggregates.
    let categories: Vec<&str> = json["failures"]
        .as_array()
        .expect("failures is an array")
        .iter()
        .map(|row| row["category"].as_str().unwrap_or_default())
        .collect();
    assert!(categories.contains(&"edit-precondition"));
    assert!(categories.contains(&"edit-preconditon"));
}
