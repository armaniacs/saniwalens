#!/usr/bin/env python3
"""PR title convention lint (PBI 11): type required, lowercase start, no
trailing Japanese/ASCII period, 70 chars max, free-form scope allowed.

Mirrors `.claude/rules/commit-convention.md`. Usage:
`commit_title_lint.py "<PR title>"` — exits 0 when the title conforms,
1 with the reason on stderr when it does not.
"""

import re
import sys

ALLOWED_TYPES = (
    "feat",
    "fix",
    "refactor",
    "perf",
    "test",
    "docs",
    "build",
    "ci",
    "chore",
)

TITLE_RE = re.compile(
    r"^(?P<type>[A-Za-z]+)(?P<scope>\([^)]*\))?(?P<bang>!)?: (?P<subject>.+)$"
)


def check(title: str) -> str | None:
    """Return the rejection reason, or None when the title conforms."""
    if len(title) > 70:
        return f"too long ({len(title)} > 70 chars)"
    m = TITLE_RE.match(title)
    if not m:
        return "must look like `type: subject` or `type(scope): subject`"
    kind = m.group("type")
    if kind not in ALLOWED_TYPES:
        return f"unknown type {kind!r} (want one of {', '.join(ALLOWED_TYPES)})"
    if kind != kind.lower():
        return f"type must be lowercase, got {kind!r}"
    subject = m.group("subject")
    if not subject or not subject[0].islower():
        return "subject must start lowercase"
    if subject[-1] in ".。．":
        return "subject must not end with a period"
    return None


def main(argv: list[str]) -> int:
    if len(argv) != 2 or not argv[1].strip():
        print("usage: commit_title_lint.py \"<PR title>\"", file=sys.stderr)
        return 1
    reason = check(argv[1])
    if reason is None:
        return 0
    print(f"bad PR title: {reason}: {argv[1]!r}", file=sys.stderr)
    return 1


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
