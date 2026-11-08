//! Behavioral classification of user prompts. The point is not what a prompt is
//! about (topic — which embeddings cluster, but that does not map to reusable
//! skills) but how the user is *steering*: approving, correcting, asking, or
//! directing. The mix is an actionable signal — heavy steering suggests room for
//! more autonomy, frequent corrections suggest clearer upfront specs. These are
//! lexical heuristics, so reports label them lower-confidence.

/// How a user prompt steers the session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptBehavior {
    /// A bare approval / go-ahead that carries no task ("go ahead", "yes", "次").
    Steer,
    /// A correction or redirect after a wrong turn ("いや…戻して", "no, instead").
    Correct,
    /// A question rather than a directive (ends with `?` / `？`).
    Question,
    /// A substantive instruction — the default.
    Instruct,
}

impl PromptBehavior {
    pub fn label(self) -> &'static str {
        match self {
            PromptBehavior::Steer => "steer",
            PromptBehavior::Correct => "correct",
            PromptBehavior::Question => "question",
            PromptBehavior::Instruct => "instruct",
        }
    }
}

/// Classify a prompt by how it steers the session. Order matters: a bare
/// approval is checked before correction/question so "yes" is steering, not a
/// (missing) directive.
pub fn classify_prompt(text: &str) -> PromptBehavior {
    let trimmed = text.trim();
    let lower = trimmed.to_lowercase();

    if is_steer(trimmed, &lower) {
        PromptBehavior::Steer
    } else if is_correction(trimmed, &lower) {
        PromptBehavior::Correct
    } else if trimmed.ends_with('?') || trimmed.ends_with('？') {
        PromptBehavior::Question
    } else {
        PromptBehavior::Instruct
    }
}

/// A whole-prompt approval that carries no task of its own.
fn is_steer(trimmed: &str, lower: &str) -> bool {
    const APPROVALS: &[&str] = &[
        "go ahead",
        "go",
        "yes",
        "yes please",
        "y",
        "ok",
        "okay",
        "sure",
        "next",
        "continue",
        "proceed",
        "続けて",
        "続け",
        "進めて",
        "完遂して",
        "完遂してね",
        "どうぞ",
        "うん",
        "はい",
        "これで",
        "お願い",
        "おねがい",
        "やって",
        "それで",
    ];
    if APPROVALS.contains(&lower) {
        return true;
    }
    // A very short prompt that is not a question is a steering token (a menu
    // pick like "A"/"2", "go ahead.", a one-word nudge).
    let chars = trimmed.chars().count();
    chars > 0 && chars <= 4 && !trimmed.ends_with('?') && !trimmed.ends_with('？')
}

/// A correction or redirect: the previous turn was wrong and is being undone or
/// re-pointed.
fn is_correction(trimmed: &str, lower: &str) -> bool {
    const LEADING_JP: &[&str] = &["いや", "ちが", "違", "そうじゃ", "じゃなくて"];
    const ANYWHERE_JP: &[&str] = &[
        "じゃなくて",
        "やり直",
        "戻して",
        "間違",
        "そんなこと",
        "違うよ",
        "ではない",
    ];
    const LEADING_EN: &[&str] = &["no,", "no ", "not ", "actually,", "wait,"];
    const ANYWHERE_EN: &[&str] = &["instead", "revert", "undo", "that's wrong", "rollback"];

    if LEADING_JP.iter().any(|m| trimmed.starts_with(m)) {
        return true;
    }
    if ANYWHERE_JP.iter().any(|m| trimmed.contains(m)) {
        return true;
    }
    if LEADING_EN.iter().any(|m| lower.starts_with(m)) {
        return true;
    }
    ANYWHERE_EN.iter().any(|m| lower.contains(m))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bare_approvals_are_steering() {
        assert_eq!(classify_prompt("go ahead"), PromptBehavior::Steer);
        assert_eq!(classify_prompt("yes"), PromptBehavior::Steer);
        assert_eq!(classify_prompt("次"), PromptBehavior::Steer);
        assert_eq!(classify_prompt("B"), PromptBehavior::Steer); // a menu pick
        assert_eq!(classify_prompt("完遂してね"), PromptBehavior::Steer);
    }

    #[test]
    fn corrections_and_redirects_are_correct() {
        assert_eq!(
            classify_prompt("いや、それは違う。戻して"),
            PromptBehavior::Correct
        );
        assert_eq!(classify_prompt("そうじゃなくて逆"), PromptBehavior::Correct);
        assert_eq!(
            classify_prompt("no, do it the other way instead"),
            PromptBehavior::Correct
        );
    }

    #[test]
    fn trailing_question_mark_is_a_question() {
        assert_eq!(
            classify_prompt("なぜそうなるの？"),
            PromptBehavior::Question
        );
        assert_eq!(classify_prompt("is that a bug?"), PromptBehavior::Question);
    }

    #[test]
    fn a_substantive_directive_is_an_instruction() {
        assert_eq!(
            classify_prompt("次の契約を TDD で実装して、テストも書いて"),
            PromptBehavior::Instruct
        );
    }

    #[test]
    fn a_correction_wins_over_a_nudge() {
        // Leading "いや" wins even though it could read as a nudge.
        assert_eq!(
            classify_prompt("いや、それは保留で"),
            PromptBehavior::Correct
        );
    }

    #[test]
    fn templates_mask_paths_numbers_and_quotes() {
        assert_eq!(
            super::prompt_template("fix the typo in foo.rs"),
            "fix the typo in <PATH>"
        );
        assert_eq!(
            super::prompt_template("run tests on port 8080"),
            "run tests on port #"
        );
        assert_eq!(super::prompt_template("say \"hi\" to bob"), "say <> to bob");
        assert_eq!(
            super::prompt_template("deploy v2 to prod"),
            "deploy v# to prod"
        );
    }

    #[test]
    fn identical_templates_cluster_at_threshold() {
        let prompts = [
            ("fix the typo in foo.rs", "a.jsonl:1"),
            ("fix the typo in bar.rs", "a.jsonl:9"),
            ("unrelated one-off", "a.jsonl:12"),
            ("fix the typo in baz.rs", "b.jsonl:3"),
        ];
        let out = super::cluster_candidates(prompts.iter().map(|(t, s)| (*t, *s)));
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].template, "fix the typo in <PATH>");
        assert_eq!(out[0].count, 3);
        assert_eq!(out[0].example, "fix the typo in foo.rs");
        assert_eq!(out[0].sources, vec!["a.jsonl:1", "a.jsonl:9", "b.jsonl:3"]);
    }

    #[test]
    fn below_threshold_and_empty_templates_stay_out() {
        let prompts = [("twice is nice", "a:1"), ("twice is nice!", "a:2")];
        assert!(super::cluster_candidates(prompts.iter().map(|(t, s)| (*t, *s))).is_empty());
        let prompts = [("", "a:1"), ("   ", "a:2"), ("\n", "a:3")];
        assert!(super::cluster_candidates(prompts.iter().map(|(t, s)| (*t, *s))).is_empty());
    }
}

/// A repeated prompt becomes a skill candidate this many times: below it the
/// repetition is noise, at it the habit is worth naming. Tuned, not derived —
/// the failure mode of a lower threshold is nagging about one-offs.
pub const MIN_CANDIDATE_REPEATS: usize = 3;

/// The most examples carried per candidate: enough to show the shape, not to
/// dump the history.
pub const CANDIDATE_EXAMPLES: usize = 3;

/// Reduce a prompt to its reusable shape (PBI 12): quoted spans, backtick
/// spans, file-ish tokens, and bare numbers become holes, so "fix the typo
/// in foo.rs" and "fix the typo in bar.rs" cluster while "fix the typo"
/// stays its own template. Language-agnostic and deterministic — no
/// embeddings, no model. What the masker cannot see (a reworded ask with the
/// same intent) stays unclustered: that is the documented recall limit, not a
/// bug to silently paper over.
pub fn prompt_template(text: &str) -> String {
    let mut out = String::new();
    let mut chars = text.chars().peekable();
    // Quoted/backtick spans collapse first: their insides are always variable.
    while let Some(c) = chars.next() {
        if c == '"' || c == '\'' || c == '`' || c == '「' || c == '」' {
            for inner in chars.by_ref() {
                if matches!(inner, '"' | '\'' | '`' | '「' | '」') {
                    break;
                }
            }
            if !out.ends_with("<> ") && !out.ends_with("<>") {
                out.push_str("<>");
            } else if !out.ends_with(' ') {
                out.push(' ');
            }
        } else {
            out.push(c);
        }
    }
    out.split_whitespace()
        .map(|token| {
            if token.contains('/') || token.contains('\\') || is_file_like(token) {
                "<PATH>".to_string()
            } else if token.chars().all(|c| c.is_ascii_digit()) && !token.is_empty() {
                "#".to_string()
            } else if token.chars().any(|c| c.is_ascii_digit())
                && token.chars().any(|c| c.is_ascii_alphabetic())
            {
                // Version-ish and mixed tokens (`v2`, `utf8`): the digits vary.
                let mut masked = String::new();
                let mut digit_run = false;
                for c in token.chars() {
                    if c.is_ascii_digit() {
                        if !digit_run {
                            masked.push('#');
                            digit_run = true;
                        }
                    } else {
                        masked.push(c);
                        digit_run = false;
                    }
                }
                masked
            } else {
                token.to_lowercase()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// A token that names a file: dotted suffix, or a path already (handled
/// above, kept here for the bare `Makefile`/`Dockerfile`-less names? No —
/// only dotted names; plain words are prose and must not become holes).
fn is_file_like(token: &str) -> bool {
    let t = token.trim_matches(|c| matches!(c, '.' | ',' | ';' | ':' | '!' | '?'));
    match t.rfind('.') {
        Some(i) if i > 0 && i + 1 < t.len() => {
            t[i + 1..].chars().all(|c| c.is_ascii_alphanumeric())
        }
        _ => false,
    }
}

/// One repeat cluster: the shared template, how many prompts wore it, one
/// exemplar in the user's own words, and where the repetitions live.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillCandidate {
    pub template: String,
    pub count: usize,
    pub example: String,
    pub sources: Vec<String>,
}

/// Group prompt texts into skill candidates: identical templates with at
/// least [`MIN_CANDIDATE_REPEATS`] sightings. Exemplar and order are first-seen
/// (input order is time order), sources are distinct origins capped at
/// [`CANDIDATE_EXAMPLES`]-plus-one accounting — deterministic for any input
/// order that is itself deterministic.
pub fn cluster_candidates<'a>(
    prompts: impl IntoIterator<Item = (&'a str, &'a str)>,
) -> Vec<SkillCandidate> {
    use std::collections::BTreeMap;
    struct Group {
        count: usize,
        example: String,
        sources: Vec<String>,
        order: usize,
    }
    let mut groups: BTreeMap<String, Group> = BTreeMap::new();
    for (text, source) in prompts {
        let template = prompt_template(text);
        if template.trim().is_empty() {
            continue;
        }
        let order = groups.len();
        let group = groups.entry(template.clone()).or_insert(Group {
            count: 0,
            example: text.to_string(),
            sources: Vec::new(),
            order,
        });
        group.count += 1;
        if !group.sources.iter().any(|s| s == source) {
            group.sources.push(source.to_string());
        }
    }
    let mut out: Vec<(usize, SkillCandidate)> = groups
        .into_iter()
        .filter(|(_, g)| g.count >= MIN_CANDIDATE_REPEATS)
        .map(|(template, g)| {
            (
                g.order,
                SkillCandidate {
                    template,
                    count: g.count,
                    example: g.example,
                    sources: g.sources,
                },
            )
        })
        .collect();
    out.sort_by(|a, b| b.1.count.cmp(&a.1.count).then_with(|| a.0.cmp(&b.0)));
    out.into_iter().map(|(_, c)| c).collect()
}
