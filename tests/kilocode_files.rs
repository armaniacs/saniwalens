//! Files-in-context end to end (PBI 23): a kilocode task whose
//! `task_metadata.json` names files gets `file_read` events; a task without
//! metadata ingests exactly as before. Synthetic task dirs only.

use std::path::Path;

mod common;
use common::ScratchDir;

fn scratch_dir(name: &str) -> ScratchDir {
    let dir = ScratchDir::new("saniwalens-kilo-files", name);
    std::fs::create_dir_all(dir.join(".claude").join("projects")).unwrap();
    dir
}

fn write_task(tasks: &Path, id: &str, metadata: Option<&str>) {
    let dir = tasks.join(id);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("api_conversation_history.json"),
        r#"[{"role":"user","ts":1700000000000,"content":[{"type":"text","text":"aaa"}]},{"role":"assistant","ts":1700000000000,"content":[{"type":"text","text":"ok"}]}]"#,
    )
    .unwrap();
    std::fs::write(dir.join("ui_messages.json"), "[]").unwrap();
    if let Some(meta) = metadata {
        std::fs::write(dir.join("task_metadata.json"), meta).unwrap();
    }
}

fn run(dir: &Path, args: &[&str]) -> std::process::Output {
    std::process::Command::new(env!("CARGO_BIN_EXE_saniwalens"))
        .args(args)
        .env("HOME", dir)
        .env("XDG_CONFIG_HOME", dir.join("cfg-home"))
        .env("CLAUDE_CONFIG_DIR", dir.join(".claude"))
        .env("XDG_STATE_HOME", dir.join("state"))
        .env_remove("XDG_DATA_HOME")
        .output()
        .expect("the binary runs")
}

#[test]
fn metadata_files_become_file_read_events_and_missing_metadata_stays_quiet() {
    let dir = scratch_dir("files-in-context");
    let tasks = dir.join("tasks");
    write_task(
        &tasks,
        "task-1",
        Some(r#"{"taskId":"task-1","files_in_context":["/repo/src/main.rs","/repo/docs/a.md"]}"#),
    );
    write_task(&tasks, "task-2", None);
    std::fs::create_dir_all(dir.join("cfg-home")).unwrap();

    let db = dir.join("kilo.db");
    let db_arg = db.display().to_string();
    let out = run(
        &dir,
        &[
            "--harness",
            "kilocode",
            "analyze",
            "--projects",
            &tasks.display().to_string(),
            "--db",
            &db_arg,
        ],
    );
    assert!(out.status.success(), "kilocode analyze exits 0: {out:?}");

    // The reads are presented through the store: two files for task-1, none
    // for task-2, and nothing from any other harness's vocabulary.
    let out = run(
        &dir,
        &[
            "--harness",
            "kilocode",
            "sql",
            "--db",
            &db_arg,
            "SELECT session_id, surface_id FROM events WHERE kind = 'file_read' ORDER BY session_id, surface_id",
        ],
    );
    assert!(out.status.success(), "sql exits 0: {out:?}");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("task-1"), "task-1 reads recorded: {stdout}");
    assert!(
        stdout.contains("main.rs"),
        "the read file is named: {stdout}"
    );
    assert!(stdout.contains("a.md"), "both files recorded: {stdout}");
    assert!(
        !stdout.contains("task-2"),
        "the metadata-less task stays quiet: {stdout}"
    );
}

/// VULN-001 (CWE-117): a hostile `kilo.json` instructions entry must not
/// reach the stderr skip-warning verbatim — raw CR/LF would forge log lines
/// and a raw ESC would restyle the terminal. The warning only fires for an
/// entry that resolves to a real file outside the config root, so the
/// payload lives in the outside file's name (legal on unix). Confinement is
/// unchanged: the entry is still skipped, loudly, on one sanitized line.
#[test]
fn hostile_instructions_entry_is_neutralized_in_stderr_warning() {
    let dir = scratch_dir("log-neutralization");
    let outside = dir.join("outside");
    std::fs::create_dir_all(&outside).unwrap();
    let hostile_name = "pwned\r\nFORGED-VULN-001-LINE\n\x1b[31mRED-VULN-001\x1b[0m.md";
    std::fs::write(outside.join(hostile_name), "host secret").unwrap();
    let entry = format!("{}/{}", outside.display(), hostile_name);

    let cfg_kilo = dir.join("cfg-home").join("kilo");
    std::fs::create_dir_all(&cfg_kilo).unwrap();
    std::fs::write(
        cfg_kilo.join("kilo.json"),
        serde_json::to_string(&serde_json::json!({ "instructions": [entry] })).unwrap(),
    )
    .unwrap();

    let tasks = dir.join("tasks");
    write_task(&tasks, "task-1", None);
    std::fs::create_dir_all(dir.join("cfg-home")).unwrap();

    let db = dir.join("kilo.db");
    let db_arg = db.display().to_string();
    let out = run(
        &dir,
        &[
            "--harness",
            "kilocode",
            "analyze",
            "--projects",
            &tasks.display().to_string(),
            "--db",
            &db_arg,
        ],
    );
    assert!(out.status.success(), "kilocode analyze exits 0: {out:?}");

    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("skipping kilo.json instructions entry"),
        "the skip warning still fires: {stderr:?}"
    );
    assert!(
        !stderr.contains('\x1b'),
        "no ESC byte from the entry in stderr: {stderr:?}"
    );
    assert!(
        !stderr
            .lines()
            .any(|line| line.trim() == "FORGED-VULN-001-LINE"),
        "no forged standalone line from the entry in stderr: {stderr:?}"
    );
    assert!(!stderr.contains('\r'), "no CR byte in stderr: {stderr:?}");
}
