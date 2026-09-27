# Architecture

## Overview

RSMultiGit follows a simple pipeline: **load the config** (the `repos` list
and the check rules) → **resolve the repo list** → **run one command across
every repo** → **print per-repo results in repo order**.

## Module structure

```
src/
  main.rs              Entry point: CLI dispatch
  cli.rs               Clap derive definitions (Cli + Commands + value enums),
                       shell-completion output
  config.rs            AppConfig: the global flags as a runtime struct
  runner.rs            The three execution patterns and the parallel scheduler
  subprocess_utils.rs  Shell command helpers, venv activation, per-thread output capture
  commands/
    mod.rs             Module declarations
    check.rs           Config-file parsing (repos, [[check]], [[exists]]), repo
                       resolution, rule evaluation (SHA-256 grouping, presence)
    check_run.rs       check-same / check-exists / check-all drivers: rule selection,
                       reporting, exit codes, the interactive flows
    interactive.rs     Prompt helpers for --diff / --copy / --fix-missing
    count.rs           git2-based repo inspection (dirty, untracked, ahead/behind)
    status.rs          Status summary via git2; per-file detail / diff via subprocess
    build.rs           Build methods and their precondition checks
    run.rs             Arbitrary command execution
    gh.rs              GitHub cleanup via the gh CLI
    uv.rs, cargo.rs, rust.rs   Tooling passthrough
    <one file per git operation>  pull, push, fetch, clean, diff, log, grep, ...
```

## Runner patterns

Every subcommand that operates on repos uses one of three runner functions
in `runner.rs`. Each command module exposes a plain `pub fn` taking a repo
path, and `main.rs` picks the runner.

### `do_count`

For `count <what>` and `tag has-local` / `tag has-remote`. Calls a boolean
test function on each repo (libgit2, no subprocess), prints the path of each
match, then a final `matched/total` line. `--print-not` inverts the test,
`--terse` drops the per-repo lines.

### `do_for_all_projects` and `do_for_all_projects_with_check`

For action commands (`pull`, `clean`, `diff`, `grep`, `run`, `build`, ...).
Runs an action in each repo directory that returns `Ok(true)` (did work) or
`Ok(false)` (skipped). The `_with_check` variant runs a cheap precondition
first (is there a `Cargo.toml`? a `.disable` file?) and only runs the action
where it passes. The `[repo]` header is printed for repos where the action
ran, or for every repo with `--verbose`. `--no-stop` turns a failing repo
into a stderr line instead of a fatal error.

### `print_if_data`

For data commands (`status`, `dirty`, `age`, `size`, `list-repos` in verbose
mode, ...). Calls a function returning `Option<String>` per repo and prints
the header plus data only when it is `Some`. `--verbose` and `--print-not`
also print the header for `None` repos; `--terse` prints only the repo
path.

## Parallel execution

All three runners go through `for_each_project_ordered`, which is a serial
loop when `--jobs` is 1 (the default) and a work-stealing thread pool
otherwise. Workers claim repo indices from an atomic counter and send
`(index, result)` pairs back over a channel; the calling thread buffers
out-of-order results and emits them strictly in repo order, so parallel
output is indistinguishable from serial output.

Subprocess output is the complication: `git pull` writes straight to the
inherited stdout. In the parallel path each worker thread enables a
thread-local capture buffer (`subprocess_utils::enter_capture`) before
running the action, so `check_call` and friends collect the child's stdout
and stderr into it instead of inheriting the parent's streams, and command
modules that format their own lines (`grep`, `gh`) route them through
`subprocess_utils::out_line`, which lands in the same buffer. The buffer
travels back with the result and is replayed on the main thread under the
repo's header. In the serial path nothing is captured and children write
live, which keeps interactive tools (credential prompts, pagers) working.
`--no-output` reuses the capture: the buffer is simply dropped, in both
paths, and attached to the error if the action failed.

An error without `--no-stop` ends the run the way the serial loop does: the
receiver is closed, a cancel flag stops idle workers from claiming more
repos, and only work already in flight completes.

When stderr is a terminal, the parallel path shows an `indicatif` progress
bar on stderr and suspends it while each repo's output is printed.

## Consistency checks

`check-same` and `check-exists` do not use the runners: they are organised
by rule, not by repo. `check.rs` parses the config, applies each rule's
`select` / `exclude` / `marker` / `marker_absent` filters to the resolved
repo list, then either hashes `path` in every selected repo with SHA-256 and
groups repos by digest (`evaluate_rule`) or records presence
(`evaluate_exists_rule`). `check_run.rs` owns rule selection
(`--checks` / `--checks-re`, shared by both commands through the `NamedRule`
trait), the reporting, the exit codes, and the interactive `--diff` /
`--copy` / `--fix-missing` flows. The drivers read prompts from a `BufRead`
and write everything to a `Write`, so the unit tests run whole commands
against in-memory buffers.

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

All functions return `anyhow::Result`, with `.context()` naming the repo the
error came from. The `--no-stop` flag controls whether an error in one repo
is fatal (default) or printed to stderr as `error in <repo>: ...` before
moving on. In the parallel path a failing repo's captured output is attached
to the error so it is not lost.

## Build script

The `build.rs` script embeds git metadata (commit SHA, branch, dirty status, describe) and the Rust compiler version at compile time. These are accessible via `env!()` macros and displayed by `rsmultigit version`.
