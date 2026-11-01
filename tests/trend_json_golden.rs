//! The trend view's JSON machine contract, end to end through the real
//! binary: the first golden for a view's JSON output (the view-landing rule's
//! obligation). A unit test cannot assert the emitted bytes — the test
//! harness captures its threads' `println!` per-thread, so it never reaches
//! stdout — here the binary's own stdout is real, and the store is synthetic.
//! The golden pins the captured bytes, so any contract change (a key, its
//! order, a typed value, the unavailable form) fails here, not in a consumer.

use saniwalens::core::span::{SessionStart, Source, Span};
use saniwalens::store::{IngestBatch, OpenMode, SessionMeta, Store};
use std::path::{Path, PathBuf};

/// A synthetic session stamped at `epoch_secs` (1970) with subagent use.
fn session(id: &str, epoch_secs: i64, sub_tokens: i64, sub_agents: i64) -> SessionMeta {
    SessionMeta {
        id: id.to_string(),
        project: "demo".to_string(),
        slug: "demo".to_string(),
        root: String::new(),
        source_path: format!("/tmp/{id}.jsonl"),
        sub_tokens,
        sub_agent_count: sub_agents,
        start: Some(SessionStart {
            timestamp_ms: epoch_secs * 1000,
            ctx: 9_000,
        }),
    }
}

/// A synthetic skill span at `epoch_secs` (1970): one `skill_invocation` of
/// 2.0s costing `out_tokens` output tokens.
fn span(skill: &str, epoch_secs: i64, out_tokens: u64) -> Span {
    Span {
        skill: skill.to_string(),
        source: Source::Slash,
        started_epoch_ms: epoch_secs * 1000,
        duration_sec: 2.0,
        out_tokens,
        ctx_growth: 5,
        ctx_start: 0,
        ctx_peak: 5,
        model: None,
        is_trailing: false,
        spawns: Vec::new(),
        sub_tokens: 0,
        sub_agent_count: 0,
        sub_tokens_estimated: false,
    }
}

/// Write a synthetic store covering both windows of the comparison: the
/// previous window ([-32_400, 53_999]) holds a session stamped at 1_500 with
/// two skill spans and a `file_edit`; the current window ([54_000, 140_399])
/// holds a session stamped at 100_000 with one skill span and a `file_edit`.
/// The point kind (no spans' duration or tokens) exercises the unavailable
/// form; the span kind and the totals exercise every direction.
fn write_store(dir: &Path) -> PathBuf {
    let db = dir.join("store.db");
    {
        let mut store = Store::open(&db, OpenMode::ReadWrite).unwrap();
        let mut past = session("past", 1_500, 400, 1);
        past.sub_tokens = 400;
        past.sub_agent_count = 1;
        let past_spans = [span("commit", 1_500, 100), span("review", 1_800, 50)];
        store
            .ingest(IngestBatch::new(&past, &past_spans, &[]).work_events(&[(
                1_900_000,
                "file_edit",
                "main.rs",
                None,
            )]))
            .unwrap();
        let mut present = session("present", 100_000, 500, 2);
        present.sub_tokens = 500;
        present.sub_agent_count = 2;
        let present_spans = [span("commit", 100_500, 300)];
        store
            .ingest(
                IngestBatch::new(&present, &present_spans, &[]).work_events(&[(
                    100_600_000,
                    "file_edit",
                    "main.rs",
                    None,
                )]),
            )
            .unwrap();
    }
    db
}

/// Write a synthetic present-zero store: both windows hold one 2.0s
/// `commit` span, but the previous window's span reported **0** output
/// tokens and **0.0s** — a legal measurement, not an absent row. The
/// comparison must be claimed (`+300`, `+2.0`), and the machine contract must
/// say so with `"reason": null`, never `"no_baseline"`.
fn write_present_zero_store(dir: &Path) -> PathBuf {
    let db = dir.join("store.db");
    {
        let mut store = Store::open(&db, OpenMode::ReadWrite).unwrap();
        let past = session("past", 1_500, 0, 0);
        let mut past_span = span("commit", 1_500, 0);
        past_span.duration_sec = 0.0;
        store
            .ingest(IngestBatch::new(&past, &[past_span], &[]))
            .unwrap();
        let present = session("present", 100_000, 0, 0);
        store
            .ingest(IngestBatch::new(
                &present,
                &[span("commit", 100_500, 300)],
                &[],
            ))
            .unwrap();
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

/// A per-test scratch dir, cleaned up afterwards — synthetic only.
fn scratch_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "saniwalens-trend-json-golden-{}-{name}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// The captured JSON machine contract: both windows' ranges under `window`,
/// every kind x three metrics (a point kind's output tokens / duration stay
/// in the contract with a null delta, `"unavailable"`, and
/// `"reason": "not_applicable"`), the skill deltas, and the session/subagent
/// totals — integers as integers, reals as reals, and `reason` either null or
/// one of the two machine-readable reasons.
const GOLDEN: &str = r#"{
  "kinds": [
    {
      "duration_sec": {
        "current": 0.0,
        "delta": null,
        "direction": "unavailable",
        "previous": 0.0,
        "reason": "not_applicable"
      },
      "events": {
        "current": 1,
        "delta": 0,
        "direction": "same",
        "previous": 1,
        "reason": null
      },
      "kind": "file_edit",
      "out_tokens": {
        "current": 0,
        "delta": null,
        "direction": "unavailable",
        "previous": 0,
        "reason": "not_applicable"
      }
    },
    {
      "duration_sec": {
        "current": 0.0,
        "delta": null,
        "direction": "unavailable",
        "previous": 0.0,
        "reason": "not_applicable"
      },
      "events": {
        "current": 1,
        "delta": 0,
        "direction": "same",
        "previous": 1,
        "reason": null
      },
      "kind": "session_start",
      "out_tokens": {
        "current": 0,
        "delta": null,
        "direction": "unavailable",
        "previous": 0,
        "reason": "not_applicable"
      }
    },
    {
      "duration_sec": {
        "current": 2.0,
        "delta": -2.0,
        "direction": "down",
        "previous": 4.0,
        "reason": null
      },
      "events": {
        "current": 1,
        "delta": -1,
        "direction": "down",
        "previous": 2,
        "reason": null
      },
      "kind": "skill_invocation",
      "out_tokens": {
        "current": 300,
        "delta": 150,
        "direction": "up",
        "previous": 150,
        "reason": null
      }
    }
  ],
  "sessions": {
    "current": 1,
    "delta": 0,
    "direction": "same",
    "previous": 1,
    "reason": null
  },
  "skills": [
    {
      "current": 0,
      "delta": -1,
      "direction": "down",
      "previous": 1,
      "reason": null,
      "skill": "review"
    },
    {
      "current": 1,
      "delta": 0,
      "direction": "same",
      "previous": 1,
      "reason": null,
      "skill": "commit"
    }
  ],
  "sub_tokens": {
    "current": 500,
    "delta": 100,
    "direction": "up",
    "previous": 400,
    "reason": null
  },
  "subagent_runs": {
    "current": 2,
    "delta": 1,
    "direction": "up",
    "previous": 1,
    "reason": null
  },
  "window": {
    "current": {
      "since": "1970-01-02T00:00:00+09:00",
      "until": "1970-01-02T23:59:59+09:00"
    },
    "previous": {
      "since": "1970-01-01T00:00:00+09:00",
      "until": "1970-01-01T23:59:59+09:00"
    }
  }
}
"#;

#[test]
fn the_trend_json_contract_is_pinned_golden() {
    let dir = scratch_dir("contract");
    let db = write_store(&dir);
    let out = trend_output(&dir, &db, "json");
    assert_eq!(
        out, GOLDEN,
        "the trend JSON contract drifted — actual:\n{out}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_trend_json_reason_separates_a_measured_zero_from_an_absent_baseline() {
    // The second contract case: a previous window whose span really ran and
    // measured 0 output tokens / 0.0s. The comparison is claimed — the metric
    // carries numbers and `"reason": null` — where an absent row would carry
    // `"reason": "no_baseline"` and a null delta. The totals carry a measured
    // zero the same way: a previous session that ran no subagent measured 0
    // runs, which compares (0 -> 0, same), not an unavailable row.
    let dir = scratch_dir("present-zero");
    let db = write_present_zero_store(&dir);
    let out = trend_output(&dir, &db, "json");
    let json: serde_json::Value = serde_json::from_str(&out).expect("the contract is JSON");
    let kinds = json["kinds"].as_array().unwrap();
    let skill = kinds
        .iter()
        .find(|kind| kind["kind"] == "skill_invocation")
        .expect("the span kind is in the contract");
    assert_eq!(
        skill["out_tokens"],
        serde_json::json!({
            "previous": 0,
            "current": 300,
            "delta": 300,
            "direction": "up",
            "reason": null
        }),
        "a measured zero token baseline compares"
    );
    assert_eq!(
        skill["duration_sec"],
        serde_json::json!({
            "previous": 0.0,
            "current": 2.0,
            "delta": 2.0,
            "direction": "up",
            "reason": null
        }),
        "a measured zero duration baseline compares"
    );
    assert_eq!(skill["events"]["direction"], "same");
    assert_eq!(skill["events"]["reason"], serde_json::Value::Null);
    // The point kind's span metrics stay in the contract as not applicable —
    // the other reason, never confused with a missing baseline.
    let start = kinds
        .iter()
        .find(|kind| kind["kind"] == "session_start")
        .expect("the session start kind is in the contract");
    for metric in ["out_tokens", "duration_sec"] {
        assert_eq!(start[metric]["direction"], "unavailable");
        assert_eq!(start[metric]["delta"], serde_json::Value::Null);
        assert_eq!(start[metric]["reason"], "not_applicable");
    }
    // The totals: a previous session exists, so its zero subagent figures are
    // measurements, not an absent axis.
    assert_eq!(json["sessions"]["previous"], 1);
    assert_eq!(json["sessions"]["reason"], serde_json::Value::Null);
    assert_eq!(json["subagent_runs"]["previous"], 0);
    assert_eq!(json["subagent_runs"]["current"], 0);
    assert_eq!(json["subagent_runs"]["direction"], "same");
    assert_eq!(json["subagent_runs"]["reason"], serde_json::Value::Null);
    assert_eq!(json["sub_tokens"]["previous"], 0);
    assert_eq!(json["sub_tokens"]["reason"], serde_json::Value::Null);
    // No metric claims an absent baseline on this store.
    assert!(
        !out.contains("\"no_baseline\""),
        "a measured zero is not an absent baseline: {out}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
