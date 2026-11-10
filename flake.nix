{
  description = "saniwalens - a lens onto your coding-agent usage";

  # Advertise the shared Cachix cache so `nix run github:armaniacs/saniwalens`
  # pulls prebuilt artifacts instead of compiling from source. Trusted users
  # get it automatically; others are prompted to accept it on first use.
  nixConfig = {
    extra-substituters = [ "https://saniwalens.cachix.org" ];
    extra-trusted-public-keys = [
      "saniwalens.cachix.org-1:0QUNU6PuVyf+yXOvg3n1rd3FksBoB3s3/Jty50iKRNQ="
    ];
  };

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    crane.url = "github:ipetkov/crane";
    nix-github-release = {
      url = "github:lambdalisue/nix-github-release/v0.1.0";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { self, nixpkgs, flake-utils, rust-overlay, crane, nix-github-release }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        overlays = [ (import rust-overlay) ];
        pkgs = import nixpkgs { inherit system overlays; };

        # Single source of the Rust version, shared by the dev shell and the
        # package build (rust-toolchain.toml).
        rustToolchain = pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;
        craneLib = (crane.mkLib pkgs).overrideToolchain rustToolchain;

        # `cleanCargoSource` keeps only Cargo inputs, but the prose reference
        # gate (`tests/prose_reference_gate.rs`) reads README, AGENTS, and the
        # current specs, and asserts that the history trees still exist. Keep
        # the prose trees (docs, pbi, .claude) plus their root files so the
        # package build runs the same gate as CI.
        proseSources = path: type:
          let
            root = toString ./.;
            rel = pkgs.lib.removePrefix (root + "/") (toString path);
          in
          craneLib.filterCargoSources path type
          || rel == "README.md"
          || rel == "AGENTS.md"
          || rel == "CHANGELOG.md"
          || rel == "docs"
          || pkgs.lib.hasPrefix "docs/" rel
          || rel == "pbi"
          || pkgs.lib.hasPrefix "pbi/" rel
          || rel == ".claude"
          || pkgs.lib.hasPrefix ".claude/" rel
          # Goldens are read at runtime by the e2e tests
          # (`tests/e2e_golden_harness.rs`, `tests/render_view_golden.rs`,
          # `tests/mod_split_views_golden.rs`), so the package build - which
          # runs those tests - needs the tree, not just Cargo sources.
          || rel == "tests/golden"
          || pkgs.lib.hasPrefix "tests/golden/" rel;

        commonArgs = {
          src = pkgs.lib.cleanSourceWith {
            src = ./.;
            filter = proseSources;
          };
          strictDeps = true;
          # rusqlite is built with the `bundled` feature, so SQLite compiles from
          # C against the stdenv C toolchain - no system sqlite/pkg-config needed.
        };
        cargoArtifacts = craneLib.buildDepsOnly commonArgs;

        # The tag this flake was fetched at, from VERSION; see
        # github:lambdalisue/nix-github-release. Only the package is stamped
        # with it, so the dependency layer above stays cached across releases.
        version = nix-github-release.lib.version { inherit self; };
        saniwalens = craneLib.buildPackage (nix-github-release.lib.stampCargo { inherit pkgs version; }
          (commonArgs // { inherit cargoArtifacts; }));
      in
      {
        # `nix build` / `nix run github:armaniacs/saniwalens -- summary`; pin a
        # release with `github:armaniacs/saniwalens/vX.Y.Z`.
        packages.default = saniwalens;
        apps.default = flake-utils.lib.mkApp { drv = saniwalens; };

        # `nix develop` - the pinned tools, same ones CI uses via `just`.
        devShells.default = pkgs.mkShell {
          packages = with pkgs; [
            rustToolchain
            just
            git
            jq
            sqlite # the `sqlite3` CLI, handy for poking at the store
            nixpkgs-fmt
          ];
          env.RUST_BACKTRACE = "1";
          shellHook = ''
            echo "saniwalens dev shell - $(rustc --version)"
          '';
        };

        formatter = pkgs.nixpkgs-fmt;
      });
}
