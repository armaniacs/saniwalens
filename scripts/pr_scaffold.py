#!/usr/bin/env python3
"""PR doc scaffold (PBI 17): generate docs/pr/<slug>.en.md and .ja.md in one
run. Never overwrites: if either file exists, it errors out and writes
nothing. Slug must be a plain file stem (letters, digits, `-`, `_`); anything
else ― empty, path separators, `..` ― is rejected so a bad argument cannot
escape docs/pr/.
"""

import re
import sys
from pathlib import Path

EN_TEMPLATE = """# {title}

## Background

<!-- why this change exists -->

## What changed

<!-- the change, file by file -->

## Verification

<!-- commands run and their results -->

```sh
just check
just test
```
"""

JA_TEMPLATE = """# {title}

## 背景

<!-- なぜこの変更が要るか -->

## 変更内容

<!-- 変更をファイル単位で -->

## 検証

<!-- 動かしたコマンドと結果 -->

```sh
just check
just test
```
"""

SLUG_RE = re.compile(r"[A-Za-z0-9][A-Za-z0-9_-]*")


def normalize_slug(raw: str) -> str:
    slug = raw.strip().replace(" ", "-").replace("_", "-")
    slug = re.sub(r"-{2,}", "-", slug)
    if not SLUG_RE.fullmatch(slug or ""):
        raise ValueError(
            f"bad slug {raw!r}: use letters, digits, '-' (e.g. my-feature)"
        )
    return slug


def scaffold(root: Path, raw_slug: str, title: str | None = None) -> tuple[Path, Path]:
    slug = normalize_slug(raw_slug)
    en = root / "docs" / "pr" / f"{slug}.en.md"
    ja = root / "docs" / "pr" / f"{slug}.ja.md"
    existing = [p for p in (en, ja) if p.exists()]
    if existing:
        raise FileExistsError(
            "refusing to overwrite: " + ", ".join(str(p) for p in existing)
        )
    heading = title or slug
    en.write_text(EN_TEMPLATE.format(title=heading), encoding="utf-8")
    ja.write_text(JA_TEMPLATE.format(title=heading), encoding="utf-8")
    return en, ja


def main(argv: list[str]) -> int:
    if len(argv) < 2 or not argv[1].strip():
        print(
            "usage: pr_scaffold.py <slug> [title]  (run from the repo root)",
            file=sys.stderr,
        )
        return 1
    try:
        en, ja = scaffold(
            Path.cwd(), argv[1], argv[2] if len(argv) > 2 else None
        )
    except (ValueError, FileExistsError, OSError) as e:
        print(f"pr_scaffold: {e}", file=sys.stderr)
        return 1
    print(f"wrote {en} and {ja}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
