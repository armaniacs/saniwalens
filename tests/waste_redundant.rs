//! Redundant wedge, end to end (PBI 15): skills whose descriptions overlap
//! enough are flagged with each other as evidence; dissimilar surfaces stay
//! silent. Synthetic catalog only — no transcripts needed, since the signal
//! is config-side.

use std::path::{Path, PathBuf};

use saniwalens::core::span::SessionStart;
use saniwalens::core::surface::{LoadMode, Scope, Surface};
use saniwalens::store::{IngestBatch, OpenMode, SessionMeta, Store};

mod common;
use common::ScratchDir;

fn scratch_dir(name: &str) -> ScratchDir {
    let dir = ScratchDir::new("saniwalens-redundant", name);
    std::fs::create_dir_all(dir.join(".claude").join("projects")).unwrap();
    dir
}

fn skill(id: &str, description: &str) -> Surface {
    Surface {
        kind: "skill".to_string(),
        id: id.to_string(),
        scope: Scope::Global,
        config_path: format!("/tmp/skills/{id}/SKILL.md"),
        static_tokens: Some(400),
        startup_tokens: Some(40),
        load_mode: LoadMode::StartupDescription,
        paths: Vec::new(),
        description: Some(description.to_string()),
    }
}

fn write_store(dir: &Path) -> PathBuf {
    let db = dir.join("store.db");
    {
        let mut store = Store::open(&db, OpenMode::ReadWrite).unwrap();
        store
            .replace_surfaces(&[
                skill("git-commit", "Write git commit messages with hooks"),
                skill("git-push", "Write git commits with hooks"),
                skill("weather", "Current weather forecast"),
            ])
            .unwrap();
        let meta = SessionMeta {
            id: "s1".into(),
            project: "demo".into(),
            slug: "demo".into(),
            root: String::new(),
            source_path: "/tmp/s1.jsonl".into(),
            sub_tokens: 0,
            sub_agent_count: 0,
            start: Some(SessionStart {
                timestamp_ms: 1_700_000_000_000,
                ctx: 9_000,
            }),
        };
        store.ingest(IngestBatch::new(&meta, &[], &[])).unwrap();
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
fn overlapping_descriptions_are_flagged_with_each_other() {
    let dir = scratch_dir("flagged");
    let db = write_store(&dir);
    let out = run(&dir, &db, &["waste", "--format", "json", "--frozen"]);

    assert!(out.status.success(), "waste json exits 0: {out:?}");
    let json: serde_json::Value = String::from_utf8_lossy(&out.stdout)
        .parse()
        .expect("valid JSON");
    let rows: Vec<_> = json["wedges"]
        .as_array()
        .expect("wedges array")
        .iter()
        .filter(|w| w["wedge"] == "REDUNDANT")
        .collect();
    assert_eq!(rows.len(), 2, "both doubles are listed: {json}");
    let ids: Vec<&str> = rows.iter().map(|w| w["id"].as_str().unwrap()).collect();
    assert!(ids.contains(&"git-commit") && ids.contains(&"git-push"));
    for row in &rows {
        assert!(
            row["suggestion"]
                .as_str()
                .unwrap_or_default()
                .contains("redundant with"),
            "the counterpart is named: {row}"
        );
    }
    assert!(
        !json["wedges"]
            .as_array()
            .unwrap()
            .iter()
            .any(|w| w["id"] == "weather" && w["wedge"] == "REDUNDANT"),
        "the dissimilar skill stays silent: {json}"
    );
}

#[test]
fn the_table_names_the_pair() {
    let dir = scratch_dir("table");
    let db = write_store(&dir);
    let out = run(&dir, &db, &["waste", "--frozen"]);

    assert!(out.status.success(), "waste exits 0: {out:?}");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("REDUNDANT"), "the wedge renders: {stdout}");
    assert!(
        stdout.contains("git-commit") && stdout.contains("git-push"),
        "both doubles render: {stdout}"
    );
}
