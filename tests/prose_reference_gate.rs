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
//! (`docs/pr`, `docs/exploration`, `pbi/`, `CHANGELOG`) and src fixture strings
//! are out of scope by construction — the scan list is the current prose only,
//! which is `docs/specs`, `.claude/rules`, README, and AGENTS (AGENTS.md points
//! at `.claude/rules` as the project guardrails), so history stays untouched
//! and cannot fail the gate.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

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

/// The current prose only: `docs/specs/*.md`, `.claude/rules/*.md`, plus
/// `README.md` and `AGENTS.md`. `docs/pr`, `docs/exploration`, `pbi/`, and
/// `CHANGELOG` are history and are deliberately absent — stale paths in
/// history are records, not anchors.
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
        let display = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .to_string_lossy()
            .to_string();
        misses.extend(file_misses(root, &display, &text));
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
         docs/exploration, pbi/ and CHANGELOG are history and out of scope.",
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
    // `docs/exploration`, `pbi/`, and `CHANGELOG` hold stale paths as records;
    // the scan list never contains them, so history cannot fail the gate and
    // stays untouched. src fixture strings are .rs, never scanned at all.
    // `.claude/rules` is current prose too (the guardrails AGENTS.md points
    // at), so the scan list must contain it (PBI 36).
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let files = scan_files(root);
    assert!(files.contains(&root.join("README.md")));
    assert!(files.contains(&root.join("AGENTS.md")));
    assert!(files.contains(&root.join("docs/specs/harness.md")));
    assert!(files.contains(&root.join(".claude/rules/spec-sync.md")));
    assert!(
        files.iter().all(|file| {
            let shown = file.to_string_lossy();
            shown.contains("docs/specs")
                || shown.contains(".claude/rules")
                || shown.ends_with("README.md")
                || shown.ends_with("AGENTS.md")
        }),
        "{files:?}"
    );
    assert!(
        !files.iter().any(|file| {
            let shown = file.to_string_lossy();
            shown.contains("docs/pr")
                || shown.contains("docs/exploration")
                || shown.contains("pbi")
                || shown.contains("CHANGELOG")
        }),
        "{files:?}"
    );
}
