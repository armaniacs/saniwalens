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
