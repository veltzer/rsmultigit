# Architecture

## Overview

RSMultiGit follows a simple pipeline: **discover projects** → **run command** → **collect results**.

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
    status.rs          Status summary via git2; per-file detail / diff via subprocess
    branch.rs          Branch listing (local, remote, github)
    pull.rs            git pull
    clean.rs           git clean -ffxd
    diff.rs            git diff
    grep.rs            git grep with project-name prefix
    build.rs           Build commands (make, pydmt, rsconstruct, bootstrap)
```

## Runner patterns

All subcommands use one of three runner functions:

### `do_count`

For count commands (`count-dirty`, `untracked`, `synchronized`). Calls a test function on each project path (using libgit2, no subprocess), counts matches, optionally prints statistics.

### `do_for_all_projects`

For action commands (`pull`, `clean-hard`, `diff`, `grep`, `branch-*`, `build-*`). Changes into each project directory, runs the action, prints a header. Respects `--no-stop` for error handling.

### `print_if_data`

For status commands (`status`, `dirty`, `list-repos`). Changes into each project directory, calls a data function. If it returns `Some(text)`, prints the project name and data. If `None`, the project is silently skipped.

## Git inspection — prefer the `git2` crate

**Policy: every git operation that libgit2 can do is done through the `git2`
crate, in-process. The `git` CLI subprocess is the fallback, not the default.**

The reasoning: rsmultigit's whole job is running the same small operation
across hundreds of repositories. A subprocess pays fork + exec + git binary
startup (config parsing, index loading) on every repo — a few milliseconds
each that pure library calls don't pay. Per repo this is negligible; times
260 repos it dominates the runtime of the fast inspection commands.

Subprocesses remain the right tool where libgit2 is genuinely weaker:
network operations (`pull`, `push`, `fetch` — credential helpers, SSH agent
and transport quirks are handled far better by the real git), and commands
whose value *is* git's own output formatting (`log`, `blame`, `grep`).

### Recorded benchmark (2026-08-21)

Measured on a real 260-repo config, release build, warm page cache, five
runs each. `rsmultigit status` spawned `git status -s` per repo;
`rsmultigit count dirty` performs the equivalent working-tree scan
in-process via `git2`, so the pair isolates the subprocess overhead:

| Command                       | Wall    | User    | Sys     |
|-------------------------------|---------|---------|---------|
| `status` (subprocess per repo)| ~0.89 s | ~0.34 s | ~0.58 s |
| `count dirty` (git2)          | ~0.52 s | ~0.30 s | ~0.21 s |

The wall-clock win is ~40%. The telling column is **sys**: 0.58 s → 0.21 s
is the cost of 260 fork+exec+startup cycles disappearing. **User** time is
nearly identical because git and libgit2 do broadly the same
index-vs-worktree comparison — the library doesn't scan faster, it just
skips the process machinery around the scan.

### Known trade-offs

- Very large working trees can regress: `git status` supports the untracked
  cache and fsmonitor extensions, libgit2 does not. None of the benchmarked
  repos was big enough to flip the result, but a kernel-sized repo could be.
- Output parity with git porcelain formats is close but not byte-perfect
  (submodule state, unusual ignore rules).

## Error handling

All functions return `anyhow::Result`. The `--no-stop` flag controls whether errors in individual projects are fatal (default) or logged and skipped.

## Build script

The `build.rs` script embeds git metadata (commit SHA, branch, dirty status, describe) and the Rust compiler version at compile time. These are accessible via `env!()` macros and displayed by `rsmultigit version`.
