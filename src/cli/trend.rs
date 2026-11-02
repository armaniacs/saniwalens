//! The usage-trend view: what each kind of event, each skill, and the session
//! and subagent totals did this window against the equal-length window right
//! before it. One window's comparisons ride the store's two-window aggregates.

use anyhow::Result;

use crate::core::bucket::JST_OFFSET_SECS;
use crate::core::period::{self, WindowPair};
use crate::session::{Session, Style};
use crate::store::{KindRollup, TrendWindowAggregates, TwoWindow, skill_deltas};

use super::{Align, Format, emit_json, framed, note, preamble, render};

/// One direction of a two-window comparison. `Unavailable` is 比較不能: the
/// previous side holds no events, so there is nothing to compare against —
/// labeled, never read as a drop to zero.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Direction {
    Up,
    Down,
    Same,
    Unavailable,
}

impl Direction {
    /// The terse table label.
    fn label(self) -> &'static str {
        match self {
            Direction::Up => "up",
            Direction::Down => "down",
            Direction::Same => "same",
            Direction::Unavailable => "n/a",
        }
    }

    /// The JSON direction value — explicit where the table is terse.
    fn json_value(self) -> &'static str {
        match self {
            Direction::Unavailable => "unavailable",
            other => other.label(),
        }
    }
}

/// Which way a metric moved between two sides both of which are present —
/// the pure comparison [`direction_of`] guards with its empty-side sentinel.
fn direction_between<T: PartialOrd>(previous: T, current: T) -> Direction {
    match current.partial_cmp(&previous) {
        Some(std::cmp::Ordering::Greater) => Direction::Up,
        Some(std::cmp::Ordering::Less) => Direction::Down,
        _ => Direction::Same,
    }
}

/// Which way a metric moved between the two windows. An empty previous side
/// (0 events) is 比較不能 — the store holds no baseline for the row, so no
/// direction is claimed whatever the current value is; a delta against an
/// empty side is a coverage fact, not a move.
pub(crate) fn direction_of<T: PartialOrd + Default>(previous: T, current: T) -> Direction {
    if previous == T::default() {
        return Direction::Unavailable;
    }
    direction_between(previous, current)
}

/// One integer metric's JSON cell: both windows' values, the delta, and the
/// direction — with the delta null when the previous side is empty, so no
/// change is claimed for a comparison that cannot be made.
fn metric_json_int(previous: i64, current: i64) -> serde_json::Value {
    let direction = direction_of(previous, current);
    let delta = match direction {
        Direction::Unavailable => serde_json::Value::Null,
        _ => serde_json::json!(current - previous),
    };
    serde_json::json!({
        "previous": previous,
        "current": current,
        "delta": delta,
        "direction": direction.json_value(),
    })
}

/// One float metric's JSON cell — the duration form of [`metric_json_int`].
fn metric_json_float(previous: f64, current: f64) -> serde_json::Value {
    let direction = direction_of(previous, current);
    let delta = match direction {
        Direction::Unavailable => serde_json::Value::Null,
        _ => serde_json::json!(current - previous),
    };
    serde_json::json!({
        "previous": previous,
        "current": current,
        "delta": delta,
        "direction": direction.json_value(),
    })
}

/// The two windows' per-kind rollups paired by kind — a kind present in only
/// one window keeps None on the other side. The table rows carry that Option
/// to the n/a judgment ([`trend_kind_rows`]); the JSON contract folds a None
/// side to the default value (`trend_json`), whose shape stays every kind ×
/// three metrics. Sorted by kind, the aggregation's own order.
fn kind_pairs(
    agg: &TwoWindow<TrendWindowAggregates>,
) -> Vec<(String, Option<KindRollup>, Option<KindRollup>)> {
    let mut pairs: std::collections::BTreeMap<String, (Option<KindRollup>, Option<KindRollup>)> =
        std::collections::BTreeMap::new();
    for rollup in &agg.current.by_kind {
        pairs.entry(rollup.kind.clone()).or_default().0 = Some(rollup.clone());
    }
    for rollup in &agg.previous.by_kind {
        pairs.entry(rollup.kind.clone()).or_default().1 = Some(rollup.clone());
    }
    pairs
        .into_iter()
        .map(|(kind, (current, previous))| (kind, current, previous))
        .collect()
}

/// The trend's JSON contract: both windows' ranges under `window`, the
/// per-kind rows, the skill deltas, and the session/subagent totals — every
/// metric carrying both windows' values, the delta, and the direction.
pub(crate) fn trend_json(
    pair: WindowPair,
    agg: &TwoWindow<TrendWindowAggregates>,
) -> serde_json::Value {
    let (current_since, current_until) = period::bounds(pair.current, JST_OFFSET_SECS);
    let (previous_since, previous_until) = period::bounds(pair.previous, JST_OFFSET_SECS);
    let kinds: Vec<serde_json::Value> = kind_pairs(agg)
        .into_iter()
        .map(|(kind, current, previous)| {
            let (c, p) = (current.as_ref(), previous.as_ref());
            serde_json::json!({
                "kind": kind,
                "events": metric_json_int(
                    p.map_or(0, |r| r.events),
                    c.map_or(0, |r| r.events),
                ),
                "out_tokens": metric_json_int(
                    p.map_or(0, |r| r.out_tokens),
                    c.map_or(0, |r| r.out_tokens),
                ),
                "duration_sec": metric_json_float(
                    p.map_or(0.0, |r| r.duration_sec),
                    c.map_or(0.0, |r| r.duration_sec),
                ),
            })
        })
        .collect();
    let skills: Vec<serde_json::Value> = skill_deltas(&agg.current.skills, &agg.previous.skills)
        .iter()
        .map(|d| {
            let direction = direction_of(d.previous, d.current);
            let delta = match direction {
                Direction::Unavailable => serde_json::Value::Null,
                _ => serde_json::json!(d.delta),
            };
            serde_json::json!({
                "skill": d.skill,
                "previous": d.previous,
                "current": d.current,
                "delta": delta,
                "direction": direction.json_value(),
            })
        })
        .collect();
    serde_json::json!({
        "window": {
            "current": { "since": current_since, "until": current_until },
            "previous": { "since": previous_since, "until": previous_until },
        },
        "kinds": kinds,
        "skills": skills,
        "sessions": metric_json_int(agg.previous.sessions, agg.current.sessions),
        "subagent_runs": metric_json_int(
            agg.previous.subagent_runs,
            agg.current.subagent_runs,
        ),
        "sub_tokens": metric_json_int(agg.previous.sub_tokens, agg.current.sub_tokens),
    })
}

/// A table cell pair for an integer metric: the signed delta and the
/// direction — both n/a when the previous side is empty.
fn delta_cells_int(previous: i64, current: i64) -> (String, String) {
    match direction_of(previous, current) {
        Direction::Unavailable => ("n/a".to_string(), "n/a".to_string()),
        direction => (
            format!("{:+}", current - previous),
            direction.label().to_string(),
        ),
    }
}

/// A float rounded to the table's display precision — the same rounding the
/// `{:.1}` cells use (round via the format itself, so the rounded value is by
/// construction the one the table shows).
fn display_precise(value: f64) -> f64 {
    format!("{value:.1}").parse().unwrap_or(value)
}

/// A table cell pair for an integer metric whose emptiness rides the type:
/// n/a only when the previous side holds no rollup (the kind had no events in
/// the window), never because a present rollup's value is the default — a
/// measured zero baseline is a fact, not 比較不能. The current side reads a
/// kind the window does not hold as zero events, which is what it is.
fn delta_cells_int_opt(previous: Option<i64>, current: i64) -> (String, String) {
    match previous {
        None => ("n/a".to_string(), "n/a".to_string()),
        Some(previous) => (
            format!("{:+}", current - previous),
            direction_between(previous, current).label().to_string(),
        ),
    }
}

/// The float form of [`delta_cells_int_opt`]: direction judged at display
/// precision, so float drift smaller than the shown 0.1 must not claim a move
/// the table renders as ±0.0, and n/a only for a missing previous side — a
/// baseline that rounds to the displayed 0.0 (a real 0.04s) still compares.
pub(crate) fn delta_cells_float_opt(previous: Option<f64>, current: f64) -> (String, String) {
    match previous {
        None => ("n/a".to_string(), "n/a".to_string()),
        Some(previous) => {
            let (previous, current) = (display_precise(previous), display_precise(current));
            (
                format!("{:+.1}", current - previous),
                direction_between(previous, current).label().to_string(),
            )
        }
    }
}

/// Whether a kind's events carry a duration and output tokens — the metrics
/// the trend renders beside the count. Ingest writes both columns as constant
/// zeros for every other kind (`store.rs`'s event writers; `events.md`: a span
/// is currently `skill_invocation` only), so those rows for a point kind would
/// always read `0.0 | 0.0 | n/a | n/a` — the n/a's stated reason (an empty
/// previous side) false for a row whose real reason is that the metric does
/// not apply. Only a span kind carries them; an unknown kind gets the count
/// row alone, so a new point kind never renders a false reason.
pub(crate) fn carries_span_metrics(kind: &str) -> bool {
    kind == "skill_invocation"
}

/// The per-kind section's rows: one row per kind and *applicable* metric, so
/// each row carries one comparison and one direction — a point kind renders
/// its count alone ([`carries_span_metrics`]), the row-level form of the usage
/// view's empty-section convention. Both windows' values keep their columns
/// even when the comparison is n/a, and the emptiness rides the type: a
/// present rollup compares even at a measured zero ([`delta_cells_int_opt`]).
pub(crate) fn trend_kind_rows(agg: &TwoWindow<TrendWindowAggregates>) -> Vec<Vec<String>> {
    let mut rows = Vec::new();
    for (kind, current, previous) in kind_pairs(agg) {
        let (c, p) = (current.as_ref(), previous.as_ref());
        let (p_events, c_events) = (p.map(|r| r.events), c.map_or(0, |r| r.events));
        let (delta, direction) = delta_cells_int_opt(p_events, c_events);
        rows.push(vec![
            kind.clone(),
            "events".to_string(),
            p_events.map_or("0".to_string(), |events| events.to_string()),
            c_events.to_string(),
            delta,
            direction,
        ]);
        if !carries_span_metrics(&kind) {
            continue;
        }
        let (p_tokens, c_tokens) = (p.map(|r| r.out_tokens), c.map_or(0, |r| r.out_tokens));
        let (delta, direction) = delta_cells_int_opt(p_tokens, c_tokens);
        rows.push(vec![
            kind.clone(),
            "out tokens".to_string(),
            p_tokens.map_or("0".to_string(), |tokens| tokens.to_string()),
            c_tokens.to_string(),
            delta,
            direction,
        ]);
        let (p_secs, c_secs) = (p.map(|r| r.duration_sec), c.map_or(0.0, |r| r.duration_sec));
        let (delta, direction) = delta_cells_float_opt(p_secs, c_secs);
        rows.push(vec![
            kind,
            "seconds".to_string(),
            format!("{:.1}", p_secs.unwrap_or(0.0)),
            format!("{c_secs:.1}"),
            delta,
            direction,
        ]);
    }
    rows
}

/// The skill section's rows: `skill_deltas`' order (largest movement first),
/// one row per skill with both windows' invocation counts.
fn trend_skill_rows(agg: &TwoWindow<TrendWindowAggregates>) -> Vec<Vec<String>> {
    skill_deltas(&agg.current.skills, &agg.previous.skills)
        .iter()
        .map(|d| {
            let (delta, direction) = delta_cells_int(d.previous, d.current);
            vec![
                d.skill.clone(),
                d.previous.to_string(),
                d.current.to_string(),
                delta,
                direction,
            ]
        })
        .collect()
}

/// The totals section's rows: sessions, subagent runs, and subagent tokens
/// along the sessions axis.
pub(crate) fn trend_total_rows(agg: &TwoWindow<TrendWindowAggregates>) -> Vec<Vec<String>> {
    [
        ("sessions", agg.previous.sessions, agg.current.sessions),
        (
            "subagent runs",
            agg.previous.subagent_runs,
            agg.current.subagent_runs,
        ),
        (
            "subagent tokens",
            agg.previous.sub_tokens,
            agg.current.sub_tokens,
        ),
    ]
    .into_iter()
    .map(|(label, previous, current)| {
        let (delta, direction) = delta_cells_int(previous, current);
        vec![
            label.to_string(),
            previous.to_string(),
            current.to_string(),
            delta,
            direction,
        ]
    })
    .collect()
}

/// The trend preamble's text: what the report compares, plus the previous
/// window's range — the current one comes from `framed`. The n/a reason
/// sentence rides only with a report that actually holds an n/a comparison:
/// with the non-applicable rows omitted, every rendered n/a's previous side
/// really is empty, and a report whose previous side holds every rendered
/// kind prints no reason at all rather than one that cannot occur. Table
/// format only.
pub(crate) fn trend_preamble(pair: WindowPair, has_unavailable: bool) -> String {
    let mut text =
        String::from("Usage trend — this window against the equal-length window right before it.");
    if has_unavailable {
        text.push_str(
            "\nA delta marked n/a: the previous side holds no events, so the comparison is\n\
             unavailable — not a drop.",
        );
    }
    text.push_str("\nprevious window: ");
    text.push_str(&period::display(pair.previous, JST_OFFSET_SECS));
    text
}

/// The trend's two-window disclosure — stderr beside the freshness line,
/// every format, so piped stdout and `--format json` stay clean.
fn eprint_trend_windows(pair: WindowPair) {
    let line = format!(
        "trend windows: current {} | previous {}",
        period::display(pair.current, JST_OFFSET_SECS),
        period::display(pair.previous, JST_OFFSET_SECS),
    );
    eprintln!("{}", Style::stderr().dim(&line));
}

/// A trend section: its label and table together — omitted entirely when the
/// section has no rows, so markdown never shows bare headers (the usage
/// view's convention: a section with no rows is omitted, not rendered empty).
fn trend_table(
    label: &str,
    headers: &[&str],
    aligns: &[Align],
    rows: &[Vec<String>],
    format: Format,
) {
    if rows.is_empty() {
        return;
    }
    trend_section(label);
    render(headers, aligns, rows, format);
}

/// A section label between a trend report's tables — printed in table and
/// markdown alike, so pasted tables keep their reading order.
fn trend_section(label: &str) {
    println!("\n{label}");
}

/// The usage trend: what each kind of event, each skill, and the session and
/// subagent totals did this window against the equal-length window right
/// before it. The pair is resolved before the store opens (`parse_pair`), so
/// a bad date fails like every other period error, and the store's numbers
/// are the same windowed queries the single-window views read.
pub(crate) fn trend(sess: &Session, pair: WindowPair, format: Format) -> Result<()> {
    let store = &sess.store;
    eprint_trend_windows(pair);
    let agg = store.trend_aggregates(pair)?;
    if format == Format::Json {
        return emit_json(&trend_json(pair, &agg));
    }
    // Nothing in either window *and* no session on either axis: the store
    // holds nothing to compare — unlike an empty previous window, this is
    // the analyze hint's case.
    if agg.current.by_kind.is_empty()
        && agg.previous.by_kind.is_empty()
        && agg.current.sessions == 0
        && agg.previous.sessions == 0
    {
        println!("no usage in either comparison window — run `saniwalens analyze` first");
        return Ok(());
    }
    let kind_rows = trend_kind_rows(&agg);
    // The reason text's condition: some rendered kind's previous side holds
    // no rollup — exactly the comparisons the rows mark n/a, so the preamble
    // never states a reason the report cannot show.
    let has_unavailable = kind_pairs(&agg)
        .iter()
        .any(|(_, _, previous)| previous.is_none());
    preamble(
        format,
        &framed(&trend_preamble(pair, has_unavailable), pair.current),
    );
    if !kind_rows.is_empty() {
        render(
            &[
                "kind",
                "metric",
                "previous",
                "current",
                "delta",
                "direction",
            ],
            &[
                Align::Left,
                Align::Left,
                Align::Right,
                Align::Right,
                Align::Right,
                Align::Left,
            ],
            &kind_rows,
            format,
        );
    }
    trend_table(
        "Skills (largest movement first):",
        &["skill", "previous", "current", "delta", "direction"],
        &[
            Align::Left,
            Align::Right,
            Align::Right,
            Align::Right,
            Align::Left,
        ],
        &trend_skill_rows(&agg),
        format,
    );
    trend_table(
        "Totals:",
        &["total", "previous", "current", "delta", "direction"],
        &[
            Align::Left,
            Align::Right,
            Align::Right,
            Align::Right,
            Align::Left,
        ],
        &trend_total_rows(&agg),
        format,
    );
    if agg.previous.by_kind.is_empty() {
        note(&format!(
            "the previous window ({}) holds no events in this store — comparisons\n\
             marked n/a have nothing to compare against, so no change is claimed for them",
            period::display(pair.previous, JST_OFFSET_SECS)
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::adapter::harness::Harness;
    use crate::core::period::TimeRange;
    use crate::store::Store;

    use super::*;
    // ---- PBI 2026-10-30-02 part 2: the trend view's CLI wiring ----

    use crate::core::period::WindowPair;
    use crate::core::span::{SessionStart, Source, Span};
    use crate::store::{IngestBatch, SessionMeta};

    fn trend_session(id: &str) -> SessionMeta {
        SessionMeta {
            id: id.to_string(),
            project: "demo".to_string(),
            slug: "demo".to_string(),
            root: String::new(),
            source_path: format!("/tmp/{id}.jsonl"),
            sub_tokens: 0,
            sub_agent_count: 0,
            start: None,
        }
    }

    fn trend_span(skill: &str, epoch_secs: i64, out_tokens: u64) -> Span {
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

    /// Usage in both comparison windows: previous [1_000, 2_000] (two skill
    /// spans, one file_edit), current [2_001, 3_000] (one skill span, one
    /// file_edit). Each window's spans ride its own session — the stamped
    /// start is the earliest of the declared start and the spans, so a
    /// session's own spans must not reach into the other window.
    fn two_window_store() -> Store {
        let mut store = Store::in_memory().unwrap();
        let mut past = trend_session("past");
        past.start = Some(SessionStart {
            timestamp_ms: 1_500 * 1000,
            ctx: 9_000,
        });
        past.sub_tokens = 400;
        past.sub_agent_count = 1;
        let past_spans = [
            trend_span("commit", 1_500, 100),
            trend_span("review", 1_800, 50),
        ];
        store
            .ingest(IngestBatch::new(&past, &past_spans, &[]).work_events(&[(
                1_900_000,
                "file_edit",
                "main.rs",
                None,
            )]))
            .unwrap();

        let mut present = trend_session("present");
        present.start = Some(SessionStart {
            timestamp_ms: 2_500 * 1000,
            ctx: 10_000,
        });
        present.sub_tokens = 500;
        present.sub_agent_count = 2;
        let present_spans = [trend_span("commit", 2_500, 300)];
        store
            .ingest(
                IngestBatch::new(&present, &present_spans, &[]).work_events(&[(
                    2_600_000,
                    "file_edit",
                    "main.rs",
                    None,
                )]),
            )
            .unwrap();
        store
    }

    /// A store with usage only in the current window: the previous side of
    /// every comparison is empty.
    fn one_sided_store() -> Store {
        let mut store = Store::in_memory().unwrap();
        let mut present = trend_session("present");
        present.start = Some(SessionStart {
            timestamp_ms: 2_500 * 1000,
            ctx: 10_000,
        });
        present.sub_tokens = 500;
        present.sub_agent_count = 2;
        let present_spans = [trend_span("commit", 2_500, 300)];
        store
            .ingest(
                IngestBatch::new(&present, &present_spans, &[]).work_events(&[(
                    2_600_000,
                    "file_edit",
                    "main.rs",
                    None,
                )]),
            )
            .unwrap();
        store
    }

    /// A store whose previous window holds a kind the current one does not:
    /// the kind-level row must read as a move to zero — the previous side has
    /// a real baseline, not 比較不能.
    fn kind_gone_store() -> Store {
        let mut store = Store::in_memory().unwrap();
        let mut past = trend_session("past");
        past.start = Some(SessionStart {
            timestamp_ms: 1_500 * 1000,
            ctx: 9_000,
        });
        store
            .ingest(IngestBatch::new(&past, &[], &[]).work_events(&[(
                1_900_000,
                "web_fetch",
                "docs",
                None,
            )]))
            .unwrap();
        let mut present = trend_session("present");
        present.start = Some(SessionStart {
            timestamp_ms: 2_500 * 1000,
            ctx: 10_000,
        });
        store
            .ingest(IngestBatch::new(&present, &[], &[]).work_events(&[(
                2_600_000,
                "file_edit",
                "main.rs",
                None,
            )]))
            .unwrap();
        store
    }

    /// One session declared in the previous window whose events happen in the
    /// current one: totals follow the stamped start, events split by epoch —
    /// the two axes never merge.
    fn straddling_store() -> Store {
        let mut store = Store::in_memory().unwrap();
        let mut meta = trend_session("straddle");
        meta.start = Some(SessionStart {
            timestamp_ms: 1_500 * 1000,
            ctx: 9_000,
        });
        meta.sub_tokens = 100;
        meta.sub_agent_count = 1;
        let spans = [trend_span("commit", 2_500, 300)];
        store
            .ingest(IngestBatch::new(&meta, &spans, &[]).work_events(&[(
                2_600_000,
                "file_edit",
                "main.rs",
                None,
            )]))
            .unwrap();
        store
    }

    fn trend_pair() -> WindowPair {
        WindowPair {
            current: TimeRange {
                start: Some(2_001),
                end: Some(3_000),
            },
            previous: TimeRange {
                start: Some(1_000),
                end: Some(2_000),
            },
        }
    }

    #[test]
    fn the_trend_direction_judgment() {
        use Direction::{Down, Same, Unavailable, Up};
        assert_eq!(direction_of(3.0, 5.0), Up);
        assert_eq!(direction_of(5.0, 3.0), Down);
        assert_eq!(direction_of(5.0, 5.0), Same);
        // An empty previous side is 比較不能: the comparison is unavailable
        // whatever the current value is — never a -100% drop to zero.
        assert_eq!(direction_of(0.0, 5.0), Unavailable);
        assert_eq!(direction_of(0.0, 0.0), Unavailable);
        // Integer sides share the judgment.
        assert_eq!(direction_of(4, 1), Down);
        assert_eq!(direction_of(1, 4), Up);
        assert_eq!(direction_of(7, 7), Same);
        assert_eq!(direction_of(0, 7), Unavailable);
    }

    #[test]
    fn the_float_delta_judges_direction_at_display_precision() {
        // The unrounded seconds can differ by float drift smaller than the
        // display's 0.1: the direction must not claim a move the table shows
        // as ±0.0 — direction and displayed delta never disagree.
        let (delta, direction) = delta_cells_float_opt(Some(3.3), 3.3000000000000003);
        assert_eq!(direction, "same", "drift above zero is not a move");
        assert_eq!(delta, "+0.0");
        // A sub-0.05 real move rounds to ±0.0 too: no direction is claimed
        // for a move the table cannot show.
        let (delta, direction) = delta_cells_float_opt(Some(1.0), 1.0 - 1e-9);
        assert_eq!(direction, "same", "drift below zero is not a move");
        assert_eq!(delta, "+0.0");
        // A move the table CAN show still reads as one.
        let (delta, direction) = delta_cells_float_opt(Some(1.0), 2.0);
        assert_eq!(direction, "up");
        assert_eq!(delta, "+1.0");
        // A missing previous side is the only 比較不能: the emptiness rides
        // the type, not the value.
        let (delta, direction) = delta_cells_float_opt(None, 2.0);
        assert_eq!((delta.as_str(), direction.as_str()), ("n/a", "n/a"));
        // A present baseline that rounds to 0.0 still compares — it is a
        // measured zero, not an empty side.
        let (delta, direction) = delta_cells_float_opt(Some(0.04), 2.0);
        assert_eq!((delta.as_str(), direction.as_str()), ("+2.0", "up"));
    }

    #[test]
    fn trend_json_carries_both_windows_and_their_deltas() {
        let store = two_window_store();
        let pair = trend_pair();
        let agg = store.trend_aggregates(pair).unwrap();
        let json = trend_json(pair, &agg);

        // Both windows' ranges are disclosed under `window`.
        assert_eq!(
            json["window"]["current"]["since"],
            "1970-01-01T09:33:21+09:00"
        );
        assert_eq!(
            json["window"]["current"]["until"],
            "1970-01-01T09:50:00+09:00"
        );
        assert_eq!(
            json["window"]["previous"]["since"],
            "1970-01-01T09:16:40+09:00"
        );
        assert_eq!(
            json["window"]["previous"]["until"],
            "1970-01-01T09:33:20+09:00"
        );

        // Per-kind rows carry both windows' values and the delta. The skill
        // invocations went 2 -> 1 (down); their tokens 150 -> 300 (up).
        let kinds = json["kinds"].as_array().unwrap();
        let skill = kinds
            .iter()
            .find(|k| k["kind"] == "skill_invocation")
            .unwrap();
        assert_eq!(skill["events"]["previous"], 2);
        assert_eq!(skill["events"]["current"], 1);
        assert_eq!(skill["events"]["delta"], -1);
        assert_eq!(skill["events"]["direction"], "down");
        assert_eq!(skill["out_tokens"]["previous"], 150);
        assert_eq!(skill["out_tokens"]["current"], 300);
        assert_eq!(skill["out_tokens"]["delta"], 150);
        assert_eq!(skill["out_tokens"]["direction"], "up");
        // A kind both windows share at the same size reads as unchanged.
        let edit = kinds.iter().find(|k| k["kind"] == "file_edit").unwrap();
        assert_eq!(edit["events"]["previous"], 1);
        assert_eq!(edit["events"]["current"], 1);
        assert_eq!(edit["events"]["delta"], 0);
        assert_eq!(edit["events"]["direction"], "same");
        // The machine contract keeps every kind × three metrics: a point
        // kind's output tokens / duration carry the same shape even at the
        // constant zeros ingest writes — only the table omits what cannot
        // apply, never the JSON.
        assert_eq!(edit["out_tokens"]["previous"], 0);
        assert_eq!(edit["out_tokens"]["current"], 0);
        assert_eq!(edit["out_tokens"]["delta"], serde_json::Value::Null);
        assert_eq!(edit["out_tokens"]["direction"], "unavailable");
        assert_eq!(edit["duration_sec"]["previous"], 0.0);
        assert_eq!(edit["duration_sec"]["current"], 0.0);
        assert_eq!(edit["duration_sec"]["delta"], serde_json::Value::Null);
        assert_eq!(edit["duration_sec"]["direction"], "unavailable");
        // A span kind's duration compares: 4.0s of spans -> 2.0s.
        assert_eq!(skill["duration_sec"]["previous"], 4.0);
        assert_eq!(skill["duration_sec"]["current"], 2.0);
        assert_eq!(skill["duration_sec"]["delta"], -2.0);
        assert_eq!(skill["duration_sec"]["direction"], "down");

        // Skills are ordered by movement: review (0 <- 1) before commit (1, 1).
        let skills = json["skills"].as_array().unwrap();
        assert_eq!(skills[0]["skill"], "review");
        assert_eq!(skills[0]["previous"], 1);
        assert_eq!(skills[0]["current"], 0);
        assert_eq!(skills[0]["delta"], -1);
        assert_eq!(skills[0]["direction"], "down");
        assert_eq!(skills[1]["skill"], "commit");
        assert_eq!(skills[1]["direction"], "same");

        // Session and subagent totals follow the sessions axis.
        assert_eq!(json["sessions"]["previous"], 1);
        assert_eq!(json["sessions"]["current"], 1);
        assert_eq!(json["sessions"]["direction"], "same");
        assert_eq!(json["subagent_runs"]["previous"], 1);
        assert_eq!(json["subagent_runs"]["current"], 2);
        assert_eq!(json["subagent_runs"]["delta"], 1);
        assert_eq!(json["subagent_runs"]["direction"], "up");
        assert_eq!(json["sub_tokens"]["previous"], 400);
        assert_eq!(json["sub_tokens"]["current"], 500);
        assert_eq!(json["sub_tokens"]["delta"], 100);
        assert_eq!(json["sub_tokens"]["direction"], "up");
    }

    #[test]
    fn trend_marks_comparisons_unavailable_when_the_previous_window_holds_no_events() {
        let store = one_sided_store();
        let pair = trend_pair();
        let agg = store.trend_aggregates(pair).unwrap();
        let json = trend_json(pair, &agg);

        // Every metric cell is labeled unavailable, with no delta claimed —
        // the previous side is empty, not a real zero baseline.
        for kind in json["kinds"].as_array().unwrap() {
            for metric in ["events", "out_tokens", "duration_sec"] {
                assert_eq!(
                    kind[metric]["direction"], "unavailable",
                    "unexpected direction for {} of {}",
                    metric, kind["kind"]
                );
                assert_eq!(kind[metric]["delta"], serde_json::Value::Null);
            }
        }
        for skill in json["skills"].as_array().unwrap() {
            assert_eq!(skill["direction"], "unavailable");
            assert_eq!(skill["delta"], serde_json::Value::Null);
        }
        for total in ["sessions", "subagent_runs", "sub_tokens"] {
            assert_eq!(json[total]["direction"], "unavailable");
            assert_eq!(json[total]["delta"], serde_json::Value::Null);
        }

        // And nothing in the payload reads as a drop to zero.
        let text = serde_json::to_string(&json).unwrap();
        assert!(!text.contains("-100%"));
        assert!(!text.contains("dropped"));
    }

    #[test]
    fn trend_rows_label_unavailable_comparisons_instead_of_drops() {
        let store = one_sided_store();
        let agg = store.trend_aggregates(trend_pair()).unwrap();

        // The kind rows keep both windows' values (the previous side renders
        // as 0) but mark the delta and direction n/a.
        let rows = trend_kind_rows(&agg);
        assert!(
            rows.iter()
                .any(|r| r[1] == "events" && r[2] == "0" && r[3] == "1"),
            "the current window's values still show: {rows:?}"
        );
        assert!(
            rows.iter().all(|r| r[4] == "n/a" && r[5] == "n/a"),
            "every comparison is n/a: {rows:?}"
        );

        // No percentage and no drop claim anywhere in the rendered cells.
        let flat = rows
            .iter()
            .flat_map(|r| r.iter())
            .cloned()
            .collect::<Vec<_>>()
            .join(" ");
        assert!(!flat.contains('%'), "no percentages: {flat:?}");
        assert!(!flat.contains("-100"), "no -100: {flat:?}");
    }

    #[test]
    fn the_trend_preamble_discloses_both_windows() {
        let pair = trend_pair();
        // An empty previous side (the one-sided stores) is what turns the
        // reason sentence on.
        let text = trend_preamble(pair, true);
        assert!(
            text.contains("previous window: since 1970-01-01T09:16:40+09:00"),
            "the previous window is named in the preamble: {text:?}"
        );
        // framed() appends the current window — both windows end up in the
        // table's framing block.
        let framed_text = framed(&text, pair.current);
        assert!(
            framed_text.contains("report window: since 1970-01-01T09:33:21+09:00"),
            "the current window is disclosed too: {framed_text:?}"
        );
    }

    #[test]
    fn the_trend_preamble_states_the_na_reason_only_when_a_side_is_empty() {
        let pair = trend_pair();
        // The reason text rides only with a report that actually holds an n/a
        // comparison — a previous side that holds every rendered kind prints
        // no reason at all rather than one that cannot occur.
        let without = trend_preamble(pair, false);
        assert!(
            !without.contains("holds no events"),
            "a report with no empty side states no reason: {without:?}"
        );
        assert!(
            without.contains("previous window: since 1970-01-01T09:16:40+09:00"),
            "the previous window is still named: {without:?}"
        );
        let with = trend_preamble(pair, true);
        assert!(
            with.contains("A delta marked n/a: the previous side holds no events"),
            "the reason text rides with an empty side: {with:?}"
        );
    }

    #[test]
    fn the_span_metric_judgment_follows_the_kind() {
        // Point kinds — every kind ingest writes without a duration or
        // output tokens — carry the count alone; a span kind carries all
        // three metrics.
        for kind in [
            "session_start",
            "prompt",
            "agent_spawn",
            "tool_use",
            "tool_error",
            "bash_cmd",
            "file_edit",
        ] {
            assert!(!carries_span_metrics(kind), "{kind} is a point kind");
        }
        assert!(carries_span_metrics("skill_invocation"));
        // An unknown kind gets the count row alone, so a new point kind never
        // renders a row whose stated reason does not apply.
        assert!(!carries_span_metrics("mcp_call"));
    }

    #[test]
    fn trend_runs_end_to_end_against_a_session() {
        // The full view path — including the empty store's early message —
        // runs against a real Session without error. The payload and row
        // structures are asserted by the tests above.
        let two = Session {
            harness: Harness::Claude,
            db_path: PathBuf::new(),
            store: two_window_store(),
        };
        trend(&two, trend_pair(), Format::Table).unwrap();
        trend(&two, trend_pair(), Format::Markdown).unwrap();
        trend(&two, trend_pair(), Format::Json).unwrap();
        let empty = Session {
            harness: Harness::Claude,
            db_path: PathBuf::new(),
            store: Store::in_memory().unwrap(),
        };
        trend(&empty, trend_pair(), Format::Table).unwrap();
        trend(&empty, trend_pair(), Format::Json).unwrap();
    }

    #[test]
    fn trend_kind_rows_read_a_gone_kind_as_a_move_to_zero() {
        // The skill-level path is covered above; this pins the kind-row path:
        // a kind present only in the PREVIOUS window has a real baseline, so
        // it reads as a move to zero — down, with a negative delta.
        let agg = kind_gone_store().trend_aggregates(trend_pair()).unwrap();
        let rows = trend_kind_rows(&agg);
        let gone = rows
            .iter()
            .find(|r| r[0] == "web_fetch" && r[1] == "events")
            .expect("the previous window's kind still shows");
        assert_eq!(gone[2], "1", "the previous side has a baseline: {rows:?}");
        assert_eq!(gone[3], "0");
        assert_eq!(gone[4], "-1", "a negative delta: {rows:?}");
        assert_eq!(gone[5], "down");
    }

    #[test]
    fn trend_kind_rows_render_only_applicable_metrics() {
        // Usage in both windows: two skill spans and one file_edit in the
        // previous, one of each in the current. A point kind carries no
        // duration or output tokens — ingest writes both as constant zeros —
        // so its rows for them would always read `0.0 | 0.0 | n/a | n/a`:
        // omitted, not rendered. A span kind renders all three metrics.
        let agg = two_window_store().trend_aggregates(trend_pair()).unwrap();
        let rows = trend_kind_rows(&agg);
        let edit: Vec<_> = rows.iter().filter(|r| r[0] == "file_edit").collect();
        assert_eq!(
            edit.len(),
            1,
            "a point kind renders its count alone: {rows:?}"
        );
        assert_eq!(edit[0][1], "events");
        assert_eq!(
            (edit[0][4].as_str(), edit[0][5].as_str()),
            ("+0", "same"),
            "the point kind's count still compares"
        );
        let span: Vec<_> = rows.iter().filter(|r| r[0] == "skill_invocation").collect();
        assert_eq!(
            span.len(),
            3,
            "a span kind renders all three metrics: {rows:?}"
        );
        for metric in ["events", "out tokens", "seconds"] {
            assert!(
                span.iter().any(|r| r[1] == metric),
                "the span kind's {metric} row is missing: {rows:?}"
            );
        }
    }

    #[test]
    fn trend_kind_rows_do_not_read_a_measured_zero_baseline_as_unavailable() {
        // A real 0.04s baseline rounds to the displayed 0.0: the comparison
        // is still one — up, at display precision — never 比較不能. The
        // sentinel fold (`map_or(0.0)`) used to turn it into an empty side,
        // so the row read as n/a whatever the current window held.
        let mut store = Store::in_memory().unwrap();
        let mut past = trend_session("past");
        past.start = Some(SessionStart {
            timestamp_ms: 1_500 * 1000,
            ctx: 9_000,
        });
        let mut span = trend_span("commit", 1_500, 50);
        span.duration_sec = 0.04;
        store.ingest(IngestBatch::new(&past, &[span], &[])).unwrap();
        let mut present = trend_session("present");
        present.start = Some(SessionStart {
            timestamp_ms: 2_500 * 1000,
            ctx: 10_000,
        });
        store
            .ingest(IngestBatch::new(
                &present,
                &[trend_span("commit", 2_500, 300)],
                &[],
            ))
            .unwrap();

        let seconds = trend_kind_rows(&store.trend_aggregates(trend_pair()).unwrap())
            .into_iter()
            .find(|r| r[0] == "skill_invocation" && r[1] == "seconds")
            .expect("a span kind renders its seconds row");
        assert_eq!(
            (seconds[2].as_str(), seconds[3].as_str()),
            ("0.0", "2.0"),
            "both sides display at the table's precision: {seconds:?}"
        );
        assert_eq!(
            (seconds[4].as_str(), seconds[5].as_str()),
            ("+2.0", "up"),
            "a measured zero baseline compares: {seconds:?}"
        );
    }

    #[test]
    fn trend_splits_a_straddling_session_across_the_two_axes() {
        // One session started in the previous window whose events happen in
        // the current one: totals follow the sessions axis (started_at), the
        // events follow their own epochs — the axes never merge.
        let agg = straddling_store().trend_aggregates(trend_pair()).unwrap();

        let rows = trend_total_rows(&agg);
        let sessions = rows.iter().find(|r| r[0] == "sessions").unwrap();
        assert_eq!(sessions[1], "1", "started_at is in the previous window");
        assert_eq!(sessions[2], "0");
        assert_eq!(sessions[3], "-1");
        assert_eq!(sessions[4], "down");
        let runs = rows.iter().find(|r| r[0] == "subagent runs").unwrap();
        assert_eq!((runs[1].as_str(), runs[2].as_str()), ("1", "0"));
        let tokens = rows.iter().find(|r| r[0] == "subagent tokens").unwrap();
        assert_eq!((tokens[1].as_str(), tokens[3].as_str()), ("100", "-100"));

        // Events split by epoch: the span and the work event happened in the
        // current window, so the previous side of those rows is empty
        // (比較不能) — and the session_start event rides its own epoch, the
        // stamped start in the previous window.
        let kind_rows = trend_kind_rows(&agg);
        let span = kind_rows
            .iter()
            .find(|r| r[0] == "skill_invocation" && r[1] == "events")
            .unwrap();
        assert_eq!(span[2], "0");
        assert_eq!(span[3], "1");
        assert_eq!(span[4], "n/a");
        let edit = kind_rows
            .iter()
            .find(|r| r[0] == "file_edit" && r[1] == "events")
            .unwrap();
        assert_eq!(edit[2], "0");
        assert_eq!(edit[3], "1");
        assert_eq!(edit[4], "n/a");
        let started = kind_rows
            .iter()
            .find(|r| r[0] == "session_start" && r[1] == "events")
            .unwrap();
        assert_eq!(started[2], "1", "the start event rides started_at");
        assert_eq!(started[3], "0");
        assert_eq!(started[4], "-1");
        assert_eq!(started[5], "down");
    }
}
