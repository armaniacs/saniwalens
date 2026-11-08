//! Never-fires wedge end to end (PBI 04): a path-conditional rule whose globs
//! matched nothing read in-window is flagged with its globs as evidence; a
//! rule with a matching read, a globless rule, and a window with no reads at
//! all stay silent. Synthetic store, real binary.

use std::path::{Path, PathBuf};

use saniwalens::core::span::SessionStart;
use saniwalens::core::surface::{LoadMode, Scope, Surface};
use saniwalens::store::{IngestBatch, OpenMode, SessionMeta, Store};

mod common;
use common::ScratchDir;

fn scratch_dir(name: &str) -> ScratchDir {
    let dir = ScratchDir::new("saniwalens-never-fires", name);
    std::fs::create_dir_all(dir.join(".claude").join("projects")).unwrap();
    dir
}

fn rule(id: &str, globs: &[&str]) -> Surface {
    Surface {
        kind: "rule".to_string(),
        id: id.to_string(),
        scope: Scope::Global,
        config_path: format!("/tmp/rules/{id}.md"),
        static_tokens: Some(1200),
        startup_tokens: Some(0),
        load_mode: LoadMode::PathConditional,
        paths: globs.iter().map(|g| g.to_string()).collect(),
    }
}

fn session(id: &str) -> SessionMeta {
    SessionMeta {
        id: id.to_string(),
        project: "demo".to_string(),
        slug: "demo".to_string(),
        root: String::new(),
        source_path: format!("/tmp/{id}.jsonl"),
        sub_tokens: 0,
        sub_agent_count: 0,
        start: Some(SessionStart {
            timestamp_ms: 100_000_000,
            ctx: 9_000,
        }),
    }
}

/// One used rule, one idle rule, one globless rule; a single read of
/// `/repo/src/main.rs` in-window.
fn write_store(dir: &Path) -> PathBuf {
    let db = dir.join("store.db");
    {
        let mut store = Store::open(&db, OpenMode::ReadWrite).unwrap();
        store
            .replace_surfaces(&[
                rule("used-rule", &["src/**"]),
                rule("idle-rule", &["docs/**"]),
                rule("globless-rule", &[]),
            ])
            .unwrap();
        let meta = session("s1");
        store
            .ingest(IngestBatch::new(&meta, &[], &[]).work_events(&[(
                100_000_000,
                "file_read",
                "main.rs",
                Some("/repo/src/main.rs"),
            )]))
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
fn an_unmatched_rule_is_flagged_with_its_globs() {
    let dir = scratch_dir("flagged");
    let db = write_store(&dir);
    let out = run(&dir, &db, &["waste", "--frozen"]);

    assert!(out.status.success(), "waste exits 0: {out:?}");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("NEVER-FIRES") && stdout.contains("idle-rule"),
        "the idle rule is flagged: {stdout}"
    );
    assert!(
        stdout.contains("docs/**"),
        "the globs ride along as evidence: {stdout}"
    );
    assert!(
        !stdout.contains("used-rule"),
        "a rule with a matching read stays silent: {stdout}"
    );
    assert!(
        !stdout.contains("globless-rule"),
        "a rule with no globs is never flagged: {stdout}"
    );
}

#[test]
fn a_window_without_reads_flags_nothing() {
    // A quiet window is no evidence: without read targets the wedge stays
    // out, even for globs that match nothing.
    let dir = scratch_dir("quiet");
    let db = dir.join("store.db");
    {
        let mut store = Store::open(&db, OpenMode::ReadWrite).unwrap();
        store
            .replace_surfaces(&[rule("idle-rule", &["docs/**"])])
            .unwrap();
        let meta = session("s1");
        store.ingest(IngestBatch::new(&meta, &[], &[])).unwrap();
    }
    let out = run(&dir, &db, &["waste", "--format", "json", "--frozen"]);

    assert!(out.status.success(), "waste json exits 0: {out:?}");
    let json: serde_json::Value = String::from_utf8_lossy(&out.stdout)
        .parse()
        .expect("valid JSON");
    let wedges = json["wedges"].as_array().expect("wedges is an array");
    assert!(
        !wedges.iter().any(|w| w["wedge"] == "NEVER-FIRES"),
        "no read targets means no never-fires: {json}"
    );
}

#[test]
fn the_json_row_carries_globs_and_body_cost() {
    let dir = scratch_dir("json");
    let db = write_store(&dir);
    let out = run(&dir, &db, &["waste", "--format", "json", "--frozen"]);

    assert!(out.status.success(), "waste json exits 0: {out:?}");
    let json: serde_json::Value = String::from_utf8_lossy(&out.stdout)
        .parse()
        .expect("valid JSON");
    let rows: Vec<_> = json["wedges"]
        .as_array()
        .expect("wedges is an array")
        .iter()
        .filter(|w| w["wedge"] == "NEVER-FIRES")
        .collect();
    assert_eq!(rows.len(), 1, "exactly the idle rule: {json}");
    assert_eq!(rows[0]["id"], "idle-rule");
    assert!(
        rows[0]["suggestion"]
            .as_str()
            .unwrap_or_default()
            .contains("docs/**"),
        "the suggestion names the unmatched globs: {json}"
    );
}
