//! Byte-compare goldens for the opencode and kilocode analyzes (PBI 24):
//! the same synthetic fixture must always produce the same stored rows.
//! The dump covers sessions + events with temp paths normalized to `<ROOT>`,
//! so only real behavior changes flip the comparison — and the failure
//! prints both sides. Intentional output changes re-bless with
//! `UPDATE_GOLDEN=1` (then review the diff before committing).
//! Everything touched is synthetic temp data (privacy rule).

use std::path::{Path, PathBuf};

use saniwalens::store::{OpenMode, Store};

mod common;
use common::ScratchDir;

fn scratch_root(name: &str) -> ScratchDir {
    ScratchDir::new("saniwalens-golden", name)
}

fn golden_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("golden")
        .join(name)
}

/// Deterministic dump: sessions and events only, ordered, with the scratch
/// root normalized away. Newlines are `\n`-joined with a trailing newline.
fn dump_store(db: &Path, root: &Path) -> String {
    let store = Store::open(db, OpenMode::ReadOnly).unwrap();
    let mut out = String::new();
    let (scols, srows) = store
        .query("SELECT id, project, slug, started_at FROM sessions ORDER BY id")
        .unwrap();
    out.push_str(&scols.join("\t"));
    out.push('\n');
    for row in srows {
        out.push_str(&row.join("\t"));
        out.push('\n');
    }
    out.push_str("---\n");
    let (ecols, erows) = store
        .query(
            "SELECT session_id, kind, surface_kind, surface_id, model, target, \
             out_tokens, started_epoch FROM events \
             ORDER BY session_id, started_epoch, kind, target",
        )
        .unwrap();
    out.push_str(&ecols.join("\t"));
    out.push('\n');
    for row in erows {
        out.push_str(&row.join("\t"));
        out.push('\n');
    }
    out.push_str("---\n");
    let (tcols, trows) = store
        .query(
            "SELECT session_id, tool, target, category, excerpt FROM tool_errors \
             ORDER BY session_id, tool, target",
        )
        .unwrap();
    out.push_str(&tcols.join("\t"));
    out.push('\n');
    for row in trows {
        out.push_str(&row.join("\t"));
        out.push('\n');
    }
    out.replace(&root.display().to_string(), "<ROOT>")
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

fn fixture_opencode_db(path: &Path) {
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

fn fixture_kilocode_tasks(root: &Path) -> PathBuf {
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

fn fixture_claude_projects(root: &Path) -> PathBuf {
    let projects = root.join("projects/-tmp-example-proj");
    std::fs::create_dir_all(&projects).unwrap();
    std::fs::write(
        projects.join("aaaaaaaa-0000-0000-0000-000000000001.jsonl"),
        concat!(
            r#"{"type":"user","cwd":"/tmp/example/proj","timestamp":"2026-01-01T00:00:00.000Z","message":{"content":"<command-name>/git-commit</command-name>"}}"#,
            "\n",
            r#"{"type":"assistant","timestamp":"2026-01-01T00:00:01.000Z","message":{"model":"claude-opus-4-7","usage":{"input_tokens":10,"cache_read_input_tokens":90,"cache_creation_input_tokens":0,"output_tokens":40}}}"#,
            "\n",
            r#"{"type":"assistant","timestamp":"2026-01-01T00:00:03.000Z","message":{"model":"claude-opus-4-7","usage":{"input_tokens":0,"cache_read_input_tokens":250,"cache_creation_input_tokens":0,"output_tokens":60}}}"#,
            "\n",
            r#"{"type":"user","timestamp":"2026-01-01T00:00:05.000Z","message":{"content":"thanks"}}"#,
        ),
    )
    .unwrap();
    root.join("projects")
}

#[test]
fn claude_analyze_matches_its_golden() {
    let root = scratch_root("claude");
    let projects = fixture_claude_projects(&root);
    let db = root.join("store.db");
    let stats = saniwalens::adapter::claude::analyze(Some(projects), &db).unwrap();
    assert_eq!((stats.sessions, stats.skipped), (1, 0));
    check_golden("claude.txt", &dump_store(&db, &root));
}

#[test]
fn opencode_analyze_matches_its_golden() {
    let root = scratch_root("opencode");
    let input = root.join("opencode.db");
    fixture_opencode_db(&input);
    let db = root.join("store.db");
    let stats = saniwalens::adapter::opencode::analyze(Some(input), &db).unwrap();
    assert_eq!((stats.sessions, stats.skipped), (1, 0));
    check_golden("opencode.txt", &dump_store(&db, &root));
}

#[test]
fn kilocode_analyze_matches_its_golden() {
    let root = scratch_root("kilocode");
    let tasks = fixture_kilocode_tasks(&root);
    let db = root.join("store.db");
    let stats = saniwalens::adapter::kilocode::analyze(Some(tasks), &db).unwrap();
    assert_eq!((stats.sessions, stats.skipped), (1, 0));
    check_golden("kilocode.txt", &dump_store(&db, &root));
}
