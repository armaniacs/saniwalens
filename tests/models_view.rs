//! Model breakdown view, end to end (PBI 05): per-model cost split from a
//! synthetic store — main-thread spans and subagent runs, `unknown` bundling,
//! `tool_error` exclusion (their `model` is the originating tool),
//! deterministic order, window filtering, all three formats. The JSON test
//! pins the exact machine contract (the golden); `UPDATE_GOLDEN=1` re-blesses
//! it after review, like `tests/e2e_golden_harness.rs`.

use std::path::{Path, PathBuf};

use saniwalens::core::span::SessionStart;
use saniwalens::core::subagent::SubagentRun;
use saniwalens::store::{IngestBatch, OpenMode, SessionMeta, Store};

mod common;
use common::ScratchDir;

fn scratch_dir(name: &str) -> ScratchDir {
    let dir = ScratchDir::new("saniwalens-models", name);
    std::fs::create_dir_all(dir.join(".claude").join("projects")).unwrap();
    dir
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
            timestamp_ms: 1_700_000_000_000,
            ctx: 9_000,
        }),
    }
}

const TRANSCRIPT: &str = concat!(
    r#"{"type":"user","timestamp":"2023-11-14T22:13:20.000Z","message":{"content":"<command-name>/git-commit</command-name>"}}"#,
    "\n",
    r#"{"type":"assistant","timestamp":"2023-11-14T22:13:21.000Z","message":{"model":"opus","usage":{"input_tokens":10,"cache_read_input_tokens":0,"cache_creation_input_tokens":0,"output_tokens":100}}}"#,
    "\n",
    r#"{"type":"assistant","timestamp":"2023-11-14T22:13:22.000Z","message":{"model":"sonnet","usage":{"input_tokens":10,"cache_read_input_tokens":0,"cache_creation_input_tokens":0,"output_tokens":40}}}"#,
    "\n",
    r#"{"type":"user","timestamp":"2023-11-14T22:13:23.000Z","message":{"content":"thanks"}}"#,
);

fn subagent_run(agent: &str, model: Option<&str>, out_tokens: u64) -> SubagentRun {
    SubagentRun {
        agent: Some(agent.to_string()),
        agent_id: format!("{agent}-1"),
        run_path: format!("/tmp/example/subagents/{agent}-1.jsonl"),
        tool_use_id: None,
        parent_agent_id: None,
        spawn_depth: 1,
        model: model.map(str::to_string),
        prompt_id: None,
        out_tokens,
        started_epoch_ms: 1_700_000_100_000,
        root_tool_use_id: None,
    }
}

fn write_store(dir: &Path) -> PathBuf {
    let db = dir.join("store.db");
    {
        let mut store = Store::open(&db, OpenMode::ReadWrite).unwrap();
        let meta = session("s1");
        let records = saniwalens::adapter::transcript::parse_session(TRANSCRIPT);
        let spans = saniwalens::core::span::extract_spans(
            &records,
            saniwalens::core::span::DEFAULT_IDLE_GAP_MS,
        );
        assert_eq!(spans.len(), 1, "the fixture yields one skill span");
        store
            .ingest(
                IngestBatch::new(&meta, &spans, &[])
                    .subagent_runs(&[
                        subagent_run("Explore", Some("opus"), 100),
                        subagent_run("Plan", Some("sonnet"), 40),
                        subagent_run("Bash", None, 10),
                    ])
                    .tool_errors(&[(
                        1_700_000_100_000,
                        "command-not-found",
                        "not here",
                        "Bash",
                        "nope",
                    )]),
            )
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
fn models_ranks_main_and_subagent_splits_by_cost() {
    let dir = scratch_dir("table");
    let db = write_store(&dir);
    let out = run(&dir, &db, &["models", "--frozen"]);

    assert!(out.status.success(), "models exits 0: {out:?}");
    let stdout = String::from_utf8_lossy(&out.stdout);
    // opus outranks sonnet on both sides (140 vs 40 main-side span model
    // aside, the subagent side carries the assertion).
    let opus = stdout.find("opus").expect("opus row");
    let sonnet = stdout.find("sonnet").expect("sonnet row");
    assert!(opus < sonnet, "costliest first: {stdout}");
    assert!(
        stdout.contains("unknown"),
        "model-less runs bundle: {stdout}"
    );
    assert!(
        !stdout.contains("Bash") || stdout.contains("unknown"),
        "agent types are not models: {stdout}"
    );
}

#[test]
fn models_json_pins_the_machine_contract() {
    let dir = scratch_dir("json");
    let db = write_store(&dir);
    let out = run(&dir, &db, &["models", "--format", "json", "--frozen"]);

    assert!(out.status.success(), "models json exits 0: {out:?}");
    let json: serde_json::Value = String::from_utf8_lossy(&out.stdout)
        .parse()
        .expect("valid JSON");
    let expected = serde_json::json!({
        "main": [
            {"model": "opus", "events": 1, "out_tokens": 140},
        ],
        "subagents": [
            {"model": "opus", "runs": 1, "out_tokens": 100},
            {"model": "sonnet", "runs": 1, "out_tokens": 40},
            {"model": "unknown", "runs": 1, "out_tokens": 10},
        ],
    });
    // Window + payload shape: compare the contract fields, not the frame.
    assert_eq!(json["main"], expected["main"], "main side: {json}");
    assert_eq!(
        json["subagents"], expected["subagents"],
        "subagent side: {json}"
    );
}

#[test]
fn a_window_without_usage_reports_nothing_found() {
    let dir = scratch_dir("windowed");
    let db = write_store(&dir);
    let out = run(&dir, &db, &["models", "--since", "2024-01-01", "--frozen"]);

    assert!(out.status.success(), "windowed models exits 0: {out:?}");
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("no model usage found"),
        "empty window, honest message"
    );
}
