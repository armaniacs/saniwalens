# saniwalens — Make wrapper around Cargo.
#
# Plain `cargo` also works (`cargo build --release`); the pinned toolchain and
# the `just` recipes live in the Nix flake / justfile. This Makefile exists so
# the familiar `make build`, `make test`, `make install` spellings work with
# nothing but a recent stable Rust and a C compiler (bundled SQLite) installed.
#
# Examples:
#   make build                          # release binary at target/release/saniwalens
#   make test                           # run the test suite
#   make install                        # install to ~/.local/bin (no sudo needed)
#   make install PREFIX=/usr/local     # install to /usr/local/bin
#   make install DESTDIR=/tmp/pkg PREFIX=/usr  # staged install for packaging
#
# Variables (all overridable on the command line):
#   PREFIX   install prefix (default: ~/.local)
#   BINDIR   binary directory (default: $(PREFIX)/bin)
#   DESTDIR  staging root prepended to BINDIR (default: empty)
#   CARGO    cargo binary (default: cargo; extra flags via CARGOFLAGS)

PREFIX  ?= $(HOME)/.local
BINDIR  ?= $(PREFIX)/bin
DESTDIR ?=
CARGO   ?= cargo
CARGOFLAGS ?=

BIN         := saniwalens
RELEASE_BIN := target/release/$(BIN)

.DEFAULT_GOAL := help

.PHONY: help build test install uninstall check fmt clean

help: ## Show this help
	@grep -E '^[a-zA-Z_-]+:.*?## ' $(MAKEFILE_LIST) | sort | awk 'BEGIN {FS = ":.*?## "}; {printf "  \033[36m%-10s\033[0m %s\n", $$1, $$2}'

build: ## Build the release binary (target/release/saniwalens)
	$(CARGO) build --release $(CARGOFLAGS)

test: ## Run the test suite
	$(CARGO) test $(CARGOFLAGS)

install: build ## Install the release binary into $(DESTDIR)$(BINDIR)
	mkdir -p "$(DESTDIR)$(BINDIR)"
	install -m 755 "$(RELEASE_BIN)" "$(DESTDIR)$(BINDIR)/$(BIN)"

uninstall: ## Remove the installed binary from $(DESTDIR)$(BINDIR)
	rm -f "$(DESTDIR)$(BINDIR)/$(BIN)"

check: ## Lint + format check (the CI gate): rustfmt --check, then clippy
	$(CARGO) fmt --check
	$(CARGO) clippy --all-targets --all-features -- -D warnings

fmt: ## Format all Rust code in place
	$(CARGO) fmt $(CARGOFLAGS)

clean: ## Remove build artifacts
	$(CARGO) clean
