//! render_view migration goldens (PBI C4): the five views' current output,
//! byte-pinned before the ritual moves into `render_view`.
//!
//! Each case runs the real binary against a synthetic store and pins stdout
//! byte-for-byte (pipes carry no ANSI codes, fixtures carry fixed epochs, so
//! the bytes are stable). `UPDATE_GOLDEN=1` re-blesses after review, like
//! `tests/e2e_golden_harness.rs`. Behavior preservation = these files do not
//! change across the migration; the prompts bespoke-empty and inventory
//! `is_all` deletions are guarded by `prompts_empty_all` /
//! `inventory_json*` here.
//!
//! Only stdout is pinned: stderr carries the freshness line and the window
//! disclosure, which the migration must keep but whose coverage lives in the
//! window tests. Windowed cases additionally assert the disclosure line.

use std::path::{Path, PathBuf};

use saniwalens::core::span::{SessionStart, Source, Span};
use saniwalens::core::surface::{LoadMode, Scope, Surface};
use saniwalens::store::{IngestBatch, OpenMode, SessionMeta, Store};

mod common;
use common::ScratchDir;

fn scratch_dir(name: &str) -> ScratchDir {
    let dir = ScratchDir::new("saniwalens-render-view", name);
    std::fs::create_dir_all(dir.join(".claude").join("projects")).unwrap();
    dir
}

fn golden_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("golden")
        .join("render_view")
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

fn session(id: &str, sub_tokens: i64, start_ms: i64) -> SessionMeta {
    SessionMeta {
        id: id.to_string(),
        project: "demo".to_string(),
        slug: "demo".to_string(),
        root: String::new(),
        source_path: format!("/tmp/{id}.jsonl"),
        sub_tokens,
        sub_agent_count: 0,
        start: Some(SessionStart {
            timestamp_ms: start_ms,
            ctx: 9_000,
        }),
    }
}

const S1_START_MS: i64 = 1_700_000_000_000;

// ---------------------------------------------------------------- models ---

const MODELS_TRANSCRIPT: &str = concat!(
    r#"{"type":"user","timestamp":"2023-11-14T22:13:20.000Z","message":{"content":"<command-name>/git-commit</command-name>"}}"#,
    "\n",
    r#"{"type":"assistant","timestamp":"2023-11-14T22:13:21.000Z","message":{"model":"opus","usage":{"input_tokens":10,"cache_read_input_tokens":0,"cache_creation_input_tokens":0,"output_tokens":100}}}"#,
    "\n",
    r#"{"type":"assistant","timestamp":"2023-11-14T22:13:23.000Z","message":{"content":"thanks"}}"#,
);

fn models_store(dir: &Path) -> PathBuf {
    let db = dir.join("store.db");
    {
        let mut store = Store::open(&db, OpenMode::ReadWrite).unwrap();
        let meta = session("s1", 0, S1_START_MS);
        let records = saniwalens::adapter::transcript::parse_session(MODELS_TRANSCRIPT);
        let spans = saniwalens::core::span::extract_spans(
            &records,
            saniwalens::core::span::DEFAULT_IDLE_GAP_MS,
        );
        assert_eq!(spans.len(), 1, "the fixture yields one skill span");
        store
            .ingest(IngestBatch::new(&meta, &spans, &[]).subagent_runs(&[
                saniwalens::core::subagent::SubagentRun {
                    agent: Some("Explore".to_string()),
                    agent_id: "Explore-1".to_string(),
                    run_path: "/tmp/example/subagents/Explore-1.jsonl".to_string(),
                    tool_use_id: None,
                    parent_agent_id: None,
                    spawn_depth: 1,
                    model: Some("opus".to_string()),
                    prompt_id: None,
                    out_tokens: 100,
                    started_epoch_ms: S1_START_MS + 100_000,
                    root_tool_use_id: None,
                },
                saniwalens::core::subagent::SubagentRun {
                    agent: Some("Plan".to_string()),
                    agent_id: "Plan-1".to_string(),
                    run_path: "/tmp/example/subagents/Plan-1.jsonl".to_string(),
                    tool_use_id: None,
                    parent_agent_id: None,
                    spawn_depth: 1,
                    model: Some("sonnet".to_string()),
                    prompt_id: None,
                    out_tokens: 40,
                    started_epoch_ms: S1_START_MS + 100_000,
                    root_tool_use_id: None,
                },
                saniwalens::core::subagent::SubagentRun {
                    agent: Some("Bash".to_string()),
                    agent_id: "Bash-1".to_string(),
                    run_path: "/tmp/example/subagents/Bash-1.jsonl".to_string(),
                    tool_use_id: None,
                    parent_agent_id: None,
                    spawn_depth: 1,
                    model: None,
                    prompt_id: None,
                    out_tokens: 10,
                    started_epoch_ms: S1_START_MS + 100_000,
                    root_tool_use_id: None,
                },
            ]))
            .unwrap();
    }
    db
}

#[test]
fn models_table_pins_the_current_bytes() {
    let dir = scratch_dir("models-table");
    let db = models_store(&dir);
    check_stdout(&dir, &db, &["models", "--frozen"], "models_table.txt");
}

#[test]
fn models_markdown_pins_the_current_bytes() {
    let dir = scratch_dir("models-markdown");
    let db = models_store(&dir);
    check_stdout(
        &dir,
        &db,
        &["models", "--format", "markdown", "--frozen"],
        "models_markdown.txt",
    );
}

#[test]
fn models_json_pins_the_current_bytes() {
    let dir = scratch_dir("models-json");
    let db = models_store(&dir);
    check_stdout(
        &dir,
        &db,
        &["models", "--format", "json", "--frozen"],
        "models_json.txt",
    );
}

#[test]
fn models_json_with_window_pins_the_window_key() {
    let dir = scratch_dir("models-json-window");
    let db = models_store(&dir);
    let out = check_stdout(
        &dir,
        &db,
        &[
            "models",
            "--format",
            "json",
            "--since",
            "2023-11-01",
            "--until",
            "2023-11-30",
            "--frozen",
        ],
        "models_json_window.txt",
    );
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("report window:"),
        "the window disclosure stays on stderr: {out:?}"
    );
}

#[test]
fn models_empty_window_pins_the_empty_message() {
    let dir = scratch_dir("models-empty");
    let db = models_store(&dir);
    check_stdout(
        &dir,
        &db,
        &[
            "models",
            "--since",
            "2024-01-01",
            "--until",
            "2024-01-02",
            "--frozen",
        ],
        "models_empty.txt",
    );
}

// --------------------------------------------------------------- sessions ---

fn sessions_store(dir: &Path) -> PathBuf {
    let db = dir.join("store.db");
    {
        let mut store = Store::open(&db, OpenMode::ReadWrite).unwrap();
        let span = Span {
            skill: "a".to_string(),
            source: Source::Slash,
            started_epoch_ms: S1_START_MS + 100_000,
            duration_sec: 60.0,
            out_tokens: 100,
            ctx_growth: 0,
            ctx_start: 0,
            ctx_peak: 0,
            model: None,
            is_trailing: false,
            spawns: vec![],
            sub_tokens: 0,
            sub_agent_count: 0,
            sub_tokens_estimated: false,
        };
        let usage = saniwalens::core::usage::UsageEvent {
            surface_kind: "skill".to_string(),
            surface_id: "a".to_string(),
            started_epoch_ms: S1_START_MS + 100_000,
            tool_use_id: None,
        };
        store
            .ingest(
                IngestBatch::new(&session("s1", 30, S1_START_MS), &[span], &[usage]).tool_errors(
                    &[
                        (S1_START_MS + 100_000, "timeout", "t", "Bash", "sleep 9"),
                        (S1_START_MS + 200_000, "timeout", "t", "Bash", "sleep 9"),
                    ],
                ),
            )
            .unwrap();
        store
            .ingest(IngestBatch::new(&session("s2", 50, S1_START_MS), &[], &[]))
            .unwrap();
    }
    db
}

#[test]
fn sessions_table_pins_the_current_bytes() {
    let dir = scratch_dir("sessions-table");
    let db = sessions_store(&dir);
    check_stdout(&dir, &db, &["sessions", "--frozen"], "sessions_table.txt");
}

#[test]
fn sessions_markdown_pins_the_current_bytes() {
    let dir = scratch_dir("sessions-markdown");
    let db = sessions_store(&dir);
    check_stdout(
        &dir,
        &db,
        &["sessions", "--format", "markdown", "--frozen"],
        "sessions_markdown.txt",
    );
}

#[test]
fn sessions_json_pins_the_current_bytes() {
    let dir = scratch_dir("sessions-json");
    let db = sessions_store(&dir);
    check_stdout(
        &dir,
        &db,
        &["sessions", "--format", "json", "--frozen"],
        "sessions_json.txt",
    );
}

#[test]
fn sessions_json_with_window_pins_the_window_key() {
    let dir = scratch_dir("sessions-json-window");
    let db = sessions_store(&dir);
    check_stdout(
        &dir,
        &db,
        &[
            "sessions",
            "--format",
            "json",
            "--since",
            "2023-11-01",
            "--until",
            "2023-11-30",
            "--frozen",
        ],
        "sessions_json_window.txt",
    );
}

#[test]
fn sessions_empty_window_pins_the_empty_message() {
    let dir = scratch_dir("sessions-empty");
    let db = sessions_store(&dir);
    check_stdout(
        &dir,
        &db,
        &[
            "sessions",
            "--since",
            "2024-01-01",
            "--until",
            "2024-01-02",
            "--frozen",
        ],
        "sessions_empty.txt",
    );
}

// ---------------------------------------------------------------- prompts ---

/// Prompt rows whose refs point at a transcript that does not exist: every
/// ref counts as pruned, so the JSON stays free of scratch paths and the
/// table pins the pruned note. Behaviors hit both flag thresholds.
fn prompts_store(dir: &Path) -> PathBuf {
    let db = dir.join("store.db");
    {
        let mut store = Store::open(&db, OpenMode::ReadWrite).unwrap();
        let mut rows: Vec<(usize, i64, &str)> = Vec::new();
        for i in 0..5 {
            rows.push((i, S1_START_MS + (i as i64) * 100_000, "instruct"));
        }
        rows.push((5, S1_START_MS + 500_000, "steer"));
        rows.push((6, S1_START_MS + 600_000, "steer"));
        rows.push((7, S1_START_MS + 700_000, "correct"));
        store
            .ingest(IngestBatch::new(&session("s1", 0, S1_START_MS), &[], &[]).prompts(&rows))
            .unwrap();
    }
    db
}

#[test]
fn prompts_table_pins_the_current_bytes() {
    let dir = scratch_dir("prompts-table");
    let db = prompts_store(&dir);
    check_stdout(&dir, &db, &["prompts", "--frozen"], "prompts_table.txt");
}

#[test]
fn prompts_markdown_pins_the_current_bytes() {
    let dir = scratch_dir("prompts-markdown");
    let db = prompts_store(&dir);
    check_stdout(
        &dir,
        &db,
        &["prompts", "--format", "markdown", "--frozen"],
        "prompts_markdown.txt",
    );
}

#[test]
fn prompts_json_pins_the_current_bytes() {
    let dir = scratch_dir("prompts-json");
    let db = prompts_store(&dir);
    check_stdout(
        &dir,
        &db,
        &["prompts", "--format", "json", "--frozen"],
        "prompts_json.txt",
    );
}

#[test]
fn prompts_json_with_window_pins_the_window_key() {
    let dir = scratch_dir("prompts-json-window");
    let db = prompts_store(&dir);
    check_stdout(
        &dir,
        &db,
        &[
            "prompts",
            "--format",
            "json",
            "--since",
            "2023-11-01",
            "--until",
            "2023-11-30",
            "--frozen",
        ],
        "prompts_json_window.txt",
    );
}

/// The bespoke-empty branch under deletion: sessions exist, prompts do not,
/// no window flags — the harness-harvest wording must survive the move to
/// the common empty path byte-for-byte.
#[test]
fn prompts_empty_without_prompts_pins_the_harness_wording() {
    let dir = scratch_dir("prompts-empty-all");
    let db = sessions_store(&dir);
    check_stdout(&dir, &db, &["prompts", "--frozen"], "prompts_empty_all.txt");
}

#[test]
fn prompts_empty_window_pins_the_windowed_wording() {
    let dir = scratch_dir("prompts-empty-window");
    let db = prompts_store(&dir);
    check_stdout(
        &dir,
        &db,
        &[
            "prompts",
            "--since",
            "2024-01-01",
            "--until",
            "2024-01-02",
            "--frozen",
        ],
        "prompts_empty_window.txt",
    );
}

// ------------------------------------------------------------------ trend ---

const PAST_START_MS: i64 = S1_START_MS - 86_400_000;

fn trend_span(out_tokens: u64, epoch_ms: i64) -> Span {
    Span {
        skill: "commit".to_string(),
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

fn trend_store(dir: &Path, with_past: bool) -> PathBuf {
    let db = dir.join("store.db");
    {
        let mut store = Store::open(&db, OpenMode::ReadWrite).unwrap();
        if with_past {
            let mut past = session("past", 400, PAST_START_MS);
            past.sub_agent_count = 1;
            store
                .ingest(
                    IngestBatch::new(&past, &[trend_span(150, PAST_START_MS)], &[])
                        .work_events(&[(PAST_START_MS, "file_edit", "main.rs", None)]),
                )
                .unwrap();
        }
        let mut present = session("present", 500, S1_START_MS);
        present.sub_agent_count = 2;
        store
            .ingest(
                IngestBatch::new(&present, &[trend_span(300, S1_START_MS)], &[]).work_events(&[(
                    S1_START_MS,
                    "file_edit",
                    "main.rs",
                    None,
                )]),
            )
            .unwrap();
    }
    db
}

/// The comparison pair covering the fixture: current JST 2023-11-15..16
/// holds `present` (2023-11-14T22:13Z = JST 11-15), previous holds `past`.
const TREND_ARGS: &[&str] = &["--since", "2023-11-15", "--until", "2023-11-16"];

fn trend_args(extra: &[&str]) -> Vec<String> {
    let mut args = vec!["trend".to_string()];
    args.extend(TREND_ARGS.iter().map(|s| s.to_string()));
    args.extend(extra.iter().map(|s| s.to_string()));
    args.push("--frozen".to_string());
    args
}

#[test]
fn trend_table_pins_the_current_bytes() {
    let dir = scratch_dir("trend-table");
    let db = trend_store(&dir, true);
    let args = trend_args(&[]);
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    check_stdout(&dir, &db, &refs, "trend_table.txt");
}

#[test]
fn trend_markdown_pins_the_current_bytes() {
    let dir = scratch_dir("trend-markdown");
    let db = trend_store(&dir, true);
    let args = trend_args(&["--format", "markdown"]);
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    check_stdout(&dir, &db, &refs, "trend_markdown.txt");
}

#[test]
fn trend_json_pins_the_current_bytes() {
    let dir = scratch_dir("trend-json");
    let db = trend_store(&dir, true);
    let args = trend_args(&["--format", "json"]);
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    check_stdout(&dir, &db, &refs, "trend_json.txt");
}

/// One-sided shape: the previous window holds nothing, so the report pins
/// the n/a reason sentence and the previous-events note.
#[test]
fn trend_onesided_table_pins_the_unavailable_wording() {
    let dir = scratch_dir("trend-onesided");
    let db = trend_store(&dir, false);
    let args = trend_args(&[]);
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    check_stdout(&dir, &db, &refs, "trend_onesided_table.txt");
}

#[test]
fn trend_empty_pair_pins_the_empty_message() {
    let dir = scratch_dir("trend-empty");
    let db = trend_store(&dir, true);
    let out = run(
        &dir,
        &db,
        &[
            "trend",
            "--since",
            "2024-01-01",
            "--until",
            "2024-01-02",
            "--frozen",
        ],
    );
    assert!(out.status.success(), "trend empty exits 0: {out:?}");
    check_golden("trend_empty.txt", &String::from_utf8_lossy(&out.stdout));
}

// ------------------------------------------------------------------ waste ---

fn waste_rule(id: &str, globs: &[&str]) -> Surface {
    Surface {
        kind: "rule".to_string(),
        id: id.to_string(),
        scope: Scope::Global,
        config_path: format!("/tmp/rules/{id}.md"),
        static_tokens: Some(1200),
        startup_tokens: Some(0),
        load_mode: LoadMode::PathConditional,
        paths: globs.iter().map(|g| g.to_string()).collect(),
        description: None,
    }
}

fn waste_skill(id: &str) -> Surface {
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

fn waste_store(dir: &Path) -> PathBuf {
    let db = dir.join("store.db");
    {
        let mut store = Store::open(&db, OpenMode::ReadWrite).unwrap();
        store
            .replace_surfaces(&[
                waste_rule("used-rule", &["src/**"]),
                waste_rule("idle-rule", &["docs/**"]),
                waste_skill("idle-skill"),
            ])
            .unwrap();
        store
            .ingest(
                IngestBatch::new(&session("s1", 0, 100_000_000), &[], &[]).work_events(&[(
                    100_000_000,
                    "file_read",
                    "main.rs",
                    Some("/repo/src/main.rs"),
                )]),
            )
            .unwrap();
    }
    db
}

#[test]
fn waste_table_pins_the_current_bytes() {
    let dir = scratch_dir("waste-table");
    let db = waste_store(&dir);
    check_stdout(&dir, &db, &["waste", "--frozen"], "waste_table.txt");
}

#[test]
fn waste_markdown_pins_the_current_bytes() {
    let dir = scratch_dir("waste-markdown");
    let db = waste_store(&dir);
    check_stdout(
        &dir,
        &db,
        &["waste", "--format", "markdown", "--frozen"],
        "waste_markdown.txt",
    );
}

#[test]
fn waste_json_pins_the_current_bytes() {
    let dir = scratch_dir("waste-json");
    let db = waste_store(&dir);
    check_stdout(
        &dir,
        &db,
        &["waste", "--format", "json", "--frozen"],
        "waste_json.txt",
    );
}

#[test]
fn waste_json_with_window_pins_the_window_key() {
    let dir = scratch_dir("waste-json-window");
    let db = waste_store(&dir);
    check_stdout(
        &dir,
        &db,
        &[
            "waste",
            "--format",
            "json",
            "--since",
            "1970-01-01",
            "--until",
            "1970-01-03",
            "--frozen",
        ],
        "waste_json_window.txt",
    );
}

/// No surfaces catalogued at all: the analyze hint, not a wedge list.
#[test]
fn waste_empty_without_catalog_pins_the_empty_message() {
    let dir = scratch_dir("waste-empty");
    let db = sessions_store(&dir);
    check_stdout(&dir, &db, &["waste", "--frozen"], "waste_empty.txt");
}

// --------------------------------------------------------------- inventory ---
//
// The `mod.rs:2150` inline `window.is_all()` lives in inventory's JSON
// status (`unused` / `unused-in-window`); these goldens pin it across the
// move to the shared vocabulary.

fn inventory_store(dir: &Path) -> PathBuf {
    let db = dir.join("store.db");
    {
        let mut store = Store::open(&db, OpenMode::ReadWrite).unwrap();
        store
            .replace_surfaces(&[waste_skill("used-skill"), waste_skill("idle-skill")])
            .unwrap();
        let usage = saniwalens::core::usage::UsageEvent {
            surface_kind: "skill".to_string(),
            surface_id: "used-skill".to_string(),
            started_epoch_ms: S1_START_MS,
            tool_use_id: None,
        };
        store
            .ingest(IngestBatch::new(
                &session("s1", 0, S1_START_MS),
                &[],
                &[usage],
            ))
            .unwrap();
    }
    db
}

#[test]
fn inventory_json_pins_the_unused_status() {
    let dir = scratch_dir("inventory-json");
    let db = inventory_store(&dir);
    check_stdout(
        &dir,
        &db,
        &["inventory", "--format", "json", "--frozen"],
        "inventory_json.txt",
    );
}

#[test]
fn inventory_json_with_window_pins_the_windowed_status() {
    let dir = scratch_dir("inventory-json-window");
    let db = inventory_store(&dir);
    check_stdout(
        &dir,
        &db,
        &[
            "inventory",
            "--format",
            "json",
            "--since",
            "2023-11-01",
            "--until",
            "2023-11-30",
            "--frozen",
        ],
        "inventory_json_window.txt",
    );
}
