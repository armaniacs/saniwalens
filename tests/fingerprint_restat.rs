//! Fingerprint read-then-restat parity (PBI C11): a quiet second `analyze`
//! must skip everything it stamped, and content appended between runs must be
//! re-ingested next run, for every harness. These pin the normal path before
//! the race fix, so the stamp gate cannot silently stop stamping (or stamp
//! the wrong pair) without flipping a pin. The mid-read mismatch itself is
//! covered by the adapters' unit tests, which drive the real stat functions
//! around a synthetic drift. All fixtures synthetic (privacy rule).

use std::path::Path;

use saniwalens::store::{OpenMode, Store};

mod common;
use common::ScratchDir;

/// The tests below mutate process-wide env vars, so they run serialized.
static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn ingested_count(db: &Path) -> String {
    let store = Store::open(db, OpenMode::ReadOnly).unwrap();
    let (_, rows) = store.query("SELECT COUNT(*) FROM ingested_files").unwrap();
    rows[0][0].clone()
}

fn session_count(db: &Path) -> String {
    let store = Store::open(db, OpenMode::ReadOnly).unwrap();
    let (_, rows) = store.query("SELECT COUNT(*) FROM sessions").unwrap();
    rows[0][0].clone()
}

fn set_mtime(path: &Path, mtime: std::time::SystemTime) {
    std::fs::File::options()
        .write(true)
        .open(path)
        .unwrap()
        .set_modified(mtime)
        .unwrap();
}

const TRANSCRIPT_BASE: &str = concat!(
    r#"{"type":"user","cwd":"/tmp/example/proj","timestamp":"2026-01-01T00:00:00.000Z","message":{"content":"<command-name>/git-commit</command-name>"}}"#,
    "\n",
    r#"{"type":"assistant","timestamp":"2026-01-01T00:00:01.000Z","message":{"model":"claude-opus-4-7","usage":{"input_tokens":10,"cache_read_input_tokens":90,"cache_creation_input_tokens":0,"output_tokens":40}}}"#,
    "\n",
    r#"{"type":"assistant","timestamp":"2026-01-01T00:00:03.000Z","message":{"model":"claude-opus-4-7","usage":{"input_tokens":0,"cache_read_input_tokens":250,"cache_creation_input_tokens":0,"output_tokens":60}}}"#,
    "\n",
    r#"{"type":"user","timestamp":"2026-01-01T00:00:05.000Z","message":{"content":"thanks"}}"#,
);

const TRANSCRIPT_TAIL: &str = r#"{"type":"user","timestamp":"2026-01-01T00:00:07.000Z","message":{"content":"and one more"}}"#;

/// One synthetic transcript plus one synthetic skill under a fixture config
/// root — the same shape the adapter's own fixture uses.
fn claude_fixture(root: &Path) -> std::path::PathBuf {
    let projects = root.join("projects/-tmp-example-proj");
    std::fs::create_dir_all(&projects).unwrap();
    std::fs::create_dir_all(root.join("skills/demo-skill")).unwrap();
    let transcript = projects.join("aaaaaaaa-0000-0000-0000-000000000001.jsonl");
    std::fs::write(&transcript, TRANSCRIPT_BASE).unwrap();
    std::fs::write(
        root.join("skills/demo-skill/SKILL.md"),
        "---\nname: demo-skill\ndescription: Demo skill for the analyze fixture\n---\n# demo\n",
    )
    .unwrap();
    transcript
}

#[test]
fn claude_quiet_second_run_stays_skipped() {
    let _guard = ENV_LOCK.lock().unwrap();
    let root = ScratchDir::new("saniwalens-fp-restat", "claude-quiet");
    let transcript = claude_fixture(&root);
    let base =
        std::time::SystemTime::UNIX_EPOCH + std::time::Duration::new(1_700_000_100, 100_000_000);
    set_mtime(&transcript, base);
    unsafe { std::env::set_var("CLAUDE_CONFIG_DIR", &*root) };

    let db = root.join("store.db");
    let projects = root.join("projects");
    let first = saniwalens::adapter::claude::analyze(Some(projects.clone()), &db).unwrap();
    assert_eq!(
        (
            first.sessions,
            first.skipped,
            first.spans_total,
            first.surface_count,
            first.denials
        ),
        (1, 0, 1, 1, 0)
    );
    assert_eq!(ingested_count(&db), "1");
    assert_eq!(session_count(&db), "1");

    // Untouched: the stamp the first run wrote still matches, so everything
    // skips and no row is rewritten.
    let second = saniwalens::adapter::claude::analyze(Some(projects), &db).unwrap();
    assert_eq!((second.sessions, second.skipped), (0, 1));
    assert_eq!(ingested_count(&db), "1");
    assert_eq!(session_count(&db), "1");

    unsafe { std::env::remove_var("CLAUDE_CONFIG_DIR") };
}

#[test]
fn claude_appended_transcript_is_reingested() {
    let _guard = ENV_LOCK.lock().unwrap();
    let root = ScratchDir::new("saniwalens-fp-restat", "claude-drift");
    let transcript = claude_fixture(&root);
    let base =
        std::time::SystemTime::UNIX_EPOCH + std::time::Duration::new(1_700_000_100, 100_000_000);
    set_mtime(&transcript, base);
    unsafe { std::env::set_var("CLAUDE_CONFIG_DIR", &*root) };

    let db = root.join("store.db");
    let projects = root.join("projects");
    let first = saniwalens::adapter::claude::analyze(Some(projects.clone()), &db).unwrap();
    assert_eq!((first.sessions, first.skipped), (1, 0));

    // An append landing between runs: the stamp no longer matches, so the
    // grown transcript is re-ingested instead of skipped.
    std::fs::write(&transcript, format!("{TRANSCRIPT_BASE}\n{TRANSCRIPT_TAIL}")).unwrap();
    set_mtime(&transcript, base + std::time::Duration::from_millis(500));
    let second = saniwalens::adapter::claude::analyze(Some(projects.clone()), &db).unwrap();
    assert_eq!((second.sessions, second.skipped), (1, 0));
    assert_eq!(ingested_count(&db), "1");
    assert_eq!(session_count(&db), "1");

    // Quiet again: the new stamp holds.
    let third = saniwalens::adapter::claude::analyze(Some(projects), &db).unwrap();
    assert_eq!((third.sessions, third.skipped), (0, 1));

    unsafe { std::env::remove_var("CLAUDE_CONFIG_DIR") };
}

fn opencode_fixture_db(path: &Path) {
    let conn = rusqlite::Connection::open(path).unwrap();
    conn.execute_batch(
        "CREATE TABLE session (id TEXT, slug TEXT, directory TEXT, time_created INTEGER, time_updated INTEGER);
         CREATE TABLE message (id TEXT, session_id TEXT, time_created INTEGER, data TEXT);
         CREATE TABLE part (id TEXT, message_id TEXT, session_id TEXT, time_created INTEGER, data TEXT);",
    )
    .unwrap();
    conn.execute(
        "INSERT INTO session (id, slug, directory, time_created, time_updated)
         VALUES ('ses_1', 'proj', '/tmp/example/proj', 1700000000000, 1700000000000)",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO message (id, session_id, time_created, data)
         VALUES ('msg_1', 'ses_1', 1700000000000, '{\"role\":\"user\"}')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO part (id, message_id, session_id, time_created, data)
         VALUES ('prt_1', 'msg_1', 'ses_1', 1700000000000, '{\"type\":\"text\",\"text\":\"do the thing\"}')",
        [],
    )
    .unwrap();
}

#[test]
fn opencode_quiet_second_run_stays_skipped() {
    let _guard = ENV_LOCK.lock().unwrap();
    let root = ScratchDir::new("saniwalens-fp-restat", "opencode-quiet");
    unsafe { std::env::set_var("XDG_CONFIG_HOME", root.join("cfg-home")) };
    let input = root.join("opencode.db");
    opencode_fixture_db(&input);

    let db = root.join("store.db");
    let first = saniwalens::adapter::opencode::analyze(Some(input.clone()), &db).unwrap();
    assert_eq!((first.sessions, first.skipped), (1, 0));
    assert_eq!(ingested_count(&db), "1");

    // Untouched rows: the stamp still matches, so the session skips.
    let second = saniwalens::adapter::opencode::analyze(Some(input), &db).unwrap();
    assert_eq!((second.sessions, second.skipped), (0, 1));
    assert_eq!(ingested_count(&db), "1");

    unsafe { std::env::remove_var("XDG_CONFIG_HOME") };
}

#[test]
fn opencode_appended_session_is_reingested() {
    let _guard = ENV_LOCK.lock().unwrap();
    let root = ScratchDir::new("saniwalens-fp-restat", "opencode-drift");
    unsafe { std::env::set_var("XDG_CONFIG_HOME", root.join("cfg-home")) };
    let input = root.join("opencode.db");
    opencode_fixture_db(&input);

    let db = root.join("store.db");
    let first = saniwalens::adapter::opencode::analyze(Some(input.clone()), &db).unwrap();
    assert_eq!((first.sessions, first.skipped), (1, 0));

    // New rows plus a bumped time_updated between runs: re-ingested.
    let conn = rusqlite::Connection::open(&input).unwrap();
    conn.execute(
        "INSERT INTO message (id, session_id, time_created, data)
         VALUES ('msg_2', 'ses_1', 1700000005000, '{\"role\":\"user\"}')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO part (id, message_id, session_id, time_created, data)
         VALUES ('prt_2', 'msg_2', 'ses_1', 1700000005000, '{\"type\":\"text\",\"text\":\"and more\"}')",
        [],
    )
    .unwrap();
    conn.execute(
        "UPDATE session SET time_updated = 1700000005000 WHERE id = 'ses_1'",
        [],
    )
    .unwrap();
    drop(conn);
    let second = saniwalens::adapter::opencode::analyze(Some(input), &db).unwrap();
    assert_eq!((second.sessions, second.skipped), (1, 0));
    assert_eq!(ingested_count(&db), "1");

    unsafe { std::env::remove_var("XDG_CONFIG_HOME") };
}

fn kilocode_fixture_tasks(root: &Path) -> std::path::PathBuf {
    let tasks = root.join("tasks");
    let task = tasks.join("task-1");
    std::fs::create_dir_all(&task).unwrap();
    std::fs::write(
        task.join("api_conversation_history.json"),
        r#"[{"role":"user","ts":1700000000000,"content":[{"type":"text","text":"fix it"}]},{"role":"assistant","ts":1700000001000,"content":[{"type":"text","text":"ok"}]}]"#,
    )
    .unwrap();
    std::fs::write(task.join("ui_messages.json"), "[]").unwrap();
    tasks
}

#[test]
fn kilocode_quiet_second_run_stays_skipped() {
    let _guard = ENV_LOCK.lock().unwrap();
    let root = ScratchDir::new("saniwalens-fp-restat", "kilocode-quiet");
    unsafe { std::env::set_var("XDG_CONFIG_HOME", root.join("cfg-home")) };
    let tasks = kilocode_fixture_tasks(&root);

    let db = root.join("store.db");
    let first = saniwalens::adapter::kilocode::analyze(Some(tasks.clone()), &db).unwrap();
    assert_eq!((first.sessions, first.skipped), (1, 0));
    assert_eq!(ingested_count(&db), "1");

    // Untouched task files: the stamp still matches, so the task skips.
    let second = saniwalens::adapter::kilocode::analyze(Some(tasks), &db).unwrap();
    assert_eq!((second.sessions, second.skipped), (0, 1));
    assert_eq!(ingested_count(&db), "1");

    unsafe { std::env::remove_var("XDG_CONFIG_HOME") };
}

#[test]
fn kilocode_appended_task_is_reingested() {
    let _guard = ENV_LOCK.lock().unwrap();
    let root = ScratchDir::new("saniwalens-fp-restat", "kilocode-drift");
    unsafe { std::env::set_var("XDG_CONFIG_HOME", root.join("cfg-home")) };
    let tasks = kilocode_fixture_tasks(&root);

    let db = root.join("store.db");
    let first = saniwalens::adapter::kilocode::analyze(Some(tasks.clone()), &db).unwrap();
    assert_eq!((first.sessions, first.skipped), (1, 0));

    // A task file growing between runs: the stamp no longer matches, so the
    // task is re-ingested instead of skipped.
    let ui = tasks.join("task-1/ui_messages.json");
    let base =
        std::time::SystemTime::UNIX_EPOCH + std::time::Duration::new(1_700_000_100, 100_000_000);
    set_mtime(&tasks.join("task-1/api_conversation_history.json"), base);
    set_mtime(&ui, base);
    std::fs::write(
        &ui,
        r#"[{"ts":1700000001000,"type":"say","say":"api_req_started","text":"{\"tokensIn\":10,\"tokensOut\":4}"}]"#,
    )
    .unwrap();
    set_mtime(&ui, base + std::time::Duration::from_millis(500));
    let second = saniwalens::adapter::kilocode::analyze(Some(tasks.clone()), &db).unwrap();
    assert_eq!((second.sessions, second.skipped), (1, 0));
    assert_eq!(ingested_count(&db), "1");

    // Quiet again: the new stamp holds.
    let third = saniwalens::adapter::kilocode::analyze(Some(tasks), &db).unwrap();
    assert_eq!((third.sessions, third.skipped), (0, 1));

    unsafe { std::env::remove_var("XDG_CONFIG_HOME") };
}
