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
