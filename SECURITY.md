# Security Policy

## Supported versions

Only the latest release line receives security fixes. If you run an older
release, upgrade first and check whether the issue persists.

## Reporting a vulnerability

Do **not** open a public issue for a suspected vulnerability. Report it
privately through [GitHub Security
Advisories](https://github.com/armaniacs/saniwalens/security/advisories/new)
― click "Report a vulnerability" on that page. The report stays visible only
to you and the maintainers until a fix is ready.

Please include:

- What you did, and what you expected to happen instead.
- The saniwalens version (`saniwalens --version`) and the harness
  (`--harness claude | opencode | kilocode`).
- Whether real transcripts or real config are needed to reproduce it. If
  they are, **do not attach them as-is** ― replace personal data and secrets
  with a minimal synthetic example (see `CONTRIBUTING.md`, "Issues and PRs").

## What happens next

Maintainers acknowledge the report, reproduce it on synthetic data, and
publish a fix release with a CHANGELOG entry crediting the reporter (unless
you ask to stay anonymous). There is no bug bounty; disclosure waits for the
fix.

## Scope notes

saniwalens reads your agent transcripts and config into a local SQLite
store. It never sends your data anywhere ― there is no network code path in
the crate. A report showing data leaving the machine would be treated as
critical; a report about the contents of your own store on your own disk is
a local file-permissions matter, not a vulnerability in this tool.
