# CONTRIBUTING

<!-- README-I18N:START -->

**English** | [日本語](./CONTRIBUTING.ja.md)

<!-- README-I18N:END -->

How to contribute to saniwalens, and what to bring when proposing support for a
new agent (harness).

## Project conventions

saniwalens is built spec-driven and test-driven. Please take in these four
points before writing code; `AGENTS.md` and `.claude/rules/` have the details.

- **Spec first.** `docs/specs/` holds the design intent (the why and the
  contract). When you change behavior, update the owning spec in the same
  change. The mapping is in `.claude/rules/spec-sync.md`.
- **Test first.** Red, then green, t-wada style. Keep the analysis core
  (records to events to aggregation) pure and I/O-free so it stays unit-testable
  against small synthetic fixtures (`.claude/rules/tdd.md`).
- **Privacy is a hard constraint.** Real transcripts and config can contain
  personal data and secrets. Fixtures are synthetic only; never commit
  harvested data or generated databases
  (`.claude/rules/session-data-privacy.md`).
- **PBI first for user-facing work.** Write a
  `pbi/YYYY-MM-DD-NN-<type>-<slug>.md` before implementing, and implement
  through subagents. See `AGENTS.md`.

## Development environment

```sh
nix develop          # dev shell with pinned rust, just, sqlite, ...
just                 # list tasks
just check           # rustfmt --check + clippy (the CI gate)
just test            # cargo test
just build           # release binary at target/release/saniwalens
```

`make` runs the same tasks without nix or just (`make install` installs to
`~/.local/bin`; override with `PREFIX=/usr/local`). Plain Cargo also works
(`cargo build --release`). CI runs `nix develop -c just check` and `just test`.

## Issues and PRs

- **Bug reports:** include the `--harness` you used, the command that
  reproduces it, and the expected versus actual output. Replace real
  transcripts and config with a minimal synthetic example.
- **Feature proposals:** open an issue first. For a new harness, read the next
  section.
- **PRs:** keep them small and focused on one concern, with spec updates and
  tests included.

## Proposing a new harness

The maintainer started on Claude Code, then added Kilo Code in VS Code, moved
to an OpenAI-compatible endpoint (Sakura AI Engine) and then to Opencode Go.
The daily driver is now opencode v2, and the DeepSeek harness app (DSH) is the
next candidate. Many agents are ones the maintainer does not use, so
suggestions are welcome. Open an issue with:

- The agent's name and version (for example opencode v2, Kilo Code, the DSH app).
- Where session records live and their format (paths, JSONL or DB, what
  changes between versions).
- Where config lives and its format (skills, rules, agents, MCP, memory, and
  whether there is an `AGENTS.md` equivalent).
- What recurring cost you feel on that agent (always-loaded volume, heavy
  definitions, and the like).

Whether an agent accepts an OpenAI-compatible endpoint does not decide support.
A new harness needs three things:

1. **Observable formats.** How to read session records and config must be
   knowable without your machine: documentation, or a description with
   reproduction steps.
2. **OSS-grounded reproducibility.** Fixtures must be synthesizable, committable,
   and runnable in CI. Formats that cannot be verified or maintained without a
   closed binary or an account are out of reach.
3. **A maintenance outlook.** Upstream format changes must be trackable. Formats
   that break often cannot be carried by adapter-layer isolation alone
   (`docs/specs/architecture.md`, `.claude/rules/format-isolation.md`).

Closed-source agents, or ones without an OpenAI-compatible endpoint, often
cannot supply all three, which makes support difficult. But the endpoint type
or the license alone is never a reason to decline. Start with an issue to
gather the materials, beginning with how to build fixtures.

The technical acceptance criteria are in the Forward compatibility section of
`docs/specs/harness.md`. A new harness lands as a `Harness` variant, its
default input root and store name, a format spec, and an adapter module with an
analyzer-version key, all behind `Harness::analyze`. The Claude Code path does
not change.

## Releasing (maintainer note)

Version numbers live only in tags. The `0.0.0` in `Cargo.toml` is a placeholder
the release workflow rewrites from the tag. Roll the CHANGELOG's Unreleased
section into the version, confirm that the README, specs, and `--help` wording
agree, then cut the tag.

Publishing a GitHub Release for a `vX.Y.Z` tag then:

- builds binaries for Linux, macOS, and Windows (x86_64 and arm64 each) and
  attaches them with `.sha256` files;
- points the
  [`armaniacs/homebrew-saniwalens`](https://github.com/armaniacs/homebrew-saniwalens)
  tap at the new archives (the tap covers Linux and macOS only);
- publishes the crate to crates.io.

The release is published before its assets are built, so the notes are visible
for a few minutes with nothing attached. The flake stamps the version without
being told the tag: `VERSION` is marked `export-subst`, so the archive GitHub
serves for the tag carries it (see
[nix-github-release](https://github.com/lambdalisue/nix-github-release)).
