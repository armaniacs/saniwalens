//! Minimal glob matching for rule `paths:` evaluation (PBI 04): `*`
//! (within one path segment), `**` (across segments, including zero),
//! `?` (one non-separator char). Case-sensitive; separators normalize
//! (`\` reads as `/`) so Windows-style transcript paths match the same way.
//! No dependency — the vocabulary is three constructs, and pinning them in
//! tests beats auditing a crate for this one call site.
//!
//! The patterns are untrusted input (an analyzed repository's rules), so a
//! naive `**` expansion is a hang: k chained `**` backtrack (segments+1)^k
//! times per suffix start. Matching is therefore memoized over
//! (pattern segment × target segment) states — each state resolves once —
//! and the budgeted entry points cap the total comparison steps, reporting
//! exhaustion instead of a guessed answer.

/// The default comparison budget for one rule's never-fires evaluation: all
/// of its globs against every read target in the window. Sized to never trip
/// for realistic rules even at the fetch limit (a few million steps), while
/// capping a hostile glob set at ~a tenth of a second instead of minutes.
pub const DEFAULT_MATCH_BUDGET: u64 = 10_000_000;

/// Segment caps bounding the memo table (one byte per state) and, with the
/// budget, the work per (pattern, target) pair. Far above anything a real
/// path or glob spells; a pair over the cap is not evaluable and reports
/// exhaustion rather than an answer.
const MAX_PATTERN_SEGMENTS: usize = 256;
const MAX_PATH_SEGMENTS: usize = 4_096;

/// A comparison-step budget shared by one rule's whole glob evaluation. Pure
/// data — core never warns; the caller turns exhaustion into a stderr warning
/// and excludes the rule from the verdict instead of guessing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MatchBudget {
    remaining: u64,
    exhausted: bool,
}

impl MatchBudget {
    /// A budget of `limit` comparison steps.
    pub fn new(limit: u64) -> Self {
        Self {
            remaining: limit,
            exhausted: false,
        }
    }

    /// Whether the budget ran out (the evaluation gave up unresolved).
    pub fn is_exhausted(&self) -> bool {
        self.exhausted
    }

    /// The unbudgeted wrappers' budget: the memoized evaluation is
    /// polynomial in the segment counts, which the segment caps bound.
    fn unlimited() -> Self {
        Self {
            remaining: u64::MAX,
            exhausted: false,
        }
    }

    /// Spend one comparison step; `false` when the budget is gone.
    fn spend(&mut self) -> bool {
        if self.remaining == 0 {
            self.exhausted = true;
            return false;
        }
        self.remaining -= 1;
        true
    }

    /// Mark the evaluation un-resolvable (segment caps exceeded).
    fn exhaust(&mut self) {
        self.exhausted = true;
    }
}

/// Whether `path` matches `pattern` under the three-construct vocabulary.
/// Both sides treat `/` as the separator (`\` is folded first).
/// This is a full-path match: `*.rs` does not match `src/main.rs`.
pub fn matches_one(pattern: &str, path: &str) -> bool {
    let pattern = pattern.replace('\\', "/");
    let path = path.replace('\\', "/");
    let mut budget = MatchBudget::unlimited();
    match_segments(&pattern, &path, &mut budget).unwrap_or(false)
}

/// Whether any of `patterns` matches `path` — at full length or at any
/// trailing segment run. Rule `paths:` are written repo-relative
/// (`src/**`), while transcript targets arrive absolute
/// (`/repo/src/main.rs`); suffix matching lets a repo-rooted glob meet the
/// absolute record without knowing the root. An empty pattern list matches
/// nothing: a rule that declares no globs can never claim a target.
pub fn matches_any<S: AsRef<str>>(patterns: &[S], path: &str) -> bool {
    let mut budget = MatchBudget::unlimited();
    matches_any_budgeted(patterns, path, &mut budget).unwrap_or(false)
}

/// Budgeted [`matches_any`]: `Some(matched)` when the evaluation resolved
/// within `budget`, `None` when it ran out — no answer, and the caller must
/// not read one into it. The budget spans every pattern and suffix start.
pub fn matches_any_budgeted<S: AsRef<str>>(
    patterns: &[S],
    path: &str,
    budget: &mut MatchBudget,
) -> Option<bool> {
    let path = path.replace('\\', "/");
    let stripped = path.strip_prefix('/').unwrap_or(&path);
    if stripped.is_empty() {
        return Some(false);
    }
    let segs: Vec<&str> = stripped.split('/').collect();
    for pattern in patterns {
        let pattern = pattern.as_ref().replace('\\', "/");
        match matches_any_suffixes(&pattern, &segs, budget) {
            Some(true) => return Some(true),
            Some(false) => {}
            None => return None,
        }
    }
    Some(false)
}

/// Whether `pattern` matches any suffix of `segs` (the suffix-start scan of
/// `matches_any`), over one memo table so every start shares the
/// (pattern × segment) states instead of re-deriving them. Each suffix
/// enters the way `match_segments` would: its leading empty segments (a
/// `//`-spelled path) are dropped before the pattern's first segment is
/// aligned — which is why the entry scan skips empties rather than trusting
/// the start index.
fn matches_any_suffixes(pattern: &str, segs: &[&str], budget: &mut MatchBudget) -> Option<bool> {
    let mut px: Vec<&str> = pattern.split('/').collect();
    if px.first() == Some(&"") {
        px.remove(0);
    }
    let mut memo = new_memo(px.len(), segs.len(), budget)?;
    let stride = segs.len() + 1;
    for start in 0..segs.len() {
        let mut entry = start;
        while entry < segs.len() && segs[entry].is_empty() {
            entry += 1;
        }
        match eval_at(&px, segs, 0, entry, &mut memo, stride, budget) {
            Some(true) => return Some(true),
            Some(false) => {}
            None => return None,
        }
    }
    Some(false)
}

/// Whether `pattern` matches `path` at full length, over one memo table.
fn match_segments(pattern: &str, path: &str, budget: &mut MatchBudget) -> Option<bool> {
    let mut px: Vec<&str> = pattern.split('/').collect();
    let mut sx: Vec<&str> = path.split('/').collect();
    // A leading `/` makes both sides start with an empty segment; drop it
    // so absolute and relative spellings of the same tree agree.
    if px.first() == Some(&"") {
        px.remove(0);
    }
    if sx.first() == Some(&"") {
        sx.remove(0);
    }
    let mut memo = new_memo(px.len(), sx.len(), budget)?;
    let stride = sx.len() + 1;
    eval_at(&px, &sx, 0, 0, &mut memo, stride, budget)
}

/// The memo table for one (pattern, path) pair, or `None` when the pair is
/// over the segment caps — too large to evaluate, reported as exhaustion.
fn new_memo(
    pattern_segments: usize,
    path_segments: usize,
    budget: &mut MatchBudget,
) -> Option<Vec<Option<bool>>> {
    if pattern_segments > MAX_PATTERN_SEGMENTS || path_segments > MAX_PATH_SEGMENTS {
        budget.exhaust();
        return None;
    }
    Some(vec![None; (pattern_segments + 1) * (path_segments + 1)])
}

/// `pat[p..]` vs `segs[s..]`, memoized at `memo[p * stride + s]`. A `**`
/// stops where it is (the rest matches from `s`) or swallows one segment and
/// stays on the same pattern slot — the two transitions walk the state graph
/// without revisiting it, so a chain of `**` resolves in
/// (pattern segments × path segments) steps, not backtracking. `None` means
/// the budget ran out mid-scan; memoized states stay valid.
fn eval_at(
    pat: &[&str],
    segs: &[&str],
    p: usize,
    s: usize,
    memo: &mut [Option<bool>],
    stride: usize,
    budget: &mut MatchBudget,
) -> Option<bool> {
    if let Some(hit) = memo[p * stride + s] {
        return Some(hit);
    }
    let resolved = match pat.get(p) {
        None => Some(s == segs.len()),
        Some(&"**") => {
            if !budget.spend() {
                None
            } else {
                match eval_at(pat, segs, p + 1, s, memo, stride, budget) {
                    None => None,
                    Some(true) => Some(true),
                    Some(false) => {
                        if s < segs.len() {
                            eval_at(pat, segs, p, s + 1, memo, stride, budget)
                        } else {
                            Some(false)
                        }
                    }
                }
            }
        }
        Some(&head) => match segs.get(s) {
            None => Some(false),
            Some(seg) => {
                if !budget.spend() {
                    None
                } else if match_one(head, seg) {
                    eval_at(pat, segs, p + 1, s + 1, memo, stride, budget)
                } else {
                    Some(false)
                }
            }
        },
    };
    match resolved {
        Some(hit) => {
            memo[p * stride + s] = Some(hit);
            Some(hit)
        }
        None => None,
    }
}

fn match_one(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let t: Vec<char> = text.chars().collect();
    let (mut pi, mut ti) = (0, 0);
    let (mut star, mut mark) = (None, 0);
    while ti < t.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == t[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            mark = ti;
            pi += 1;
        } else if let Some(s) = star {
            pi = s + 1;
            mark += 1;
            ti = mark;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn star_stays_inside_one_segment() {
        assert!(matches_one("src/*.rs", "src/main.rs"));
        assert!(!matches_one("src/*.rs", "src/sub/main.rs"));
        assert!(!matches_one("*.rs", "src/main.rs"));
        // An empty path is not a file: nothing matches it (except "").
        assert!(!matches_one("*", ""));
        assert!(matches_one("", ""));
    }

    #[test]
    fn star_star_crosses_segments_including_zero() {
        assert!(matches_one("src/**", "src/a/b/c.rs"));
        assert!(matches_one("src/**", "src/x.rs"));
        assert!(matches_one("**/*.md", "docs/a/b/note.md"));
        assert!(matches_one("**", "anything/at/all.rs"));
        assert!(!matches_one("src/**", "other/x.rs"));
    }

    #[test]
    fn question_mark_matches_one_non_separator_char() {
        assert!(matches_one("src/?.rs", "src/a.rs"));
        assert!(!matches_one("src/?.rs", "src/ab.rs"));
        assert!(!matches_one("src/?.rs", "src/.rs"));
    }

    #[test]
    fn separators_fold_and_case_is_exact() {
        assert!(matches_one("src/*.rs", "src\\main.rs"));
        assert!(!matches_one("SRC/*.rs", "src/main.rs"));
        assert!(matches_one("/src/*.rs", "src/main.rs"));
    }

    #[test]
    fn any_match_wins_and_empty_matches_nothing() {
        assert!(matches_any(&["docs/**", "src/*.rs"], "src/main.rs"));
        assert!(!matches_any(&["docs/**", "src/*.rs"], "test/main.rs"));
        assert!(!matches_any::<&str>(&[], "src/main.rs"));
    }

    #[test]
    fn relative_globs_meet_absolute_targets_at_any_depth() {
        // Rule globs are repo-relative; transcript targets are absolute.
        assert!(matches_any(&["src/**"], "/repo/src/main.rs"));
        assert!(matches_any(&["*.rs"], "/repo/src/main.rs"));
        assert!(!matches_any(&["docs/**"], "/repo/src/main.rs"));
        assert!(!matches_any(&["src/**"], ""));
    }

    #[test]
    fn a_chained_star_star_pattern_resolves_in_bounded_time() {
        // Red: rule globs are untrusted input (an analyzed repository's
        // `paths:`), and k chained `**` used to backtrack (segments+1)^k
        // times per suffix start — a hang, the same threat class the
        // symlink-recursion PBI closed for crashes. The evaluation must
        // finish, not explore the product.
        let pattern = "**/**/**/**/**/**/**/**/**/**/z";
        let path = (0..26)
            .map(|i| format!("seg{i}"))
            .collect::<Vec<_>>()
            .join("/");
        let started = std::time::Instant::now();
        assert!(!matches_any(&[pattern], &path));
        assert!(
            started.elapsed() < std::time::Duration::from_secs(2),
            "the chained-** scan took {:?}, unbounded",
            started.elapsed()
        );
    }

    #[test]
    fn the_memoized_matcher_answers_what_the_naive_one_did() {
        // Equivalence grid over the shapes the old recursion grew on:
        // `**` chains, empty segments (`//` spellings), empty patterns,
        // absolute targets. The budgeted entry must agree with the plain one.
        let patterns: &[&[&str]] = &[
            &["src/**"],
            &["**/**/x/**/y"],
            &["**"],
            &["*.rs"],
            &[""],
            &["a//b"],
            &["**/a/**/b"],
            &["/x/**"],
            &["docs/**", "src/*.rs"],
        ];
        let paths = [
            "",
            "/",
            "///",
            "a//b",
            "src/main.rs",
            "/repo/src/main.rs",
            "x/y/z",
            "/x/y",
            "a/x/b/y/c/b",
            "src/",
            "/src/",
            "a/b/c/d/e/f/g",
        ];
        for patterns in patterns {
            for path in paths {
                let mut budget = MatchBudget::new(DEFAULT_MATCH_BUDGET);
                assert_eq!(
                    matches_any_budgeted(patterns, path, &mut budget),
                    Some(matches_any(patterns, path)),
                    "budgeted disagrees with plain for {patterns:?} vs {path:?}"
                );
                assert!(!budget.is_exhausted(), "default budget exhausted: {path:?}");
            }
        }
    }

    #[test]
    fn a_tiny_budget_gives_up_observably_and_a_sufficient_one_resolves() {
        // "a/b/c/d" needs one comparison step per pattern segment: a budget
        // of three cannot finish, four fits exactly — and the caller can
        // tell the difference instead of reading exhaustion as a match.
        let mut budget = MatchBudget::new(3);
        assert_eq!(
            matches_any_budgeted(&["a/b/c/d"], "a/b/c/d", &mut budget),
            None
        );
        assert!(budget.is_exhausted());

        let mut budget = MatchBudget::new(4);
        assert_eq!(
            matches_any_budgeted(&["a/b/c/d"], "a/b/c/d", &mut budget),
            Some(true)
        );
        assert!(!budget.is_exhausted());
    }

    #[test]
    fn the_budget_spans_patterns_and_suffix_starts_until_it_runs_out() {
        // The budget is the caller's total for the call: a pattern that
        // fails late spends as much as one that matches, and the next
        // pattern finds the budget gone — observable, not silently false.
        let mut budget = MatchBudget::new(4);
        assert_eq!(
            matches_any_budgeted(&["a/b/c/d", "a/b/c/d"], "a/b/c/e", &mut budget),
            None,
            "the second pattern finds the budget gone"
        );
        assert!(budget.is_exhausted());
    }
}
