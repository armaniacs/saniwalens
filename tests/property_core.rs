//! Property tests over the pure-core invariants that boundary fixtures
//! hand-write today: glob budget determinism, usage-field clamping, and the
//! RFC 3339 round-trip. Each property states a contract the docs already
//! declare — the generator just attacks it far past what a fixture would.

use proptest::prelude::*;

use saniwalens::adapter::transcript::{MAX_REALISTIC_TOKENS, sanitize_usage_field};
use saniwalens::core::glob_match::{MatchBudget, matches_any, matches_any_budgeted, matches_one};
use saniwalens::time::{epoch_to_rfc3339_utc, parse_rfc3339_millis};

/// Glob patterns assembled from segments: literals, `*`, `**`, `?`, and
/// `?` + literal mixes. Segment-shaped so `**` expansion paths are always
/// exercised against real directory boundaries.
fn glob_pattern() -> impl Strategy<Value = String> {
    prop::collection::vec(
        prop::sample::select(vec!["*", "**", "?", "ab", "x", "a.md", "pro?"]),
        1..8,
    )
    .prop_map(|segs| segs.join("/"))
}

/// Paths from the same segment vocabulary, with a leading slash sometimes:
/// transcript targets arrive absolute, and the matcher strips the prefix
/// before splitting.
fn target_path() -> impl Strategy<Value = String> {
    (
        prop::bool::ANY,
        prop::collection::vec(
            prop::sample::select(vec!["ab", "x", "a.md", "proj", "pro?"]),
            1..6,
        ),
    )
        .prop_map(|(absolute, segs)| {
            if absolute {
                format!("/{}", segs.join("/"))
            } else {
                segs.join("/")
            }
        })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    /// The documented contract (`core/glob_match.rs`): `matches_one` is a
    /// full-path match and `matches_any` is suffix matching, so a full match
    /// must always be included in the suffix answer; and a finite budget
    /// that resolves agrees with the unlimited route exactly — a `None`
    /// means "no answer", never a different answer.
    #[test]
    fn glob_budget_never_changes_the_answer(pattern in glob_pattern(), path in target_path()) {
        let full = matches_one(&pattern, &path);
        let any = matches_any(&[&pattern], &path);
        prop_assert!(any || !full, "a full-path match must reach the suffix scan");

        // A tight-but-generous budget must still either agree or decline.
        let mut tight = MatchBudget::new(64);
        if let Some(answer) = matches_any_budgeted(&[&pattern], &path, &mut tight) {
            prop_assert_eq!(answer, any);
        }
    }

    /// `sanitize_usage_field` clamps to the documented ceiling, never
    /// invents, and never double-counts: a realistic value passes through
    /// untouched with zero drops.
    #[test]
    fn usage_clamp_is_bounded_and_lossless_below_the_ceiling(value in any::<u64>()) {
        let (clamped, dropped) = sanitize_usage_field(value);
        prop_assert!(clamped <= MAX_REALISTIC_TOKENS);
        prop_assert!(clamped <= value);
        if value <= MAX_REALISTIC_TOKENS {
            prop_assert_eq!(clamped, value);
            prop_assert_eq!(dropped, 0);
        } else {
            prop_assert_eq!(dropped, 1);
        }
    }

    /// Every four-digit-year epoch round-trips through the UTC text form:
    /// the formatter's output is exactly what the parser accepts. The
    /// calendar's extension years (10000+, rendered as `+NNNNN`) are a
    /// documented exception — `cli.md` Calendar bound notes edge-year
    /// renders do not parse back — so the property covers the years real
    /// data lives in.
    #[test]
    fn rfc3339_utc_round_trips_for_every_four_digit_second(
        day in 0i64..=2_932_896,
        second_of_day in 0i64..86_400,
    ) {
        let epoch = day * 86_400 + second_of_day;
        let text = epoch_to_rfc3339_utc(epoch);
        prop_assert!(!text.is_empty(), "in-calendar epoch formatted to empty: {epoch}");
        let parsed = parse_rfc3339_millis(&text)
            .unwrap_or_else(|| panic!("generated date does not parse: {text} (epoch {epoch})"));
        prop_assert_eq!(parsed, epoch * 1000, "round trip mismatch for {}", text);
    }
}
