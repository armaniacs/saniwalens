//! The trend view's rendered markdown, end to end through the real binary.
//! A unit test cannot assert the rendered output: the test harness captures
//! its threads' `println!` per-thread, so it never reaches stdout — here the
//! binary's own stdout is real, and the store is synthetic.

use saniwalens::core::span::{SessionStart, Source, Span};
use saniwalens::store::{IngestBatch, OpenMode, SessionMeta, Store};
use std::path::{Path, PathBuf};

mod common;
use common::ScratchDir;

/// A synthetic session stamped at `epoch_secs` (1970) with no subagent use.
fn session(id: &str, epoch_secs: i64) -> SessionMeta {
    SessionMeta {
        id: id.to_string(),
        project: "demo".to_string(),
        slug: "demo".to_string(),
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

/// Write a synthetic store: one `file_edit` work event per session at the
/// session's epoch, and no skill spans anywhere.
fn write_store(dir: &Path, sessions: &[(&str, i64)]) -> PathBuf {
    let db = dir.join("store.db");
    {
        let mut store = Store::open(&db, OpenMode::ReadWrite).unwrap();
        for (id, epoch_secs) in sessions {
            let meta = session(id, *epoch_secs);
            store
                .ingest(IngestBatch::new(&meta, &[], &[]).work_events(&[(
                    epoch_secs * 1000,
                    "file_edit",
                    "main.rs",
                    None,
                )]))
                .unwrap();
        }
    }
    db
}

/// Write a synthetic store with one skill span per session, stamped at the
/// session's epoch, as `(id, epoch_secs, out_tokens, duration_sec)` — the
/// present-zero fixture: a previous window whose span really happened but
/// measured zero.
fn write_span_store(dir: &Path, spans: &[(&str, i64, u64, f64)]) -> PathBuf {
    let db = dir.join("store.db");
    {
        let mut store = Store::open(&db, OpenMode::ReadWrite).unwrap();
        for (id, epoch_secs, out_tokens, duration_sec) in spans {
            let meta = session(id, *epoch_secs);
            let span = Span {
                skill: "commit".to_string(),
                source: Source::Slash,
                started_epoch_ms: epoch_secs * 1000,
                duration_sec: *duration_sec,
                out_tokens: *out_tokens,
                ctx_growth: 5,
                ctx_start: 0,
                ctx_peak: 5,
                model: None,
                is_trailing: false,
                spawns: Vec::new(),
                sub_tokens: 0,
                sub_agent_count: 0,
                sub_tokens_estimated: false,
            };
            store.ingest(IngestBatch::new(&meta, &[span], &[])).unwrap();
        }
    }
    db
}

/// Run the binary's `trend` on `db` in `format`, frozen (the store is read
/// as-is; the synthetic home holds nothing to refresh from), and return its
/// stdout.
fn trend_output(dir: &Path, db: &Path, format: &str) -> String {
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_saniwalens"))
        .args([
            "trend",
            "--since",
            "1970-01-02",
            "--until",
            "1970-01-02",
            "--format",
            format,
            "--frozen",
        ])
        .arg("--db")
        .arg(db)
        .env("HOME", dir)
        .env("CLAUDE_CONFIG_DIR", dir.join(".claude"))
        .env("XDG_STATE_HOME", dir.join("state"))
        .output()
        .expect("the binary runs");
    assert!(
        out.status.success(),
        "trend failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap()
}

#[test]
fn trend_markdown_omits_an_empty_section_instead_of_bare_headers() {
    // Both windows hold file_edit events but no skill usage anywhere: the
    // Skills section has no rows while the kind and totals sections do. The
    // windows: current 1970-01-02 JST ([56_400, 142_799]) holds the 100_000
    // session; the equal-length window right before it ([−30_000, 56_399])
    // holds the 1_000 one.
    let dir = ScratchDir::new("saniwalens-trend-markdown", "empty-skills");
    let db = write_store(&dir, &[("past", 1_000), ("present", 100_000)]);
    let out = trend_output(&dir, &db, "markdown");

    // No bare skill table, and no section label without its table — a section
    // with no rows is omitted, per the usage view's convention.
    let bare =
        "| skill | previous | current | delta | direction |\n| --- | --- | --- | --- | --- |";
    assert!(!out.contains(bare), "a bare skill table rendered: {out:?}");
    assert!(
        !out.contains("Skills (largest movement first):"),
        "the empty section's label still rendered: {out:?}"
    );
    // The sections that do have rows still render.
    assert!(out.contains("| kind |"), "the kind table is gone: {out:?}");
    assert!(
        out.contains("| total |"),
        "the totals table is gone: {out:?}"
    );
}

#[test]
fn trend_note_names_the_previous_window_that_holds_no_events() {
    // Only the current window holds events: the previous side of every
    // comparison is empty, so the note explains what n/a means and names the
    // previous window with its range.
    let dir = ScratchDir::new("saniwalens-trend-markdown", "one-sided");
    let db = write_store(&dir, &[("present", 100_000)]);
    let out = trend_output(&dir, &db, "markdown");

    assert!(
        out.contains("holds no events in this store"),
        "the n/a note is missing: {out:?}"
    );
    assert!(
        out.contains("previous window (since "),
        "the note does not name the previous window: {out:?}"
    );
}

#[test]
fn trend_markdown_omits_a_point_kinds_non_applicable_metric_rows() {
    // Both windows hold `file_edit` events and no skill spans anywhere:
    // ingest writes a point kind's output tokens and duration as constant
    // zeros, so its "out tokens" / "seconds" rows would always read
    // `0.0 | 0.0 | n/a | n/a` — the n/a's stated reason (an empty previous
    // side) false for a row whose real reason is that the metric does not
    // apply. The rows are omitted, not rendered.
    let dir = ScratchDir::new("saniwalens-trend-markdown", "point-kinds");
    let db = write_store(&dir, &[("past", 1_000), ("present", 100_000)]);
    let out = trend_output(&dir, &db, "markdown");

    assert!(
        out.contains("| file_edit | events |"),
        "the kind's count row is gone: {out:?}"
    );
    assert!(
        !out.contains("| file_edit | out tokens |"),
        "the out tokens row rendered: {out:?}"
    );
    assert!(
        !out.contains("| file_edit | seconds |"),
        "the seconds row rendered: {out:?}"
    );
}

#[test]
fn trend_states_the_na_reason_only_when_a_previous_side_is_empty() {
    // Both windows hold events: every rendered kind has a baseline, so no
    // comparison is n/a and the reason text ("A delta marked n/a: ...")
    // prints nowhere — the table format renders the preamble. The one-sided
    // store does hold an empty previous side, and there the reason text
    // explains its n/a rows.
    let two_sided = ScratchDir::new("saniwalens-trend-markdown", "reason-two-sided");
    let db = write_store(&two_sided, &[("past", 1_000), ("present", 100_000)]);
    let out = trend_output(&two_sided, &db, "table");
    assert!(
        !out.contains("A delta marked n/a"),
        "the n/a reason rendered without an n/a row: {out:?}"
    );
    assert!(
        !out.contains("holds no events in this store"),
        "the empty-side note rendered without an empty side: {out:?}"
    );

    let one_sided = ScratchDir::new("saniwalens-trend-markdown", "reason-one-sided");
    let db = write_store(&one_sided, &[("present", 100_000)]);
    let out = trend_output(&one_sided, &db, "table");
    assert!(
        out.contains("A delta marked n/a"),
        "the n/a reason is missing where a previous side is empty: {out:?}"
    );
    assert!(
        out.contains("holds no events in this store"),
        "the empty-side note is missing: {out:?}"
    );
}

#[test]
fn trend_compares_a_previous_window_that_measured_zero() {
    // Both windows hold a real 2.0s `commit` span; the previous one reported
    // 0 output tokens. A measured zero is a baseline: the output-token row
    // compares (0 -> 300, +300, up) instead of collapsing to n/a with no
    // stated reason. The store is legal — a span's out_tokens is whatever its
    // assistant records reported, and 0 is a measurement, not an absence.
    let dir = ScratchDir::new("saniwalens-trend-markdown", "present-zero");
    let db = write_span_store(
        &dir,
        &[("past", 1_000, 0, 2.0), ("present", 100_000, 300, 2.0)],
    );
    let out = trend_output(&dir, &db, "table");

    let tokens = out
        .lines()
        .find(|line| line.contains("out tokens"))
        .unwrap_or_else(|| panic!("the output-token row is missing: {out:?}"));
    assert!(
        tokens.contains("+300") && tokens.contains("up"),
        "a measured zero baseline compares: {tokens:?}"
    );
    assert!(
        !tokens.contains("n/a"),
        "the row is not unavailable: {tokens:?}"
    );
    // No rendered n/a row means no reason text and no empty-window note: the
    // reasons ride only with a report that holds such a comparison.
    assert!(
        !out.contains("A delta marked n/a"),
        "an n/a reason rendered without an n/a row: {out:?}"
    );
    assert!(
        !out.contains("holds no events in this store"),
        "the empty-side note rendered without an empty side: {out:?}"
    );
}
