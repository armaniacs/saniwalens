//! The trend view's row types and its pure two-window joins.
//!
//! The store's windowed queries fetch the rows; everything here only shapes
//! them: pairing the two windows' per-kind rollups, folding the two windows'
//! per-skill usage into delta rows, and ordering those rows for display. There
//! is no SQL, clock, or I/O, so the trend contract is unit-testable against
//! small synthetic values without a database (`docs/specs/architecture.md`,
//! the pure-join placement rule).

use std::collections::BTreeMap;

/// One row of the per-skill usage rollup. Subagent cost is deliberately not
/// rolled up per skill: under the flat-span model a skill's window can absorb
/// subagents spawned by later same-window work, so a per-skill figure
/// over-counts for skills that do not themselves spawn agents. The exact figure
/// is the session-level total (`subagent_totals`). See `docs/specs/events.md`.
#[derive(Debug, PartialEq, serde::Serialize)]
pub struct SkillUsage {
    pub skill: String,
    pub invocations: i64,
    pub out_tokens: i64,
    pub ctx_growth: i64,
    pub duration_sec: f64,
}

/// One row of the trend view's per-kind event rollup: how many events of the
/// kind ran in a window, and the output tokens / wall-clock they cost.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct KindRollup {
    pub kind: String,
    pub events: i64,
    pub out_tokens: i64,
    pub duration_sec: f64,
}

/// One aggregate computed for both windows of a comparison.
#[derive(Debug, Clone, PartialEq)]
pub struct TwoWindow<T> {
    pub current: T,
    pub previous: T,
}

/// The trend view's aggregates for one window: the per-kind event rollup, the
/// per-skill usage, the per-surface usage, the session count, and the subagent
/// runs/tokens total.
#[derive(Debug, PartialEq)]
pub struct TrendWindowAggregates {
    pub by_kind: Vec<KindRollup>,
    pub skills: Vec<SkillUsage>,
    /// Per-surface usage counts `(kind, id, events)` — the raw material for
    /// the surface-movement section, paired across windows by `surface_deltas`.
    pub surfaces: Vec<(String, String, i64)>,
    pub sessions: i64,
    pub sub_tokens: i64,
    pub subagent_runs: i64,
}

/// One skill's two-window row: the invocations in each window and the delta
/// between them ― the row the trend view orders by |delta|.
#[derive(Debug, Clone, PartialEq)]
pub struct SkillDelta {
    pub skill: String,
    pub current: i64,
    pub previous: i64,
    /// `current - previous`.
    pub delta: i64,
}

/// Pair the two windows' per-skill usage into delta rows, largest absolute
/// delta first (ties by skill id) ― the order the trend view lists skills in.
/// A skill used in only one window keeps a 0 on the other side.
pub fn skill_deltas(current: &[SkillUsage], previous: &[SkillUsage]) -> Vec<SkillDelta> {
    let mut invocations: BTreeMap<&str, [i64; 2]> = BTreeMap::new();
    for usage in current {
        invocations.entry(usage.skill.as_str()).or_default()[0] += usage.invocations;
    }
    for usage in previous {
        invocations.entry(usage.skill.as_str()).or_default()[1] += usage.invocations;
    }
    let mut rows: Vec<SkillDelta> = invocations
        .into_iter()
        .map(|(skill, sides)| SkillDelta {
            skill: skill.to_string(),
            current: sides[0],
            previous: sides[1],
            delta: sides[0] - sides[1],
        })
        .collect();
    rows.sort_by(|a, b| {
        b.delta
            .abs()
            .cmp(&a.delta.abs())
            .then_with(|| a.skill.cmp(&b.skill))
    });
    rows
}

/// One surface's two-window row: the usage events in each window and the
/// delta between them ― the row the trend view orders by |delta|.
#[derive(Debug, Clone, PartialEq)]
pub struct SurfaceDelta {
    pub kind: String,
    pub id: String,
    pub current: i64,
    pub previous: i64,
    /// `current - previous`.
    pub delta: i64,
}

/// Pair the two windows' per-surface usage counts into delta rows, largest
/// absolute delta first (ties by kind, then id) ― the order the trend view
/// lists surfaces in. A surface used in only one window keeps a 0 on the
/// other side. The catalog is deliberately not consulted: a surface renamed
/// or removed between windows still compares by its (kind, id) readings,
/// which is what the events actually named.
pub fn surface_deltas(
    current: &[(String, String, i64)],
    previous: &[(String, String, i64)],
) -> Vec<SurfaceDelta> {
    use std::collections::BTreeMap;
    let mut counts: BTreeMap<(&str, &str), [i64; 2]> = BTreeMap::new();
    for (kind, id, events) in current {
        counts.entry((kind.as_str(), id.as_str())).or_default()[0] += events;
    }
    for (kind, id, events) in previous {
        counts.entry((kind.as_str(), id.as_str())).or_default()[1] += events;
    }
    let mut rows: Vec<SurfaceDelta> = counts
        .into_iter()
        .map(|((kind, id), sides)| SurfaceDelta {
            kind: kind.to_string(),
            id: id.to_string(),
            current: sides[0],
            previous: sides[1],
            delta: sides[0] - sides[1],
        })
        .collect();
    rows.sort_by(|a, b| {
        b.delta
            .abs()
            .cmp(&a.delta.abs())
            .then_with(|| a.kind.cmp(&b.kind))
            .then_with(|| a.id.cmp(&b.id))
    });
    rows
}

/// The two windows' per-kind rollups paired by kind ― a kind present in only
/// one window keeps None on the other side. The pair's presence is what the
/// payload's judgment reads: a kind present in the previous window is a
/// baseline (its events, output tokens and duration are measurements, zero
/// included), and only an absent kind is 比較不能. Sorted by kind, the
/// aggregation's own order.
pub fn kind_pairs(
    agg: &TwoWindow<TrendWindowAggregates>,
) -> Vec<(String, Option<KindRollup>, Option<KindRollup>)> {
    let mut pairs: BTreeMap<String, (Option<KindRollup>, Option<KindRollup>)> = BTreeMap::new();
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

#[cfg(test)]
mod tests {
    use super::*;

    fn skill_row(skill: &str, invocations: i64) -> SkillUsage {
        SkillUsage {
            skill: skill.to_string(),
            invocations,
            out_tokens: 0,
            ctx_growth: 0,
            duration_sec: 0.0,
        }
    }

    fn kind_row(kind: &str, events: i64) -> KindRollup {
        KindRollup {
            kind: kind.to_string(),
            events,
            out_tokens: 0,
            duration_sec: 0.0,
        }
    }

    fn window(kinds: &[KindRollup]) -> TrendWindowAggregates {
        TrendWindowAggregates {
            by_kind: kinds.to_vec(),
            skills: Vec::new(),
            surfaces: Vec::new(),
            sessions: 0,
            sub_tokens: 0,
            subagent_runs: 0,
        }
    }

    #[test]
    fn skill_deltas_order_by_the_largest_absolute_delta() {
        let current = [
            skill_row("commit", 1),
            skill_row("review", 5),
            skill_row("deploy", 2),
            skill_row("fresh", 1),
        ];
        let previous = [
            skill_row("commit", 4),
            skill_row("review", 3),
            skill_row("retired", 7),
        ];
        let deltas = skill_deltas(&current, &previous);
        // |delta| first, ties by skill: retired was used only in the previous
        // window, deploy only in the current one, and deploy/review tie.
        assert_eq!(
            deltas
                .iter()
                .map(|d| (d.skill.as_str(), d.current, d.previous, d.delta))
                .collect::<Vec<_>>(),
            [
                ("retired", 0, 7, -7),
                ("commit", 1, 4, -3),
                ("deploy", 2, 0, 2),
                ("review", 5, 3, 2),
                ("fresh", 1, 0, 1),
            ]
        );
    }

    #[test]
    fn surface_deltas_order_by_absolute_delta_then_kind_then_id() {
        let current = vec![
            ("skill".to_string(), "commit".to_string(), 1),
            ("skill".to_string(), "review".to_string(), 5),
            ("agent".to_string(), "fresh".to_string(), 1),
        ];
        let previous = vec![
            ("skill".to_string(), "commit".to_string(), 4),
            ("skill".to_string(), "review".to_string(), 3),
            ("agent".to_string(), "retired".to_string(), 7),
        ];
        let deltas = surface_deltas(&current, &previous);
        // |delta| first; the agent/skill tie breaks by kind, then id.
        assert_eq!(
            deltas
                .iter()
                .map(|d| (
                    d.kind.as_str(),
                    d.id.as_str(),
                    d.current,
                    d.previous,
                    d.delta
                ))
                .collect::<Vec<_>>(),
            [
                ("agent", "retired", 0, 7, -7),
                ("skill", "commit", 1, 4, -3),
                ("skill", "review", 5, 3, 2),
                ("agent", "fresh", 1, 0, 1),
            ]
        );
    }

    #[test]
    fn kind_pairs_keeps_a_one_sided_kind_with_none_on_the_other_side() {
        // Pairing is by kind, not by position: a kind only the current window
        // has keeps None as its previous side (and the reverse), which is what
        // the payload reads as 比較不能 rather than as a drop to zero.
        let agg = TwoWindow {
            current: window(&[kind_row("both", 2), kind_row("fresh", 1)]),
            previous: window(&[kind_row("both", 3), kind_row("retired", 4)]),
        };
        let pairs = kind_pairs(&agg);
        assert_eq!(
            pairs
                .iter()
                .map(|(kind, current, previous)| (
                    kind.as_str(),
                    current.as_ref().map(|r| r.events),
                    previous.as_ref().map(|r| r.events)
                ))
                .collect::<Vec<_>>(),
            [
                ("both", Some(2), Some(3)),
                ("fresh", Some(1), None),
                ("retired", None, Some(4)),
            ],
        );
    }
}
