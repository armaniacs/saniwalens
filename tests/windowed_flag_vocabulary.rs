//! The windowed views' flag vocabulary, end to end through the real binary:
//! `--scope` must actually change the rows a scoped view reports, and
//! `doctor --format markdown` must be refused **before the store opens** — no
//! analyze, no freshness line, no db created (`docs/specs/cli.md`, the
//! flags-before-store contract). A unit test cannot assert the process exit,
//! the streams, or a file's absence; here the binary's own stdio is real and
//! every path it touches is synthetic.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use saniwalens::core::span::SessionStart;
use saniwalens::core::surface::{LoadMode, Scope, Surface};
use saniwalens::store::{IngestBatch, OpenMode, SessionMeta, Store};

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
        paths: Vec::new(),
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

/// The windowed views that take `--scope`, derived from clap the same way as
/// `table_markdown_views`: a view is windowed iff its arguments name `--last`,
/// and scoped iff they name `--scope`. `optimize` takes `--scope` without
/// being a windowed view, so it is not one of these. A view added without the
/// flag is not silently expected to filter, and a view that gains it has to
/// grow a row extractor (`scoped_rows`).
fn scoped_views() -> Vec<String> {
    use clap::CommandFactory;
    saniwalens::cli::Cli::command()
        .get_subcommands()
        .filter(|command| {
            let takes = |flag: &str| {
                command
                    .get_arguments()
                    .any(|arg| arg.get_long() == Some(flag))
            };
            takes("last") && takes("scope")
        })
        .map(|command| command.get_name().to_string())
        .collect()
}

/// The named column out of the first markdown table in `stdout` (the bare
/// form: no framing block to strip). The column is found by its header name,
/// so a reordered table fails on the name rather than silently reading
/// another column.
fn column_values(stdout: &[u8], column: &str) -> Vec<String> {
    let text = String::from_utf8_lossy(stdout);
    let mut rows = text.lines().filter(|line| line.starts_with('|'));
    let header = rows.next().expect("the table has a header row");
    let index = header
        .split('|')
        .position(|cell| cell.trim() == column)
        .unwrap_or_else(|| panic!("the table names no {column} column: {header:?}"));
    rows.skip(1) // the separator under the header
        .filter_map(|line| line.split('|').nth(index))
        .map(|cell| cell.trim().to_string())
        .collect()
}

/// The catalog row's `id` column — what identifies an inventory row.
fn inventory_ids(stdout: &[u8]) -> Vec<String> {
    column_values(stdout, "id")
}

/// A failures row's category out of the `error` column. A globally routed row
/// carries its spread in the same cell (`edit-precondition (across 2
/// projects)`); the row's identity is the category, so the suffix is dropped.
fn failure_categories(stdout: &[u8]) -> Vec<String> {
    column_values(stdout, "error")
        .into_iter()
        .map(|cell| cell.split(" (").next().unwrap_or_default().to_string())
        .collect()
}

/// The `WHAT TO FIX FIRST` lines out of doctor's text report — the routed
/// findings, i.e. the rows `--scope` chooses between. The section runs from
/// its heading to the blank line before the next one.
fn doctor_routed_lines(stdout: &[u8]) -> Vec<String> {
    let text = String::from_utf8_lossy(stdout);
    text.lines()
        .skip_while(|line| !line.contains("WHAT TO FIX FIRST"))
        .skip(1) // the heading itself
        .take_while(|line| !line.trim().is_empty())
        .map(|line| line.trim().to_string())
        .collect()
}

#[test]
fn the_row_extractor_finds_its_column_by_header_name() {
    // The views share the extractor, so its contract is "the column named
    // `id`, wherever it sits": a reordered table still yields the right cells,
    // and a renamed column fails on the name instead of reading a neighbour.
    let reordered = b"| surface | id |\n| --- | --- |\n| skill/a | a |\n| skill/b | b |\n";
    assert_eq!(
        column_values(reordered, "id"),
        vec!["a".to_string(), "b".to_string()]
    );
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

/// A synthetic session stamped at `epoch_secs` (1970) for `project`.
fn session(id: &str, project: &str, epoch_secs: i64) -> SessionMeta {
    SessionMeta {
        id: id.to_string(),
        project: project.to_string(),
        slug: project.to_string(),
        root: String::new(),
        source_path: format!("/tmp/{id}.jsonl"),
        sub_tokens: 0,
        sub_agent_count: 0,
        start: Some(SessionStart {
            timestamp_ms: epoch_secs * 1000,
            ctx: 9_000,
        }),
    }
}

/// A synthetic store whose two layers really differ: one unused skill per
/// layer, one failure category spread across two projects (no majority owner,
/// so it routes to global) and one only `demo` has (routed to that project).
/// `write_store`'s surfaces alone cannot move a waste / failures / doctor row:
/// waste needs a usage join, failures need a failed tool_use, and doctor needs
/// sessions to report at all.
fn write_scoped_store(dir: &Path) -> PathBuf {
    let db = dir.join("store.db");
    {
        let mut store = Store::open(&db, OpenMode::ReadWrite).unwrap();
        store
            .replace_surfaces(&[
                surface("global-skill", Scope::Global),
                surface("project-skill", Scope::Project("demo".to_string())),
            ])
            .unwrap();
        let demo = session("demo-session", "demo", 100_000);
        store
            .ingest(IngestBatch::new(&demo, &[], &[]).tool_errors(&[
                (
                    100_000_000,
                    "edit-precondition",
                    "String to replace not found",
                    "Edit",
                    "main.rs",
                ),
                (
                    100_001_000,
                    "test-failure",
                    "1 test failed",
                    "Bash",
                    "cargo test",
                ),
            ]))
            .unwrap();
        let other = session("other-session", "other", 100_000);
        store
            .ingest(IngestBatch::new(&other, &[], &[]).tool_errors(&[(
                100_000_000,
                "edit-precondition",
                "File has been modified",
                "Edit",
                "lib.rs",
            )]))
            .unwrap();
    }
    db
}

/// A row extractor: the binary's stdout to the row identities a view reports.
type RowExtractor = fn(&[u8]) -> Vec<String>;

/// Run `view` under `scope` and return the rows it identifies a layer by.
/// Doctor has no markdown form, so it is read as its text report; the others
/// are markdown tables. A scoped view without an extractor fails loudly here
/// rather than being skipped.
fn scoped_rows(dir: &Path, db: &Path, view: &str, scope: &str) -> Vec<String> {
    let (format, extract): (&str, RowExtractor) = match view {
        "inventory" => ("markdown", inventory_ids),
        "waste" => ("markdown", |stdout| column_values(stdout, "surface")),
        "failures" => ("markdown", failure_categories),
        "doctor" => ("table", doctor_routed_lines),
        other => {
            panic!("no row extractor for the scoped view {other}: add one so --scope stays pinned")
        }
    };
    let out = run(
        dir,
        db,
        &[view, "--scope", scope, "--format", format, "--frozen"],
    );
    assert!(
        out.status.success(),
        "{view} --scope {scope} must run: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    extract(&out.stdout)
}

#[test]
fn the_scope_flag_changes_the_rows_in_every_scoped_view() {
    // Every view that takes `--scope` renders through the same filter, but
    // each has its own rows: wiring one arm to `All` (or the other layer)
    // leaves the flag accepted and that view's rows unchanged. The set is
    // derived from clap, so a scoped view added later has to be exercised
    // here too. Both directions are asserted: a filter that returns everything
    // keeps the other layer's rows, which makes the sets intersect.
    let dir = scratch_dir("scope-rows");
    let db = write_scoped_store(&dir);
    let views = scoped_views();
    assert!(
        !views.is_empty(),
        "no view takes --scope: this fixture checks nothing"
    );
    for view in views {
        let global: BTreeSet<String> = scoped_rows(&dir, &db, &view, "global")
            .into_iter()
            .collect();
        let project: BTreeSet<String> = scoped_rows(&dir, &db, &view, "project")
            .into_iter()
            .collect();
        assert!(
            !global.is_empty(),
            "{view}: --scope global rendered no rows"
        );
        assert!(
            !project.is_empty(),
            "{view}: --scope project rendered no rows"
        );
        assert!(
            global.is_disjoint(&project),
            "{view}: a row rendered under both scopes: {:?}",
            global.intersection(&project).collect::<Vec<_>>()
        );
    }
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
