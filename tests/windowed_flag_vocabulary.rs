//! The windowed views' flag vocabulary, end to end through the real binary:
//! `--scope` must actually change the rows a scoped view reports, and
//! `doctor --format markdown` must be refused **before the store opens** — no
//! analyze, no freshness line, no db created (`docs/specs/cli.md`, the
//! flags-before-store contract). A unit test cannot assert the process exit,
//! the streams, or a file's absence; here the binary's own stdio is real and
//! every path it touches is synthetic.

use std::path::{Path, PathBuf};

use saniwalens::core::surface::{LoadMode, Scope, Surface};
use saniwalens::store::{OpenMode, Store};

mod common;
use common::ScratchDir;

/// A per-test scratch dir holding an empty synthetic transcript root, so a
/// non-frozen read walks a real (empty) tree instead of the user's.
fn scratch_dir(name: &str) -> ScratchDir {
    let dir = ScratchDir::new("saniwalens-flag-vocabulary", name);
    std::fs::create_dir_all(dir.join(".claude").join("projects")).unwrap();
    dir
}

/// The windowed views whose `--format` vocabulary is the shared three-value
/// one, derived from clap and minus doctor (whose human form is its own text
/// layout). The same `--last` discriminator the help-golden tripwire uses.
fn table_markdown_views() -> Vec<String> {
    use clap::CommandFactory;
    saniwalens::cli::Cli::command()
        .get_subcommands()
        .filter(|command| {
            command
                .get_arguments()
                .any(|arg| arg.get_long() == Some("last"))
        })
        .map(|command| command.get_name().to_string())
        .filter(|name| name != "doctor")
        .collect()
}

/// One catalog row: a skill installed in `scope`, weighing 40 tokens.
fn surface(id: &str, scope: Scope) -> Surface {
    Surface {
        kind: "skill".to_string(),
        id: id.to_string(),
        scope,
        config_path: format!("/tmp/{id}/SKILL.md"),
        static_tokens: Some(40),
        startup_tokens: Some(40),
        load_mode: LoadMode::StartupDescription,
    }
}

/// A synthetic store holding one global skill and one skill installed by
/// project `demo` — the two layers `--scope` chooses between.
fn write_store(dir: &Path) -> PathBuf {
    let db = dir.join("store.db");
    let mut store = Store::open(&db, OpenMode::ReadWrite).unwrap();
    store
        .replace_surfaces(&[
            surface("global-skill", Scope::Global),
            surface("project-skill", Scope::Project("demo".to_string())),
        ])
        .unwrap();
    db
}

/// Run the binary on the synthetic home and store, and return its raw output.
/// `db` is appended so every case reads the same store. The XDG config/data
/// roots are removed rather than redirected: the ambient ones would let the
/// binary read the user's real `config.toml` or profiles, and this file's
/// contract is that every path it touches is synthetic.
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

/// The catalog row's `id` column out of a markdown table (the bare form: no
/// framing block to strip). The column is found by its header name, so a
/// reordered table fails on the name rather than silently reading another
/// column.
fn inventory_ids(stdout: &[u8]) -> Vec<String> {
    let text = String::from_utf8_lossy(stdout);
    let mut rows = text.lines().filter(|line| line.starts_with('|'));
    let header = rows.next().expect("the table has a header row");
    let id = header
        .split('|')
        .position(|cell| cell.trim() == "id")
        .expect("the inventory table names its id column");
    rows.skip(1) // the separator under the header
        .filter_map(|line| line.split('|').nth(id))
        .map(|cell| cell.trim().to_string())
        .collect()
}

#[test]
fn the_scope_flag_changes_the_rows_inventory_reports() {
    // The scoped arms resolve `--scope` into the filter the view renders
    // through; wiring the filter to the wrong value (`All`, or the other
    // layer) leaves every other assertion in this repository green — the flag
    // would be accepted and silently ignored. The two layers' rows must
    // actually differ, in both directions.
    let dir = scratch_dir("scope");
    let db = write_store(&dir);
    let scoped = |scope: &str| {
        let out = run(
            &dir,
            &db,
            &[
                "inventory",
                "--scope",
                scope,
                "--format",
                "markdown",
                "--frozen",
            ],
        );
        assert!(
            out.status.success(),
            "inventory --scope {scope} must run: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        inventory_ids(&out.stdout)
    };

    let global = scoped("global");
    let project = scoped("project");

    assert!(
        global.contains(&"global-skill".to_string()),
        "--scope global dropped the global row: {global:?}"
    );
    assert!(
        !global.contains(&"project-skill".to_string()),
        "--scope global kept a project row: {global:?}"
    );
    assert!(
        project.contains(&"project-skill".to_string()),
        "--scope project dropped the project row: {project:?}"
    );
    assert!(
        !project.contains(&"global-skill".to_string()),
        "--scope project kept a global row: {project:?}"
    );
    assert_ne!(global, project, "the flag changed nothing");
}

#[test]
fn only_doctor_refuses_markdown() {
    // The vocabulary is per view, not global: the eight table views (trend
    // included) take `markdown`, and doctor — whose human form is its own
    // text layout — does not. Narrowing doctor without narrowing the rest (or
    // widening it back) fails here.
    let dir = scratch_dir("markdown-values");
    let db = write_store(&dir);
    for view in table_markdown_views() {
        let out = run(
            &dir,
            &db,
            &[view.as_str(), "--format", "markdown", "--frozen"],
        );
        assert!(
            out.status.success(),
            "{view} --format markdown must run: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    let out = run(&dir, &db, &["doctor", "--format", "markdown", "--frozen"]);
    assert!(!out.status.success(), "doctor must refuse markdown");
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("unknown --format 'markdown' (table|json)"),
        "doctor must name its own values: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn doctor_rejects_markdown_before_the_store_opens() {
    // The order is the contract: an unaccepted `--format` must fail on its
    // own, before the analyze refresh, the freshness line, and the store's
    // creation. Under the auto-refresh default the store used to be created
    // first and the refusal arrived afterwards.
    let dir = scratch_dir("doctor-markdown");
    let db = dir.join("store.db");
    let out = run(&dir, &db, &["doctor", "--format", "markdown"]);

    assert!(
        !out.status.success(),
        "doctor --format markdown must fail, not render: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        out.stdout.is_empty(),
        "stdout must stay empty on the error path: {:?}",
        String::from_utf8_lossy(&out.stdout)
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("unknown --format 'markdown' (table|json)"),
        "the refusal must name the values doctor accepts: {stderr}"
    );
    assert!(
        !stderr.contains("store: refreshed"),
        "the analyze refresh ran ahead of the flag refusal: {stderr}"
    );
    assert!(
        !stderr.contains("store: analyzed") && !stderr.contains("store: freshness"),
        "a freshness line rode ahead of the flag refusal: {stderr}"
    );
    assert!(
        !db.exists(),
        "the store was created before the flag was refused: {}",
        db.display()
    );
}
