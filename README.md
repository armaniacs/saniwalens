# saniwalens

**English** | [日本語](./README.ja.md)

A lens onto your coding-agent usage. `saniwalens` reads your agent's session
transcripts and live configuration, loads them into a local SQLite store, and
shows **where time, tokens, and effort are wasted ― and how to fix it**: unused
or heavy config, recurring tool failures, places where work gets stuck, and the
cost picture behind them.

The design intent lives in [`docs/specs/`](docs/specs/); this README covers how
to run it.

## Install

```sh
brew install armaniacs/saniwalens/saniwalens   # macOS / Linux
cargo install saniwalens                       # from crates.io; needs a C compiler for bundled SQLite
nix run github:armaniacs/saniwalens -- doctor  # try it without installing
nix profile install github:armaniacs/saniwalens  # put it on PATH (append /vX.Y.Z to pin a release)
```

On Windows, download the `-pc-windows-msvc` archive for your architecture from
the GitHub Release, or build from source. For building from source and the dev
tasks, see [`CONTRIBUTING.md`](CONTRIBUTING.md).

## Quick start

```sh
# 1. Start here: a one-screen health check, split into global (~/.claude)
#    and per-project findings. The first run analyzes automatically.
saniwalens doctor

# 2. Narrow it to the layer you are tuning.
saniwalens doctor --scope global
saniwalens doctor --scope project:<slug>

# 3. Act on the findings: an advisor session seeded with the full analysis
#    investigates each root cause and proposes fixes.
saniwalens optimize --scope global
```

On an interactive terminal, plain `saniwalens` (or `saniwalens tui`) asks for
the same choices step by step; see [Guided entry](#guided-entry).

### What it reads and where it writes

Analysis is **read-only** and incremental. It reads transcripts
(`~/.claude/projects`) and live config (`~/.claude/*`, plus each project's
`.claude/` and `CLAUDE.md`), and writes only the store. Nothing leaves your
machine. If `CLAUDE_CONFIG_DIR` is set to a non-empty path, it replaces
`~/.claude`, as in Claude Code itself.

The store is a single per-user file, because the analysis spans every project
rather than the directory you run from. The default is
`${XDG_STATE_HOME:-~/.local/state}/saniwalens/saniwalens.db`; every command
accepts `--db <path>` to override it (gitignore it if you keep it in a project).

Read commands refresh the store before reporting, skipping unchanged
transcripts, and print a freshness line on stderr. Pass `--frozen` to read the
store as it is. Run `saniwalens analyze` yourself only to refresh without
reading.

## Commands

### Pipeline

| Command | What it does |
| --- | --- |
| `saniwalens doctor` | Health check, split global / per-project. **Start here.** |
| `saniwalens analyze` | Load transcripts and config into the store (incremental). Read commands run it automatically. |
| `saniwalens sql '<query>'` | Run any read-only SQL against the store. |
| `saniwalens optimize` | Hand the findings to an advisor session to act on. |
| `saniwalens cleanup` | List (`--list`, the default) or prune (`--keep [N]`, default 10) saved `--apply` plans and backups. Needs no store. |

### Guided entry

`saniwalens` with no subcommand on an interactive terminal (or `saniwalens
tui`) asks, in order:

1. Which harness (`claude`, `opencode`, `kilocode`). `--harness` or `--profile` skips this.
2. What to look at (`doctor`, `waste`, `failures`).
3. Which scope.

It prints the equivalent command before running it (for example `saniwalens
--harness kilocode waste --scope global`) and again afterwards, so you can
repeat the run without the guide. After the report it offers to run `optimize`
for the same harness and scope; a failure there does not stop the guide. On a
headless harness that command carries `--apply`. Finally it offers to save the
harness to `config.toml` as `default_harness` or as a named profile, and asks
before writing.

### Scope

`--scope global | project | project:<slug>` narrows `doctor`, `failures`,
`waste`, `inventory`, and `optimize` to the layer you are tuning. Unused or
heavy config is routed by where its file is installed. A recurring failure
belongs to the project that owns a strict majority of it; otherwise it is a
cross-project habit and counts as global.

### Views

Each view carries logic a raw query cannot (a classification, an algorithm, a
suggestion). Add `--format markdown` to paste into a PR or note, or `--format
json` for typed output on every command. With JSON, stdout carries only the
JSON; freshness messages go to stderr. Terminal output is colored unless
`NO_COLOR` is set or the output is piped.

```sh
saniwalens waste                 # ranked optimization opportunities, with a suggested action
saniwalens inventory             # every installed surface (with scope) × its actual usage
saniwalens overhead              # always-on context per session, reconciled with your config
saniwalens usage                 # per-skill usage, most-invoked first
saniwalens usage --by month      # per time bucket (JST: year|month|week|day|hour)
saniwalens failures              # recurring tool failures by category, with fixes
saniwalens prompts               # how you steer sessions (steer/correct/question/instruct)
saniwalens stuck                 # files the agent kept re-editing in rapid bursts
```

### `sql`: query the store directly

For anything the views don't cover, pass a query as an argument or on stdin.
The database is opened read-only, so a query cannot change the store.

```sh
saniwalens sql "SELECT category, tool, COUNT(*) n FROM tool_errors GROUP BY 1,2 ORDER BY n DESC"
echo "SELECT excerpt FROM tool_errors WHERE category='path-not-found'" | saniwalens sql
saniwalens sql "SELECT sql FROM sqlite_master"   # discover the schema
```

The `tool_errors` view exposes `category`, `excerpt`, `tool`, and `project`.

### `optimize`

`optimize` launches the harness's own advisor, briefed with the analysis. The
briefing goes through a private temp file, never the command line.

- **Interactive harnesses** (`claude`; `kilo --prompt` under `--harness
  kilocode`): the session applies edits once you agree.
- **Headless harnesses** (opencode): the run prints a plan and changes nothing;
  its output begins with a line saying so. Add `--apply` to save the plan under
  `$XDG_STATE_HOME/saniwalens/plans/` and open the harness's interactive
  session on it. Each item shows its planned diff and asks `y` (apply), `n`
  (skip), or `q` (quit). Every target file is backed up first under
  `.../backups/`, and the session ends with a summary of applied versus
  proposal-only items.

Prune saved plans and backups with `saniwalens cleanup --keep [N]`. A bare
`saniwalens cleanup` only lists them.

## Supported harnesses

One run analyzes one harness, chosen with `--harness`. The default is `claude`,
so existing invocations are unchanged.

| `--harness` | Agent | Transcripts | Config catalog | Store | `optimize` launch |
| --- | --- | --- | --- | --- | --- |
| `claude` (default) | Claude Code | `~/.claude/projects` | skills, rules, agents, MCP servers, `CLAUDE.md` | `saniwalens.db` | interactive `claude` |
| `opencode` | opencode | opencode database (`~/.local/share/opencode/opencode.db`) | commands, agents, `AGENTS.md` | `saniwalens-opencode.db` | headless `opencode run` |
| `kilocode` | Kilo Code | editor `globalStorage` task files (Code / Cursor / VSCodium) | commands, agents, skills, `AGENTS.md`, `.kilorules`, `kilo.json` `instructions[]` | `saniwalens-kilocode.db` | interactive `kilo --prompt` |

```sh
saniwalens analyze --harness kilocode
saniwalens doctor --harness kilocode
saniwalens optimize --harness kilocode
```

Each harness has its own store and reads its own config (`~/.claude/`,
`~/.config/opencode/`, `~/.config/kilo/`, plus each project's files), so
findings never mix. There is deliberately no `auto`: with several harnesses
installed, guessing would misattribute rows silently.
[`docs/specs/harness.md`](docs/specs/harness.md) holds the per-harness contract,
including what each harness cannot report yet (for example, no subagent split
under opencode or kilocode, and no `prompts` view under opencode).

DSH (the DeepSeek harness app) and other harnesses are not supported; DSH is the
next candidate. [`CONTRIBUTING.md`](CONTRIBUTING.md) lists what a new harness
needs: an OpenAI-compatible endpoint alone is not enough, and the transcript and
config formats must be observable and reproducible with open source. Report
security issues through [`SECURITY.md`](SECURITY.md) (private advisories, never
public issues).

## Config file

`~/.config/saniwalens/config.toml` is optional; without it everything behaves
as above.

```toml
default_harness = "kilocode"
language = "ja"   # advisor output language (optimize only); report tables stay English

[profiles.work]
harness = "claude"
db = "work.db"   # relative to the config file's directory
```

`saniwalens --profile work doctor` runs under that profile. Explicit flags
always beat the file. `language` resolves as `--language`, then the file's
top-level `language`, then English. Only the `optimize` prompt uses it today;
report tables stay English. The full resolution order is in
[`docs/specs/cli.md`](docs/specs/cli.md).

## What it covers

- **Every configuration surface** (skills, rules, agents, MCP servers,
  `CLAUDE.md` / `AGENTS.md`), catalogued with startup and on-demand token cost
  and load mode, then joined against actual usage to flag unused (delete),
  always-on heavy (slim), and costly but rare (trim).
- **Where work stumbles**: recurring tool failures by category and originating
  tool (`failures`), and files the agent kept re-editing (`stuck`), each with
  examples and the project it concentrates in.
- **Where tokens go**: main-thread skill output versus subagent cost, with the
  subagent share split per agent type (nested spawns included), and the
  always-on context floor reconciled against your readable config. The residual
  is system prompt, tools, and MCP, which you cannot trim from files.
- **How you prompt**: the mix of steer, correct, question, and instruct.

## Notes

- Counts are usage signals for ranking, **not a billing ledger**. Static cost is
  a token estimate, not a measured runtime figure.
- **Do not delete the store.** It outlives your transcripts: Claude Code prunes
  those on its own schedule, while saniwalens keeps the rows it already
  extracted. That is why it lives under `${XDG_STATE_HOME:-~/.local/state}/`
  and not in a cache directory a cleaner may empty. Upgrades migrate the file in
  place.
- The store grows with every analyze, and the windowed views read all of it. The
  index that keeps reads fast is created on the next read-write open;
  `--frozen` and `sql` (read-only) never create it. Check the size with
  `saniwalens sql "SELECT COUNT(*) FROM events"`.
- Deliberately flat (see [`docs/specs/`](docs/specs/)): nesting of meta-skills
  such as `loop` is not inferred. Transcripts carry no signal separating nesting
  from sequencing, so each invocation stays its own span.
