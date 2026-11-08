//! Skill candidates from repeated prompts (PBI 12), end to end: identical
//! template shapes repeated past the threshold surface as candidates with a
//! count, an exemplar, and their sources; rotated/missing transcripts count
//! as pruned instead of failing the view. Synthetic store + transcripts only.

use std::path::{Path, PathBuf};

use saniwalens::core::span::SessionStart;
use saniwalens::store::{IngestBatch, OpenMode, SessionMeta, Store};

mod common;
use common::ScratchDir;

fn scratch_dir(name: &str) -> ScratchDir {
    let dir = ScratchDir::new("saniwalens-candidates", name);
    std::fs::create_dir_all(dir.join(".claude").join("projects")).unwrap();
    dir
}

fn session(id: &str, source_path: &Path) -> SessionMeta {
    SessionMeta {
        id: id.to_string(),
        project: "demo".to_string(),
        slug: "demo".to_string(),
        root: String::new(),
        source_path: source_path.display().to_string(),
        sub_tokens: 0,
        sub_agent_count: 0,
        start: Some(SessionStart {
            timestamp_ms: 1_700_000_000_000,
            ctx: 9_000,
        }),
    }
}

fn user_line(text: &str) -> String {
    format!(
        "{{\"type\":\"user\",\"timestamp\":\"2023-11-14T22:13:20.000Z\",\"message\":{{\"content\":{}}}}}",
        serde_json::to_string(text).unwrap()
    )
}

/// Transcript 1: the same fix-typo shape three times (two spellings apart
/// only by target file) plus a one-off. Transcript 2 is absent (rotated).
fn write_fixtures(dir: &Path) -> PathBuf {
    let t1 = dir.join("s1.jsonl");
    let lines = [
        user_line("fix the typo in foo.rs"),
        user_line("buy milk"),
        user_line("fix the typo in bar.rs"),
        user_line("fix the typo in baz.rs"),
    ];
    std::fs::write(&t1, lines.join("\n")).unwrap();
    let db = dir.join("store.db");
    {
        let mut store = Store::open(&db, OpenMode::ReadWrite).unwrap();
        let meta = session("s1", &t1);
        store
            .ingest(IngestBatch::new(&meta, &[], &[]).prompts(&[
                (0, 1_700_000_000_000, "instruct"),
                (2, 1_700_000_100_000, "instruct"),
                (3, 1_700_000_200_000, "instruct"),
            ]))
            .unwrap();
        // A pointer into a transcript that no longer exists: pruned, not fatal.
        let meta2 = session("gone", &dir.join("gone.jsonl"));
        store
            .ingest(IngestBatch::new(&meta2, &[], &[]).prompts(&[(
                0,
                1_700_000_300_000,
                "instruct",
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
fn repeated_prompts_surface_as_a_candidate() {
    let dir = scratch_dir("candidate");
    let db = write_fixtures(&dir);
    let out = run(&dir, &db, &["prompts", "--format", "json", "--frozen"]);

    assert!(out.status.success(), "prompts json exits 0: {out:?}");
    let json: serde_json::Value = String::from_utf8_lossy(&out.stdout)
        .parse()
        .expect("valid JSON");
    let cands = json["candidates"].as_array().expect("candidates array");
    assert_eq!(cands.len(), 1, "one repeated shape: {json}");
    assert_eq!(cands[0]["template"], "fix the typo in <PATH>");
    assert_eq!(cands[0]["count"], 3);
    assert!(
        cands[0]["example"]
            .as_str()
            .unwrap()
            .contains("fix the typo in"),
        "the exemplar keeps the user's own words: {json}"
    );
    assert_eq!(json["pruned_prompts"], 1, "the rotated ref is counted");
}

#[test]
fn the_table_names_the_candidate_and_the_gap() {
    let dir = scratch_dir("table");
    let db = write_fixtures(&dir);
    let out = run(&dir, &db, &["prompts", "--frozen"]);

    assert!(out.status.success(), "prompts exits 0: {out:?}");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("fix the typo in <PATH>") && stdout.contains('3'),
        "the candidate row shows template and count: {stdout}"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("rotated or missing") || stdout.contains("rotated or missing"),
        "the rotated ref is disclosed, not silent: {stderr} / {stdout}"
    );
}
