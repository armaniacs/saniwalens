//! The configuration-surface catalog: the domain model for "what is installed
//! and what it costs" — the other half of the catalog×usage model
//! (`docs/specs/surfaces.md`). The adapter fills these from live config; the
//! join against events lives in the store.

/// Where a surface is installed: the shared `~/.claude`, or one specific
/// project's config. The project scope names its project (the normalized slug)
/// because many projects coexist in one catalog and shadowing is per-project
/// (`docs/specs/surfaces.md`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Scope {
    Global,
    Project(String),
}

impl Scope {
    pub fn label(&self) -> &'static str {
        match self {
            Scope::Global => "global",
            Scope::Project(_) => "project",
        }
    }

    /// The owning project's normalized slug — empty for the global scope.
    pub fn project(&self) -> &str {
        match self {
            Scope::Global => "",
            Scope::Project(project) => project,
        }
    }
}

/// How a surface's text reaches the model's context — the distinction that
/// separates always-on tax from per-use cost (`docs/specs/config-format.md`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoadMode {
    /// Paid every session, unconditionally (CLAUDE.md, memory).
    StartupFull,
    /// Only the description is loaded at startup; the body is on-demand (skills, agents).
    StartupDescription,
    /// Loaded only when a `paths:` glob matches (rules with `paths:`).
    PathConditional,
    /// Paid per use when invoked (skill/agent bodies).
    OnDemand,
    /// A tool definition injected into the system/tool context (MCP, built-ins).
    ToolSchema,
}

impl LoadMode {
    pub fn label(self) -> &'static str {
        match self {
            LoadMode::StartupFull => "startup_full",
            LoadMode::StartupDescription => "startup_description",
            LoadMode::PathConditional => "path_conditional",
            LoadMode::OnDemand => "on_demand",
            LoadMode::ToolSchema => "tool_schema",
        }
    }

    /// Whether this surface is paid on every session start regardless of use —
    /// the load modes the "always-on heavy" wedge cares about.
    pub fn is_always_on(self) -> bool {
        matches!(self, LoadMode::StartupFull)
    }

    /// Parse a stored load-mode label back into the enum.
    pub fn from_label(label: &str) -> Option<LoadMode> {
        match label {
            "startup_full" => Some(LoadMode::StartupFull),
            "startup_description" => Some(LoadMode::StartupDescription),
            "path_conditional" => Some(LoadMode::PathConditional),
            "on_demand" => Some(LoadMode::OnDemand),
            "tool_schema" => Some(LoadMode::ToolSchema),
            _ => None,
        }
    }
}

/// One configurable thing, with its static cost and how it loads. `(kind, id)`
/// is the join key into the events spine (`docs/specs/surfaces.md`).
#[derive(Debug, Clone, PartialEq)]
pub struct Surface {
    /// `skill` | `rule` | `mcp_server` | `mcp_tool` | `hook` | `claude_md` | `permission` | `agent`.
    pub kind: String,
    pub id: String,
    pub scope: Scope,
    pub config_path: String,
    /// Token weight of the whole injected definition; `None` when it cannot be
    /// weighed (e.g. an MCP tool schema with no available source). For a surface
    /// with an on-demand body this is the *invocation* cost, not the startup one
    /// — `startup_tokens` is what a session pays for merely having it installed.
    pub static_tokens: Option<u64>,
    /// Token weight of the part loaded into **every** session's startup context:
    /// the whole text for always-on surfaces, only the description for skills and
    /// agents, nothing for a body that waits to be invoked. `None` when unknown
    /// (an MCP server's tool schema). Keeping this apart from `static_tokens` is
    /// what stops a body cost from being read as a startup saving
    /// (`docs/specs/surfaces.md`).
    pub startup_tokens: Option<u64>,
    pub load_mode: LoadMode,
    /// Glob patterns from a rule's `paths:` frontmatter (PBI 04) — what the
    /// rule fires on. Empty for every other kind; the never-fires wedge reads
    /// these against the window's file targets.
    pub paths: Vec<String>,
    /// A skill/agent/command's own description, for the redundant-purpose
    /// wedge (PBI 15). `None` when the kind declares none.
    pub description: Option<String>,
}

/// Whether a surface kind can ever acquire usage events. Absence of events is
/// only evidence of disuse for usage-measurable kinds; for catalog-only kinds
/// Static-token threshold above which an always-on or rarely-used surface
/// counts as "heavy" — a tuning knob, not a hard rule. Shared by the briefing's
/// wedge routing (`core::findings`) and the `waste` view (`cli/mod.rs`).
pub const HEAVY_TOKENS: u64 = 800;

/// (rules, CLAUDE.md) it means nothing (`docs/specs/surfaces.md`).
/// Hooks joined their measurable peers once firing became observable
/// (PBI 21): a hook with firings is used; one without is a delete candidate.
pub fn is_usage_measurable(kind: &str) -> bool {
    matches!(
        kind,
        "skill" | "agent" | "mcp_server" | "mcp_tool" | "permission" | "hook"
    )
}

/// An optimization opportunity for a surface (`docs/specs/surfaces.md`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wedge {
    /// Installed, usage-measurable, never invoked — a delete candidate.
    Unused,
    /// Always-on context with a large static cost — slim or make conditional.
    AlwaysOnHeavy,
    /// Costly but rarely used — trim or make on-demand.
    CostlyRare,
    /// Path-conditional rule whose globs matched nothing read in-window —
    /// narrow its paths or delete it.
    NeverFires,
    /// A skill, agent, or command whose purpose overlaps another's —
    /// merge them or delete one.
    Redundant,
}

impl Wedge {
    pub fn label(self) -> &'static str {
        match self {
            Wedge::Unused => "UNUSED",
            Wedge::AlwaysOnHeavy => "ALWAYS-ON HEAVY",
            Wedge::CostlyRare => "COSTLY+RARE",
            Wedge::NeverFires => "NEVER-FIRES",
            Wedge::Redundant => "REDUNDANT",
        }
    }

    pub fn suggestion(self) -> &'static str {
        match self {
            Wedge::Unused => "delete / disable",
            Wedge::AlwaysOnHeavy => "slim, or make path-conditional / on-demand",
            Wedge::CostlyRare => "trim, or make on-demand",
            Wedge::NeverFires => "narrow its paths or delete it",
            Wedge::Redundant => "merge it with its double or delete one",
        }
    }

    /// Ranking priority (lower sorts first).
    pub fn priority(self) -> u8 {
        match self {
            Wedge::AlwaysOnHeavy => 0,
            Wedge::Unused => 1,
            Wedge::CostlyRare => 2,
            Wedge::NeverFires => 3,
            Wedge::Redundant => 4,
        }
    }
}

/// What removing a surface actually saves from the context loaded at session
/// start. The distinction matters because skills and agents load only their
/// description at startup (their body is on-demand), so deleting an unused one
/// is decluttering — not a token win — whereas always-on config and tool schemas
/// are paid every session. See `docs/specs/surfaces.md`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartupSavings {
    /// Real, measured: removing it saves this many tokens every session.
    Tokens(u64),
    /// Real but unmeasured: an MCP tool schema loaded every session, weight unknown.
    UnknownSchema,
    /// ~No startup cost (on-demand / path-conditional body) — removal is
    /// decluttering only.
    Declutter,
}

/// What a surface costs when its body is actually loaded — the invocation cost
/// of a skill or agent, the firing cost of a path-conditional rule. `None` for
/// surfaces with no separate body: an always-on file is already paid in full at
/// startup, and a tool schema is not a body at all. Reports pair this with
/// `startup_tokens` so a body figure is never read as a startup saving.
pub fn on_demand_tokens(load_mode: LoadMode, static_tokens: Option<u64>) -> Option<u64> {
    match load_mode {
        LoadMode::StartupDescription | LoadMode::PathConditional | LoadMode::OnDemand => {
            static_tokens
        }
        LoadMode::StartupFull | LoadMode::ToolSchema => None,
    }
}

/// The startup token saving from removing a surface, by load mode.
pub fn startup_savings(load_mode: LoadMode, startup_tokens: Option<u64>) -> StartupSavings {
    match load_mode {
        LoadMode::StartupFull => StartupSavings::Tokens(startup_tokens.unwrap_or(0)),
        LoadMode::ToolSchema => StartupSavings::UnknownSchema,
        LoadMode::StartupDescription | LoadMode::PathConditional | LoadMode::OnDemand => {
            StartupSavings::Declutter
        }
    }
}

/// Classify a surface into its optimization wedge, if any. `uses` is the
/// invocation count (meaningful only for `measurable` kinds); `heavy_tokens`
/// is the injected threshold for "large" static cost. The "unused" verdict is
/// gated on measurability so a rule or CLAUDE.md is never called unused for
/// emitting no events (`docs/specs/surfaces.md`).
///
/// Precedence is deliberate and pinned by test: an unused heavy always-on
/// surface reports [`Wedge::AlwaysOnHeavy`], never [`Wedge::Unused`] — the
/// startup cost dominates the verdict (it also sorts first via
/// [`Wedge::priority`]). No current adapter produces that combination
/// (measurable kinds are never `StartupFull`), so the arm is latent until a
/// future kind does; the test below locks the order against an innocent
/// reorder.
pub fn classify_wedge(
    measurable: bool,
    load_mode: LoadMode,
    static_tokens: Option<u64>,
    uses: i64,
    heavy_tokens: u64,
) -> Option<Wedge> {
    let heavy = static_tokens.is_some_and(|tokens| tokens >= heavy_tokens);
    if load_mode.is_always_on() && heavy {
        return Some(Wedge::AlwaysOnHeavy);
    }
    if measurable && uses == 0 {
        return Some(Wedge::Unused);
    }
    if measurable && (1..=2).contains(&uses) && heavy {
        return Some(Wedge::CostlyRare);
    }
    None
}

/// One path-conditional rule's matchable surface for [`never_fires`]: its
/// identity plus the declared globs and static cost that become the wedge's
/// evidence.
#[derive(Debug, Clone)]
pub struct RuleGlobs {
    pub id: String,
    pub scope: String,
    pub project: String,
    pub globs: Vec<String>,
    pub static_tokens: Option<u64>,
}

/// A path-conditional rule that fired nowhere in the window, ready for the
/// waste view: its identity, its declared globs (the evidence), and its
/// static cost.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NeverFireHit {
    pub id: String,
    pub scope: String,
    pub project: String,
    pub globs: Vec<String>,
    pub static_tokens: Option<u64>,
}

/// Route path-conditional rules with declared globs against the window's read
/// file targets. A rule whose globs matched no read target never fired
/// in-window — provided the window observed reads at all. No targets means
/// no evidence (a quiet window, or a harness that does not report reads),
/// never a flag: without this guard every rule would light up on an idle
/// window. Catalog order in, same order out.
pub fn never_fires(rules: &[RuleGlobs], targets: &[String]) -> Vec<NeverFireHit> {
    if targets.is_empty() {
        return Vec::new();
    }
    rules
        .iter()
        .filter(|rule| !rule.globs.is_empty())
        .filter(|rule| {
            !targets
                .iter()
                .any(|t| crate::core::glob_match::matches_any(&rule.globs, t))
        })
        .map(|rule| NeverFireHit {
            id: rule.id.clone(),
            scope: rule.scope.clone(),
            project: rule.project.clone(),
            globs: rule.globs.clone(),
            static_tokens: rule.static_tokens,
        })
        .collect()
}

/// The overlap coefficient at or above which two descriptions count as the
/// same purpose: |A∩B| / min(|A|, |B|). Overlap (not Jaccard) because a
/// narrower description contained in a broader one is still a duplicate to
/// merge, not a partial match to keep. Tuned, not derived — the boundary is
/// pinned below.
pub const MIN_REDUNDANT_OVERLAP: f64 = 0.6;

/// Filler words that carry no purpose: without them, two unrelated blurbs
/// sharing "and the" would read as similar.
const DESCRIPTION_STOPWORDS: &[&str] = &[
    "a", "an", "the", "and", "or", "for", "to", "of", "in", "on", "with", "by", "from", "at", "as",
    "is", "are", "was", "were", "be", "been", "it", "its", "this", "that", "these", "those", "you",
    "your", "we", "our", "they", "their", "will", "can", "should", "would", "could", "do", "does",
    "did", "done", "has", "have", "had", "not", "no", "nor", "but", "if", "then", "than", "so",
    "such", "into", "over", "under", "about", "up", "out", "all", "any", "each", "more", "most",
    "other", "some", "only", "own", "same", "too", "very", "just", "also", "via", "per", "s", "t",
    "d",
];

/// A description reduced to its purpose vocabulary: lowercase alphanumeric
/// runs of length 2+, minus stopwords. CJK text without spaces stays one
/// token — identical strings still match, paraphrases do not — which is the
/// documented recall limit, not a bug to paper over.
fn purpose_tokens(description: &str) -> std::collections::BTreeSet<String> {
    let mut current = String::new();
    let mut tokens = std::collections::BTreeSet::new();
    let flush = |current: &mut String, tokens: &mut std::collections::BTreeSet<String>| {
        if current.len() >= 2 && !DESCRIPTION_STOPWORDS.contains(&current.as_str()) {
            tokens.insert(std::mem::take(current));
        } else {
            current.clear();
        }
    };
    for c in description.chars() {
        if c.is_alphanumeric() {
            for lc in c.to_lowercase() {
                current.push(lc);
            }
        } else {
            flush(&mut current, &mut tokens);
        }
    }
    flush(&mut current, &mut tokens);
    tokens
}

/// One surface's duplicate-check input: identity plus the description the
/// similarity runs on.
pub struct DuplicateCandidate<'a> {
    pub id: &'a str,
    pub kind: &'a str,
    pub scope: &'a str,
    pub project: &'a str,
    pub description: &'a str,
}

/// Pairs of indexes into `rows` whose descriptions overlap enough to be the
/// same purpose, in deterministic (first, second) order with no pair listed
/// twice. Same kind only: a skill and an agent with the same words are
/// different mechanisms, not duplicates. Empty descriptions never match —
/// nothing to compare is not a resemblance.
pub fn redundant_pairs(rows: &[DuplicateCandidate]) -> Vec<(usize, usize)> {
    let tokenized: Vec<std::collections::BTreeSet<String>> = rows
        .iter()
        .map(|row| purpose_tokens(row.description))
        .collect();
    let mut out = Vec::new();
    for i in 0..rows.len() {
        if tokenized[i].is_empty() {
            continue;
        }
        for j in (i + 1)..rows.len() {
            if rows[i].kind != rows[j].kind || tokenized[j].is_empty() {
                continue;
            }
            let inter = tokenized[i].intersection(&tokenized[j]).count() as f64;
            let denom = tokenized[i].len().min(tokenized[j].len()) as f64;
            if inter / denom >= MIN_REDUNDANT_OVERLAP {
                out.push((i, j));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate<'a>(
        id: &'a str,
        kind: &'a str,
        scope: &'a str,
        project: &'a str,
        description: &'a str,
    ) -> DuplicateCandidate<'a> {
        DuplicateCandidate {
            id,
            kind,
            scope,
            project,
            description,
        }
    }

    #[test]
    fn only_startup_full_is_always_on() {
        assert!(LoadMode::StartupFull.is_always_on());
        assert!(!LoadMode::StartupDescription.is_always_on());
        assert!(!LoadMode::OnDemand.is_always_on());
    }

    #[test]
    fn rules_and_claude_md_are_catalog_only() {
        assert!(is_usage_measurable("skill"));
        assert!(is_usage_measurable("mcp_tool"));
        assert!(is_usage_measurable("hook"));
        assert!(!is_usage_measurable("rule"));
        assert!(!is_usage_measurable("claude_md"));
    }

    #[test]
    fn an_unused_measurable_surface_is_a_delete_wedge() {
        let wedge = classify_wedge(true, LoadMode::StartupDescription, Some(500), 0, 1000);
        assert_eq!(wedge, Some(Wedge::Unused));
    }

    #[test]
    fn a_catalog_only_surface_with_no_uses_is_not_unused() {
        // A rule emits no events; zero uses must not read as unused.
        let wedge = classify_wedge(false, LoadMode::PathConditional, Some(500), 0, 1000);
        assert_eq!(wedge, None);
    }

    #[test]
    fn a_big_always_on_surface_is_always_on_heavy() {
        let wedge = classify_wedge(false, LoadMode::StartupFull, Some(1500), 0, 1000);
        assert_eq!(wedge, Some(Wedge::AlwaysOnHeavy));
    }

    #[test]
    fn a_small_always_on_surface_is_not_flagged() {
        let wedge = classify_wedge(false, LoadMode::StartupFull, Some(200), 0, 1000);
        assert_eq!(wedge, None);
    }

    #[test]
    fn a_costly_rarely_used_surface_is_costly_rare() {
        let wedge = classify_wedge(true, LoadMode::StartupDescription, Some(2000), 1, 1000);
        assert_eq!(wedge, Some(Wedge::CostlyRare));
    }

    #[test]
    fn an_unused_heavy_always_on_surface_stays_always_on_heavy() {
        // Precedence pin (PBI 16): the always-on arm runs before the unused
        // arm on purpose — the startup cost dominates the verdict. No
        // current adapter produces this combination (measurable kinds are
        // never StartupFull), so this locks the order for the future kind
        // that does; swapping the arms must fail here, not in a report.
        let wedge = classify_wedge(true, LoadMode::StartupFull, Some(1500), 0, 1000);
        assert_eq!(wedge, Some(Wedge::AlwaysOnHeavy));
    }

    fn rule_row(id: &str, globs: &[&str]) -> RuleGlobs {
        RuleGlobs {
            id: id.to_string(),
            scope: "global".to_string(),
            project: String::new(),
            globs: globs.iter().map(|g| g.to_string()).collect(),
            static_tokens: Some(1200),
        }
    }

    #[test]
    fn a_rule_whose_globs_match_nothing_read_is_never_fires() {
        let rules = vec![
            rule_row("used", &["src/**"]),
            rule_row("idle", &["docs/**"]),
        ];
        let hits = never_fires(&rules, &["src/main.rs".to_string()]);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].id, "idle");
        assert_eq!(hits[0].globs, vec!["docs/**".to_string()]);
        assert_eq!(hits[0].static_tokens, Some(1200));
    }

    #[test]
    fn a_rule_without_globs_or_reads_is_never_flagged() {
        // No declared globs: nothing can claim a target for it.
        let rules = vec![rule_row("globless", &[])];
        assert!(never_fires(&rules, &["src/main.rs".to_string()]).is_empty());
        // No read targets at all: a quiet window (or a harness that does not
        // report reads) is no evidence, never a flag.
        let rules = vec![rule_row("idle", &["docs/**"])];
        assert!(never_fires(&rules, &[]).is_empty());
    }

    #[test]
    fn any_matching_glob_saves_the_rule() {
        let rules = vec![rule_row("multi", &["docs/**", "src/*.rs"])];
        assert!(never_fires(&rules, &["src/a.rs".to_string()]).is_empty());
    }

    #[test]
    fn a_well_used_surface_has_no_wedge() {
        let wedge = classify_wedge(true, LoadMode::StartupDescription, Some(2000), 50, 1000);
        assert_eq!(wedge, None);
    }

    #[test]
    fn on_demand_cost_exists_only_for_a_body_that_waits_to_be_loaded() {
        // A skill's file weight is paid on invocation...
        assert_eq!(
            on_demand_tokens(LoadMode::StartupDescription, Some(1015)),
            Some(1015)
        );
        // ...as is a path-conditional rule's, when its glob fires.
        assert_eq!(
            on_demand_tokens(LoadMode::PathConditional, Some(500)),
            Some(500)
        );
        // An always-on file has no separate on-demand cost — it is already paid.
        assert_eq!(on_demand_tokens(LoadMode::StartupFull, Some(900)), None);
        assert_eq!(on_demand_tokens(LoadMode::ToolSchema, None), None);
    }

    #[test]
    fn startup_savings_reflects_what_is_actually_paid_at_startup() {
        // An always-on rule: removing it saves real tokens every session.
        assert_eq!(
            startup_savings(LoadMode::StartupFull, Some(900)),
            StartupSavings::Tokens(900)
        );
        // An MCP server: real but unmeasured.
        assert_eq!(
            startup_savings(LoadMode::ToolSchema, None),
            StartupSavings::UnknownSchema
        );
        // A skill: only its description is startup-loaded, so removal is
        // decluttering, not a token win — however large the body.
        assert_eq!(
            startup_savings(LoadMode::StartupDescription, Some(7)),
            StartupSavings::Declutter
        );
    }

    #[test]
    fn unknown_load_mode_labels_parse_to_none() {
        // A future harness may write a label this version never knew (PBI 07):
        // it must stay unknown, never read as a known mode.
        assert_eq!(LoadMode::from_label("startup_preload"), None);
    }

    #[test]
    fn empty_and_case_variant_labels_are_unknown() {
        // No normalization: the store holds exact labels, so "" and case
        // drift must not quietly become a known mode either.
        assert_eq!(LoadMode::from_label(""), None);
        assert_eq!(LoadMode::from_label("Startup_Full"), None);
        assert_eq!(LoadMode::from_label("ON_DEMAND"), None);
    }

    #[test]
    fn all_five_known_labels_round_trip() {
        for mode in [
            LoadMode::StartupFull,
            LoadMode::StartupDescription,
            LoadMode::PathConditional,
            LoadMode::OnDemand,
            LoadMode::ToolSchema,
        ] {
            assert_eq!(LoadMode::from_label(mode.label()), Some(mode));
        }
    }

    #[test]
    fn identical_descriptions_are_redundant() {
        let rows = vec![
            candidate("a", "skill", "global", "", "Helps with git commits"),
            candidate("b", "skill", "global", "", "Helps with git commits"),
        ];
        let pairs = redundant_pairs(&rows);
        assert_eq!(pairs.len(), 1);
        assert!((pairs[0].0 == 0 && pairs[0].1 == 1) || (pairs[0].0 == 1 && pairs[0].1 == 0));
    }

    #[test]
    fn overlapping_purposes_are_redundant_unrelated_ones_are_not() {
        let rows = vec![
            candidate(
                "git-commit",
                "skill",
                "global",
                "",
                "Write git commit messages with hooks",
            ),
            candidate(
                "git-push",
                "skill",
                "global",
                "",
                "Write git commits with hooks",
            ),
            candidate("weather", "skill", "global", "", "Current weather forecast"),
        ];
        let pairs = redundant_pairs(&rows);
        assert_eq!(
            pairs.len(),
            1,
            "only the git pair overlaps enough: {pairs:?}"
        );
    }

    #[test]
    fn the_threshold_is_inclusive_and_short_texts_never_match() {
        // Empty and single-token descriptions carry no signal.
        let rows = vec![
            candidate("a", "skill", "global", "", ""),
            candidate("b", "skill", "global", "", "x"),
        ];
        assert!(redundant_pairs(&rows).is_empty());
        // Different kinds never pair: a skill and an agent with the same
        // words are different surfaces, not duplicates.
        let rows = vec![
            candidate("a", "skill", "global", "", "Review code thoroughly"),
            candidate("b", "agent", "global", "", "Review code thoroughly"),
        ];
        assert!(redundant_pairs(&rows).is_empty());
    }
}
