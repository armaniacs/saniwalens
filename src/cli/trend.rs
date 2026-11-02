//! The usage-trend view: what each kind of event, each skill, and the session
//! and subagent totals did this window against the equal-length window right
//! before it. One window's comparisons ride the store's two-window aggregates,
//! and both of the view's forms — the table's cells and the JSON machine
//! contract — derive from one typed payload ([`trend_payload`]), so the two
//! forms state one judged comparison per metric and cannot drift apart.

use anyhow::Result;

use crate::core::bucket::JST_OFFSET_SECS;
use crate::core::period::{self, WindowPair};
use crate::session::{Session, Style};
use crate::store::{KindRollup, TrendWindowAggregates, TwoWindow, skill_deltas};

use super::{Align, Format, emit_json, framed, note, preamble, render};

/// One direction of a two-window comparison. `Unavailable` is 比較不能: the
/// previous side holds no events, so there is nothing to compare against —
/// labeled, never read as a drop to zero. The JSON value is the variant's own
/// snake_case name ("unavailable"), explicit where the table's label is terse.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
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

/// One metric's two-window comparison as the payload carries it: both sides'
/// values, the delta, and the direction. The judgment already happened at
/// construction, so every derivation — the JSON machine contract and the
/// table's cells — reads this one judged form; the delta is `None` when the
/// comparison cannot be made, and no change is claimed for it.
#[derive(Debug, Clone, serde::Serialize)]
pub(crate) struct Metric<T> {
    previous: T,
    current: T,
    delta: Option<T>,
    direction: Direction,
}

/// One kind's payload: its name and its three metrics — every kind × three
/// metrics, the machine contract's full shape even for a point kind whose
/// output tokens / duration ingest writes as constant zeros (only the table
/// omits what cannot apply, never the JSON).
#[derive(Debug, Clone, serde::Serialize)]
pub(crate) struct KindPayload {
    kind: String,
    events: Metric<i64>,
    out_tokens: Metric<i64>,
    duration_sec: Metric<f64>,
}

/// One skill's payload: the invocations in each window with the delta and the
/// direction — the largest-movement-first order the aggregation gives.
#[derive(Debug, Clone, serde::Serialize)]
pub(crate) struct SkillPayload {
    skill: String,
    #[serde(flatten)]
    invocations: Metric<i64>,
}

/// One window's resolved bounds as the JSON carries them — the Period
/// section's `since` / `until` display strings, the Option the bounds
/// function hands over (a resolved trend pair is always bounded, so both are
/// present; a null would say the end is open).
#[derive(Debug, Clone, serde::Serialize)]
pub(crate) struct WindowBounds {
    since: Option<String>,
    until: Option<String>,
}

/// The payload's `window` object: both windows' resolved bounds, so the
/// machine contract names the windows it compares. Built here, by the payload
/// function itself — the window attachment is never delegated to the
/// single-window views' `with_window`.
#[derive(Debug, Clone, serde::Serialize)]
pub(crate) struct PayloadWindows {
    current: WindowBounds,
    previous: WindowBounds,
}

/// The trend's typed payload: both windows' ranges under `window`, the
/// per-kind rows, the skill deltas, and the session/subagent totals. The
/// table's cells and the JSON machine contract both derive from this one
/// structure, so a metric's delta and direction are judged once and the two
/// forms state the same comparison.
#[derive(Debug, Clone, serde::Serialize)]
pub(crate) struct TrendPayload {
    window: PayloadWindows,
    kinds: Vec<KindPayload>,
    skills: Vec<SkillPayload>,
    sessions: Metric<i64>,
    subagent_runs: Metric<i64>,
    sub_tokens: Metric<i64>,
}

impl TrendPayload {
    /// The machine contract: the payload serialized to the Value `emit_json`
    /// pretty-prints — the same shape the hand-built `json!` form produced
    /// (the Value map sorts the keys, integers stay integers, reals stay
    /// reals, and an unclaimable delta stays null). Serializing the typed
    /// payload is total, so the expectation cannot fire.
    fn json(&self) -> serde_json::Value {
        serde_json::to_value(self).expect("the typed trend payload serializes")
    }
}

/// One count metric's judged comparison (the kind rollups' events and output
/// tokens, the skill invocations, and the totals): the value sentinel
/// ([`direction_of`]) folds a zero previous side to 比較不能 — the store
/// holds no baseline for it, so no delta is claimed. A kind rollup the window
/// does not hold folds to that zero; a rollup the window holds always carries
/// its events, so a present events baseline never reads as empty.
fn int_metric(previous: i64, current: i64) -> Metric<i64> {
    let direction = direction_of(previous, current);
    Metric {
        previous,
        current,
        delta: match direction {
            Direction::Unavailable => None,
            _ => Some(current - previous),
        },
        direction,
    }
}

/// The trend's ONE precision rule for a float metric: the empty previous side
/// (the 0.0 an absent rollup folds to) is 比較不能 — the store holds no
/// baseline for the row, so no delta is claimed — and a present baseline is
/// judged at the display's precision, the same rounding the `{:.1}` cells
/// show, so a drift smaller than the shown 0.1 (an ms-derived duration's
/// addition error, e.g. 3.3s vs 3.3000000000000003s) cannot claim a move the
/// table renders as ±0.0. The delta is that same display-precise difference,
/// so the JSON number and the table's `+0.0` are one fact. A present baseline
/// that rounds to the displayed 0.0 (a real 0.04s) still compares.
fn float_comparison(previous: f64, current: f64) -> (Option<f64>, Direction) {
    if previous == 0.0 {
        return (None, Direction::Unavailable);
    }
    let (previous, current) = (display_precise(previous), display_precise(current));
    (
        Some(display_precise(current - previous)),
        direction_between(previous, current),
    )
}

/// One float metric's judged comparison (the kind durations), by
/// [`float_comparison`].
fn float_metric(previous: f64, current: f64) -> Metric<f64> {
    let (delta, direction) = float_comparison(previous, current);
    Metric {
        previous,
        current,
        delta,
        direction,
    }
}

/// The trend's typed payload: both windows' ranges under `window`, the
/// per-kind rows, the skill deltas, and the session/subagent totals — every
/// metric carrying both windows' values, the delta, and the direction, each
/// judged once by the one emptiness sentinel and the one float precision
/// rule. The table's cells and the JSON machine contract both derive from
/// this structure.
pub(crate) fn trend_payload(
    pair: WindowPair,
    agg: &TwoWindow<TrendWindowAggregates>,
) -> TrendPayload {
    let (current_since, current_until) = period::bounds(pair.current, JST_OFFSET_SECS);
    let (previous_since, previous_until) = period::bounds(pair.previous, JST_OFFSET_SECS);
    let kinds: Vec<KindPayload> = kind_pairs(agg)
        .into_iter()
        .map(|(kind, current, previous)| {
            let (c, p) = (current.as_ref(), previous.as_ref());
            KindPayload {
                kind: kind.clone(),
                events: int_metric(p.map_or(0, |r| r.events), c.map_or(0, |r| r.events)),
                out_tokens: int_metric(
                    p.map_or(0, |r| r.out_tokens),
                    c.map_or(0, |r| r.out_tokens),
                ),
                duration_sec: float_metric(
                    p.map_or(0.0, |r| r.duration_sec),
                    c.map_or(0.0, |r| r.duration_sec),
                ),
            }
        })
        .collect();
    let skills: Vec<SkillPayload> = skill_deltas(&agg.current.skills, &agg.previous.skills)
        .iter()
        .map(|d| SkillPayload {
            skill: d.skill.clone(),
            invocations: int_metric(d.previous, d.current),
        })
        .collect();
    TrendPayload {
        window: PayloadWindows {
            current: WindowBounds {
                since: current_since,
                until: current_until,
            },
            previous: WindowBounds {
                since: previous_since,
                until: previous_until,
            },
        },
        kinds,
        skills,
        sessions: int_metric(agg.previous.sessions, agg.current.sessions),
        subagent_runs: int_metric(agg.previous.subagent_runs, agg.current.subagent_runs),
        sub_tokens: int_metric(agg.previous.sub_tokens, agg.current.sub_tokens),
    }
}

/// The two windows' per-kind rollups paired by kind — a kind present in only
/// one window keeps None on the other side. The payload's kind metrics fold a
/// None side to the default value, whose shape stays every kind × three
/// metrics; the folded zero is the empty side the sentinel reads as 比較不能.
/// Sorted by kind, the aggregation's own order.
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

/// A float rounded to the display's precision — the same rounding the
/// `{:.1}` cells show (round via the format itself, so the rounded value is
/// by construction the one the table displays). The one precision rule
/// ([`float_comparison`]) judges at this rounding.
fn display_precise(value: f64) -> f64 {
    format!("{value:.1}").parse().unwrap_or(value)
}

/// A table cell pair from one judged integer metric: the signed delta and the
/// direction — both n/a when the comparison cannot be made. The judgment
/// happened in the payload; the cells render it.
fn int_cells(metric: &Metric<i64>) -> (String, String) {
    let delta = metric
        .delta
        .map_or_else(|| "n/a".to_string(), |delta| format!("{:+}", delta));
    (delta, metric.direction.label().to_string())
}

/// The float form: the delta at the display's `{:+.1}` — the precision the
/// payload already judged the comparison at.
fn float_cells(metric: &Metric<f64>) -> (String, String) {
    let delta = metric
        .delta
        .map_or_else(|| "n/a".to_string(), |delta| format!("{:+.1}", delta));
    (delta, metric.direction.label().to_string())
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

/// The per-kind section's rows, derived from the payload: one row per kind
/// and *applicable* metric, so each row carries one comparison and one
/// direction — a point kind renders its count alone ([`carries_span_metrics`]),
/// the row-level form of the usage view's empty-section convention. Both
/// windows' values keep their columns even when the comparison is n/a, and
/// the judgment rides the payload ([`int_cells`] / [`float_cells`]): a rollup
/// the window does not hold folds to the empty-side zero, a rollup the window
/// holds compares (a present events baseline always carries its events).
pub(crate) fn trend_kind_rows(payload: &TrendPayload) -> Vec<Vec<String>> {
    let mut rows = Vec::new();
    for kind in &payload.kinds {
        let (delta, direction) = int_cells(&kind.events);
        rows.push(vec![
            kind.kind.clone(),
            "events".to_string(),
            kind.events.previous.to_string(),
            kind.events.current.to_string(),
            delta,
            direction,
        ]);
        if !carries_span_metrics(&kind.kind) {
            continue;
        }
        let (delta, direction) = int_cells(&kind.out_tokens);
        rows.push(vec![
            kind.kind.clone(),
            "out tokens".to_string(),
            kind.out_tokens.previous.to_string(),
            kind.out_tokens.current.to_string(),
            delta,
            direction,
        ]);
        let (delta, direction) = float_cells(&kind.duration_sec);
        rows.push(vec![
            kind.kind.clone(),
            "seconds".to_string(),
            format!("{:.1}", kind.duration_sec.previous),
            format!("{:.1}", kind.duration_sec.current),
            delta,
            direction,
        ]);
    }
    rows
}

/// The skill section's rows, derived from the payload: `skill_deltas`' order
/// (largest movement first), one row per skill with both windows' invocation
/// counts.
fn trend_skill_rows(payload: &TrendPayload) -> Vec<Vec<String>> {
    payload
        .skills
        .iter()
        .map(|skill| {
            let (delta, direction) = int_cells(&skill.invocations);
            vec![
                skill.skill.clone(),
                skill.invocations.previous.to_string(),
                skill.invocations.current.to_string(),
                delta,
                direction,
            ]
        })
        .collect()
}

/// The totals section's rows, derived from the payload: sessions, subagent
/// runs, and subagent tokens along the sessions axis.
pub(crate) fn trend_total_rows(payload: &TrendPayload) -> Vec<Vec<String>> {
    [
        ("sessions", &payload.sessions),
        ("subagent runs", &payload.subagent_runs),
        ("subagent tokens", &payload.sub_tokens),
    ]
    .into_iter()
    .map(|(label, metric)| {
        let (delta, direction) = int_cells(metric);
        vec![
            label.to_string(),
            metric.previous.to_string(),
            metric.current.to_string(),
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
/// are the same windowed queries the single-window views read. One typed
/// payload is built and both of the view's forms derive from it.
pub(crate) fn trend(sess: &Session, pair: WindowPair, format: Format) -> Result<()> {
    let store = &sess.store;
    eprint_trend_windows(pair);
    let agg = store.trend_aggregates(pair)?;
    let payload = trend_payload(pair, &agg);
    if format == Format::Json {
        return emit_json(&payload.json());
    }
    // Nothing in either window *and* no session on either axis: the store
    // holds nothing to compare — unlike an empty previous window, this is
    // the analyze hint's case. The payload carries both facts.
    if payload.kinds.is_empty() && payload.sessions.previous == 0 && payload.sessions.current == 0 {
        println!("no usage in either comparison window — run `saniwalens analyze` first");
        return Ok(());
    }
    let kind_rows = trend_kind_rows(&payload);
    // The reason text's condition: some rendered kind's previous side holds
    // no rollup — exactly the comparisons the rows mark n/a, so the preamble
    // never states a reason the report cannot show. The judgment rides the
    // payload, so the condition reads its unavailable directions.
    let has_unavailable = payload
        .kinds
        .iter()
        .any(|kind| kind.events.direction == Direction::Unavailable);
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
        &trend_skill_rows(&payload),
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
        &trend_total_rows(&payload),
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
        // as ±0.0 — direction and displayed delta never disagree. The cells
        // read the payload's one judged form, so the same assertions hold for
        // the JSON side.
        let metric = float_metric(3.3, 3.3000000000000003);
        assert_eq!(
            metric.direction,
            Direction::Same,
            "drift above zero is not a move"
        );
        let (delta, direction) = float_cells(&metric);
        assert_eq!(delta, "+0.0");
        assert_eq!(direction, "same");
        // A sub-0.05 real move rounds to ±0.0 too: no direction is claimed
        // for a move the table cannot show.
        let metric = float_metric(1.0, 1.0 - 1e-9);
        assert_eq!(
            metric.direction,
            Direction::Same,
            "drift below zero is not a move"
        );
        let (delta, direction) = float_cells(&metric);
        assert_eq!(delta, "+0.0");
        assert_eq!(direction, "same");
        // A move the table CAN show still reads as one.
        let metric = float_metric(1.0, 2.0);
        assert_eq!(metric.direction, Direction::Up);
        let (delta, direction) = float_cells(&metric);
        assert_eq!(delta, "+1.0");
        assert_eq!(direction, "up");
        // An empty previous side (the 0.0 an absent rollup folds to) is the
        // only 比較不能: no delta is claimed, whatever the current value is.
        let metric = float_metric(0.0, 2.0);
        assert_eq!(metric.direction, Direction::Unavailable);
        assert_eq!(metric.delta, None);
        let (delta, direction) = float_cells(&metric);
        assert_eq!((delta.as_str(), direction.as_str()), ("n/a", "n/a"));
        // A present baseline that rounds to 0.0 still compares — it is a
        // measured zero, not an empty side.
        let metric = float_metric(0.04, 2.0);
        assert_eq!(metric.direction, Direction::Up);
        let (delta, direction) = float_cells(&metric);
        assert_eq!((delta.as_str(), direction.as_str()), ("+2.0", "up"));
    }

    #[test]
    fn the_precision_rule_is_one_for_the_table_and_the_json() {
        // The precision-rule drift: a previous window's 3.3s and the current
        // window's ms-derived 3.3000000000000003s form of the same duration
        // differ by less than the display's 0.1 — the table reads
        // "+0.0 | same", so the JSON must read the same judgment, never a
        // move the table cannot show (the drifted "4.4e-16 | up").
        let mut store = Store::in_memory().unwrap();
        let mut past = trend_session("past");
        past.start = Some(SessionStart {
            timestamp_ms: 1_500 * 1000,
            ctx: 9_000,
        });
        let mut past_span = trend_span("commit", 1_500, 50);
        past_span.duration_sec = 3.3;
        store
            .ingest(IngestBatch::new(&past, &[past_span], &[]))
            .unwrap();
        let mut present = trend_session("present");
        present.start = Some(SessionStart {
            timestamp_ms: 2_500 * 1000,
            ctx: 10_000,
        });
        let mut present_span = trend_span("commit", 2_500, 50);
        present_span.duration_sec = 3.3000000000000003;
        store
            .ingest(IngestBatch::new(&present, &[present_span], &[]))
            .unwrap();

        let pair = trend_pair();
        let payload = trend_payload(pair, &store.trend_aggregates(pair).unwrap());

        // The JSON side: no move may be claimed for a drift the table shows
        // as ±0.0 — the delta and the direction state one judgment.
        let json = payload.json();
        let skill = json["kinds"]
            .as_array()
            .unwrap()
            .iter()
            .find(|k| k["kind"] == "skill_invocation")
            .unwrap();
        assert_eq!(
            skill["duration_sec"]["direction"], "same",
            "a drift smaller than the display's 0.1 is not a move"
        );
        assert_eq!(skill["duration_sec"]["delta"], 0.0);

        // The table side: the same judgment, derived from the same payload.
        let rows = trend_kind_rows(&payload);
        let seconds = rows
            .iter()
            .find(|r| r[0] == "skill_invocation" && r[1] == "seconds")
            .expect("a span kind renders its seconds row");
        assert_eq!((seconds[4].as_str(), seconds[5].as_str()), ("+0.0", "same"));
    }

    #[test]
    fn trend_json_carries_both_windows_and_their_deltas() {
        let store = two_window_store();
        let pair = trend_pair();
        let agg = store.trend_aggregates(pair).unwrap();
        let json = trend_payload(pair, &agg).json();

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
        let json = trend_payload(pair, &agg).json();

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
        let payload = trend_payload(trend_pair(), &agg);

        // The kind rows keep both windows' values (the previous side renders
        // as 0) but mark the delta and direction n/a.
        let rows = trend_kind_rows(&payload);
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
        let payload = trend_payload(
            trend_pair(),
            &kind_gone_store().trend_aggregates(trend_pair()).unwrap(),
        );
        let rows = trend_kind_rows(&payload);
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
        let payload = trend_payload(trend_pair(), &agg);
        let rows = trend_kind_rows(&payload);
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

        let seconds = {
            let payload =
                trend_payload(trend_pair(), &store.trend_aggregates(trend_pair()).unwrap());
            trend_kind_rows(&payload)
        }
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
        let payload = trend_payload(
            trend_pair(),
            &straddling_store().trend_aggregates(trend_pair()).unwrap(),
        );

        let rows = trend_total_rows(&payload);
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
        let kind_rows = trend_kind_rows(&payload);
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
