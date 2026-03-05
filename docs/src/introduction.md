# RMG - Rust Multi-Git

A fast CLI tool for managing multiple git repositories at once, written in Rust. RMG is a rewrite of [pymultigit](https://github.com/veltzer/pymultigit) with native performance.

## Features

- **Repository discovery** — automatically finds git repos via glob patterns or explicit folder lists
- **Status inspection** — count dirty, untracked, or non-synchronized repos using libgit2 (no subprocess overhead)
- **Bulk operations** — pull, clean, diff, grep, and branch inspection across all repos
- **Build orchestration** — run make, pydmt, rsbuild, or bootstrap across all projects
- **Flexible output** — terse mode, statistics, inverted selection, and suppress-output options
- **Error control** — stop on first error or continue through all projects with `--no-stop`

## Philosophy

RMG follows the Unix philosophy: do one thing well. It discovers git repositories in the current directory tree and runs a single operation across all of them. No configuration files needed — everything is controlled via CLI flags.
