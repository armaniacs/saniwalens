# saniwalens task runner. Run inside the Nix dev shell so rustc / clippy / cargo
# resolve to the pinned versions:  nix develop -c just <recipe>
# (or `nix develop` once, then `just <recipe>`).

# default: list the recipes
default:
    @just --list

# lint + format check (the CI gate): rustfmt --check, then clippy with warnings denied
check:
    cargo fmt --check
    cargo clippy --all-targets --all-features -- -D warnings

# run the test suite
test:
    cargo test

# format all Rust in place
fmt:
    cargo fmt

# build the release binary
build:
    cargo build --release

# print a CHANGELOG section draft for a git range (PBI 26):
# `just release-notes v0.5.4..HEAD`. Draft only — the maintainer pastes,
# edits, and commits by hand (CHANGELOG.md stays hand-written).
# Requires git-cliff (not in the dev shell): `cargo install git-cliff`.
release-notes range:
    git-cliff --config cliff.toml {{range}}
