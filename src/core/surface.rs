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
    /// `skill` | `rule` | `mcp_server` | `mcp_tool` | `hook` | `claude_md` | `permission` | `agent` | `command`.
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

/// One surface kind's wedge traits — the kind semantics every predicate and
/// wedge reads from [`KIND_TRAITS`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct KindTraits {
    /// The kind can acquire usage events, so absent events are evidence of
    /// disuse (the `Unused` / `CostlyRare` wedges).
    measurable: bool,
    /// The kind declares a description of its own purpose, so two of its
    /// surfaces can be compared for a shared one (the `Redundant` wedge).
    describable: bool,
    /// The kind carries `paths:` globs whose non-matching is evidence
    /// (the `NeverFires` wedge).
    globbable: bool,
}

impl KindTraits {
    /// The fallback for a kind the table does not know: no wedge may use it.
    const NONE: KindTraits = KindTraits {
        measurable: false,
        describable: false,
        globbable: false,
    };
}

/// Every known surface kind with its wedge traits, in one table: adding a
/// kind means adding one row here, and every predicate — and every wedge —
/// answers consistently (`docs/specs/surfaces.md`). An unknown kind falls
/// back to [`KindTraits::NONE`]: it stays silently outside every wedge,
/// never mis-accused from partial evidence.
static KIND_TRAITS: &[(&str, KindTraits)] = &[
    (
        "skill",
        KindTraits {
            measurable: true,
            describable: true,
            globbable: false,
        },
    ),
    (
        "agent",
        KindTraits {
            measurable: true,
            describable: true,
            globbable: false,
        },
    ),
    (
        "command",
        KindTraits {
            measurable: false,
            describable: true,
            globbable: false,
        },
    ),
    (
        "mcp_server",
        KindTraits {
            measurable: true,
            describable: false,
            globbable: false,
        },
    ),
    (
        "mcp_tool",
        KindTraits {
            measurable: true,
            describable: false,
            globbable: false,
        },
    ),
    (
        "permission",
        KindTraits {
            measurable: true,
            describable: false,
            globbable: false,
        },
    ),
    (
        "hook",
        KindTraits {
            measurable: true,
            describable: false,
            globbable: false,
        },
    ),
    (
        "rule",
        KindTraits {
            measurable: false,
            describable: false,
            globbable: true,
        },
    ),
    (
        "claude_md",
        KindTraits {
            measurable: false,
            describable: false,
            globbable: false,
        },
    ),
];

/// A kind's traits from the table, or [`KindTraits::NONE`] for an unknown one.
fn kind_traits(kind: &str) -> KindTraits {
    KIND_TRAITS
        .iter()
        .find(|(known, _)| *known == kind)
        .map_or(KindTraits::NONE, |(_, traits)| *traits)
}

/// Static-token threshold above which an always-on or rarely-used surface
/// counts as "heavy" — a tuning knob, not a hard rule. Shared by the
/// briefing's wedge routing (`core::findings`) and the `waste` view
/// (`cli/mod.rs`).
pub const HEAVY_TOKENS: u64 = 800;

/// Whether a surface kind can ever acquire usage events. Absence of events is
/// only evidence of disuse for usage-measurable kinds; for catalog-only kinds
/// (rules, CLAUDE.md) it means nothing (`docs/specs/surfaces.md`). Hooks
/// joined their measurable peers once firing became observable (PBI 21): a
/// hook with firings is used; one without is a delete candidate.
pub fn is_usage_measurable(kind: &str) -> bool {
    kind_traits(kind).measurable
}

/// Whether this kind's surfaces declare a description of their own purpose,
/// so two of them can be compared for a shared one. Consumed by the
/// `Redundant` wedge (`cli::views::waste`) as its kind gate: only describable
/// kinds enter the duplicate comparison, and a surface whose description is
/// `None` never matches even then.
pub fn has_described_purpose(kind: &str) -> bool {
    kind_traits(kind).describable
}

/// Whether non-matching `paths:` globs are meaningful evidence for this kind.
/// Consumed by the `NeverFires` wedge (`cli::views::waste`) as its kind gate
/// selecting which catalog rows the glob matcher may accuse.
pub fn supports_never_fires(kind: &str) -> bool {
    kind_traits(kind).globbable
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

/// The never-fires verdict for a catalog's path-conditional rules: the rules
/// whose globs matched no read target (flag these), plus the rules the
/// comparison budget could not judge in bounds (no evidence either way —
/// excluded from the verdict, and named so the caller can warn instead of
/// guessing).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NeverFiresVerdict {
    pub hits: Vec<NeverFireHit>,
    /// Ids of rules whose glob evaluation exceeded the comparison budget
    /// before resolving.
    pub unresolved: Vec<String>,
}

/// Route path-conditional rules with declared globs against the window's read
/// file targets. A rule whose globs matched no read target never fired
/// in-window — provided the window observed reads at all, and provided the
/// rule's globs could actually be evaluated: a rule whose evaluation exceeds
/// `budget` comparison steps is **excluded** and named in `unresolved`, never
/// accused from an unfinished scan (the "no evidence, never an accusation"
/// direction — the budget runs out as easily on a rule that would have
/// matched as on one that never fires). No targets means no evidence (a
/// quiet window, or a harness that does not report reads), never a flag.
/// Catalog order in, same order out.
pub fn never_fires(rules: &[RuleGlobs], targets: &[String], budget: u64) -> NeverFiresVerdict {
    let mut hits = Vec::new();
    let mut unresolved = Vec::new();
    if targets.is_empty() {
        return NeverFiresVerdict { hits, unresolved };
    }
    for rule in rules {
        if rule.globs.is_empty() {
            continue;
        }
        let mut budget = crate::core::glob_match::MatchBudget::new(budget);
        let mut matched = false;
        let mut judged = true;
        for target in targets {
            match crate::core::glob_match::matches_any_budgeted(&rule.globs, target, &mut budget) {
                Some(true) => {
                    matched = true;
                    break;
                }
                Some(false) => {}
                None => {
                    judged = false;
                    break;
                }
            }
        }
        if !judged {
            unresolved.push(rule.id.clone());
        } else if !matched {
            hits.push(NeverFireHit {
                id: rule.id.clone(),
                scope: rule.scope.clone(),
                project: rule.project.clone(),
                globs: rule.globs.clone(),
                static_tokens: rule.static_tokens,
            });
        }
    }
    NeverFiresVerdict { hits, unresolved }
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
    fn every_known_kind_answers_its_traits_from_one_table() {
        // The wedge gates exactly as the pre-consolidation view filters
        // answered them: measurable (Unused / CostlyRare routing), describable
        // (the redundant wedge's kind gate), globbable (the never-fires
        // wedge's kind gate). `command` is described but not usage-measurable
        // today; the table preserves that, byte-identical output.
        let expected: &[(&str, bool, bool, bool)] = &[
            ("skill", true, true, false),
            ("agent", true, true, false),
            ("mcp_server", true, false, false),
            ("mcp_tool", true, false, false),
            ("permission", true, false, false),
            ("hook", true, false, false),
            ("rule", false, false, true),
            ("claude_md", false, false, false),
            ("command", false, true, false),
        ];
        for (kind, measurable, describable, globbable) in expected {
            assert_eq!(is_usage_measurable(kind), *measurable, "measurable: {kind}");
            assert_eq!(
                has_described_purpose(kind),
                *describable,
                "describable: {kind}"
            );
            assert_eq!(supports_never_fires(kind), *globbable, "globbable: {kind}");
        }
    }

    #[test]
    fn the_traits_table_covers_each_known_kind_exactly_once() {
        // One row per known kind, no aliases: a duplicated row would let two
        // entries drift apart while every predicate keeps answering.
        let mut seen: Vec<&str> = KIND_TRAITS.iter().map(|(kind, _)| *kind).collect();
        seen.sort_unstable();
        assert_eq!(
            seen,
            [
                "agent",
                "claude_md",
                "command",
                "hook",
                "mcp_server",
                "mcp_tool",
                "permission",
                "rule",
                "skill",
            ]
        );
    }

    #[test]
    fn an_unknown_kind_is_false_for_every_predicate() {
        // A kind that slips registration (or a future harness's new label)
        // stays silently outside every wedge — the pre-table semantics, never
        // a mis-accusation from partial evidence.
        for kind in ["", "future_thing", "Skill"] {
            assert!(!is_usage_measurable(kind), "measurable: {kind}");
            assert!(!has_described_purpose(kind), "describable: {kind}");
            assert!(!supports_never_fires(kind), "globbable: {kind}");
        }
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
        let verdict = never_fires(
            &rules,
            &["src/main.rs".to_string()],
            crate::core::glob_match::DEFAULT_MATCH_BUDGET,
        );
        assert!(verdict.unresolved.is_empty());
        assert_eq!(verdict.hits.len(), 1);
        assert_eq!(verdict.hits[0].id, "idle");
        assert_eq!(verdict.hits[0].globs, vec!["docs/**".to_string()]);
        assert_eq!(verdict.hits[0].static_tokens, Some(1200));
    }

    #[test]
    fn a_rule_without_globs_or_reads_is_never_flagged() {
        // No declared globs: nothing can claim a target for it.
        let rules = vec![rule_row("globless", &[])];
        let verdict = never_fires(
            &rules,
            &["src/main.rs".to_string()],
            crate::core::glob_match::DEFAULT_MATCH_BUDGET,
        );
        assert!(verdict.hits.is_empty() && verdict.unresolved.is_empty());
        // No read targets at all: a quiet window (or a harness that does not
        // report reads) is no evidence, never a flag.
        let rules = vec![rule_row("idle", &["docs/**"])];
        let verdict = never_fires(&rules, &[], crate::core::glob_match::DEFAULT_MATCH_BUDGET);
        assert!(verdict.hits.is_empty() && verdict.unresolved.is_empty());
    }

    #[test]
    fn any_matching_glob_saves_the_rule() {
        let rules = vec![rule_row("multi", &["docs/**", "src/*.rs"])];
        let verdict = never_fires(
            &rules,
            &["src/a.rs".to_string()],
            crate::core::glob_match::DEFAULT_MATCH_BUDGET,
        );
        assert!(verdict.hits.is_empty() && verdict.unresolved.is_empty());
    }

    #[test]
    fn a_rule_the_budget_cannot_judge_is_excluded_and_named() {
        // A hostile `**` chain against a deep target: with a tiny budget the
        // scan gives up unresolved. The rule is excluded from never-fires —
        // exhaustion is not evidence — and named so the view can warn.
        let rules = vec![rule_row(
            "hostile",
            &["**/**/**/**/**/**/**/**/**/**/**/**/z"],
        )];
        let deep = (0..26)
            .map(|i| format!("seg{i}"))
            .collect::<Vec<_>>()
            .join("/");
        let targets = vec![deep];
        let verdict = never_fires(&rules, &targets, 30);
        assert!(verdict.hits.is_empty(), "exhaustion never accuses");
        assert_eq!(verdict.unresolved, vec!["hostile".to_string()]);
        // The default budget resolves the same scan: no match anywhere,
        // so the rule is a never-fires hit again.
        let verdict = never_fires(
            &rules,
            &targets,
            crate::core::glob_match::DEFAULT_MATCH_BUDGET,
        );
        assert!(verdict.unresolved.is_empty());
        assert_eq!(verdict.hits.len(), 1);
        assert_eq!(verdict.hits[0].id, "hostile");
    }

    #[test]
    fn a_matching_rule_is_not_accused_when_the_budget_runs_out_first() {
        // The glob WOULD match the deep target's tail, but the tiny budget
        // gives up before reaching it. Exhaustion must suppress the flag,
        // not flip it into a false never-fires.
        let rules = vec![rule_row(
            "would-fire",
            &["**/**/**/**/**/**/**/**/**/**/**/**/seg25"],
        )];
        let targets = vec![
            (0..26)
                .map(|i| format!("seg{i}"))
                .collect::<Vec<_>>()
                .join("/"),
        ];
        let verdict = never_fires(&rules, &targets, 30);
        assert!(verdict.hits.is_empty(), "exhaustion never accuses");
        assert_eq!(verdict.unresolved, vec!["would-fire".to_string()]);
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
