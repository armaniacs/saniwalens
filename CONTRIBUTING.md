# CONTRIBUTING

<!-- README-I18N:START -->

**English** | [日本語](./CONTRIBUTING.ja.md)

<!-- README-I18N:END -->

How to contribute to saniwalens, and what material to bring when proposing
support for a new agent (harness) in particular.

## Project conventions

saniwalens is built spec-driven and test-driven. Before writing code, please
take in the following three points. See `AGENTS.md` and `.claude/rules/` for
details.

- Spec first: `docs/specs/` holds the design intent (the why and the
  contract). When you change behavior, update the owning spec in the same
  change. The mapping lives in `.claude/rules/spec-sync.md`.
- Test first: red to green, t-wada style. Keep the analysis core (records to
  events to aggregation) pure and I/O-free so it stays unit-testable against
  small synthetic fixtures (`.claude/rules/tdd.md`).
- Privacy is a hard constraint: real transcripts and real config can contain
  personal data and secrets. Test fixtures are synthetic only; never commit
  harvested real data or generated databases
  (`.claude/rules/session-data-privacy.md`).
- PBI first for user-facing work: when proceeding the usual way ("make a
  PBI"), write a `pbi/YYYY-MM-DD-NN-<type>-<slug>.md` before implementing.
  Implementation is subagent-driven. See `AGENTS.md` for details.

## Development environment

```sh
nix develop          # 固定のrust、just、sqliteなどが入る開発シェル
just                 # タスク一覧
just check           # rustfmt --check + clippy(CIの関門)
just test            # cargo test
just build           # target/release/saniwalensにリリースバイナリ
```

Plain Cargo works too (`cargo build --release`). CI runs
`nix develop -c just check` and `just test`.

## Issues and PRs

- Bug reports: please include the `--harness` you used, the reproducing
  command, and the expected vs. actual output. Do not paste real transcripts
  or real config as they are; replace them with a minimal synthetic example.
- Feature proposals are welcome as an issue first. For a new harness in
  particular, read the section below.
- Keep PRs small and focused on one concern. Include spec updates and tests.

## Proposing a new harness

Disclosing the maintainer's usage first. Originally a Claude Code user, then
started using Kilo Code alongside VS Code, moved from kilo's free endpoints
to an OpenAI-compatible endpoint (Sakura AI engine), and then to Opencode Go.
The daily driver at the moment is opencode v2. Recently started using the
DeepSeek harness app, which is the next support candidate. There are many
agents the maintainer does not use, so recommendations for agents to support
are welcome. Please tell us in an issue, in this shape:

- Agent name and version (e.g. opencode v2, Kilo Code, DSH app)
- Where session records live and in what format (file paths, JSONL vs. DB,
  what changes per version)
- Where config files live and in what format (skills, rules, agents, MCP,
  memory, whether there is an `AGENTS.md` equivalent)
- What the recurring cost feels like on that agent (always-loaded volume,
  heavy definitions, and the like)

Support decisions are not made on endpoint compatibility. Whether an agent
accepts an OpenAI-compatible endpoint is not a support condition. What a new
harness needs is the following three:

1. Observable formats: how session records and config files are read must be
   knowable without the actual machine. Docs, or a description with
   reproduction steps, are needed.
2. OSS-grounded reproducibility: test fixtures must be synthesizable,
   committable to the repo, and runnable in CI. Formats that cannot be
   verified or maintained without a closed binary or an account are out of
   reach here.
3. A maintenance outlook: upstream format changes must be trackable. Formats
   that break often cannot be carried by adapter-layer isolation alone
   (`docs/specs/architecture.md`, `.claude/rules/format-isolation.md`).

For agents that are not OSS, or that do not accept an OpenAI-compatible
endpoint, the three materials above often cannot be assembled, which makes
support difficult. If the materials are there, the endpoint kind or the OSS
status alone will never be the reason to say no. Start with an issue to
gather the materials, beginning with how to build fixtures.

The technical acceptance criteria live in the Forward compatibility section
of `docs/specs/harness.md`. A new harness lands as a `Harness` variant, its
default input root and store name, a format spec, and an adapter module with
an analyzer-version key, behind `Harness::analyze`. The Claude Code path does
not move.

## A release-time note (for the maintainer)

Version numbers live only in tags. The `0.0.0` in `Cargo.toml` is a
placeholder the release workflow rewrites from the tag. Roll CHANGELOG's
Unreleased into the version, confirm README, spec, and `--help` wording
agree, then cut the tag.
