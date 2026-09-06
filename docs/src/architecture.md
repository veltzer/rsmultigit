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
    build.rs           Build commands (make, rsconstruct, cargo, bootstrap)
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

## Subprocess environments — venv activation vs. clean env

**Policy: tools that resolve from `PATH` get the repo's `.venv` activated.
Tools that select their own target environment get a clean env instead, and
choose from the working directory. Never pass the caller's `VIRTUAL_ENV`
through to either.**

Three helpers in `subprocess_utils.rs` implement this:

| Helper | Environment | Used by |
|---|---|---|
| `check_call` | inherited, unchanged | plain commands with no venv stake |
| `check_call_ve_env` | `.venv/bin` prepended to `PATH`, `VIRTUAL_ENV` set | `run`, `build`, `clean make` |
| `check_call_clean_env` | `VIRTUAL_ENV` and `UV_PROJECT_ENVIRONMENT` removed | `uv` |

Commands honouring `--venv` do not call the first two directly: they call
`check_call_maybe_ve`, which dispatches to `check_call_ve_env` when the flag
is on and `check_call` when it is off. A repo with no `.venv` runs with the
environment unchanged either way.

### Why activation is right for `run`/`build`

These run tools *from* the environment — `pytest`, `mypy`, `ruff`. The tool
name resolves through `PATH`, so activation is what makes the repo's own
pinned version win over whatever `~/.venv` happens to provide. This mirrors
the global rule that builds happen inside an already-entered environment;
`--venv` is rsmultigit entering it on your behalf, once per repo.

### Why activation is wrong for `uv`

`uv` is not run *from* an environment, it *manages* one. It already locates
the target from the working directory — the project's `.venv` for `uv sync`
and `uv lock`, `./.venv` for the `uv pip` interface — and since rsmultigit
sets the working directory per repo, that discovery is already correct.

An inherited `VIRTUAL_ENV` can then only make it wrong, because it names the
venv the *calling shell* was in, never the repo being operated on. The two
uv interfaces fail differently on it, and the quiet one is the dangerous one:

| Invocation | Inherited `VIRTUAL_ENV` | Result |
|---|---|---|
| `uv sync` / `uv lock` | `~/.venv` | Warns `does not match the project environment path .venv and will be ignored`, then does the right thing |
| `uv pip install` | `~/.venv` | **Installs into `~/.venv`**, silently, once per repo |

The first is the visible annoyance that prompted the change. The second is
the reason the fix is a clean environment rather than a suppressed warning:
`uv pip` treats an active venv as a perfectly legitimate target, so there is
nothing to warn about, and a fleet-wide run would quietly write into the
shared toolbox hundreds of times.

Setting `VIRTUAL_ENV` explicitly to the repo's own `.venv` would also be
*correct* for both — it agrees with what uv discovers anyway — but it still
trips the `uv sync` warning, because uv compares the absolute path it was
given against the project's relative `.venv`. Unsetting is the only option
that is both correct and quiet, and it extends to uv subcommands not yet
wired up: anything added to `commands/uv.rs` gets the right behaviour by
calling `check_call_clean_env`.

Consequently the global `--venv`/`--no-venv` flag does not apply to `uv`.
It still parses there (it is a global flag) but has no effect.

## Error handling

All functions return `anyhow::Result`. The `--no-stop` flag controls whether errors in individual projects are fatal (default) or logged and skipped.

## Build script

The `build.rs` script embeds git metadata (commit SHA, branch, dirty status, describe) and the Rust compiler version at compile time. These are accessible via `env!()` macros and displayed by `rsmultigit version`.
