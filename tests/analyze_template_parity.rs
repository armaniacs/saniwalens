//! Per-harness analyze parity pins (PBI C1): the current output and counters
//! of every harness's `analyze`, pinned before the template refactor so any
//! behavior drift flips a pin. Each harness pins its full `AnalyzeStats`, the
//! quiet-second-run skip, the catalogued surfaces, and the stable `meta` rows
//! (`analyzed_at` excluded: it carries the wall clock). The golden files in
//! `tests/golden/` pin sessions/events/tool_errors for opencode and kilocode;
//! `tests/e2e_golden_harness.rs` gains the claude golden beside them.
//! Everything touched is synthetic temp data (privacy rule).

use std::path::{Path, PathBuf};

use saniwalens::store::{OpenMode, Store};

mod common;
use common::ScratchDir;

/// The tests below mutate process-wide env vars, so they run serialized.
/// A poisoned lock is recovered so one failing test does not cascade into
/// the others.
static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn env_guard() -> std::sync::MutexGuard<'static, ()> {
    ENV_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn set_mtime(path: &Path, mtime: std::time::SystemTime) {
    std::fs::File::options()
        .write(true)
        .open(path)
        .unwrap()
        .set_modified(mtime)
        .unwrap();
}

fn fixed_mtime() -> std::time::SystemTime {
    std::time::SystemTime::UNIX_EPOCH + std::time::Duration::new(1_700_000_100, 100_000_000)
}

/// Point HOME at an empty scratch dir so home-adjacent skill sites contribute
/// nothing on any machine; the harness config var points at the fixture root.
fn isolate_env(root: &Path, key: &str, value: &Path) {
    let home = root.join("home");
    std::fs::create_dir_all(&home).unwrap();
    unsafe { std::env::set_var("HOME", &home) };
    unsafe { std::env::set_var(key, value) };
}

fn clear_env(key: &str) {
    unsafe { std::env::remove_var(key) };
    unsafe { std::env::remove_var("HOME") };
}

fn normalize(root: &Path, text: &str) -> String {
    text.replace(&root.display().to_string(), "<ROOT>")
}

/// The stable meta rows: every key except the wall-clock `analyzed_at`.
fn dump_meta(db: &Path, root: &Path) -> String {
    let store = Store::open(db, OpenMode::ReadOnly).unwrap();
    let (cols, rows) = store
        .query("SELECT key, value FROM meta WHERE key != 'analyzed_at' ORDER BY key")
        .unwrap();
    let mut out = cols.join("\t") + "\n";
    for row in rows {
        out.push_str(&row.join("\t"));
        out.push('\n');
    }
    normalize(root, &out)
}

fn dump_surfaces(db: &Path, root: &Path) -> String {
    let store = Store::open(db, OpenMode::ReadOnly).unwrap();
    let (cols, rows) = store
        .query("SELECT kind, id, scope, config_path FROM surfaces ORDER BY kind, id, scope")
        .unwrap();
    let mut out = cols.join("\t") + "\n";
    for row in rows {
        out.push_str(&row.join("\t"));
        out.push('\n');
    }
    normalize(root, &out)
}

fn count(db: &Path, table: &str) -> String {
    let store = Store::open(db, OpenMode::ReadOnly).unwrap();
    let (_, rows) = store
        .query(&format!("SELECT COUNT(*) FROM {table}"))
        .unwrap();
    rows[0][0].clone()
}

const TRANSCRIPT: &str = concat!(
    r#"{"type":"user","cwd":"/tmp/example/proj","timestamp":"2026-01-01T00:00:00.000Z","message":{"content":"<command-name>/git-commit</command-name>"}}"#,
    "\n",
    r#"{"type":"assistant","timestamp":"2026-01-01T00:00:01.000Z","message":{"model":"claude-opus-4-7","usage":{"input_tokens":10,"cache_read_input_tokens":90,"cache_creation_input_tokens":0,"output_tokens":40}}}"#,
    "\n",
    r#"{"type":"assistant","timestamp":"2026-01-01T00:00:03.000Z","message":{"model":"claude-opus-4-7","usage":{"input_tokens":0,"cache_read_input_tokens":250,"cache_creation_input_tokens":0,"output_tokens":60}}}"#,
    "\n",
    r#"{"type":"user","timestamp":"2026-01-01T00:00:05.000Z","message":{"content":"thanks"}}"#,
);

fn claude_fixture(root: &Path) -> PathBuf {
    let projects = root.join("projects/-tmp-example-proj");
    std::fs::create_dir_all(&projects).unwrap();
    std::fs::create_dir_all(root.join("skills/demo-skill")).unwrap();
    let transcript = projects.join("aaaaaaaa-0000-0000-0000-000000000001.jsonl");
    std::fs::write(&transcript, TRANSCRIPT).unwrap();
    std::fs::write(
        root.join("skills/demo-skill/SKILL.md"),
        "---\nname: demo-skill\ndescription: Demo skill for the analyze fixture\n---\n# demo\n",
    )
    .unwrap();
    set_mtime(&transcript, fixed_mtime());
    root.join("projects")
}

#[test]
fn claude_analyze_pins_stats_catalog_and_meta() {
    let _guard = env_guard();
    let root = ScratchDir::new("saniwalens-parity", "claude");
    let projects = claude_fixture(&root);
    isolate_env(&root, "CLAUDE_CONFIG_DIR", &root);

    let db = root.join("store.db");
    let stats = saniwalens::adapter::claude::analyze(Some(projects), &db).unwrap();
    assert_eq!(
        (
            stats.sessions,
            stats.skipped,
            stats.spans_total,
            stats.surface_count,
            stats.denials
        ),
        (1, 0, 1, 1, 0)
    );
    assert_eq!(count(&db, "ingested_files"), "1");
    assert_eq!(count(&db, "sessions"), "1");
    assert_eq!(
        dump_meta(&db, &root),
        "key\tvalue\n\
         analyzer_version\t9\n\
         config_dir\t<ROOT>\n\
         harness\tclaude\n\
         projects_dir\t<ROOT>/projects\n"
    );
    assert_eq!(
        dump_surfaces(&db, &root),
        "kind\tid\tscope\tconfig_path\n\
         skill\tdemo-skill\tglobal\t<ROOT>/skills/demo-skill/SKILL.md\n"
    );

    let db_path = db.clone();
    let projects_again = root.join("projects");
    let second = saniwalens::adapter::claude::analyze(Some(projects_again), &db_path).unwrap();
    assert_eq!((second.sessions, second.skipped), (0, 1));

    clear_env("CLAUDE_CONFIG_DIR");
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
        "INSERT INTO message (id, session_id, time_created, data)
         VALUES ('msg_2', 'ses_1', 1700000001000, '{\"role\":\"assistant\",\"model\":\"synthetic-model\",\"tokens\":{\"input\":100,\"output\":40,\"cache\":{\"read\":900,\"write\":10}}}')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO part (id, message_id, session_id, time_created, data)
         VALUES ('prt_1', 'msg_1', 'ses_1', 1700000000000, '{\"type\":\"text\",\"text\":\"do the thing\"}')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO part (id, message_id, session_id, time_created, data)
         VALUES ('prt_2', 'msg_2', 'ses_1', 1700000001000, '{\"type\":\"text\",\"text\":\"done\"}')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO part (id, message_id, session_id, time_created, data)
         VALUES ('prt_3', 'msg_2', 'ses_1', 1700000002000, '{\"type\":\"tool\",\"tool\":\"skill\",\"callID\":\"call_1\",\"state\":{\"status\":\"completed\",\"input\":{\"name\":\"git-commit\"}}}')",
        [],
    )
    .unwrap();
}

fn opencode_fixture_config(root: &Path) -> PathBuf {
    let cfg = root.join("cfg-home");
    std::fs::create_dir_all(cfg.join("opencode/command")).unwrap();
    std::fs::create_dir_all(cfg.join("opencode/agent")).unwrap();
    std::fs::write(
        cfg.join("opencode/command/review.md"),
        "---\ndescription: review\n---\nbody",
    )
    .unwrap();
    std::fs::write(
        cfg.join("opencode/agent/scout.md"),
        "---\ndescription: scouts\n---\nbody",
    )
    .unwrap();
    cfg
}

#[test]
fn opencode_analyze_pins_stats_catalog_and_meta() {
    let _guard = env_guard();
    let root = ScratchDir::new("saniwalens-parity", "opencode");
    let cfg = opencode_fixture_config(&root);
    let input = root.join("opencode.db");
    opencode_fixture_db(&input);
    isolate_env(&root, "XDG_CONFIG_HOME", &cfg);

    let db = root.join("store.db");
    let stats = saniwalens::adapter::opencode::analyze(Some(input.clone()), &db).unwrap();
    assert_eq!(
        (
            stats.sessions,
            stats.skipped,
            stats.spans_total,
            stats.surface_count,
            stats.denials
        ),
        (1, 0, 1, 2, 0)
    );
    assert_eq!(count(&db, "ingested_files"), "1");
    assert_eq!(count(&db, "sessions"), "1");
    assert_eq!(
        dump_meta(&db, &root),
        "key\tvalue\n\
         config_dir\t<ROOT>/cfg-home/opencode\n\
         harness\topencode\n\
         opencode_analyzer_version\t5\n\
         projects_dir\t<ROOT>/opencode.db\n"
    );
    assert_eq!(
        dump_surfaces(&db, &root),
        "kind\tid\tscope\tconfig_path\n\
         agent\tscout\tglobal\t<ROOT>/cfg-home/opencode/agent/scout.md\n\
         command\treview\tglobal\t<ROOT>/cfg-home/opencode/command/review.md\n"
    );

    let second = saniwalens::adapter::opencode::analyze(Some(input), &db).unwrap();
    assert_eq!((second.sessions, second.skipped), (0, 1));

    clear_env("XDG_CONFIG_HOME");
}

fn kilocode_fixture_tasks(root: &Path) -> PathBuf {
    let tasks = root.join("tasks");
    let task = tasks.join("task-1");
    std::fs::create_dir_all(&task).unwrap();
    std::fs::write(
        task.join("api_conversation_history.json"),
        r#"[{"role":"user","ts":1700000000000,"content":[{"type":"text","text":"fix it"}]},{"role":"assistant","ts":1700000001000,"content":[{"type":"text","text":"ok"}]},{"role":"assistant","ts":1700000002000,"content":[{"type":"tool_use","id":"t1","name":"execute_command","input":{"command":"cargo build --release"}}]},{"role":"user","ts":1700000003000,"content":[{"type":"tool_result","tool_use_id":"t1","is_error":true,"content":"command not found: cargo"}]}]"#,
    )
    .unwrap();
    std::fs::write(
        task.join("ui_messages.json"),
        r#"[{"ts":1700000001000,"type":"say","say":"api_req_started","text":"{\"apiProtocol\":\"example\",\"tokensIn\":12000,\"tokensOut\":400,\"cacheWrites\":800,\"cacheReads\":9000,\"cost\":0.01,\"usageMissing\":false,\"inferenceProvider\":\"example-provider\"}"}]"#,
    )
    .unwrap();
    tasks
}

fn kilocode_fixture_config(root: &Path) -> PathBuf {
    let cfg = root.join("cfg-home");
    std::fs::create_dir_all(cfg.join("kilo/command")).unwrap();
    std::fs::create_dir_all(cfg.join("kilo/agent")).unwrap();
    std::fs::write(
        cfg.join("kilo/command/review.md"),
        "---\ndescription: review\n---\nbody",
    )
    .unwrap();
    std::fs::write(
        cfg.join("kilo/agent/scout.md"),
        "---\ndescription: scouts\n---\nbody",
    )
    .unwrap();
    std::fs::write(cfg.join("kilo/AGENTS.md"), "global instructions").unwrap();
    cfg
}

#[test]
fn kilocode_analyze_pins_stats_catalog_and_meta() {
    let _guard = env_guard();
    let root = ScratchDir::new("saniwalens-parity", "kilocode");
    let cfg = kilocode_fixture_config(&root);
    let tasks = kilocode_fixture_tasks(&root);
    isolate_env(&root, "XDG_CONFIG_HOME", &cfg);

    let db = root.join("store.db");
    let stats = saniwalens::adapter::kilocode::analyze(Some(tasks.clone()), &db).unwrap();
    assert_eq!(
        (
            stats.sessions,
            stats.skipped,
            stats.spans_total,
            stats.surface_count,
            stats.denials
        ),
        (1, 0, 0, 3, 0)
    );
    assert_eq!(count(&db, "ingested_files"), "1");
    assert_eq!(count(&db, "sessions"), "1");
    assert_eq!(
        dump_meta(&db, &root),
        "key\tvalue\n\
         config_dir\t<ROOT>/cfg-home/kilo\n\
         harness\tkilocode\n\
         kilocode_analyzer_version\t5\n\
         projects_dir\t<ROOT>/tasks\n"
    );
    assert_eq!(
        dump_surfaces(&db, &root),
        "kind\tid\tscope\tconfig_path\n\
         agent\tscout\tglobal\t<ROOT>/cfg-home/kilo/agent/scout.md\n\
         claude_md\tAGENTS.md\tglobal\t<ROOT>/cfg-home/kilo/AGENTS.md\n\
         command\treview\tglobal\t<ROOT>/cfg-home/kilo/command/review.md\n"
    );

    let second = saniwalens::adapter::kilocode::analyze(Some(tasks), &db).unwrap();
    assert_eq!((second.sessions, second.skipped), (0, 1));

    clear_env("XDG_CONFIG_HOME");
}
