//! Doctor view: the one-screen health check: what to fix first, what a session costs, and what is fine as-is. Landed here under the view-landing rule
//! (`docs/specs/architecture.md`): new views live as one file under
//! `src/cli/views/` instead of growing the `src/cli/mod.rs` shell.

use anyhow::Result;

use crate::adapter::harness::home_dir;
use crate::core::friction::ErrorCategory;
use crate::core::optimize as optimize_mod;
use crate::core::period::TimeRange;
use crate::core::scope::ScopeFilter;
use crate::session::{Session, Style};

use super::super::{
    Format, collect_findings, emit_json, empty_in_window, eprint_window, warn_missing_agent_split,
    window_line, with_window,
};

/// One-screen health check: the few most actionable findings from every view,
/// split by the config layer that owns each fix — the global picture first,
/// then each project's own findings ("optimize my global setup" and "optimize
/// this project" are different tasks). `--scope` narrows to one layer.
pub(crate) fn doctor(
    sess: &Session,
    filter: &ScopeFilter,
    window: TimeRange,
    format: Format,
) -> Result<()> {
    let store = &sess.store;
    eprint_window(window);
    let f = collect_findings(store, window)?;

    if format == Format::Json {
        // The routed findings, filtered the same way the text form is: the
        // global picture (minus the projects list) and each visible project.
        let global = filter.includes_global().then(|| {
            let mut value = serde_json::to_value(&f).expect("findings serialize");
            value.as_object_mut().expect("object").remove("projects");
            value
        });
        let projects: Vec<&optimize_mod::ProjectFindings> = f
            .projects
            .iter()
            .filter(|p| filter.includes_project(&p.project))
            .collect();
        return emit_json(&with_window(
            serde_json::json!({
                "global": global,
                "projects": projects,
            }),
            window,
        ));
    }

    // The text form is written for someone who has never seen saniwalens's
    // vocabulary: every item says what happened, why it costs, and what to do
    // — prioritized, so the report reads top to bottom as a to-do list.
    let style = Style::stdout();
    let (session_count, project_count) = store.session_stats(window)?;
    if session_count == 0 {
        println!(
            "{}",
            empty_in_window(
                "nothing analyzed yet — run `saniwalens analyze` first",
                window
            )
        );
        return Ok(());
    }
    println!(
        "{}",
        style.heading(&format!(
            "{} usage health check — {session_count} sessions across \
             {project_count} projects",
            sess.harness.display_name()
        ))
    );
    // The evaluation window, in-band for a text reader: the session count and
    // every section below are already window-filtered.
    if let Some(line) = window_line(window) {
        println!("{}", style.dim(&line));
    }

    let visible: Vec<_> = f
        .projects
        .iter()
        .filter(|p| filter.includes_project(&p.project))
        .collect();
    let display = |p: &optimize_mod::ProjectFindings| {
        if p.root.is_empty() {
            p.project.clone()
        } else {
            tilde_path(&p.root)
        }
    };

    // ---- WHAT TO FIX FIRST: recurring problems, biggest first. -------------
    struct Item {
        weight: i64,
        lines: Vec<String>,
    }
    let mut items: Vec<Item> = Vec::new();
    for p in &visible {
        if p.total_friction() == 0 {
            continue;
        }
        let mut lines = vec![format!(
            "In {}, {} tool calls failed in recurring ways:",
            style.bold(&display(p)),
            style.warn(&p.total_friction().to_string())
        )];
        for cat in p.friction.iter().take(3) {
            lines.push(format!(
                "  {:>4}× {} {}",
                cat.count,
                cat.label,
                style.dim(&format!(
                    "— {}",
                    ErrorCategory::from_label(&cat.label)
                        .suggestion(sess.harness.instruction_file())
                ))
            ));
        }
        if let Some(worst) = p.thrash.first() {
            lines.push(format!(
                "  …and the agent got stuck re-editing {} ({} edits in {}m).",
                worst.file,
                worst.edits,
                worst.span_secs.div_euclid(60).max(1)
            ));
        }
        lines.push(format!(
            "  Fix in this project's own {} — details: {}",
            sess.harness.project_config_ref(),
            style.command(&format!(
                "saniwalens failures --scope project:{}",
                p.project
            ))
        ));
        items.push(Item {
            weight: p.total_friction(),
            lines,
        });
    }
    if filter.includes_global() {
        for cat in &f.friction_global {
            items.push(Item {
                weight: cat.count,
                lines: vec![
                    format!(
                        "The same failure hit {} different projects ({}× total): {} {}",
                        cat.projects,
                        style.warn(&cat.count.to_string()),
                        style.bold(&cat.label),
                        style.dim(&format!(
                            "— {}",
                            ErrorCategory::from_label(&cat.label)
                                .suggestion(sess.harness.instruction_file())
                        ))
                    ),
                    format!(
                        "  No single project owns it, so the fix belongs in {} — \
                         details: {}",
                        sess.harness.global_config_ref(),
                        style.command("saniwalens failures --scope global")
                    ),
                ],
            });
        }
    }
    items.sort_by_key(|item| std::cmp::Reverse(item.weight));
    let dropped_items = items.len().saturating_sub(4);
    items.truncate(4);
    if filter.includes_global() {
        if let Some(cd_pct) = f.cd_pct.filter(|pct| *pct >= 25) {
            items.push(Item {
                weight: 0,
                lines: vec![format!(
                    "{} of all Bash commands are just `cd` — a working-directory \
                     convention in your global {} would cut the churn.",
                    style.warn(&format!("{cd_pct}%")),
                    sess.harness.instruction_file()
                )],
            });
        }
        if let (Some(steer), Some(correct)) = (f.steer_pct, f.correct_pct)
            && (correct >= 10 || steer >= 25)
        {
            items.push(Item {
                weight: 0,
                lines: vec![format!(
                    "{steer}% of your prompts steer and {correct}% correct — rework that \
                     clearer upfront specs could cut. Details: {}",
                    style.command("saniwalens prompts")
                )],
            });
        }
    }

    println!("\n{}", style.heading("WHAT TO FIX FIRST"));
    if items.is_empty() {
        println!("  {}", style.good("nothing recurring found — looking good"));
    }
    for (i, item) in items.iter().enumerate() {
        let (first, rest) = item.lines.split_first().expect("item has lines");
        println!("  {}. {first}", i + 1);
        for line in rest {
            println!("   {line}");
        }
    }
    // Never truncate silently (reporting honesty, cli.md).
    if dropped_items > 0 {
        println!(
            "  {}",
            style.dim(&format!(
                "… and {dropped_items} smaller recurring problem(s) — see `saniwalens failures --scope project`"
            ))
        );
    }

    // ---- COST: what a session pays, in plain words. -------------------------
    if filter.includes_global() {
        println!("\n{}", style.heading("COST (token estimates, not billing)"));
        if f.floor > 0 {
            let residual = (f.floor - f.config_tokens).max(0);
            println!(
                "  Every session starts with ~{} tokens of context before any work:",
                style.bold(&fmt_tokens(f.floor))
            );
            if residual > 0 {
                println!(
                    "  {} comes from your own global config files; the other {} is {}",
                    fmt_tokens(f.config_tokens),
                    fmt_tokens(residual),
                    sess.harness.display_name()
                );
                println!(
                    "  {}",
                    style.dim(
                        "itself (system prompt, built-in tools, MCP schemas) plus each \
                         project's own config (AGENTS.md, skill descriptions) — `saniwalens \
                         overhead` splits it per project."
                    )
                );
            } else {
                // The global config estimate already covers the observed floor —
                // attributing a "0" to the harness would read as a claim.
                println!(
                    "  ~all of it is accounted for by your own config files (skill \
                     descriptions, AGENTS.md) — nothing is left over to attribute; \
                     `saniwalens overhead` splits it per project."
                );
            }
        }
        println!(
            "  Output written so far: {} tokens by skills on the main thread, {} by {} subagent runs.",
            fmt_tokens(f.main_out),
            fmt_tokens(f.sub_tokens),
            f.sub_agents
        );
        warn_missing_agent_split(&f.agent_split, window);
        for line in doctor_agent_lines(&f, &style.command("saniwalens usage"), window) {
            if line.starts_with('…') {
                println!("  {}", style.dim(&line));
            } else {
                println!("  {line}");
            }
        }
    }

    // ---- CONFIG WORTH PRUNING: installed-but-dead weight, by owner. ---------
    let mut pruning: Vec<String> = Vec::new();
    if filter.includes_global() && (!f.unused.is_empty() || !f.always_on_heavy.is_empty()) {
        pruning.push(format!(
            "  {}: {} → {}",
            sess.harness.global_config_ref(),
            prune_phrase(&f.unused, &f.always_on_heavy),
            style.command("saniwalens waste --scope global")
        ));
    }
    let mut prunable: Vec<_> = visible
        .iter()
        .filter(|p| !p.unused.is_empty() || !p.always_on_heavy.is_empty())
        .collect();
    prunable.sort_by_key(|p| std::cmp::Reverse(p.unused.len() + p.always_on_heavy.len()));
    for p in prunable.iter().take(3) {
        pruning.push(format!(
            "  {}: {} → {}",
            display(p),
            prune_phrase(&p.unused, &p.always_on_heavy),
            style.command(&format!("saniwalens waste --scope project:{}", p.project))
        ));
    }
    if !pruning.is_empty() {
        println!("\n{}", style.heading("CONFIG WORTH PRUNING"));
        println!(
            "  {}",
            style.dim("Installed config that costs attention (or context) without earning it:")
        );
        for line in pruning {
            println!("{line}");
        }
        if prunable.len() > 3 {
            println!(
                "  {}",
                style.dim(&format!(
                    "… and {} more project(s) with prunable config — see `saniwalens waste --scope project`",
                    prunable.len() - 3
                ))
            );
        }
    }

    // ---- LOOKS HEALTHY: say explicitly what needs no action. ----------------
    if filter.includes_global() {
        let mut healthy: Vec<String> = Vec::new();
        if let (Some(steer), Some(correct)) = (f.steer_pct, f.correct_pct)
            && correct < 10
            && steer < 25
        {
            healthy.push(format!(
                "prompting mix ({steer}% steering / {correct}% corrections) — good autonomy balance"
            ));
        }
        if f.friction_global.is_empty() {
            healthy.push("no failure habit spans multiple projects".to_string());
        }
        if f.always_on_heavy.is_empty() && f.floor > 0 {
            healthy.push(format!(
                "your global config is lean ({} of the ~{} startup tokens)",
                fmt_tokens(f.config_tokens),
                fmt_tokens(f.floor)
            ));
        }
        if !healthy.is_empty() {
            println!("\n{}", style.heading("LOOKS HEALTHY"));
            for line in healthy {
                println!("  {}", style.good(&line));
            }
        }
    }
    Ok(())
}

/// View-only (doctor view): `"3 unused surfaces, 4 always-on files costing ~8.5k
/// tokens/session"` — the pruning line's plain-language phrase, zero parts
/// dropped; it moves with the doctor view, not with the shell.
fn prune_phrase(
    unused: &[optimize_mod::SurfaceRef],
    always_on_heavy: &[optimize_mod::SurfaceRef],
) -> String {
    let mut parts = Vec::new();
    if !unused.is_empty() {
        parts.push(format!(
            "{} installed but never used",
            plural(unused.len(), "surface")
        ));
    }
    if !always_on_heavy.is_empty() {
        let tokens: i64 = always_on_heavy
            .iter()
            .filter_map(|s| s.startup_tokens)
            .sum();
        parts.push(format!(
            "{} loaded every session (~{} tokens each time)",
            plural(always_on_heavy.len(), "heavy file"),
            fmt_tokens(tokens)
        ));
    }
    parts.join(", ")
}

/// View-only (doctor view): the singular/plural phrasing behind `prune_phrase` alone.
fn plural(n: usize, noun: &str) -> String {
    if n == 1 {
        format!("1 {noun}")
    } else {
        format!("{n} {noun}s")
    }
}

/// The doctor's lines naming the agents behind the subagent figure — "which
/// agent" is the actionable half of it. The top rows are introduced as "most
/// of" the output only when they actually exceed half of it; when the rows
/// are a partial split (`SplitCoverage`), the gap is stated first so the
/// names are not read as the whole bill. `full_split` is the styled command
/// that shows every row. An unavailable split yields nothing: that case is
/// reported on stderr with its refresh hint. Under a drawn window the gap is
/// the two time axes at work, so the sentence names the window instead of the
/// pruned transcripts (`window.is_all()` keeps the full-history wording).
fn doctor_agent_lines(
    f: &optimize_mod::Findings,
    full_split: &str,
    window: TimeRange,
) -> Vec<String> {
    let mut lines = Vec::new();
    let top: Vec<String> = f
        .agents
        .iter()
        .take(3)
        .map(|a| {
            format!(
                "{} {} ({} run{})",
                a.label(),
                fmt_tokens(a.out_tokens),
                a.runs,
                if a.runs == 1 { "" } else { "s" }
            )
        })
        .collect();
    if top.is_empty() {
        return lines;
    }
    let coverage = &f.agent_split;
    let shown: i64 = f.agents.iter().take(top.len()).map(|a| a.out_tokens).sum();
    let intro = if coverage.is_partial() {
        if window.is_all() {
            lines.push(format!(
                "Only {} of those tokens ({} of {} runs) have a per-agent split — the rest were \
                 analyzed before runs were extracted and their transcripts are gone.",
                fmt_tokens(coverage.known_tokens),
                coverage.known_runs,
                coverage.total_runs
            ));
        } else {
            lines.push(format!(
                "Only {} of those tokens ({} of {} runs) have a per-agent split — the rest \
                 fall outside the report window.",
                fmt_tokens(coverage.known_tokens),
                coverage.known_runs,
                coverage.total_runs
            ));
        }
        "Among the runs that have one"
    } else if coverage.is_majority(shown) {
        "Most of that subagent output"
    } else {
        "The largest agent types"
    };
    lines.push(format!(
        "{intro}: {}. Full split: {full_split}",
        top.join(", ")
    ));
    // Never truncate silently (reporting honesty, cli.md).
    if f.agents.len() > top.len() {
        lines.push(format!(
            "… and {} more agent type(s) not shown",
            f.agents.len() - top.len()
        ));
    }
    lines
}

/// View-only (doctor view): humanize a token count: `6467406` → `6.5M`, `32139` → `32.1k`.
fn fmt_tokens(n: i64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1e6)
    } else if n >= 1_000 {
        format!("{:.1}k", n as f64 / 1e3)
    } else {
        n.to_string()
    }
}

/// View-only (doctor view): shorten an absolute path under the home directory to `~/…` for display.
fn tilde_path(path: &str) -> String {
    shorten_to_tilde(path, home_dir())
}

/// View-only (doctor view, via `tilde_path`): the shortening itself, separated from
/// the environment lookup so it stays pure and testable. A separator must follow the
/// prefix, so `/home/metadata` is not mistaken for something under `/home/me`; either
/// separator counts, so a Windows path shortens against a Windows home.
fn shorten_to_tilde(path: &str, home: Option<&str>) -> String {
    let Some(home) = home.filter(|home| !home.is_empty()) else {
        return path.to_string();
    };
    // The two sides come from different places — `home` from the environment,
    // `path` from the cwd a transcript recorded — so on Windows they can spell
    // the same directory differently; the OS accepts either. Only the
    // comparison is folded. The suffix is sliced from the original at the same
    // offset, which stays valid because folding swaps one ASCII byte for
    // another, so the display keeps whatever spelling the path really had.
    let fold = |s: &str| s.replace('\\', "/");
    match fold(path).strip_prefix(&fold(home)) {
        Some("") => "~".to_string(),
        Some(rest) if rest.starts_with('/') => format!("~{}", &path[home.len()..]),
        _ => path.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::subagent::SplitCoverage;

    fn agent(name: &str, runs: i64, out_tokens: i64) -> optimize_mod::AgentCost {
        optimize_mod::AgentCost {
            agent: Some(name.to_string()),
            runs,
            out_tokens,
        }
    }

    fn findings_with_agents(
        sub_tokens: i64,
        sub_agents: i64,
        agents: Vec<optimize_mod::AgentCost>,
    ) -> optimize_mod::Findings {
        optimize_mod::Findings {
            sub_tokens,
            sub_agents,
            agent_split: SplitCoverage::new(
                sub_tokens,
                sub_agents,
                agents.iter().map(|a| (a.runs, a.out_tokens)),
            ),
            agents,
            ..Default::default()
        }
    }

    #[test]
    fn doctor_calls_the_top_rows_most_of_the_output_only_when_they_are() {
        let f = findings_with_agents(
            1_000_000,
            10,
            vec![agent("a", 4, 700_000), agent("b", 6, 300_000)],
        );
        let lines = doctor_agent_lines(&f, "saniwalens usage", TimeRange::ALL);
        assert_eq!(
            lines,
            vec![
                "Most of that subagent output: a 700.0k (4 runs), b 300.0k (6 runs). \
                 Full split: saniwalens usage"
            ]
        );
    }

    #[test]
    fn doctor_does_not_call_a_minority_of_the_output_most_of_it() {
        // A long tail: ten types at a tenth each, so the three shown cover 30%
        // of the output, and "most" would be false even with a complete split.
        let agents: Vec<_> = (0..10)
            .map(|i| agent(&format!("t{i}"), 1, 100_000))
            .collect();
        let f = findings_with_agents(1_000_000, 10, agents);
        let lines = doctor_agent_lines(&f, "saniwalens usage", TimeRange::ALL);
        assert_eq!(
            lines,
            vec![
                "The largest agent types: t0 100.0k (1 run), t1 100.0k (1 run), t2 100.0k \
                 (1 run). Full split: saniwalens usage",
                "… and 7 more agent type(s) not shown",
            ]
        );
    }

    #[test]
    fn doctor_labels_a_partial_split_before_naming_its_agents() {
        // The rows explain 12% of the total: saying "most of that output" here
        // would blame the listed types for tokens no row accounts for.
        let f = findings_with_agents(
            3_093_466,
            375,
            vec![
                agent("general-purpose", 11, 315_663),
                agent("codex:codex-rescue", 14, 62_067),
            ],
        );
        let lines = doctor_agent_lines(&f, "saniwalens usage", TimeRange::ALL);
        assert_eq!(
            lines,
            vec![
                "Only 377.7k of those tokens (25 of 375 runs) have a per-agent split — the \
                 rest were analyzed before runs were extracted and their transcripts are gone.",
                "Among the runs that have one: general-purpose 315.7k (11 runs), \
                 codex:codex-rescue 62.1k (14 runs). Full split: saniwalens usage",
            ]
        );
    }

    #[test]
    fn doctor_labels_a_windowed_partial_split_with_the_window() {
        // Under a drawn window the gap is the straddle — the total reads the
        // session's stamped start, the rows read each run's own epoch — so the
        // doctor names the window instead of the pruned transcripts.
        let f = findings_with_agents(
            3_093_466,
            375,
            vec![
                agent("general-purpose", 11, 315_663),
                agent("codex:codex-rescue", 14, 62_067),
            ],
        );
        let window = TimeRange {
            start: Some(1_790_780_400),
            end: Some(1_791_385_199),
        };
        let lines = doctor_agent_lines(&f, "saniwalens usage", window);
        assert!(
            lines[0].contains("fall outside the report window"),
            "{}",
            lines[0]
        );
        assert!(
            !lines[0].contains("analyzed before runs were extracted"),
            "{}",
            lines[0]
        );
        // Without a window the pruned-transcript wording stays, byte for byte.
        let lines = doctor_agent_lines(&f, "saniwalens usage", TimeRange::ALL);
        assert!(
            lines[0].contains("analyzed before runs were extracted"),
            "{}",
            lines[0]
        );
    }

    #[test]
    fn doctor_names_no_agents_when_the_split_is_unavailable() {
        // The gap is reported on stderr with its refresh hint; there is
        // nothing to list in-band.
        let f = findings_with_agents(3_093_466, 375, vec![]);
        assert!(doctor_agent_lines(&f, "saniwalens usage", TimeRange::ALL).is_empty());
    }

    #[test]
    fn shorten_to_tilde_replaces_the_home_prefix() {
        assert_eq!(
            shorten_to_tilde("/tmp/example/me/.claude", Some("/tmp/example/me")),
            "~/.claude"
        );
        // Home itself collapses to a bare tilde.
        assert_eq!(
            shorten_to_tilde("/tmp/example/me", Some("/tmp/example/me")),
            "~"
        );
    }

    #[test]
    fn shorten_to_tilde_accepts_a_backslash_separator() {
        assert_eq!(
            shorten_to_tilde(r"C:\example\me\.claude", Some(r"C:\example\me")),
            r"~\.claude"
        );
    }

    #[test]
    fn shorten_to_tilde_matches_a_home_spelled_with_the_other_separator() {
        // Windows takes either separator, and the home comes from the
        // environment while the path comes from a recorded cwd — so the two can
        // disagree on spelling while naming the same directory.
        assert_eq!(
            shorten_to_tilde("C:/example/me/.claude", Some(r"C:\example\me")),
            "~/.claude"
        );
        assert_eq!(
            shorten_to_tilde(r"C:\example\me\.claude", Some("C:/example/me")),
            r"~\.claude"
        );
        // The suffix keeps the spelling the path actually had, not the folded
        // one used for the comparison.
        assert_eq!(
            shorten_to_tilde(r"C:\example\me\a/b\c", Some("C:/example/me")),
            r"~\a/b\c"
        );
    }

    #[test]
    fn shorten_to_tilde_leaves_a_path_outside_home_alone() {
        assert_eq!(
            shorten_to_tilde("/tmp/other", Some("/tmp/example/me")),
            "/tmp/other"
        );
        // A sibling that merely shares the prefix is not under home.
        assert_eq!(
            shorten_to_tilde("/tmp/example/metadata", Some("/tmp/example/me")),
            "/tmp/example/metadata"
        );
    }

    #[test]
    fn shorten_to_tilde_passes_the_path_through_without_a_home() {
        assert_eq!(shorten_to_tilde("/tmp/example/me", None), "/tmp/example/me");
        assert_eq!(
            shorten_to_tilde("/tmp/example/me", Some("")),
            "/tmp/example/me"
        );
    }
}
