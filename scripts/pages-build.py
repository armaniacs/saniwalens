#!/usr/bin/env python3
"""Generate GitHub Pages content from the repo's own docs.

Reads `docs/specs/*.md` and (optionally) the built `saniwalens` binary's
`--help` output, and writes `site/specs.html` / `site/cli.html` using the same
header/nav/footer shell as the hand-written pages (so the modern-web-guidance
patterns ― landmarks, popover nav, color-scheme handling ― carry over).

The generated files are committed: Pages deploys `site/` with no build step,
so what you see here is what ships. Re-run after editing specs or CLI help:

    python3 scripts/pages-build.py
"""

from __future__ import annotations

import html
import re
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SPECS_DIR = ROOT / "docs" / "specs"
SITE_DIR = ROOT / "site"
GITHUB_SPECS = "https://github.com/armaniacs/saniwalens/blob/main/docs/specs"

SPEC_FILES = [
    "architecture.md",
    "cli.md",
    "surfaces.md",
    "events.md",
    "storage.md",
    "harness.md",
    "session-format.md",
    "session-format-opencode.md",
    "session-format-kilocode.md",
    "config-format.md",
    "config-format-opencode.md",
    "config-format-kilocode.md",
]

NAV_BASE_EN = [
    ("index.html", "Home"),
    ("docs.html", "Docs"),
    ("specs.html", "Specs"),
    ("cli.html", "CLI"),
    ("https://github.com/armaniacs/saniwalens", "GitHub"),
]


def nav_html(items: list[tuple[str, str]], base: str, current: str | None) -> str:
    lis = []
    for path, label in items:
        href = path if "://" in path else f"{base}/{path}"
        aria = ' aria-current="page"' if label == current else ""
        lis.append(f'          <li><a href="{href}"{aria}>{html.escape(label)}</a></li>')
    return "\n".join(lis)


def page_shell(
    *,
    title: str,
    desc: str,
    body: str,
    base: str = ".",
    current: str | None = None,
) -> str:
    return f"""<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1.0">
  <title>{html.escape(title)} ― saniwalens</title>
  <meta name="description" content="{html.escape(desc)}">
  <meta name="color-scheme" content="light dark">
  <script>
    try {{
      document.querySelector('meta[name="color-scheme"]').content =
        localStorage.getItem("color-scheme") ?? "light dark";
    }} catch (e) {{}}
  </script>
  <link rel="icon" type="image/svg+xml" href="{base}/assets/favicon.svg">
  <meta name="theme-color" content="#1a1a24">
  <link rel="alternate" hreflang="ja" href="{base}/index_ja.html">
  <link rel="stylesheet" href="{base}/styles.css">
  <script defer src="{base}/app.js"></script>
</head>
<body>
  <a class="skip-link" href="#main">Skip to content</a>

  <header class="site-header">
    <div class="header-inner">
      <a class="site-logo" href="{base}/index.html" aria-label="saniwalens home"><img src="{base}/assets/favicon.svg" alt="" aria-hidden="true" width="28" height="28">saniwalens</a>

      <nav class="site-nav" aria-label="Primary">
        <button
          class="menu-button"
          type="button"
          popovertarget="site-menu"
          popovertargetaction="toggle"
          aria-controls="site-menu"
        >
          <svg width="16" height="16" viewBox="0 0 16 16" aria-hidden="true" focusable="false">
            <path d="M2 4h12M2 8h12M2 12h12" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" fill="none"/>
          </svg>
          <span>Menu</span>
        </button>

        <ul id="site-menu" class="site-menu" popover="auto">
{nav_html(NAV_BASE_EN, base, current)}
        </ul>
      </nav>

      <a class="lang-toggle" href="{base}/index_ja.html" hreflang="ja" lang="ja">日本語</a>

      <button
        id="theme-toggle"
        class="theme-toggle"
        type="button"
        aria-live="polite"
      >
        <span class="theme-toggle-dark" aria-hidden="true"><svg width="14" height="14" viewBox="0 0 16 16" focusable="false"><path d="M14 10.2A6 6 0 0 1 5.8 2 6 6 0 1 0 14 10.2Z" fill="currentColor"/></svg></span>
        <span class="theme-toggle-light" aria-hidden="true"><svg width="14" height="14" viewBox="0 0 16 16" focusable="false"><circle cx="8" cy="8" r="3.2" fill="currentColor"/><g stroke="currentColor" stroke-width="1.6" stroke-linecap="round"><path d="M8 1v1.8M8 13.2V15M1 8h1.8M13.2 8H15M3 3l1.3 1.3M11.7 11.7 13 13M13 3l-1.3 1.3M4.3 11.7 3 13"/></g></svg></span>
        <span class="visually-hidden" id="theme-toggle-label">Switch to dark theme</span>
      </button>
    </div>
  </header>

  <main id="main">
{body}
  </main>

  <footer>
    <p>
      saniwalens ― MIT. Source on <a href="https://github.com/armaniacs/saniwalens">GitHub</a>.
    </p>
  </footer>
</body>
</html>
"""


def inline_md(text: str) -> str:
    """Inline markdown: `code`, **bold**, [links](...), leaving the rest escaped."""
    parts: list[str] = []
    # Split out inline code first so its contents are never linkified.
    tokens = re.split(r"(`[^`]+`)", text)
    for i, tok in enumerate(tokens):
        if i % 2 == 1:
            parts.append(f"<code>{html.escape(tok[1:-1])}</code>")
            continue
        seg = html.escape(tok)
        seg = re.sub(r"\[([^\]]+)\]\(([^)\s]+)(?:\s+\"[^\"]*\")?\)", _link_repl, seg)
        seg = re.sub(r"\*\*([^*]+)\*\*", r"<strong>\1</strong>", seg)
        parts.append(seg)
    return "".join(parts)


SPEC_STEMS = {name[:-3] for name in SPEC_FILES}

# docs-charset 置換表のうち本文に出るもの。コードブロックは対象外にする
# (生成物を --fix しても再生成で戻るため、生成時に正規化する)。
PROSE_CHAR_MAP = str.maketrans({"\u2014": "\u2015", "\u2013": "-", "\u2194": "\u21d4", "\u2195": "\u21d4", "\u2248": "\u2252"})


def normalize_prose(fragment: str) -> str:
    """Apply PROSE_CHAR_MAP outside <pre>…</pre> regions."""
    parts = re.split(r"(<pre>.*?</pre>)", fragment, flags=re.DOTALL)
    return "".join(
        p if p.startswith("<pre>") else p.translate(PROSE_CHAR_MAP) for p in parts
    )


def _link_repl(m: re.Match[str]) -> str:
    label, href = m.group(1), m.group(2)
    if href.endswith(".md") and "://" not in href:
        stem = href.split("/")[-1][:-3]
        if stem in SPEC_STEMS:
            # Called from site/specs/<stem>.html: siblings live side by side.
            href = f"{stem}.html"
    elif href.startswith("docs/"):
        href = f"https://github.com/armaniacs/saniwalens/blob/main/{href}"
    return f'<a href="{html.escape(href)}">{label}</a>'


def md_to_html(src: str, *, demote: int = 1, section_id: str = "") -> str:
    """A small markdown subset: headings, fences, tables, lists, quotes, rules."""
    lines = src.splitlines()
    out: list[str] = []
    i = 0
    in_code = False
    code_lang = ""

    def close_lists(stack: list[str]) -> None:
        while stack:
            out.append(f"</{stack.pop()}>")

    list_stack: list[str] = []
    in_quote = False

    while i < len(lines):
        line = lines[i]
        stripped = line.strip()

        fence = re.match(r"^```(\S*)\s*$", stripped)
        if fence:
            if not in_code:
                in_code = True
                code_lang = fence.group(1)
                close_lists(list_stack)
                if in_quote:
                    out.append("</blockquote>")
                    in_quote = False
                cls = f' class="language-{html.escape(code_lang)}"' if code_lang else ""
                out.append(f"<pre><code{cls}>")
            else:
                in_code = False
                out.append("</code></pre>")
            i += 1
            continue
        if in_code:
            out.append(html.escape(line))
            out.append("\n")
            i += 1
            continue

        if not stripped:
            close_lists(list_stack)
            if in_quote:
                out.append("</blockquote>")
                in_quote = False
            i += 1
            continue

        if re.match(r"^---+\s*$", stripped):
            close_lists(list_stack)
            out.append("<hr>")
            i += 1
            continue

        hm = re.match(r"^(#{1,6})\s+(.*)$", stripped)
        if hm:
            close_lists(list_stack)
            level = min(6, len(hm.group(1)) + demote)
            out.append(f"<h{level}>{inline_md(hm.group(2))}</h{level}>")
            i += 1
            continue

        if stripped.startswith("|") and i + 1 < len(lines) and re.match(
            r"^\|?[\s:|-]+\|?\s*$", lines[i + 1].strip()
        ):
            close_lists(list_stack)
            if in_quote:
                out.append("</blockquote>")
                in_quote = False
            headers = [c.strip() for c in stripped.strip("|").split("|")]
            out.append("<div class=\"table-scroll\" role=\"region\" tabindex=\"0\">")
            out.append("<table><thead><tr>")
            out.extend(f"<th scope=\"col\">{inline_md(h)}</th>" for h in headers)
            out.append("</tr></thead><tbody>")
            i += 2
            while i < len(lines) and lines[i].strip().startswith("|"):
                cells = [c.strip() for c in lines[i].strip().strip("|").split("|")]
                out.append("<tr>")
                for j, cell in enumerate(cells):
                    tag = "th scope=\"row\"" if j == 0 else "td"
                    out.append(f"<{tag}>{inline_md(cell)}</{tag}>")
                out.append("</tr>")
                i += 1
            out.append("</tbody></table></div>")
            continue

        lm = re.match(r"^(\s*)([-*]|\d+[.)])\s+(.*)$", line)
        if lm:
            tag = "ul" if lm.group(2) in ("-", "*") else "ol"
            if not list_stack or list_stack[-1] != tag:
                close_lists(list_stack)
                out.append(f"<{tag}>")
                list_stack.append(tag)
            out.append(f"<li>{inline_md(lm.group(3))}</li>")
            i += 1
            continue

        if stripped.startswith(">"):
            close_lists(list_stack)
            if not in_quote:
                out.append("<blockquote>")
                in_quote = True
            out.append(f"<p>{inline_md(stripped[1:].strip())}</p>")
            i += 1
            continue

        close_lists(list_stack)
        if in_quote:
            out.append("</blockquote>")
            in_quote = False
        out.append(f"<p>{inline_md(stripped)}</p>")
        i += 1

    close_lists(list_stack)
    if in_quote:
        out.append("</blockquote>")
    if in_code:
        out.append("</code></pre>")
    return "\n".join(out)


def spec_source(name: str) -> tuple[str, str]:
    """(title-markdown, body-markdown) with the leading `# ` line lifted out."""
    src = (SPECS_DIR / name).read_text(encoding="utf-8")
    lines = src.splitlines()
    if lines and lines[0].startswith("# "):
        return lines[0][2:].strip(), "\n".join(lines[1:])
    return name[:-3], src


def build_specs() -> None:
    specs_dir = SITE_DIR / "specs"
    specs_dir.mkdir(exist_ok=True)
    infos = [(name, *spec_source(name)) for name in SPEC_FILES]

    # Index: one h1, then the table of contents.
    toc = ["    <ul>"]
    for name, title_md, _ in infos:
        stem = name[:-3]
        toc.append(
            f'      <li><a href="./specs/{stem}.html"><code>{stem}.md</code> ― '
            f"{inline_md(title_md).translate(PROSE_CHAR_MAP)}</a></li>"
        )
    toc.append("    </ul>")
    index_body = (
        "    <h1>Design specs</h1>\n"
        "    <p class=\"lead\">The design intent: the <em>why</em> and the contract. "
        "Each spec has its own page; the files on "
        f"<a href=\"{GITHUB_SPECS}\">GitHub</a> are the record.</p>\n"
        "    <nav aria-label=\"Specs\">\n" + "\n".join(toc) + "\n    </nav>\n"
    )
    (SITE_DIR / "specs.html").write_text(
        page_shell(
            title="Design specs",
            desc="saniwalens design specs: the why and the contract, mirrored from docs/specs/.",
            body=index_body,
            current="Specs",
        ),
        encoding="utf-8",
    )

    # One page per spec, with prev / index / next navigation.
    for i, (name, title_md, body_md) in enumerate(infos):
        stem = name[:-3]
        prev_link = next_link = ""
        if i > 0:
            p_name, p_title, _ = infos[i - 1]
            prev_link = (
                f'      <li><a rel="prev" href="./{p_name[:-3]}.html">'
                f"← {inline_md(p_title).translate(PROSE_CHAR_MAP)}</a></li>\n"
            )
        if i < len(infos) - 1:
            n_name, n_title, _ = infos[i + 1]
            next_link = (
                f'      <li><a rel="next" href="./{n_name[:-3]}.html">'
                f"{inline_md(n_title).translate(PROSE_CHAR_MAP)} →</a></li>\n"
            )
        body = (
            f"    <h1>{inline_md(title_md).translate(PROSE_CHAR_MAP)}</h1>\n"
            + normalize_prose(md_to_html(body_md, demote=0))
            + "\n    <nav aria-label=\"Spec pages\">\n"
            + '      <ul class="prev-next">\n'
            + prev_link
            + '      <li><a href="../specs.html">Specs index</a></li>\n'
            + next_link
            + "      </ul>\n    </nav>\n"
            + f'    <p><a href="{GITHUB_SPECS}/{name}">Source: {name}</a></p>\n'
        )
        (specs_dir / f"{stem}.html").write_text(
            page_shell(
                title=re.sub(r"[`*_]", "", title_md).translate(PROSE_CHAR_MAP),
                desc=f"saniwalens spec {name}, mirrored from docs/specs/.",
                body=body,
                base="..",
                current="Specs",
            ),
            encoding="utf-8",
        )
    print(f"wrote site/specs.html + {len(infos)} spec pages")


def find_binary() -> Path | None:
    for cand in (ROOT / "target" / "debug" / "saniwalens", ROOT / "target" / "release" / "saniwalens"):
        if cand.exists():
            return cand
    return None


def help_text(binary: Path, args: list[str]) -> str:
    try:
        out = subprocess.run(
            [str(binary), *args], capture_output=True, text=True, timeout=30
        )
        return out.stdout or out.stderr
    except (OSError, subprocess.SubprocessError) as exc:
        return f"(unavailable: {exc})"


def build_cli() -> None:
    binary = find_binary()
    if binary is None:
        print("no binary found; skipping site/cli.html (build first, then re-run)")
        return
    top = help_text(binary, ["--help"])
    # Subcommands listed under a "Commands:" section of clap's help.
    subs: list[str] = []
    in_cmds = False
    for line in top.splitlines():
        if line.strip().lower().startswith("commands:"):
            in_cmds = True
            continue
        if in_cmds:
            if not line.strip():
                break
            m = re.match(r"^\s{2}(\S+)", line)
            if m and m.group(1) not in ("help",):
                subs.append(m.group(1).rstrip(","))
    blocks = [("saniwalens --help", top)]
    for sub in subs:
        blocks.append((f"saniwalens {sub} --help", help_text(binary, [sub, "--help"])))
    figs = []
    for n, (cmd, text) in enumerate(blocks):
        figs.append(
            f'    <figure>\n      <figcaption id="cli-{n}"><code>{html.escape(cmd)}</code></figcaption>\n'
            f'      <pre role="region" aria-labelledby="cli-{n}" tabindex="0"><code>{html.escape(text.rstrip())}</code></pre>\n'
            "    </figure>"
        )
    body = (
        "    <h1>CLI reference</h1>\n"
        "    <p class=\"lead\">Generated from the binary's own <code>--help</code> "
        f"output. Re-run <code>python3 scripts/pages-build.py</code> after CLI changes. "
        "The <a href=\"./docs.html\">docs page</a> explains the pipeline and views.</p>\n"
        + "\n".join(figs)
    )
    (SITE_DIR / "cli.html").write_text(
        page_shell(
            title="CLI reference",
            desc="saniwalens CLI reference, generated from --help output.",
            body=body,
            current="CLI",
        ),
        encoding="utf-8",
    )
    print(f"wrote site/cli.html ({len(blocks)} commands)")


if __name__ == "__main__":
    SITE_DIR.mkdir(exist_ok=True)
    build_specs()
    build_cli()
