//! mod-split goldens (PBI C6): the six views still rendered from `cli/mod.rs`
//! get the same byte-pinning the five migrated views carry in
//! `tests/render_view_golden.rs`, so the mechanical move into
//! `src/cli/views/` proves byte-identical output.
//!
//! Each case runs the real binary against a synthetic store and pins stdout
//! byte-for-byte (pipes carry no ANSI codes, fixtures carry fixed epochs, so
//! the bytes are stable). `UPDATE_GOLDEN=1` re-blesses after review, like
//! `tests/render_view_golden.rs`. Only stdout is pinned: stderr carries the
//! freshness line and the window disclosure, whose coverage lives in the
//! window tests. Windowed cases additionally assert the disclosure line.

use std::path::{Path, PathBuf};

use saniwalens::core::span::{SessionStart, Source, Span};
use saniwalens::core::subagent::SubagentRun;
use saniwalens::core::surface::{LoadMode, Scope, Surface};
use saniwalens::store::{IngestBatch, OpenMode, SessionMeta, Store};

mod common;
use common::ScratchDir;

fn scratch_dir(name: &str) -> ScratchDir {
    let dir = ScratchDir::new("saniwalens-mod-split", name);
    std::fs::create_dir_all(dir.join(".claude").join("projects")).unwrap();
    dir
}

fn golden_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("golden")
        .join("mod_split_views")
        .join(name)
}

fn check_golden(name: &str, actual: &str) {
    let path = golden_path(name);
    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, actual).unwrap();
        return;
    }
    let expected =
        std::fs::read_to_string(&path).unwrap_or_else(|_| panic!("missing golden {name}"));
    assert_eq!(
        actual, expected,
        "golden mismatch for {name}: re-run with UPDATE_GOLDEN=1 after reviewing the diff"
    );
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

fn check_stdout(dir: &Path, db: &Path, args: &[&str], golden: &str) -> std::process::Output {
    let out = run(dir, db, args);
    assert!(out.status.success(), "{} exits 0: {out:?}", args.join(" "));
    check_golden(golden, &String::from_utf8_lossy(&out.stdout));
    out
}

const S1_START_MS: i64 = 1_700_000_000_000;

fn session(id: &str, project: &str, sub_tokens: i64, start_ms: i64) -> SessionMeta {
    SessionMeta {
        id: id.to_string(),
        project: project.to_string(),
        slug: project.to_string(),
        root: String::new(),
        source_path: format!("/tmp/{id}.jsonl"),
        sub_tokens,
        sub_agent_count: 0,
        start: Some(SessionStart {
            timestamp_ms: start_ms,
            ctx: 12_000,
        }),
    }
}

fn span(skill: &str, out_tokens: u64, epoch_ms: i64) -> Span {
    Span {
        skill: skill.to_string(),
        source: Source::Slash,
        started_epoch_ms: epoch_ms,
        duration_sec: 2.0,
        out_tokens,
        ctx_growth: 5,
        ctx_start: 0,
        ctx_peak: 5,
        model: None,
        is_trailing: false,
        spawns: vec![],
        sub_tokens: 0,
        sub_agent_count: 0,
        sub_tokens_estimated: false,
    }
}

/// A store with nothing ingested: every view's empty path, byte-pinned.
fn fresh_store(dir: &Path) -> PathBuf {
    let db = dir.join("store.db");
    drop(Store::open(&db, OpenMode::ReadWrite).unwrap());
    db
}

fn skill_surface(id: &str) -> Surface {
    Surface {
        kind: "skill".to_string(),
        id: id.to_string(),
        scope: Scope::Global,
        config_path: format!("/tmp/skills/{id}/SKILL.md"),
        static_tokens: Some(500),
        startup_tokens: Some(100),
        load_mode: LoadMode::OnDemand,
        paths: vec![],
        description: Some(format!("{id} does things")),
    }
}

// ------------------------------------------------------------------- usage ---

fn usage_store(dir: &Path) -> PathBuf {
    let db = dir.join("store.db");
    {
        let mut store = Store::open(&db, OpenMode::ReadWrite).unwrap();
        let meta = session("s1", "demo", 500, S1_START_MS);
        store
            .ingest(
                IngestBatch::new(
                    &meta,
                    &[
                        span("commit", 300, S1_START_MS + 100_000),
                        span("review", 100, S1_START_MS + 200_000),
                    ],
                    &[],
                )
                .subagent_runs(&[
                    SubagentRun {
                        agent: Some("Explore".to_string()),
                        agent_id: "Explore-1".to_string(),
                        run_path: "/tmp/example/subagents/Explore-1.jsonl".to_string(),
                        tool_use_id: None,
                        parent_agent_id: None,
                        spawn_depth: 1,
                        model: Some("opus".to_string()),
                        prompt_id: None,
                        out_tokens: 400,
                        started_epoch_ms: S1_START_MS + 100_000,
                        root_tool_use_id: None,
                    },
                    SubagentRun {
                        agent: Some("Plan".to_string()),
                        agent_id: "Plan-1".to_string(),
                        run_path: "/tmp/example/subagents/Plan-1.jsonl".to_string(),
                        tool_use_id: None,
                        parent_agent_id: None,
                        spawn_depth: 1,
                        model: None,
                        prompt_id: None,
                        out_tokens: 40,
                        started_epoch_ms: S1_START_MS + 100_000,
                        root_tool_use_id: None,
                    },
                ]),
            )
            .unwrap();
    }
    db
}

#[test]
fn usage_table_pins_the_current_bytes() {
    let dir = scratch_dir("usage-table");
    let db = usage_store(&dir);
    check_stdout(&dir, &db, &["usage", "--frozen"], "usage_table.txt");
}

#[test]
fn usage_markdown_pins_the_current_bytes() {
    let dir = scratch_dir("usage-markdown");
    let db = usage_store(&dir);
    check_stdout(
        &dir,
        &db,
        &["usage", "--format", "markdown", "--frozen"],
        "usage_markdown.txt",
    );
}

#[test]
fn usage_json_pins_the_current_bytes() {
    let dir = scratch_dir("usage-json");
    let db = usage_store(&dir);
    check_stdout(
        &dir,
        &db,
        &["usage", "--format", "json", "--frozen"],
        "usage_json.txt",
    );
}

#[test]
fn usage_json_with_window_pins_the_window_key() {
    let dir = scratch_dir("usage-json-window");
    let db = usage_store(&dir);
    let out = check_stdout(
        &dir,
        &db,
        &[
            "usage",
            "--format",
            "json",
            "--since",
            "2023-11-01",
            "--until",
            "2023-11-30",
            "--frozen",
        ],
        "usage_json_window.txt",
    );
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("report window:"),
        "the window disclosure stays on stderr: {out:?}"
    );
}

#[test]
fn usage_empty_window_pins_the_empty_message() {
    let dir = scratch_dir("usage-empty-window");
    let db = usage_store(&dir);
    check_stdout(
        &dir,
        &db,
        &[
            "usage",
            "--since",
            "2024-01-01",
            "--until",
            "2024-01-02",
            "--frozen",
        ],
        "usage_empty_window.txt",
    );
}

#[test]
fn usage_by_month_pins_the_current_bytes() {
    let dir = scratch_dir("usage-by-month");
    let db = usage_store(&dir);
    check_stdout(
        &dir,
        &db,
        &["usage", "--by", "month", "--frozen"],
        "usage_by_month.txt",
    );
}

// --------------------------------------------------------------- inventory ---

fn inventory_store(dir: &Path) -> PathBuf {
    let db = dir.join("store.db");
    {
        let mut store = Store::open(&db, OpenMode::ReadWrite).unwrap();
        store
            .replace_surfaces(&[skill_surface("used-skill"), skill_surface("idle-skill")])
            .unwrap();
        let usage = saniwalens::core::usage::UsageEvent {
            surface_kind: "skill".to_string(),
            surface_id: "used-skill".to_string(),
            started_epoch_ms: S1_START_MS,
            tool_use_id: None,
        };
        store
            .ingest(IngestBatch::new(
                &session("s1", "demo", 0, S1_START_MS),
                &[],
                &[usage],
            ))
            .unwrap();
    }
    db
}

#[test]
fn inventory_table_pins_the_current_bytes() {
    let dir = scratch_dir("inventory-table");
    let db = inventory_store(&dir);
    check_stdout(&dir, &db, &["inventory", "--frozen"], "inventory_table.txt");
}

#[test]
fn inventory_markdown_pins_the_current_bytes() {
    let dir = scratch_dir("inventory-markdown");
    let db = inventory_store(&dir);
    check_stdout(
        &dir,
        &db,
        &["inventory", "--format", "markdown", "--frozen"],
        "inventory_markdown.txt",
    );
}

#[test]
fn inventory_empty_without_catalog_pins_the_empty_message() {
    let dir = scratch_dir("inventory-empty");
    let db = fresh_store(&dir);
    check_stdout(&dir, &db, &["inventory", "--frozen"], "inventory_empty.txt");
}

// ---------------------------------------------------------------- overhead ---

fn overhead_store(dir: &Path) -> PathBuf {
    let db = dir.join("store.db");
    {
        let mut store = Store::open(&db, OpenMode::ReadWrite).unwrap();
        store
            .ingest(IngestBatch::new(
                &session("s1", "demo", 0, S1_START_MS),
                &[],
                &[],
            ))
            .unwrap();
    }
    db
}

#[test]
fn overhead_table_pins_the_current_bytes() {
    let dir = scratch_dir("overhead-table");
    let db = overhead_store(&dir);
    check_stdout(&dir, &db, &["overhead", "--frozen"], "overhead_table.txt");
}

#[test]
fn overhead_json_pins_the_current_bytes() {
    let dir = scratch_dir("overhead-json");
    let db = overhead_store(&dir);
    check_stdout(
        &dir,
        &db,
        &["overhead", "--format", "json", "--frozen"],
        "overhead_json.txt",
    );
}

#[test]
fn overhead_empty_without_data_pins_the_empty_message() {
    let dir = scratch_dir("overhead-empty");
    let db = fresh_store(&dir);
    check_stdout(&dir, &db, &["overhead", "--frozen"], "overhead_empty.txt");
}

// ---------------------------------------------------------------- failures ---

fn failures_store(dir: &Path) -> PathBuf {
    let db = dir.join("store.db");
    {
        let mut store = Store::open(&db, OpenMode::ReadWrite).unwrap();
        store
            .ingest(
                IngestBatch::new(&session("s1", "demo", 0, S1_START_MS), &[], &[]).tool_errors(&[
                    (S1_START_MS + 100_000, "timeout", "t", "Bash", "sleep 9"),
                    (S1_START_MS + 200_000, "timeout", "t", "Bash", "sleep 9"),
                    (S1_START_MS + 300_000, "timeout", "t", "Bash", "sleep 9"),
                ]),
            )
            .unwrap();
    }
    db
}

#[test]
fn failures_table_pins_the_current_bytes() {
    let dir = scratch_dir("failures-table");
    let db = failures_store(&dir);
    check_stdout(&dir, &db, &["failures", "--frozen"], "failures_table.txt");
}

#[test]
fn failures_markdown_pins_the_current_bytes() {
    let dir = scratch_dir("failures-markdown");
    let db = failures_store(&dir);
    check_stdout(
        &dir,
        &db,
        &["failures", "--format", "markdown", "--frozen"],
        "failures_markdown.txt",
    );
}

#[test]
fn failures_json_pins_the_current_bytes() {
    let dir = scratch_dir("failures-json");
    let db = failures_store(&dir);
    check_stdout(
        &dir,
        &db,
        &["failures", "--format", "json", "--frozen"],
        "failures_json.txt",
    );
}

#[test]
fn failures_empty_window_pins_the_empty_message() {
    let dir = scratch_dir("failures-empty-window");
    let db = failures_store(&dir);
    check_stdout(
        &dir,
        &db,
        &[
            "failures",
            "--since",
            "2024-01-01",
            "--until",
            "2024-01-02",
            "--frozen",
        ],
        "failures_empty_window.txt",
    );
}

// ------------------------------------------------------------------- stuck ---

fn stuck_store(dir: &Path) -> PathBuf {
    let db = dir.join("store.db");
    {
        let mut store = Store::open(&db, OpenMode::ReadWrite).unwrap();
        let mut edits: Vec<(i64, &str, &str, Option<&str>)> = Vec::new();
        for i in 0..5 {
            edits.push((
                S1_START_MS + (i as i64) * 60_000,
                "file_edit",
                "main.rs",
                Some("/repo/src/main.rs"),
            ));
        }
        store
            .ingest(
                IngestBatch::new(&session("s1", "demo", 0, S1_START_MS), &[], &[])
                    .work_events(&edits),
            )
            .unwrap();
    }
    db
}

#[test]
fn stuck_table_pins_the_current_bytes() {
    let dir = scratch_dir("stuck-table");
    let db = stuck_store(&dir);
    check_stdout(&dir, &db, &["stuck", "--frozen"], "stuck_table.txt");
}

#[test]
fn stuck_json_pins_the_current_bytes() {
    let dir = scratch_dir("stuck-json");
    let db = stuck_store(&dir);
    check_stdout(
        &dir,
        &db,
        &["stuck", "--format", "json", "--frozen"],
        "stuck_json.txt",
    );
}

#[test]
fn stuck_empty_without_episodes_pins_the_empty_message() {
    let dir = scratch_dir("stuck-empty");
    let db = fresh_store(&dir);
    check_stdout(&dir, &db, &["stuck", "--frozen"], "stuck_empty.txt");
}

// ------------------------------------------------------------------ doctor ---

fn doctor_store(dir: &Path) -> PathBuf {
    let db = dir.join("store.db");
    {
        let mut store = Store::open(&db, OpenMode::ReadWrite).unwrap();
        store
            .replace_surfaces(&[skill_surface("used-skill"), skill_surface("idle-skill")])
            .unwrap();
        let usage = saniwalens::core::usage::UsageEvent {
            surface_kind: "skill".to_string(),
            surface_id: "used-skill".to_string(),
            started_epoch_ms: S1_START_MS + 100_000,
            tool_use_id: None,
        };
        let mut prompt_rows: Vec<(usize, i64, &str)> = Vec::new();
        for i in 0..5 {
            prompt_rows.push((i, S1_START_MS + (i as i64) * 100_000, "instruct"));
        }
        prompt_rows.push((5, S1_START_MS + 500_000, "steer"));
        prompt_rows.push((6, S1_START_MS + 600_000, "steer"));
        prompt_rows.push((7, S1_START_MS + 700_000, "correct"));
        store
            .ingest(
                IngestBatch::new(
                    &session("s1", "demo", 500, S1_START_MS),
                    &[span("used-skill", 300, S1_START_MS + 100_000)],
                    &[usage],
                )
                .tool_errors(&[
                    (S1_START_MS + 100_000, "timeout", "t", "Bash", "sleep 9"),
                    (S1_START_MS + 200_000, "timeout", "t", "Bash", "sleep 9"),
                ])
                .prompts(&prompt_rows)
                .subagent_runs(&[SubagentRun {
                    agent: Some("Explore".to_string()),
                    agent_id: "Explore-1".to_string(),
                    run_path: "/tmp/example/subagents/Explore-1.jsonl".to_string(),
                    tool_use_id: None,
                    parent_agent_id: None,
                    spawn_depth: 1,
                    model: Some("opus".to_string()),
                    prompt_id: None,
                    out_tokens: 400,
                    started_epoch_ms: S1_START_MS + 100_000,
                    root_tool_use_id: None,
                }]),
            )
            .unwrap();
    }
    db
}

#[test]
fn doctor_table_pins_the_current_bytes() {
    let dir = scratch_dir("doctor-table");
    let db = doctor_store(&dir);
    check_stdout(&dir, &db, &["doctor", "--frozen"], "doctor_table.txt");
}

#[test]
fn doctor_json_pins_the_current_bytes() {
    let dir = scratch_dir("doctor-json");
    let db = doctor_store(&dir);
    check_stdout(
        &dir,
        &db,
        &["doctor", "--format", "json", "--frozen"],
        "doctor_json.txt",
    );
}

#[test]
fn doctor_empty_without_sessions_pins_the_empty_message() {
    let dir = scratch_dir("doctor-empty");
    let db = fresh_store(&dir);
    check_stdout(&dir, &db, &["doctor", "--frozen"], "doctor_empty.txt");
}

#[test]
fn doctor_table_with_window_pins_the_window_line() {
    let dir = scratch_dir("doctor-table-window");
    let db = doctor_store(&dir);
    let out = check_stdout(
        &dir,
        &db,
        &[
            "doctor",
            "--since",
            "2023-11-01",
            "--until",
            "2023-11-30",
            "--frozen",
        ],
        "doctor_table_window.txt",
    );
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("report window:"),
        "the window disclosure stays on stderr: {out:?}"
    );
}
