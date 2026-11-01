//! Prose reference-integrity gate for `docs/specs/*.md`, `.claude/rules/*.md`,
//! `README.md`, and `AGENTS.md` (PBI 35).
//!
//! A prose file-path reference lives outside the Rust symbol system, so nothing
//! else fails when a file is renamed or moved and its prose anchors die: the
//! dangling `cli.rs` references in `docs/specs/harness.md` survived review
//! because every guarantee before this gate was attention plus diff scope, and
//! an anchor in a file the diff never touches is unreachable from there. This
//! gate makes anchor rot fail `cargo test` like any other broken contract.
//!
//! The rule, pinned: a single-backtick span that is exactly a `.rs` path is an
//! anchor. It resolves when the file exists as written from the manifest root,
//! under `src/`, or under `src/adapter/` — the two bases the specs'
//! src-relative notation (`cli/mod.rs`, `store.rs`, `adapter/config.rs`) is
//! written against. Fenced code blocks hold illustrative code, not anchors, so
//! they are skipped; double-backtick spans are not scanned. History
//! (`docs/pr`, `docs/exploration`, `pbi/`, `CHANGELOG.md`) and src fixture
//! strings are out of scope by construction: the scan list is the current prose
//! only, which is `docs/specs/*.md`, `.claude/rules/*.md`, `README.md`, and
//! `AGENTS.md` (AGENTS.md points at `.claude/rules` as the project guardrails).
//! Both sides are classified from root-relative path components
//! (`rel` / `is_current_prose` / `is_history`), never from a rendered absolute
//! path, so a checkout below `/tmp/pbi-work` or `C:\pbi\saniwalens`, a `\`
//! separator, and a lookalike component (`docs/privacy.md`, `pbi-notes.md`,
//! `docs/specs-history/`) all keep the same verdict. History therefore stays
//! untouched and cannot fail the gate.

use std::ffi::OsStr;
use std::path::{Component, Path, PathBuf};

/// One dangling anchor, reported with the file and 1-based line that hold it.
#[derive(Debug, PartialEq, Eq)]
struct Miss {
    file: String,
    line: usize,
    reference: String,
}

/// A span is an anchor only when it is exactly a `.rs` path: no whitespace, no
/// prose, nothing but path characters. A span naming two files (`a.rs and
/// b.rs`) or trailing prose is not a path, and a bare `.rs` names nothing.
fn is_rs_path(span: &str) -> bool {
    span.len() > 3
        && span.ends_with(".rs")
        && span
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '/' | '_' | '-'))
}

/// Inline code spans on one line: the odd segments between backticks. A plain
/// (unbackticked) mention of a file is text, not an anchor.
fn refs_in_line(line: &str) -> Vec<&str> {
    line.split('`')
        .enumerate()
        .filter(|(i, _)| i % 2 == 1)
        .map(|(_, span)| span)
        .filter(|span| is_rs_path(span))
        .collect()
}

/// A triple-backtick fence opens and closes where its first non-space token is
/// one; its body is illustrative code and is never scanned.
fn fence_boundary(line: &str) -> bool {
    line.trim_start().starts_with("```")
}

/// A reference resolves when it exists as written from the manifest root, or
/// under `src/`, or under `src/adapter/`. The two prefixes are the bases the
/// specs' src-relative notation is written against (`store.rs` and
/// `cli/mod.rs` name files under `src/`; `config.rs` and `transcript.rs` are
/// written as `adapter/config.rs` and `adapter/transcript.rs`).
fn resolves(root: &Path, reference: &str) -> bool {
    [
        root.join(reference),
        root.join("src").join(reference),
        root.join("src").join("adapter").join(reference),
    ]
    .iter()
    .any(|candidate| candidate.is_file())
}

/// Dangling anchors in one file's text. `display` is the root-relative path the
/// report shows. Exposed with the text as a parameter so the boundary tests pin
/// the extraction on synthetic fixtures — real docs are never rewritten for a
/// test.
fn file_misses(root: &Path, display: &str, text: &str) -> Vec<Miss> {
    let mut in_fence = false;
    let mut misses = Vec::new();
    for (index, line) in text.lines().enumerate() {
        if fence_boundary(line) {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }
        for reference in refs_in_line(line) {
            if !resolves(root, reference) {
                misses.push(Miss {
                    file: display.to_string(),
                    line: index + 1,
                    reference: reference.to_string(),
                });
            }
        }
    }
    misses
}

/// Every `.md` file directly under one directory, sorted. Listing fails loud:
/// a missing directory is a broken scan, not an empty one.
fn md_files_under(root: &Path, dir: &Path) -> Vec<PathBuf> {
    let base = root.join(dir);
    let mut files: Vec<PathBuf> = std::fs::read_dir(&base)
        .unwrap_or_else(|e| panic!("cannot list {}: {e}", base.display()))
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == OsStr::new("md")))
        .collect();
    files.sort();
    files
}

/// Directories whose files are history: records of what was true once, never
/// anchors. Matched as whole path components, so `docs/privacy.md` is not under
/// `docs/pr`, `docs/specs-history/` is not under `docs/specs/`, and
/// `pbi-notes.md` is not under `pbi/`.
const HISTORY_DIRS: &[&str] = &["docs/pr", "docs/exploration", "pbi"];

/// Root-level files whose text is history rather than current prose.
const HISTORY_FILES: &[&str] = &["CHANGELOG.md"];

/// Directories whose files are the current prose the gate scans.
const CURRENT_PROSE_DIRS: &[&str] = &["docs/specs", ".claude/rules"];

/// Root-level files that are current prose.
const CURRENT_PROSE_FILES: &[&str] = &["README.md", "AGENTS.md"];

/// `file` relative to the manifest root, or `None` when it lies outside it.
/// The classification below never reads a rendered path string: components are
/// the contract, so a checkout under `/tmp/pbi-work` (or `C:\pbi\saniwalens`)
/// yields the same verdict as one under `/tmp/repo`, and `strip_prefix` compares
/// components rather than a textual prefix (a sibling `saniwalens-old` is
/// outside). On Windows `Path` splits `\` into the same components.
fn rel<'a>(root: &Path, file: &'a Path) -> Option<&'a Path> {
    file.strip_prefix(root).ok()
}

/// Whether a root-relative path is history. Only a whole leading component
/// counts: `docs/privacy.md`, `pbi-notes.md`, and `docs/specs-history/x.md` are
/// not history, and a `..` (ParentDir) component never matches a prefix.
fn is_history(relative: &Path) -> bool {
    HISTORY_DIRS.iter().any(|dir| relative.starts_with(dir))
        || HISTORY_FILES.iter().any(|file| relative == Path::new(file))
}

/// Whether a root-relative path is current prose: `docs/specs/*.md`,
/// `.claude/rules/*.md`, `README.md`, or `AGENTS.md`. The positive twin of
/// `is_history`, matched the same way, so neither side can drift into the
/// other through a lookalike prefix.
fn is_current_prose(relative: &Path) -> bool {
    CURRENT_PROSE_DIRS
        .iter()
        .any(|dir| relative.starts_with(dir))
        || CURRENT_PROSE_FILES
            .iter()
            .any(|file| relative == Path::new(file))
}

/// The current prose only: `docs/specs/*.md`, `.claude/rules/*.md`, plus
/// `README.md` and `AGENTS.md`. History (`docs/pr`, `docs/exploration`, `pbi/`,
/// `CHANGELOG.md`) is deliberately absent: stale paths in history are records,
/// not anchors. `is_current_prose` re-checks every returned path relative to
/// the root, so this list cannot drift into history unnoticed.
fn scan_files(root: &Path) -> Vec<PathBuf> {
    let mut files = vec![root.join("README.md"), root.join("AGENTS.md")];
    files.extend(md_files_under(root, Path::new("docs/specs")));
    files.extend(md_files_under(root, Path::new(".claude/rules")));
    files
}

/// Every dangling anchor across the current prose.
fn collect_misses(root: &Path) -> Vec<Miss> {
    let mut misses = Vec::new();
    for path in scan_files(root) {
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
        // A scan file outside the root is a broken scan, not a display
        // problem: report it as a gate failure instead of showing an absolute
        // path and classifying it by its checkout location.
        let display = rel(root, &path).unwrap_or_else(|| {
            panic!(
                "gate failure: scan file {} is outside the manifest root {}",
                path.display(),
                root.display()
            )
        });
        misses.extend(file_misses(root, &display.to_string_lossy(), &text));
    }
    misses
}

/// The miss list a failing gate prints: one line per anchor, then the remedy.
fn format_misses(misses: &[Miss]) -> String {
    let mut report = String::from(
        "dangling .rs references in prose (docs/specs/*.md, .claude/rules/*.md, \
         README.md, AGENTS.md):\n",
    );
    for miss in misses {
        report.push_str(&format!(
            "  {}:{}: `{}`\n",
            miss.file, miss.line, miss.reference
        ));
    }
    report.push_str(
        "re-point each anchor at the renamed or moved file (src doc comments, \
         docs/specs/, .claude/rules/, README, AGENTS); docs/pr, \
         docs/exploration, pbi/ and CHANGELOG.md are history and out of scope.",
    );
    report
}

#[test]
fn prose_rs_references_resolve() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let misses = collect_misses(&root);
    assert!(misses.is_empty(), "{}", format_misses(&misses));
}

#[test]
fn a_dangling_reference_is_reported_with_file_line_and_ref() {
    // BDD: the gate fails on a nonexistent `src/foo.rs` backtick reference and
    // prints the miss list. The fixture is a synthetic string — real docs are
    // never rewritten to exercise the failure path.
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let misses = file_misses(root, "docs/specs/harness.md", "see `src/foo.rs` here\n");
    assert_eq!(misses.len(), 1, "{misses:?}");
    assert_eq!(misses[0].file, "docs/specs/harness.md");
    assert_eq!(misses[0].line, 1);
    assert_eq!(misses[0].reference, "src/foo.rs");
    let report = format_misses(&misses);
    assert!(
        report.contains("docs/specs/harness.md:1: `src/foo.rs`"),
        "the report must list the miss with its location: {report}"
    );
}

#[test]
fn every_occurrence_of_a_dead_anchor_is_listed() {
    // The miss list is a list, not a first hit: two dangling anchors in one
    // file are two lines, or a repair would stop at the first one.
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let misses = file_misses(
        root,
        "docs/specs/harness.md",
        "one `cli.rs` here\nand `cli.rs` there\n",
    );
    assert_eq!(
        misses,
        vec![
            Miss {
                file: "docs/specs/harness.md".to_string(),
                line: 1,
                reference: "cli.rs".to_string(),
            },
            Miss {
                file: "docs/specs/harness.md".to_string(),
                line: 2,
                reference: "cli.rs".to_string(),
            },
        ]
    );
}

#[test]
fn only_backticked_spans_are_anchors() {
    // A plain-text mention is prose, and a span holding two names or trailing
    // prose is not a path. Only a span that is exactly a `.rs` path anchors.
    let refs = refs_in_line("plain foo.rs text, `cli/mod.rs` inline, `a.rs and b.rs` two names");
    assert_eq!(refs, vec!["cli/mod.rs"]);
    assert!(!is_rs_path(".rs"), "a bare .rs names no file");
    // BDD: a double-backtick span (``src/foo.rs``) is no anchor either — the
    // doubled backticks pair off into empty odd spans, so nothing is scanned.
    assert_eq!(refs_in_line("``src/foo.rs``"), Vec::<&str>::new());
}

#[test]
fn fenced_code_blocks_are_not_scanned() {
    // Pinned boundary: a fence holds illustrative code, not anchors. The ref
    // inside the fence is skipped; the inline ref outside it still resolves
    // (and here misses, since the synthetic path exists nowhere).
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let text = "before `src/bar.rs` after\n\
                ```\n\
                ignore `src/foo.rs` inside a fence\n\
                ```\n\
                `store.rs` after the fence\n";
    let misses = file_misses(root, "docs/specs/harness.md", text);
    let references: Vec<&str> = misses.iter().map(|miss| miss.reference.as_str()).collect();
    assert_eq!(references, vec!["src/bar.rs"], "{misses:?}");
}

#[test]
fn resolution_matches_the_real_tree() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    // Multi-segment: as written (src/- and tests/-prefixed) and src-relative.
    assert!(resolves(root, "src/cli/mod.rs"));
    assert!(resolves(root, "cli/mod.rs"));
    assert!(resolves(root, "tests/cli_mod_line_budget.rs"));
    // Single-segment: the src/ root.
    assert!(resolves(root, "store.rs"));
    assert!(resolves(root, "session.rs"));
    // Single-segment: the src/adapter/ base (the unified notation).
    assert!(resolves(root, "adapter/config.rs"));
    assert!(resolves(root, "adapter/transcript.rs"));
    // The dead anchors this gate exists for: no src/cli.rs and no
    // src/adapter/cli.rs exist anywhere.
    assert!(!resolves(root, "cli.rs"));
}

#[test]
fn the_scan_list_is_current_prose_only() {
    // BDD: history and fixtures are out of scope. `docs/pr`,
    // `docs/exploration`, `pbi/`, and `CHANGELOG.md` hold stale paths as
    // records; the scan list never contains them, so history cannot fail the
    // gate and stays untouched. src fixture strings are .rs, never scanned at
    // all. `.claude/rules` is current prose too (the guardrails AGENTS.md
    // points at), so the scan list must contain it (PBI 36).
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let files = scan_files(root);

    // The contract, in one place: every scanned file is classified from its
    // root-relative path, so the checkout location cannot move it out of the
    // current prose or into history.
    for file in &files {
        let relative = rel(root, file).unwrap_or_else(|| {
            panic!(
                "gate failure: scan file {} is outside the manifest root {}",
                file.display(),
                root.display()
            )
        });
        assert!(
            is_current_prose(relative),
            "{} is not current prose",
            relative.display()
        );
        assert!(
            !is_history(relative),
            "{} is history and must not be scanned",
            relative.display()
        );
    }

    assert!(files.contains(&root.join("README.md")));
    assert!(files.contains(&root.join("AGENTS.md")));
    assert!(files.contains(&root.join("docs/specs/harness.md")));
    assert!(files.contains(&root.join(".claude/rules/spec-sync.md")));

    // The real tree keeps its history directories and file, so the negative
    // side is not vacuous, and nothing under them is scanned.
    for &history in HISTORY_DIRS {
        let dir = root.join(history);
        assert!(
            dir.is_dir(),
            "history directory {history} is missing from the real tree"
        );
        assert!(
            !files
                .iter()
                .any(|file| rel(root, file).is_some_and(|relative| relative.starts_with(history))),
            "{history} must not be scanned: {files:?}"
        );
    }
    for &history in HISTORY_FILES {
        assert!(
            root.join(history).is_file(),
            "history file {history} is missing from the real tree"
        );
        assert!(
            !files
                .iter()
                .any(|file| rel(root, file) == Some(Path::new(history))),
            "{history} must not be scanned: {files:?}"
        );
    }
}

#[test]
fn classification_is_by_component_not_by_absolute_path_substring() {
    // BDD: a checkout under `/tmp/pbi-work/saniwalens` must not turn current
    // prose into history, whether the root spells `pbi` or `docs/pr`. Each
    // path is judged after `rel`, so the root's own spelling never enters, and
    // a whole component (not a textual prefix) decides the rest.
    let root = Path::new("/tmp/pbi-work/saniwalens");
    for (path, current, history) in [
        ("README.md", true, false),
        ("AGENTS.md", true, false),
        ("docs/specs/architecture.md", true, false),
        (".claude/rules/spec-sync.md", true, false),
        ("docs/pr/2026-01-01-01-x.md", false, true),
        ("docs/exploration/x.md", false, true),
        ("pbi/2026-01-01-43-x.md", false, true),
        ("CHANGELOG.md", false, true),
        // Lookalike names: `docs/privacy.md` is not `docs/pr`, `pbi-notes.md`
        // is not `pbi/`, and `docs/specs-history/` is not `docs/specs/`.
        ("docs/privacy.md", false, false),
        ("pbi-notes.md", false, false),
        ("docs/specs-history/x.md", false, false),
    ] {
        let file = root.join(path);
        let relative = rel(root, &file)
            .unwrap_or_else(|| panic!("gate failure: {path} is outside the synthetic root"));
        assert_eq!(relative, Path::new(path), "{path} must stay root-relative");
        assert_eq!(
            is_current_prose(relative),
            current,
            "current prose verdict for {path}"
        );
        assert_eq!(is_history(relative), history, "history verdict for {path}");
    }
}

#[test]
fn rel_is_component_wise_and_rejects_paths_outside_the_root() {
    // `strip_prefix` compares components, so a sibling whose name merely
    // starts with the root's name is outside, and a lookalike directory is not
    // a prefix match.
    let root = Path::new("/tmp/pbi-work/saniwalens");
    assert_eq!(
        rel(root, &root.join("docs/specs/architecture.md")),
        Some(Path::new("docs/specs/architecture.md"))
    );
    assert_eq!(
        rel(root, Path::new("/tmp/pbi-work/saniwalens-old/README.md")),
        None
    );
    assert_eq!(rel(root, Path::new("/tmp/pbi-work/README.md")), None);
}

#[test]
fn parent_dir_is_not_history() {
    // BDD: `..` is a ParentDir component, not a history prefix. A path that
    // climbs out of the root must not be excused from the gate as history, nor
    // scanned as current prose.
    let climbing = Path::new("../pbi/x.md");
    assert_eq!(climbing.components().next(), Some(Component::ParentDir));
    assert!(!is_history(climbing), "ParentDir must not read as history");
    assert!(
        !is_current_prose(climbing),
        "ParentDir must not read as current prose"
    );
    // The same holds when a history-looking suffix follows `..`: the leading
    // component is ParentDir, not `docs/pr`, and nothing normalizes it away.
    assert!(!is_history(Path::new("../docs/pr/x.md")));
    assert!(!is_history(Path::new("docs/../pbi/x.md")));
}

#[test]
fn native_separators_are_absorbed_by_path_components() {
    // Windows joins with `\`, and `Path` splits it into components there; this
    // pin is built with `push`, so it carries `\` on Windows and `/` here.
    // The verdict comes from components, never from a rendered string, where
    // `docs\pr` would miss a `docs/pr` substring and leave the exclusion
    // empty.
    let root = PathBuf::from("pbi-work").join("saniwalens");

    let mut history = root.clone();
    history.push("docs");
    history.push("pr");
    history.push("2026-01-01-01-x.md");
    let relative = rel(&root, &history).expect("a file under the root has a relative path");
    assert_eq!(relative, Path::new("docs/pr/2026-01-01-01-x.md"));
    assert!(is_history(relative), "{relative:?} must be history");

    let mut prose = root.clone();
    prose.push("docs");
    prose.push("specs");
    prose.push("architecture.md");
    let relative = rel(&root, &prose).expect("a file under the root has a relative path");
    assert!(
        is_current_prose(relative),
        "{relative:?} must be current prose"
    );
}
