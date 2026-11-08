//! `sql` stays bounded: a huge SELECT is truncated at `--limit` (default) with
//! a warning and an opt-out, never an unbounded in-memory materialization.
//! End to end through the real binary on a synthetic store (10_005 error rows).

use std::path::{Path, PathBuf};

use saniwalens::core::span::SessionStart;
use saniwalens::store::{IngestBatch, OpenMode, SessionMeta, Store};

mod common;
use common::ScratchDir;

const ROWS: usize = 10_005;
const DEFAULT_LIMIT: usize = 10_000;

fn scratch_dir(name: &str) -> ScratchDir {
    let dir = ScratchDir::new("saniwalens-sql-limit", name);
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
            timestamp_ms: 100_000_000,
            ctx: 9_000,
        }),
    }
}

fn write_store(dir: &Path) -> PathBuf {
    let db = dir.join("store.db");
    {
        let mut store = Store::open(&db, OpenMode::ReadWrite).unwrap();
        let meta = session("big-session");
        let errors: Vec<(i64, &str, String, &str, &str)> = (0..ROWS as i64)
            .map(|n| {
                (
                    100_000_000 + n,
                    "timeout",
                    format!("excerpt-{n}"),
                    "Bash",
                    "slow.sh",
                )
            })
            .collect();
        let refs: Vec<(i64, &str, &str, &str, &str)> = errors
            .iter()
            .map(|(epoch, cat, excerpt, tool, target)| {
                (*epoch, *cat, excerpt.as_str(), *tool, *target)
            })
            .collect();
        store
            .ingest(IngestBatch::new(&meta, &[], &[]).tool_errors(&refs))
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

fn table_rows(stdout: &[u8]) -> Vec<String> {
    // The table path prints a bare header, a dashes separator, the data
    // rows, then the `N row(s)` note — no pipes (those are the markdown
    // path's). Data rows are the lines between the separator and the note.
    let text = String::from_utf8_lossy(stdout);
    let mut lines = text.lines();
    let _header = lines.next().expect("a header line");
    let sep = lines.next().expect("a separator line");
    assert!(
        !sep.is_empty() && sep.chars().all(|c| c == '-'),
        "the separator under the header: {sep:?}"
    );
    lines
        .take_while(|line| {
            !(line.ends_with(" row(s)")
                && line[..line.len() - " row(s)".len()]
                    .chars()
                    .all(|c| c.is_ascii_digit()))
        })
        .filter(|line| !line.trim().is_empty())
        .map(|line| line.to_string())
        .collect()
}

#[test]
fn a_huge_select_is_truncated_at_the_default_limit_with_a_warning() {
    let dir = scratch_dir("truncated");
    let db = write_store(&dir);
    let out = run(&dir, &db, &["sql", "SELECT excerpt FROM tool_errors"]);

    assert!(out.status.success(), "sql exits 0: {out:?}");
    assert_eq!(
        table_rows(&out.stdout).len(),
        DEFAULT_LIMIT,
        "only the default limit renders"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("--limit"),
        "the truncation names the opt-out: {stderr}"
    );
}

#[test]
fn limit_zero_means_no_truncation() {
    let dir = scratch_dir("unlimited");
    let db = write_store(&dir);
    let out = run(
        &dir,
        &db,
        &["sql", "--limit", "0", "SELECT excerpt FROM tool_errors"],
    );

    assert!(out.status.success(), "sql --limit 0 exits 0: {out:?}");
    assert_eq!(
        table_rows(&out.stdout).len(),
        ROWS,
        "opt-out renders every row"
    );
}

#[test]
fn json_stays_an_array_and_warns_on_stderr_when_truncated() {
    // The machine contract keeps its shape (a bare array of row objects);
    // truncation is signaled on stderr, like the table path.
    let dir = scratch_dir("json");
    let db = write_store(&dir);
    let out = run(
        &dir,
        &db,
        &["sql", "--format", "json", "SELECT excerpt FROM tool_errors"],
    );

    assert!(out.status.success(), "sql json exits 0: {out:?}");
    let json: serde_json::Value = String::from_utf8_lossy(&out.stdout)
        .parse()
        .expect("valid JSON");
    assert_eq!(
        json.as_array().map(|a| a.len()),
        Some(DEFAULT_LIMIT),
        "the array holds exactly the limit"
    );
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("--limit"),
        "truncation warns on stderr"
    );
}

#[test]
fn a_small_select_is_unchanged_and_silent() {
    let dir = scratch_dir("small");
    let db = write_store(&dir);
    let out = run(&dir, &db, &["sql", "SELECT COUNT(*) FROM tool_errors"]);

    assert!(out.status.success(), "small sql exits 0: {out:?}");
    let rows = table_rows(&out.stdout);
    assert_eq!(rows.len(), 1, "one aggregate row: {rows:?}");
    assert!(
        !String::from_utf8_lossy(&out.stderr).contains("--limit"),
        "no truncation warning for a small result"
    );
}
