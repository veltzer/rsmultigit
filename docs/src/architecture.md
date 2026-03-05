# Architecture

## Overview

RMG follows a simple pipeline: **discover projects** → **run command** → **collect results**.

## Module structure

```
src/
  main.rs              Entry point, CLI dispatch
  cli.rs               Clap derive definitions (Cli + Commands)
  config.rs            AppConfig runtime struct
  discovery.rs         Project discovery via glob or folder list
  runner.rs            Three execution patterns
  subprocess_utils.rs  Shell command helpers
  commands/
    mod.rs             Module declarations
    count.rs           git2-based repo inspection (dirty, untracked, synchronized)
    status.rs          git status / diff via subprocess
    branch.rs          Branch listing (local, remote, github)
    pull.rs            git pull
    clean.rs           git clean -ffxd
    diff.rs            git diff
    grep.rs            git grep with project-name prefix
    build.rs           Build commands (make, pydmt, rsbuild, bootstrap)
```

## Runner patterns

All subcommands use one of three runner functions:

### `do_count`

For count commands (`count-dirty`, `untracked`, `synchronized`). Calls a test function on each project path (using libgit2, no subprocess), counts matches, optionally prints statistics.

### `do_for_all_projects`

For action commands (`pull`, `clean-hard`, `diff`, `grep`, `branch-*`, `build-*`). Changes into each project directory, runs the action, prints a header. Respects `--no-stop` for error handling.

### `print_if_data`

For status commands (`status`, `dirty`, `list-projects`). Changes into each project directory, calls a data function. If it returns `Some(text)`, prints the project name and data. If `None`, the project is silently skipped.

## Git inspection

The count commands (`count-dirty`, `untracked`, `synchronized`) use the `git2` crate for direct repository inspection. This avoids forking `git` subprocesses and is significantly faster for large numbers of repos.

All other git operations use `std::process::Command` to run the `git` CLI, which provides familiar output formatting and handles edge cases that libgit2 may not cover.

## Error handling

All functions return `anyhow::Result`. The `--no-stop` flag controls whether errors in individual projects are fatal (default) or logged and skipped.

## Build script

The `build.rs` script embeds git metadata (commit SHA, branch, dirty status, describe) and the Rust compiler version at compile time. These are accessible via `env!()` macros and displayed by `rmg version`.
