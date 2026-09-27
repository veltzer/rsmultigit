# RSMultiGit - Rust Multi-Git

A fast CLI tool for managing multiple git repositories at once, written in
Rust. RSMultiGit is a rewrite of [pymultigit](https://github.com/veltzer/pymultigit)
with native performance.

## Features

- **Configured repository list** — the repos to operate on come from glob
  patterns in `~/.config/rsmultigit/config.toml`, so every command sees the
  same fleet no matter where it is run from
- **Native git inspection** — status, dirty, untracked and sync checks use
  libgit2 in-process, avoiding a `git` subprocess per repo
- **Bulk operations** — pull, push, fetch, diff, grep, clean, commit, stash,
  reset, and branch inspection across all repos
- **Consistency checks** — `check-same` verifies that shared files are
  byte-identical across the fleet, `check-exists` verifies that required files
  are present, and interactive `--diff`, `--copy` and `--fix-missing` modes
  repair drift
- **Build orchestration** — run make, rsconstruct, cargo, or bootstrap across
  all projects, with each repo's `.venv` activated automatically
- **Tooling passthrough** — `uv lock` / `uv sync`, `cargo update`,
  `cargo release`, and `gh` cleanup on the repos where they apply
- **Parallel execution** — `-j N` runs repos concurrently while output stays
  in repo order
- **Flexible output** — verbose, terse, headerless, and inverted-selection
  modes
- **Error control** — stop on the first error or continue through all
  projects with `--no-stop`

## Philosophy

RSMultiGit does one thing well: it runs a single operation across a fixed
set of git repositories and prints only what needs attention. The repository
list lives in one config file, so a fleet of hundreds of repos is one
command away from anywhere.
