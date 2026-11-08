//! Usage view: how often each skill ran and what it cost, per skill or over time with --by. Landed here under the view-landing rule
//! (`docs/specs/architecture.md`): new views live as one file under
//! `src/cli/views/` instead of growing the `src/cli/mod.rs` shell.

use anyhow::{Context, Result};

use crate::core::bucket::{Bucket, JST_OFFSET_SECS, bucket_label};
use crate::core::period::TimeRange;
use crate::core::subagent::SplitCoverage;
use crate::session::Session;
use crate::store::Store;

use super::super::{
    Align, Format, TableSpec, ViewCtx, empty_in_window, render_view, warn_missing_agent_split,
};

pub(crate) fn usage(
    sess: &Session,
    by: Option<&str>,
    window: TimeRange,
    format: Format,
) -> Result<()> {
    let store = &sess.store;

    if let Some(by) = by {
        let bucket = Bucket::parse(by)
            .with_context(|| format!("unknown --by value '{by}' (year|month|week|day|hour)"))?;
        return usage_by_time(store, bucket, window, format);
    }

    let skills = store.skill_usage(window)?;
    // Where the tokens actually go: main-thread skill output vs subagents. The
    // subagent figure is usually the larger by far — worth seeing before reading
    // the per-skill table.
    let main_out: i64 = skills.iter().map(|row| row.out_tokens).sum();
    let (sub_tokens, sub_agents) = store.subagent_totals(window)?;
    let agents = store.subagent_by_agent(window)?;
    // How much of the total the rows actually account for: a store that kept
    // totals for sessions whose transcripts were pruned before their runs were
    // extracted has rows for only part of it, and the rows must not pass for
    // the whole. It travels with the rows in every format.
    let agent_split = SplitCoverage::new(
        sub_tokens,
        sub_agents,
        agents.iter().map(|a| (a.runs, a.out_tokens)),
    );
    // Ahead of the format branches: a machine consumer reading an empty `agents`
    // beside a nonzero total needs the reason as much as a human does, and it
    // goes to stderr precisely so it can be given to both.
    warn_missing_agent_split(&agent_split, window);
    let json = serde_json::json!({
        "tokens": {
            "main_out": main_out,
            "sub_tokens": sub_tokens,
            "sub_agents": sub_agents,
        },
        "agent_split": agent_split,
        "agents": agents,
        "skills": skills,
    });
    // A subagent total with neither table behind it is not "nothing happened" —
    // it is a store whose per-agent split has not been extracted yet, and saying
    // "no usage" there would be false.
    let empty_msg = (skills.is_empty() && agents.is_empty() && sub_tokens == 0).then(|| {
        empty_in_window(
            "no skill usage found — run `saniwalens analyze` first",
            window,
        )
    });
    let agents_table = TableSpec {
        headers: &["agent", "runs", "out tokens"],
        aligns: &[Align::Left, Align::Right, Align::Right],
        rows: agents
            .iter()
            .map(|row| {
                vec![
                    row.agent.clone().unwrap_or_else(|| "(unknown)".to_string()),
                    row.runs.to_string(),
                    row.out_tokens.to_string(),
                ]
            })
            .collect(),
    };
    let skills_table = TableSpec {
        headers: &["skill", "runs", "out tokens", "ctx growth", "seconds"],
        aligns: &[
            Align::Left,
            Align::Right,
            Align::Right,
            Align::Right,
            Align::Right,
        ],
        rows: skills
            .iter()
            .map(|row| {
                vec![
                    row.skill.clone(),
                    row.invocations.to_string(),
                    row.out_tokens.to_string(),
                    row.ctx_growth.to_string(),
                    format!("{:.0}", row.duration_sec),
                ]
            })
            .collect(),
    };
    render_view(
        ViewCtx::single(format, window),
        json,
        empty_msg.as_deref(),
        "What ran and what it cost. First the subagent bill per agent type\n\
         (costliest first), then how often each skill ran — output tokens written,\n\
         context growth while it ran, and wall-clock seconds (most-invoked first).\n\
         A section with nothing to show is omitted.",
        |format| {
            println!(
                "tokens: main-thread skill output {main_out}, subagents {sub_tokens} ({sub_agents} agents)"
            );
            // Stated between the total and the table, so the table below is read as
            // the part it is. Under a drawn window the gap is the straddle, so the
            // note names the window rather than the pruned transcripts.
            if let Some(note) = agent_split.partial_note_in(window) {
                println!("{note}");
            }
            println!();

            // The subagent figure is usually the larger one, so break it down by agent
            // type before the per-skill table: a spawn count alone cannot tell an
            // expensive agent from a cheap one that runs often.
            if !agents_table.rows.is_empty() {
                agents_table.render(format);
                println!();
            }

            if skills_table.rows.is_empty() {
                return;
            }
            skills_table.render(format);
        },
    )
}

/// Skill usage rolled up per time bucket (JST). A long span is assigned whole to
/// its start bucket (`docs/specs/cli.md`).
pub(crate) fn usage_by_time(
    store: &Store,
    bucket: Bucket,
    window: TimeRange,
    format: Format,
) -> Result<()> {
    let events = store.skill_event_costs(window)?;

    let mut totals: std::collections::BTreeMap<String, (i64, i64, i64, f64)> =
        std::collections::BTreeMap::new();
    for event in &events {
        let label = bucket_label(event.started_epoch, bucket, JST_OFFSET_SECS);
        let row = totals.entry(label).or_default();
        row.0 += 1;
        row.1 += event.out_tokens;
        row.2 += event.ctx_growth;
        row.3 += event.duration_sec;
    }

    let buckets: Vec<serde_json::Value> = totals
        .iter()
        .map(|(label, (count, out_tokens, ctx_growth, duration_sec))| {
            serde_json::json!({
                "bucket": label,
                "count": count,
                "out_tokens": out_tokens,
                "ctx_growth": ctx_growth,
                "duration_sec": duration_sec,
            })
        })
        .collect();
    let empty_msg = events.is_empty().then(|| {
        empty_in_window(
            "no skill usage found — run `saniwalens analyze` first",
            window,
        )
    });
    let table = TableSpec {
        headers: &["bucket", "runs", "out tokens", "ctx growth", "seconds"],
        aligns: &[
            Align::Left,
            Align::Right,
            Align::Right,
            Align::Right,
            Align::Right,
        ],
        rows: totals
            .into_iter()
            .map(|(label, (count, out_tokens, ctx_growth, duration_sec))| {
                vec![
                    label,
                    count.to_string(),
                    out_tokens.to_string(),
                    ctx_growth.to_string(),
                    format!("{duration_sec:.0}"),
                ]
            })
            .collect(),
    };
    render_view(
        ViewCtx::single(format, window),
        serde_json::json!({ "buckets": buckets }),
        empty_msg.as_deref(),
        "Skill activity per time bucket (times in JST). A long-running skill counts\n\
         whole in the bucket it started in, so fine buckets can show spikes.",
        |format| table.render(format),
    )
}
