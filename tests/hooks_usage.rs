//! Hook usage end to end (PBI 21): hook firings recorded from transcript
//! attachments join their catalogued hook surfaces — a fired hook is not
//! `UNUSED`, an unfired one is. Synthetic settings + transcript only.

use std::path::{Path, PathBuf};

use saniwalens::adapter::config::read_hook_surfaces;
use saniwalens::adapter::transcript::{extract_hook_usages, parse_session};
use saniwalens::core::span::SessionStart;
use saniwalens::core::surface::Scope;
use saniwalens::store::{IngestBatch, OpenMode, SessionMeta, Store};

mod common;
use common::ScratchDir;

fn scratch_dir(name: &str) -> ScratchDir {
    let dir = ScratchDir::new("saniwalens-hook-usage", name);
    std::fs::create_dir_all(dir.join(".claude").join("projects")).unwrap();
    dir
}

fn write_fixture(dir: &Path) -> (PathBuf, PathBuf) {
    let settings = dir.join("settings.json");
    std::fs::write(
        &settings,
        r#"{"hooks":{"PreToolUse":[{"matcher":"Bash","hooks":[{"type":"command","command":"check.sh"}]}]}}"#,
    )
    .unwrap();
    // One firing hook line plus one human line so the session has a start.
    let transcript = dir.join("s1.jsonl");
    std::fs::write(
        &transcript,
        concat!(
            r#"{"type":"attachment","timestamp":"2026-10-14T20:06:16.756Z","attachment":{"type":"hook_success","hookName":"PreToolUse:Bash","hookEvent":"PreToolUse","toolUseID":"toolu_1"}}"#,
            "\n",
            r#"{"type":"user","timestamp":"2026-10-14T20:06:17.000Z","message":{"content":"go"}}"#,
            "\n",
        ),
    )
    .unwrap();
    (settings, transcript)
}

fn write_store(dir: &Path, settings: &Path, transcript: &Path) -> PathBuf {
    let text = std::fs::read_to_string(transcript).unwrap();
    let records = parse_session(&text);
    let mut usage = saniwalens::core::usage::extract_usage_events(&records);
    usage.extend(extract_hook_usages(&text));
    let db = dir.join("store.db");
    {
        let mut store = Store::open(&db, OpenMode::ReadWrite).unwrap();
        let mut surfaces = read_hook_surfaces(settings, &Scope::Global);
        // A configured-but-never-fired hook stays measurable-but-unused.
        surfaces.push(saniwalens::adapter::config::hook_surface(
            "PreToolUse:Silent",
            &settings.display().to_string(),
            "quiet.sh",
            &Scope::Global,
        ));
        store.replace_surfaces(&surfaces).unwrap();
        let meta = SessionMeta {
            id: "s1".into(),
            project: "demo".into(),
            slug: "demo".into(),
            root: String::new(),
            source_path: transcript.display().to_string(),
            sub_tokens: 0,
            sub_agent_count: 0,
            start: Some(SessionStart {
                timestamp_ms: 1_760_469_600_000,
                ctx: 9_000,
            }),
        };
        store.ingest(IngestBatch::new(&meta, &[], &usage)).unwrap();
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
fn a_fired_hook_is_used_and_a_quiet_one_is_unused() {
    let dir = scratch_dir("fired-vs-quiet");
    let (settings, transcript) = write_fixture(&dir);
    let db = write_store(&dir, &settings, &transcript);
    let out = run(&dir, &db, &["waste", "--format", "json", "--frozen"]);

    assert!(out.status.success(), "waste json exits 0: {out:?}");
    let json: serde_json::Value = String::from_utf8_lossy(&out.stdout)
        .parse()
        .expect("valid JSON");
    let unused: Vec<&str> = json["wedges"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|w| w["wedge"] == "UNUSED")
        .map(|w| w["id"].as_str().unwrap())
        .collect();
    assert!(
        unused.contains(&"PreToolUse:Silent"),
        "the never-fired hook is unused: {json}"
    );
    assert!(
        !unused.contains(&"PreToolUse:Bash"),
        "the fired hook is not unused: {json}"
    );
}
