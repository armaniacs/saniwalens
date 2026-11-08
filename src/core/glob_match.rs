//! Minimal glob matching for rule `paths:` evaluation (PBI 04): `*`
//! (within one path segment), `**` (across segments, including zero),
//! `?` (one non-separator char). Case-sensitive; separators normalize
//! (`\` reads as `/`) so Windows-style transcript paths match the same way.
//! No dependency — the vocabulary is three constructs, and pinning them in
//! tests beats auditing a crate for this one call site.

/// Whether `path` matches `pattern` under the three-construct vocabulary.
/// Both sides treat `/` as the separator (`\` is folded first).
/// This is a full-path match: `*.rs` does not match `src/main.rs`.
pub fn matches_one(pattern: &str, path: &str) -> bool {
    let pattern = pattern.replace('\\', "/");
    let path = path.replace('\\', "/");
    match_segments(&pattern, &path)
}

/// Whether any of `patterns` matches `path` — at full length or at any
/// trailing segment run. Rule `paths:` are written repo-relative
/// (`src/**`), while transcript targets arrive absolute
/// (`/repo/src/main.rs`); suffix matching lets a repo-rooted glob meet the
/// absolute record without knowing the root. An empty pattern list matches
/// nothing: a rule that declares no globs can never claim a target.
pub fn matches_any<S: AsRef<str>>(patterns: &[S], path: &str) -> bool {
    let path = path.replace('\\', "/");
    let stripped = path.strip_prefix('/').unwrap_or(&path);
    if stripped.is_empty() {
        return false;
    }
    let segs: Vec<&str> = stripped.split('/').collect();
    (0..segs.len()).any(|start| {
        let suffix = segs[start..].join("/");
        patterns.iter().any(|p| {
            let pattern = p.as_ref().replace('\\', "/");
            match_segments(&pattern, &suffix)
        })
    })
}

fn match_segments(pattern: &str, path: &str) -> bool {
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
    match_segs(&px, &sx)
}

fn match_segs(pat: &[&str], segs: &[&str]) -> bool {
    match pat.split_first() {
        None => segs.is_empty(),
        Some((&"**", rest)) => {
            // `**` crosses separators, including zero of them.
            (0..=segs.len()).any(|skip| match_segs(rest, &segs[skip..]))
        }
        Some((head, rest)) => match segs.split_first() {
            None => false,
            Some((seg, tail)) => match_one(head, seg) && match_segs(rest, tail),
        },
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
}
