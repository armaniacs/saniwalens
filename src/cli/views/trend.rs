//! The usage-trend view: what each kind of event, each skill, and the session
//! and subagent totals did this window against the equal-length window right
//! before it. One window's comparisons ride the store's two-window aggregates,
//! and both of the view's forms — the table's cells and the JSON machine
//! contract — derive from one typed payload ([`trend_payload`]), so the two
//! forms state one judged comparison per metric and cannot drift apart.

use anyhow::Result;

use crate::core::bucket::JST_OFFSET_SECS;
use crate::core::period::{self, WindowPair};
use crate::core::trend::{
    TrendWindowAggregates, TwoWindow, kind_pairs, skill_deltas, surface_deltas,
};
use crate::session::{Session, Style};

use super::super::{Align, Format, emit_json, framed, note, preamble, render};

/// One direction of a two-window comparison. `Unavailable` is 比較不能: no
/// comparison was made — that row's previous side has no baseline, or the
/// metric does not apply to the kind — labeled, never read as a drop to zero.
/// The JSON value is the variant's own snake_case name ("unavailable"),
/// explicit where the table's label is terse.
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

/// Why a metric carries no comparison. `NoBaseline` is the only reason a
/// window can give — the row's previous side holds nothing (`Baseline::Absent`)
/// — and `NotApplicable` is the kind's own shape (a point kind has no output
/// tokens or duration, so the metric is not judged at all). The payload's
/// judgment and the machine contract's `reason` are one fact: the reason is set
/// exactly when the direction is `Unavailable`, so the table's n/a rows, the
/// preamble's sentence, the note, and the JSON all read the same bit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum UnavailableReason {
    NoBaseline,
    NotApplicable,
}

/// The previous side of one metric: a measured value — **0 included** — or
/// nothing at all. Presence is a fact about the aggregate's row, never about
/// the value, so a window whose span really reported 0 output tokens is a
/// baseline to compare against (`Measured(0)`); only a window that holds no
/// row is `Absent`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Baseline<T> {
    Absent,
    Measured(T),
}

/// Which way a metric moved between two sides both of which are present —
/// the pure comparison the payload's one judgment calls.
fn direction_between<T: PartialOrd>(previous: T, current: T) -> Direction {
    match current.partial_cmp(&previous) {
        Some(std::cmp::Ordering::Greater) => Direction::Up,
        Some(std::cmp::Ordering::Less) => Direction::Down,
        _ => Direction::Same,
    }
}

/// One metric's two-window comparison as the payload carries it: both sides'
/// values, the delta, the direction, and — when nothing was claimed — why. The
/// judgment already happened at construction ([`metric`]), so every derivation
/// (the JSON machine contract and the table's cells) reads this one judged
/// form; the delta is `None` exactly when the comparison cannot be made, and
/// `reason` is `Some` exactly then.
#[derive(Debug, Clone, serde::Serialize)]
pub(crate) struct Metric<T> {
    previous: T,
    current: T,
    delta: Option<T>,
    direction: Direction,
    reason: Option<UnavailableReason>,
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

/// One surface's payload: the usage events in each window with the delta and
/// the direction — the largest-movement-first order the aggregation gives.
/// The row names `kind/id`: usage is joined on that pair everywhere else.
#[derive(Debug, Clone, serde::Serialize)]
pub(crate) struct SurfacePayload {
    kind: String,
    id: String,
    #[serde(flatten)]
    events: Metric<i64>,
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
    surfaces: Vec<SurfacePayload>,
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

    /// Whether any metric the report renders carries no baseline — the only
    /// n/a reason a window can give, and exactly what the preamble's reason
    /// sentence explains. A metric that does not apply is not this case
    /// (`NotApplicable` rows are omitted from the table), so the sentence
    /// never states a reason the report cannot show.
    fn has_no_baseline(&self) -> bool {
        let reasons = self
            .kinds
            .iter()
            .flat_map(|kind| {
                [
                    kind.events.reason,
                    kind.out_tokens.reason,
                    kind.duration_sec.reason,
                ]
            })
            .chain(self.skills.iter().map(|skill| skill.invocations.reason))
            .chain(self.surfaces.iter().map(|surface| surface.events.reason))
            .chain([
                self.sessions.reason,
                self.subagent_runs.reason,
                self.sub_tokens.reason,
            ]);
        reasons
            .into_iter()
            .any(|reason| reason == Some(UnavailableReason::NoBaseline))
    }

    /// Whether the previous window holds no event of any kind — the judgment
    /// the note's text ("holds no events") states, and the same fact the
    /// aggregates' former `by_kind.is_empty()` read. This is the **events**
    /// axis only, not the totals axis: a previous window that holds a session
    /// but no events still fires the note, because the note speaks about
    /// events, and its totals rows may well compare while its kind rows are
    /// n/a.
    fn previous_events_are_empty(&self) -> bool {
        self.kinds
            .iter()
            .all(|kind| kind.events.reason == Some(UnavailableReason::NoBaseline))
    }
}

/// A previous side as the payload carries it: an aggregate row that exists is
/// a measured baseline — its value is the measurement, zero included — and a
/// row the window does not hold is no baseline at all.
fn baseline<T>(measured: Option<T>) -> Baseline<T> {
    match measured {
        Some(value) => Baseline::Measured(value),
        None => Baseline::Absent,
    }
}

/// The one count comparison: both sides present, so the delta is claimed and
/// the direction is the two values' own order.
fn int_judge(previous: i64, current: i64) -> (Option<i64>, Direction) {
    (
        Some(current - previous),
        direction_between(previous, current),
    )
}

/// The trend's ONE precision rule for a float metric: a present baseline is
/// measured against the current side at the display's precision, the same
/// rounding the `{:.1}` cells show, so a drift smaller than the shown 0.1 (an
/// ms-derived duration's addition error, e.g. 3.3s vs 3.3000000000000003s)
/// cannot claim a move the table renders as ±0.0. The delta is that same
/// display-precise difference, so the JSON number and the table's `+0.0` are
/// one fact. A measured 0.0 is a baseline like any other: only an absent
/// rollup is 比較不能.
fn float_judge(previous: f64, current: f64) -> (Option<f64>, Direction) {
    let (previous, current) = (display_precise(previous), display_precise(current));
    (
        Some(display_precise(current - previous)),
        direction_between(previous, current),
    )
}

/// One metric nothing was claimed for, with the reason it carries. This and
/// [`metric`] are the only two places a `Metric` is built, so the invariant
/// `direction == Unavailable` ⇔ `reason.is_some()` holds by construction:
/// `NoBaseline` folds the absent side to the zero the previous column has
/// always shown, and `NotApplicable` keeps the values ingest writes (constant
/// zeros), so the machine contract's shape stays whole either way.
fn unavailable<T>(previous: T, current: T, reason: UnavailableReason) -> Metric<T> {
    Metric {
        previous,
        current,
        delta: None,
        direction: Direction::Unavailable,
        reason: Some(reason),
    }
}

/// The trend's one metric judgment: a baseline that exists is compared (a
/// measured zero included), and only `Baseline::Absent` is `NoBaseline`. Every
/// metric of every section — the kind rollups' events, output tokens and
/// duration, the skill invocations, and the totals — is built here, so the
/// table's cells and the JSON contract cannot judge one row two ways.
fn metric<T: Default + Copy>(
    baseline: Baseline<T>,
    current: T,
    judge: impl FnOnce(T, T) -> (Option<T>, Direction),
) -> Metric<T> {
    match baseline {
        Baseline::Absent => unavailable(T::default(), current, UnavailableReason::NoBaseline),
        Baseline::Measured(previous) => {
            let (delta, direction) = judge(previous, current);
            // `judge` is one of the two comparisons below, neither of which
            // can return `Unavailable` — the reason a measured baseline always
            // carries none. Asserted, not debug-asserted, so a release build
            // enforces the same invariant.
            assert_ne!(
                direction,
                Direction::Unavailable,
                "a measured baseline always compares"
            );
            Metric {
                previous,
                current,
                delta,
                direction,
                reason: None,
            }
        }
    }
}

/// The trend's typed payload: both windows' ranges under `window`, the
/// per-kind rows, the skill deltas, and the session/subagent totals — every
/// metric carrying both windows' values, the delta, the direction, and the
/// reason it has none, each judged once by [`metric`]. The per-kind rollups
/// keep their `Option` here: presence is what tells a measured zero from an
/// absent row, so the fold happens inside the judgment, not before it. The
/// table's cells and the JSON machine contract both derive from this structure.
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
            let events = metric(
                baseline(p.map(|r| r.events)),
                c.map_or(0, |r| r.events),
                int_judge,
            );
            // A point kind carries no duration or output tokens: ingest writes
            // both columns as constant zeros, so judging them would render a
            // reason that does not apply. They stay in the machine contract as
            // `not_applicable` with a null delta (PBI 30's decision) and only
            // the table omits them.
            let (out_tokens, duration_sec) = if carries_span_metrics(&kind) {
                (
                    metric(
                        baseline(p.map(|r| r.out_tokens)),
                        c.map_or(0, |r| r.out_tokens),
                        int_judge,
                    ),
                    metric(
                        baseline(p.map(|r| r.duration_sec)),
                        c.map_or(0.0, |r| r.duration_sec),
                        float_judge,
                    ),
                )
            } else {
                (
                    unavailable(
                        p.map_or(0, |r| r.out_tokens),
                        c.map_or(0, |r| r.out_tokens),
                        UnavailableReason::NotApplicable,
                    ),
                    unavailable(
                        p.map_or(0.0, |r| r.duration_sec),
                        c.map_or(0.0, |r| r.duration_sec),
                        UnavailableReason::NotApplicable,
                    ),
                )
            };
            KindPayload {
                kind,
                events,
                out_tokens,
                duration_sec,
            }
        })
        .collect();
    // A skill's previous side exists when the previous window holds a usage
    // row for it — the name set is the presence signal (`skill_deltas` folds
    // the two windows' counts, not their rows).
    let previous_skills: std::collections::BTreeSet<&str> = agg
        .previous
        .skills
        .iter()
        .map(|usage| usage.skill.as_str())
        .collect();
    let skills: Vec<SkillPayload> = skill_deltas(&agg.current.skills, &agg.previous.skills)
        .iter()
        .map(|d| SkillPayload {
            skill: d.skill.clone(),
            invocations: metric(
                if previous_skills.contains(d.skill.as_str()) {
                    Baseline::Measured(d.previous)
                } else {
                    Baseline::Absent
                },
                d.current,
                int_judge,
            ),
        })
        .collect();
    // A surface's previous side exists when the previous window holds a usage
    // row for that (kind, id) pair — the same presence signal as skills, one
    // level more specific (`surface_deltas` folds the two windows' counts, not
    // their rows).
    let previous_surfaces: std::collections::BTreeSet<(&str, &str)> = agg
        .previous
        .surfaces
        .iter()
        .map(|(kind, id, _)| (kind.as_str(), id.as_str()))
        .collect();
    let surfaces: Vec<SurfacePayload> =
        surface_deltas(&agg.current.surfaces, &agg.previous.surfaces)
            .iter()
            .map(|d| SurfacePayload {
                kind: d.kind.clone(),
                id: d.id.clone(),
                events: metric(
                    if previous_surfaces.contains(&(d.kind.as_str(), d.id.as_str())) {
                        Baseline::Measured(d.previous)
                    } else {
                        Baseline::Absent
                    },
                    d.current,
                    int_judge,
                ),
            })
            .collect();
    // The totals follow the sessions axis: the previous window's baseline
    // exists when it holds a session, whose columns are the subagent figures.
    // A session that ran no subagent measures 0 runs — a real baseline, so
    // 0 -> 2 compares. Count rows themselves (kind rollups, skill usage) only
    // exist with at least one event, so a count's measured zero cannot arrive
    // that way and stays `NoBaseline` as before: a window with no row has
    // nothing to compare against, deliberately unchanged here.
    let sessions_present = agg.previous.sessions > 0;
    let baseline_of_total = |measured: i64| {
        if sessions_present {
            Baseline::Measured(measured)
        } else {
            Baseline::Absent
        }
    };
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
        surfaces,
        sessions: metric(
            baseline_of_total(agg.previous.sessions),
            agg.current.sessions,
            int_judge,
        ),
        subagent_runs: metric(
            baseline_of_total(agg.previous.subagent_runs),
            agg.current.subagent_runs,
            int_judge,
        ),
        sub_tokens: metric(
            baseline_of_total(agg.previous.sub_tokens),
            agg.current.sub_tokens,
            int_judge,
        ),
    }
}

/// A float rounded to the display's precision — the same rounding the
/// `{:.1}` cells show (round via the format itself, so the rounded value is
/// by construction the one the table displays). The one precision rule
/// ([`float_judge`]) judges at this rounding.
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
/// direction. Applicability is the payload's own judgment — a `NotApplicable`
/// metric renders no row (PBI 30's decision: a point kind would otherwise show
/// `0.0 | 0.0 | n/a | n/a` with a reason that does not apply), the row-level
/// form of the usage view's empty-section convention. Both windows' values keep
/// their columns even when the comparison is n/a, and the judgment rides the
/// payload ([`int_cells`] / [`float_cells`]): a kind the window does not hold
/// has no baseline, a kind it holds compares (a measured zero included).
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
        if kind.out_tokens.reason == Some(UnavailableReason::NotApplicable) {
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

/// The per-surface section's rows, derived from the payload: `kind/id`,
/// both windows' event counts, and the judged delta — largest movement first.
pub(crate) fn trend_surface_rows(payload: &TrendPayload) -> Vec<Vec<String>> {
    payload
        .surfaces
        .iter()
        .map(|surface| {
            let (delta, direction) = int_cells(&surface.events);
            vec![
                format!("{}/{}", surface.kind, surface.id),
                surface.events.previous.to_string(),
                surface.events.current.to_string(),
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
/// sentence rides only with a report that actually holds an n/a comparison
/// (`TrendPayload::has_no_baseline`), and it names the scope the reason holds
/// in: *that row's* previous side, so one sentence covers a kind row with no
/// events, a skill row with no invocations, and a totals row with no session in
/// the previous window. The applied-out rows never reach the table, so the
/// sentence is never printed for a metric that does not apply. Table format
/// only.
pub(crate) fn trend_preamble(pair: WindowPair, has_unavailable: bool) -> String {
    let mut text =
        String::from("Usage trend — this window against the equal-length window right before it.");
    if has_unavailable {
        text.push_str(
            "\nA delta marked n/a: that row's previous side has nothing to compare — not a drop.",
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
    // The reason text's condition: some rendered metric has no baseline.
    // `has_no_baseline` is the payload's own judgment of the n/a rows, so the
    // preamble never states a reason the report cannot show — and an
    // inapplicable metric (omitted from the table) never turns it on.
    preamble(
        format,
        &framed(
            &trend_preamble(pair, payload.has_no_baseline()),
            pair.current,
        ),
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
        "Surfaces (largest movement first):",
        &["surface", "previous", "current", "delta", "direction"],
        &[
            Align::Left,
            Align::Right,
            Align::Right,
            Align::Right,
            Align::Left,
        ],
        &trend_surface_rows(&payload),
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
    if payload.previous_events_are_empty() {
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

    /// The invariant every derivation relies on: a metric has a reason exactly
    /// when it has no direction, and claims a delta exactly when it compares.
    fn assert_unavailable_pairing<T: std::fmt::Debug + PartialEq>(metric: &Metric<T>) {
        assert_eq!(
            metric.direction == Direction::Unavailable,
            metric.reason.is_some(),
            "direction and reason must agree: {metric:?}"
        );
        assert_eq!(
            metric.delta.is_none(),
            metric.reason.is_some(),
            "an unavailable metric claims no delta: {metric:?}"
        );
        if metric.reason == Some(UnavailableReason::NotApplicable) {
            assert_eq!(metric.delta, None);
        }
    }

    #[test]
    fn the_metric_invariants_hold_for_every_construction() {
        // A metric nothing was claimed for carries a reason; one that compares
        // carries none — the invariant every derivation relies on.
        let compared = metric(Baseline::Measured(4), 1, int_judge);
        assert_eq!(compared.direction, Direction::Down);
        assert_eq!(compared.delta, Some(-3));
        assert_eq!(compared.reason, None);
        let absent = metric(Baseline::Absent, 7, int_judge);
        assert_eq!(absent.direction, Direction::Unavailable);
        assert_eq!(absent.reason, Some(UnavailableReason::NoBaseline));
        assert_eq!(absent.delta, None);
        // A metric that does not apply never claims a delta and names its own
        // reason, so the table can omit the row the reason would be false for.
        let inapplicable = unavailable(0, 0, UnavailableReason::NotApplicable);
        assert_eq!(inapplicable.direction, Direction::Unavailable);
        assert_eq!(inapplicable.reason, Some(UnavailableReason::NotApplicable));
        assert_eq!(inapplicable.delta, None);
        // Every payload metric obeys the same pairing, on every fixture shape:
        // both windows populated, and a previous side that holds nothing.
        for payload in [
            trend_payload(
                trend_pair(),
                &two_window_store().trend_aggregates(trend_pair()).unwrap(),
            ),
            trend_payload(
                trend_pair(),
                &one_sided_store().trend_aggregates(trend_pair()).unwrap(),
            ),
        ] {
            for metric in payload
                .kinds
                .iter()
                .flat_map(|kind| [&kind.events, &kind.out_tokens])
                .chain(payload.skills.iter().map(|skill| &skill.invocations))
                .chain([
                    &payload.sessions,
                    &payload.subagent_runs,
                    &payload.sub_tokens,
                ])
            {
                assert_unavailable_pairing(metric);
            }
            for metric in payload.kinds.iter().map(|kind| &kind.duration_sec) {
                assert_unavailable_pairing(metric);
            }
        }
    }

    #[test]
    fn a_measured_zero_is_a_baseline() {
        use Direction::Up;
        // A previous window whose span really reported 0 output tokens has a
        // baseline: the comparison is claimed, not folded to n/a.
        let tokens = metric(Baseline::Measured(0), 300, int_judge);
        assert_eq!(tokens.direction, Up);
        assert_eq!(tokens.delta, Some(300));
        assert_eq!(tokens.reason, None);
        assert_eq!(tokens.previous, 0);
        let (delta, direction) = int_cells(&tokens);
        assert_eq!((delta.as_str(), direction.as_str()), ("+300", "up"));
        // The float side: a measured 0.0 compares at the display's precision.
        let seconds = metric(Baseline::Measured(0.0), 2.0, float_judge);
        assert_eq!(seconds.direction, Up);
        assert_eq!(seconds.delta, Some(2.0));
        assert_eq!(seconds.reason, None);
        let (delta, direction) = float_cells(&seconds);
        assert_eq!((delta.as_str(), direction.as_str()), ("+2.0", "up"));
        // A measured baseline that rounds to the displayed 0.0 still compares.
        let rounded = metric(Baseline::Measured(0.04), 2.0, float_judge);
        assert_eq!(rounded.direction, Up);
        assert_eq!(rounded.reason, None);
        // A measured zero that truly did not move reads as unchanged, never as
        // unavailable: the row has a baseline.
        let still = metric(Baseline::Measured(0), 0, int_judge);
        assert_eq!(still.direction, Direction::Same);
        assert_eq!(still.delta, Some(0));
        assert_eq!(still.reason, None);
    }

    #[test]
    fn absent_is_the_only_no_baseline() {
        // `Baseline::Absent` is the one way to `NoBaseline`, on every value
        // shape (a measured zero never is) — the three baseline sources the
        // payload reads: an event rollup, a skill row, a session.
        let absent_count = metric(Baseline::<i64>::Absent, 5, int_judge);
        assert_eq!(absent_count.reason, Some(UnavailableReason::NoBaseline));
        assert_eq!(absent_count.direction, Direction::Unavailable);
        assert_eq!(absent_count.delta, None);
        let absent_duration = metric(Baseline::<f64>::Absent, 5.0, float_judge);
        assert_eq!(absent_duration.reason, Some(UnavailableReason::NoBaseline));
        assert_eq!(absent_duration.direction, Direction::Unavailable);
        assert_eq!(absent_duration.delta, None);
        assert_eq!(
            unavailable(0, 3, UnavailableReason::NoBaseline).reason,
            Some(UnavailableReason::NoBaseline)
        );
        assert_ne!(
            metric(Baseline::Measured(0), 0, int_judge).reason,
            Some(UnavailableReason::NoBaseline),
            "a measured zero is not an absent side"
        );
    }

    #[test]
    fn the_float_delta_judges_direction_at_display_precision() {
        // The unrounded seconds can differ by float drift smaller than the
        // display's 0.1: the direction must not claim a move the table shows
        // as ±0.0 — direction and displayed delta never disagree. The cells
        // read the payload's one judged form, so the same assertions hold for
        // the JSON side.
        let judged = metric(Baseline::Measured(3.3), 3.3000000000000003, float_judge);
        assert_eq!(
            judged.direction,
            Direction::Same,
            "drift above zero is not a move"
        );
        let (delta, direction) = float_cells(&judged);
        assert_eq!(delta, "+0.0");
        assert_eq!(direction, "same");
        // A sub-0.05 real move rounds to ±0.0 too: no direction is claimed
        // for a move the table cannot show.
        let judged = metric(Baseline::Measured(1.0), 1.0 - 1e-9, float_judge);
        assert_eq!(
            judged.direction,
            Direction::Same,
            "drift below zero is not a move"
        );
        let (delta, direction) = float_cells(&judged);
        assert_eq!(delta, "+0.0");
        assert_eq!(direction, "same");
        // A move the table CAN show still reads as one.
        let judged = metric(Baseline::Measured(1.0), 2.0, float_judge);
        assert_eq!(judged.direction, Direction::Up);
        let (delta, direction) = float_cells(&judged);
        assert_eq!(delta, "+1.0");
        assert_eq!(direction, "up");
        // An absent previous side is the only 比較不能: no delta is claimed,
        // whatever the current value is.
        let judged = metric(Baseline::<f64>::Absent, 2.0, float_judge);
        assert_eq!(judged.direction, Direction::Unavailable);
        assert_eq!(judged.delta, None);
        let (delta, direction) = float_cells(&judged);
        assert_eq!((delta.as_str(), direction.as_str()), ("n/a", "n/a"));
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
            !without.contains("that row's previous side"),
            "a report with no empty side states no reason: {without:?}"
        );
        assert!(
            without.contains("previous window: since 1970-01-01T09:16:40+09:00"),
            "the previous window is still named: {without:?}"
        );
        let with = trend_preamble(pair, true);
        // The sentence names the scope it holds in — that row's previous side
        // — so one wording covers a kind, a skill, and a totals row.
        assert!(
            with.contains(
                "A delta marked n/a: that row's previous side has nothing to compare — not a drop."
            ),
            "the reason text rides with an empty side: {with:?}"
        );
    }

    #[test]
    fn the_reason_text_and_note_read_the_payload() {
        // The two explanations a report can print must agree with the n/a rows
        // it actually renders: a rendered n/a row is a `NoBaseline` metric, the
        // reason sentence turns on for it, and the note turns on only when the
        // previous window holds no event of any kind (the events axis — the
        // note says "holds no events", not "holds no sessions"). A point
        // kind's inapplicable metric never turns the sentence on — its row is
        // omitted.
        let shapes = [
            ("two windows", two_window_store()),
            ("one sided", one_sided_store()),
            ("kind gone", kind_gone_store()),
            ("straddling", straddling_store()),
            ("empty", Store::in_memory().unwrap()),
        ];
        for (label, store) in shapes {
            let payload =
                trend_payload(trend_pair(), &store.trend_aggregates(trend_pair()).unwrap());
            let kind_rows = trend_kind_rows(&payload);
            let skill_rows = trend_skill_rows(&payload);
            let total_rows = trend_total_rows(&payload);
            let rendered_na = kind_rows
                .iter()
                .chain(&skill_rows)
                .chain(&total_rows)
                .filter(|row| row[row.len() - 1] == "n/a")
                .count();
            let empty_previous = payload.previous_events_are_empty();
            let no_baseline = payload.has_no_baseline();
            assert_eq!(
                rendered_na > 0,
                no_baseline,
                "{label}: a rendered n/a row and the reason sentence are one fact"
            );
            if empty_previous {
                assert!(
                    no_baseline,
                    "{label}: an empty previous window is a report of n/a rows"
                );
                // The note speaks about the events axis, so every kind's
                // previous side is absent.
                assert!(
                    payload
                        .kinds
                        .iter()
                        .all(|kind| kind.events.reason == Some(UnavailableReason::NoBaseline)),
                    "{label}: the note holds only when every previous kind is absent"
                );
            }
        }
        // The two shared windows are the shape whose note must stay off.
        let payload = trend_payload(
            trend_pair(),
            &two_window_store().trend_aggregates(trend_pair()).unwrap(),
        );
        assert!(!payload.has_no_baseline());
        assert!(!payload.previous_events_are_empty());
        // The one-sided shape holds n/a rows and an empty previous window.
        let payload = trend_payload(
            trend_pair(),
            &one_sided_store().trend_aggregates(trend_pair()).unwrap(),
        );
        assert!(payload.has_no_baseline());
        assert!(payload.previous_events_are_empty());
        // The straddling shape: the previous window holds a session *and* the
        // `session_start` event ingest stamps at that start, so the events
        // axis is not empty and the note stays off. The condition is the
        // events axis alone — the shape it must keep firing for is a store
        // whose previous window holds a session but no event at all (a store
        // predating `session_start` events, read with `--frozen`), which
        // `IngestBatch` cannot synthesize without touching the store layer.
        let payload = trend_payload(
            trend_pair(),
            &straddling_store().trend_aggregates(trend_pair()).unwrap(),
        );
        assert!(
            !payload.previous_events_are_empty(),
            "the stamped session_start is an event in the previous window"
        );
        assert_ne!(
            payload.sessions.reason,
            Some(UnavailableReason::NoBaseline),
            "the previous window does hold a session, so the totals axis is not empty"
        );
    }

    #[test]
    fn the_note_fires_on_an_empty_events_axis_even_with_a_session() {
        // The reachable shape for this payload is a store read with
        // `--frozen` that predates `session_start` events: the previous
        // window holds a session (so the totals compare) but no event of any
        // kind (so every kind row is n/a). The note is about the events axis,
        // and it must fire there — otherwise markdown prints n/a rows with no
        // explanation at all.
        let payload = TrendPayload {
            window: PayloadWindows {
                current: WindowBounds {
                    since: None,
                    until: None,
                },
                previous: WindowBounds {
                    since: None,
                    until: None,
                },
            },
            kinds: vec![KindPayload {
                kind: "file_edit".to_string(),
                events: metric(Baseline::Absent, 1, int_judge),
                out_tokens: unavailable(0, 0, UnavailableReason::NotApplicable),
                duration_sec: unavailable(0.0, 0.0, UnavailableReason::NotApplicable),
            }],
            skills: Vec::new(),
            surfaces: Vec::new(),
            sessions: metric(Baseline::Measured(1), 0, int_judge),
            subagent_runs: metric(Baseline::Measured(0), 0, int_judge),
            sub_tokens: metric(Baseline::Measured(0), 0, int_judge),
        };
        assert!(
            payload.previous_events_are_empty(),
            "no event of any kind in the previous window, whatever the sessions axis holds"
        );
        assert!(payload.has_no_baseline(), "the kind row is n/a");
        assert_ne!(
            payload.sessions.reason,
            Some(UnavailableReason::NoBaseline),
            "the totals axis compares: the previous window holds a session"
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
        // The omission is the payload's own judgment: a point kind's span
        // metrics are `NotApplicable` with no delta, never `NoBaseline`.
        let edit = payload
            .kinds
            .iter()
            .find(|kind| kind.kind == "file_edit")
            .unwrap();
        assert_eq!(
            edit.out_tokens.reason,
            Some(UnavailableReason::NotApplicable)
        );
        assert_eq!(
            edit.duration_sec.reason,
            Some(UnavailableReason::NotApplicable)
        );
        assert_eq!(
            edit.out_tokens.delta, None,
            "an inapplicable metric claims no delta"
        );
        assert_eq!(edit.duration_sec.delta, None);
        // The span kind's metrics are judged, not inapplicable.
        let span = payload
            .kinds
            .iter()
            .find(|kind| kind.kind == "skill_invocation")
            .unwrap();
        assert_eq!(span.out_tokens.reason, None);
        assert_eq!(span.duration_sec.reason, None);
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
        // baseline is the previous row's presence (`Baseline::Measured`), not
        // the value it folds to, so a rounded-to-zero measurement compares.
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

    #[test]
    fn the_unavailable_reason_serializes_as_the_machine_contract() {
        // The JSON value is a consumer contract: a rename or a lost
        // `rename_all` must fail here, not only in a downstream consumer.
        let no_baseline =
            serde_json::to_value(unavailable(0, 3, UnavailableReason::NoBaseline)).unwrap();
        assert_eq!(no_baseline["reason"], "no_baseline");
        assert_eq!(no_baseline["direction"], "unavailable");
        assert!(no_baseline["delta"].is_null());

        let not_applicable =
            serde_json::to_value(unavailable(0, 0, UnavailableReason::NotApplicable)).unwrap();
        assert_eq!(not_applicable["reason"], "not_applicable");
        assert_eq!(not_applicable["direction"], "unavailable");
    }
}
