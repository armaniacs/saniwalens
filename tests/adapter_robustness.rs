//! Adversarial-shape robustness (PBI 20): unknown fields, missing keys, and
//! wrong-typed values must never crash the adapters, and known records must
//! survive noise around them. A deterministic xorshift RNG (fixed seed) feeds
//! arbitrary JSON fragments to every string-taking parser — a std-only
//! property loop (no proptest/cargo-fuzz dependency available offline), with
//! the seed fixed so failures reproduce exactly. All fixtures synthetic.

use saniwalens::adapter::{kilocode, transcript};

/// xorshift64*: deterministic, no dependencies.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545F4914F6CDD1D)
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }

    fn pick<'a>(&mut self, xs: &'a [&'a str]) -> &'a str {
        xs[self.below(xs.len())]
    }
}

const KNOWN_KEYS: &[&str] = &[
    "type",
    "message",
    "content",
    "timestamp",
    "usage",
    "input_tokens",
    "output_tokens",
    "tool_use",
    "tool_result",
    "is_error",
    "role",
    "model",
    "cwd",
    "text",
    "toolUse",
    "skill",
    "command",
    "ts",
    "say",
    "error",
    "env",
    "sessionId",
];

const UNKNOWN_KEYS: &[&str] = &[
    "frobnicate",
    "x-new-field",
    "zzz",
    "metadata2",
    "unknown",
    "",
    "null",
    "123",
    "type!",
    "CoNtEnT",
];

const WEIRD_STRINGS: &[&str] = &[
    "",
    " ",
    "\n",
    "nul\0byte",
    "xylophone",
    "tool_result",
    "{not json",
    "[1,2",
    "9999999999999999999999",
    "-0.0",
    "∞",
    "yes",
];

fn gen_json(rng: &mut Rng, depth: usize) -> serde_json::Value {
    match rng.below(if depth == 0 { 4 } else { 7 }) {
        0 => serde_json::Value::Null,
        1 => serde_json::Value::Bool(rng.next() & 1 == 0),
        2 => serde_json::Value::Number([0i64, -1, i64::MAX, i64::MIN, 42][rng.below(5)].into()),
        3 => serde_json::Value::String(rng.pick(WEIRD_STRINGS).to_string()),
        4 if depth > 0 => {
            let mut arr = Vec::new();
            for _ in 0..rng.below(4) {
                arr.push(gen_json(rng, depth - 1));
            }
            serde_json::Value::Array(arr)
        }
        _ => {
            let mut map = serde_json::Map::new();
            for _ in 0..rng.below(4) {
                let key = if rng.next() & 1 == 0 {
                    rng.pick(KNOWN_KEYS)
                } else {
                    rng.pick(UNKNOWN_KEYS)
                };
                map.insert(key.to_string(), gen_json(rng, depth.saturating_sub(1)));
            }
            // Occasionally a huge string or deep nesting, on purpose.
            if rng.below(16) == 0 {
                map.insert(
                    "blob".to_string(),
                    serde_json::Value::String("x".repeat(100_000)),
                );
            }
            serde_json::Value::Object(map)
        }
    }
}

/// One known-good assistant cost line: must survive any amount of noise
/// around it (known records are ingested, noise is ignored).
const KNOWN_LINE: &str = r#"{"type":"assistant","timestamp":"2026-01-01T00:00:01.000Z","message":{"model":"m","usage":{"input_tokens":10,"cache_read_input_tokens":0,"cache_creation_input_tokens":0,"output_tokens":40}}}"#;

#[test]
fn arbitrary_jsonl_never_panics_and_keeps_known_records() {
    let mut rng = Rng(0x1234_5678_9abc_def0);
    for _ in 0..512 {
        let mut lines = vec![KNOWN_LINE.to_string()];
        for _ in 0..rng.below(4) {
            lines.push(gen_json(&mut rng, 5).to_string());
        }
        // Deeply nested single line, on purpose.
        if rng.below(8) == 0 {
            let mut deep = serde_json::Value::String("x".into());
            for _ in 0..12 {
                deep = serde_json::Value::Array(vec![deep]);
            }
            lines.push(deep.to_string());
        }
        let jsonl = lines.join("\n");
        // None of these may panic, whatever the shapes are.
        let records = transcript::parse_session(&jsonl);
        let _ = transcript::extract_prompt_pointers(&jsonl);
        let _ = transcript::extract_tool_errors(&jsonl);
        let _ = transcript::extract_work_events(&jsonl);
        // The known cost line is always found.
        let total = saniwalens::core::usage::output_tokens(&records);
        assert!(
            total >= 40,
            "the known assistant cost must survive noise: {jsonl:.200}"
        );
    }
}

#[test]
fn arbitrary_kilocode_envelopes_never_panic() {
    let mut rng = Rng(0x0fed_cba9_8765_4321);
    for _ in 0..512 {
        let conv =
            serde_json::Value::Array((0..rng.below(4)).map(|_| gen_json(&mut rng, 5)).collect())
                .to_string();
        let ui =
            serde_json::Value::Array((0..rng.below(4)).map(|_| gen_json(&mut rng, 4)).collect())
                .to_string();
        let _ = kilocode::parse_session(&conv, &ui);
        let _ = kilocode::extract_tool_errors(&conv);
        let _ = kilocode::extract_work_events(&conv);
    }
}

#[test]
fn an_unknown_denial_kind_falls_back_to_the_error_text() {
    // `toolDenialKind` names a future kind: the error must classify from its
    // text (edit-precondition here), not vanish into an unknown bucket.
    let jsonl = r#"{"type":"user","timestamp":"2026-01-01T00:00:00.000Z","toolDenialKind":"future-kind","message":{"content":[{"type":"tool_result","is_error":true,"content":"String to replace not found"}]}}"#;
    let errors = transcript::extract_tool_errors(jsonl);
    assert_eq!(errors.len(), 1);
    assert_eq!(
        errors[0].category,
        saniwalens::core::friction::ErrorCategory::EditPrecondition
    );
}
