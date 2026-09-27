# CLAUDE.md — rsmultigit

## What is this project?

A Rust CLI tool for managing multiple Git repositories at once. Reads the list of target repos from `~/.config/rsmultigit/config.toml` and runs bulk operations (status, pull, build, grep, check-same, etc.) across all of them. Rewrite of [pymultigit](https://github.com/veltzer/pymultigit) for native speed.

## Configuration

rsmultigit requires a config file at `~/.config/rsmultigit/config.toml`. Tests override this via the `RSMULTIGIT_CONFIG` env var. Run `rsmultigit config-example` to print a worked example; its source is `assets/config-example.toml` (embedded at compile time via `include_str!`).

- `repos = [...]` — list of shell-expanded globs. Matches that aren't git repos are filtered out.
- `default_build_method = "..."` — optional; what a bare `rsmultigit build` runs (same spellings as the CLI: `bootstrap`, `make`, `rsconstruct`, `cargo`, `cargo-publish`). Deserialized straight into `cli::BuildWhat`, so the CLI enum and the config key can never drift. An explicit method on the command line wins.
- `[[check]]` blocks — consumed by `check-same` (and `check-all`). Fields: `name`, `select`, `exclude?`, `marker?`, `marker_absent?` (drop repos containing this file — the in-repo opt-out, e.g. `.noci`), `path`, `enabled?` (default true), `must_have?` (default false; when true, in-scope repos missing `path` are violations).
- `[[exists]]` blocks — consumed by `check-exists` (and `check-all`). Presence-only: they assert every selected repo has `path`, and never compare content, which is what makes them right for files that legitimately differ per repo (README.md). Same selection fields as `[[check]]` (`name`, `select`, `exclude?`, `marker?`, `marker_absent?`, `path`, `enabled?`); no `must_have`, since requiring the file is the whole rule.

## Build & Test

```bash
cargo build                     # Debug build
cargo build --release           # Release build
cargo nextest run               # Run tests (preferred runner)
cargo nextest run --release     # Run tests in release mode
cargo nt                        # Alias for nextest run
make test                       # Runs nextest in both release and debug
```

Tests use `cargo-nextest` (not `cargo test`). Config in `.config/nextest.toml` (4 threads, fail-level reporting).

## Project Structure

```
src/
├── main.rs              # Entry point, command dispatch
├── cli.rs               # Clap derive CLI definitions (commands + global flags)
├── config.rs            # AppConfig: transforms CLI args to runtime config
├── runner.rs            # Three runner patterns for executing across repos
├── subprocess_utils.rs  # Shell command helpers (capture_output, check_call)
└── commands/            # Command modules (one per operation); `check.rs` owns config-file
                         # parsing and rule evaluation, `check_run.rs` the check-* drivers
tests/
├── main.rs              # Integration test entry
├── common/mod.rs        # Test helpers (setup_git_repos, run_rsmultigit)
└── tests_mod/           # Integration test modules
docs/                    # mdBook documentation
build.rs                 # Embeds git metadata at compile time
```

## Architecture — Three Runner Patterns

All commands use one of three patterns in `runner.rs`:

1. **`do_count`** — Boolean test per repo using git2 (no subprocess). Prints count summary. Used by: `count dirty/untracked/synchronized`.
2. **`do_for_all_projects`** — Runs an action (`Result<()>`) in each repo dir; the `_with_check` variant runs a cheap predicate first and skips repos where it is false. Skips are decided only there, never inside the action, because the serial path prints the header before the action runs. Used by: `pull, push, fetch, clean, build`, etc.
3. **`print_if_data`** — Calls data function returning `Option<String>`, prints only if Some. Used by: `status, dirty, grep, age, authors, size`.

## Key Conventions

- **Edition 2024** Rust
- **Error handling**: `anyhow::Result<T>` everywhere, with `.context()` for error messages
- **Git inspection**: Prefer the `git2` crate for everything libgit2 can do — subprocess startup times 260 repos dominates runtime (see "Git inspection" in `docs/src/architecture.md` for the recorded benchmark). Use the `git` CLI subprocess only for network ops (`pull`/`push`/`fetch`) and commands whose value is git's own output formatting (`log`, `blame`, `grep`).
- **Command module pattern**: Each command is a simple `pub fn` returning `Result<()>` (action), `Result<bool>` (predicate or count test) or `Result<Option<String>>` (data). A module that prints its own lines uses `subprocess_utils::out_line`, never `println!`, so the parallel runner and `--no-output` see them
- **No rustfmt.toml or clippy.toml** — uses Rust defaults
- **Release profile**: `strip = true`, `lto = true`
- **Tests**: Unit tests in `#[cfg(test)]` modules within source files. Integration tests in `tests/`. Use `tempfile::TempDir` for isolation; tests take explicit repo paths and never change the working directory.

## CI/CD

This repo uses the canonical `.github/workflows/ci.yml` shared byte-identically
by all rs* repos (canonical copy in rsconstruct — edit it there, not here; the
`rs-workflow-ci` rule in `check-same` guards against drift). The same applies
to `build.rs`, `release.toml`, `deny.toml`, `.config/nextest.toml`,
`scripts/ci-install-tools.sh` and `docs/src/release-info.md`.

- **Every branch push**: `cargo fmt --check`, build, clippy (`-D warnings`),
  `cargo deny check`, `cargo nextest run`.
- **Release**: triggered by a `chore: Release ...` commit (as written by
  `cargo release`) landing on the default branch — the `v*` tag push itself
  triggers nothing. Builds binaries for Linux x64/ARM64 and macOS x64/ARM64,
  named `rsmultigit-{linux,macos}-{x86_64,aarch64}`, and attaches them to a
  GitHub release for the tag computed from `Cargo.toml` (openssl is vendored
  unconditionally via the `git2` feature in Cargo.toml, so no `--features`
  flag is needed).
- **Docs**: mdBook deployed to GitHub Pages on release and on manual dispatch.
  The book lives in `docs/`; keep `docs/src/commands.md` and README.md in step
  with `cli.rs` when adding or renaming a subcommand or flag.

## Dependencies

Runtime deps — keep it minimal:
- `clap` (derive) — CLI parsing
- `clap_complete` — shell completions
- `git2` — native git operations
- `glob` — pattern matching
- `anyhow` — error handling
- `serde` + `toml` — config-file parsing
- `sha2` — SHA-256 hashing for `check-same`
- `shellexpand` — tilde/env expansion in config paths
- `regex-lite` — `--checks-re` rule matching
- `similar` — unified diffs for `check-same --diff`
- `camino` — UTF-8 paths throughout
- `indicatif` — progress bar in the parallel runner

Licenses and advisories are gated by the fleet-shared `deny.toml`.
