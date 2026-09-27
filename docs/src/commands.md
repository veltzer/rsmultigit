# Command Reference

## Global Flags

These flags are accepted by every subcommand and may appear before or after
the subcommand name.

| Flag | Description |
|------|-------------|
| `-v`, `--verbose` | Print every repo, even when no action is taken (adds the header for skipped repos; `status` switches to per-file output) |
| `--terse` | Minimal, machine-readable output: repo names only for data commands, just the `N/total` line for count commands, failing rule names only for the check commands |
| `--no-header` | Suppress the `[repo]` header line printed before per-repo output |
| `--no-output` | Suppress command output, keeping only the `[repo]` headers. Action commands run their subprocesses with output captured and discarded; a failing repo's output is still attached to its error |
| `--print-not` | Invert selection: print the repos that do NOT match |
| `--no-stop` | On error, print `error in <repo>: ...` to stderr and continue with the next repo instead of stopping |
| `--short-circuit` | Stop at the first failing rule. Off by default; honoured by `check-same` and `check-exists` |
| `-j`, `--jobs <N>` | Run up to N repos in parallel (default 1; 0 means one worker per CPU). Output is still printed in repo order |
| `--venv` | Activate each repo's local `.venv` (prepend `.venv/bin` to `PATH`, set `VIRTUAL_ENV`) before running tool subprocesses. On by default; honoured by `run`, `build`, `cargo update` and `clean make`; not by `uv` |
| `--no-venv` | Turn `--venv` off: run tool subprocesses with the ambient environment |

Example:

```bash
rsmultigit --terse count dirty               # Just print "3/50"
rsmultigit --no-stop pull                    # Pull all, report failures, keep going
rsmultigit -j 8 fetch                        # Fetch eight repos at a time
rsmultigit --short-circuit check-same        # Stop at the first broken rule
```

## Output conventions

Most commands print a `[repo]` header followed by that repo's output, and
print nothing for repos where nothing happened (no data, nothing to do, or
the repo was skipped by a precondition such as a missing `Cargo.toml`).
`--verbose` prints the header for those repos too, `--no-header` drops the
header everywhere, and `--terse` reduces the output to bare repo paths.

## Count Commands

`rsmultigit count <what>` tests each repo with libgit2 (no subprocess),
prints the path of every matching repo, then a final `matched/total` line.

### `rsmultigit count dirty`

Repos with changes in the working tree or index: modified, deleted, renamed,
type-changed, or newly staged files. Untracked files do not count.

```bash
rsmultigit count dirty
rsmultigit --terse count dirty               # Only the "N/total" line
rsmultigit --print-not count dirty           # Clean repos instead
```

### `rsmultigit count untracked`

Repos that have untracked files.

### `rsmultigit count synchronized`

Repos that are **not** synchronized with their upstream: ahead of or behind
it. The upstream is the branch's configured tracking branch (as set by
`clone` or `push -u`); when none is configured, `origin/<branch>` is used.
A repo with no upstream at all has nothing to be out of sync with and is not
counted, which matches how `status` and `push` treat it.

```bash
rsmultigit count synchronized
rsmultigit --print-not count synchronized    # Repos that ARE in sync
```

## Data Commands

These commands compute a value per repo and print it under the `[repo]`
header. Repos with nothing to report are skipped unless `--verbose` is set.
With `--terse`, only the repo path is printed.

### `rsmultigit status`

One-line summary of every repo that needs attention: counts of conflicted,
staged, modified, deleted and untracked files, plus `ahead N` / `behind N`
when the branch has diverged from its upstream (see `count synchronized`
for how the upstream is chosen). Clean, in-sync repos are skipped. Computed
with libgit2.

```bash
rsmultigit status
# [/home/me/git/myrepo]
# 2 modified, 1 untracked, ahead 1
rsmultigit --verbose status                  # Full per-file `git status -s` instead
```

### `rsmultigit dirty`

`git diff --stat` for repos with unstaged modifications; falls back to
`git diff --cached --stat` when only staged changes exist.

### `rsmultigit list-repos`

Print the absolute path of every configured repo, one per line and with no
header. `--verbose` adds the `[repo]` header for each entry.

### `rsmultigit age`

The age of the last commit as a relative date (`3 days ago`).

### `rsmultigit authors`

`git shortlog -sne HEAD`: authors with commit counts and emails.

### `rsmultigit config <KEY>`

A git config value per repo. Repos where the key is unset are skipped.

```bash
rsmultigit config user.email
rsmultigit config remote.origin.url
```

### `rsmultigit size`

Size of the `.git` directory per repo, in human-readable units. Symlinks are
not followed.

### `rsmultigit last-tag`

The most recent tag reachable from HEAD (`git describe --tags --abbrev=0`).
Repos without tags are skipped.

## Action Commands

These commands run an action in each repo. The `[repo]` header is printed
for repos where the action ran; with `--verbose` it is printed for skipped
repos too.

### `rsmultigit pull [--quiet]`

`git pull` in every repo. `--quiet` is forwarded to git.

### `rsmultigit push`

`git push` in every repo that is ahead of its upstream (the configured
tracking branch, else `origin/<branch>`). Repos with nothing to push, or
with no upstream, are skipped.

### `rsmultigit fetch`

`git fetch` in every repo.

### `rsmultigit commit -m <MESSAGE>`

`git add -A` then `git commit -m <MESSAGE>` in every repo that has changes.
Clean repos are skipped.

### `rsmultigit checkout <BRANCH>`

`git checkout <BRANCH>` in every repo.

### `rsmultigit stash push` / `rsmultigit stash pop`

Stash, or pop the most recent stash, in every repo.

### `rsmultigit reset hard|soft|mixed`

`git reset --<mode> HEAD` in every repo.

```bash
rsmultigit reset hard       # Discard all changes
rsmultigit reset soft       # Keep changes staged
rsmultigit reset mixed      # Unstage changes
```

### `rsmultigit clean <what>`

| What | Runs | Notes |
|------|------|-------|
| `hard` | `git clean -ffxd` | Removes untracked **and ignored** files |
| `soft` | `git clean -fd` | Removes untracked files only |
| `git` | `git checkout .` | Discards unstaged working-tree changes |
| `make` | `make clean` | Honours `--venv` |
| `cargo` | `cargo clean` | Skips repos without `Cargo.toml` |

### `rsmultigit diff`

`git diff` in every repo.

### `rsmultigit log [--count N]`

`git log --oneline -n N` in every repo (default 10).

```bash
rsmultigit log
rsmultigit log --count 5
```

### `rsmultigit blame <FILE>`

`git blame <FILE>` in every repo that contains the file; others are skipped.

### `rsmultigit grep [-l|--files] <REGEXP>`

`git grep -n <REGEXP>` in every repo. Each output line is prefixed with the
repo name; `-l` prints matching filenames only. Repos with no match are
skipped.

```bash
rsmultigit grep "TODO"
rsmultigit grep -l "TODO"
```

### `rsmultigit branch local|remote|github`

`git branch`, `git branch -r`, or the GitHub default branch via
`gh repo view` (requires the `gh` CLI).

### `rsmultigit tag local|remote|has-local|has-remote`

`local` and `remote` list tags (`git tag`, `git ls-remote --tags origin`).
`has-local` and `has-remote` behave like `count` commands: they print the
repos that have any tags, then a `matched/total` line.

### `rsmultigit remote`

`git remote -v` in every repo.

### `rsmultigit prune`

`git remote prune origin` in every repo.

### `rsmultigit gc`

`git gc` in every repo.

### `rsmultigit submodule-update`

`git submodule update --init --recursive` in every repo.

### `rsmultigit run <COMMAND...>` (alias: `rsmultigit exec`)

Run an arbitrary command in every repo. A single argument is run through the
shell (`sh -c`), so pipes, redirects and quoting work; several arguments are
executed directly without a shell.

By default (`--venv`), a repo that has a local `.venv` gets it activated
first, so the command and anything it spawns resolve from the repo's own
venv. Pass `--no-venv` to run with the ambient environment everywhere.

```bash
rsmultigit run touch marker.txt
rsmultigit run "echo hello > greeting.txt"
rsmultigit exec cargo check
rsmultigit run pytest                  # each repo's own pytest, via its .venv
rsmultigit --no-venv run which python  # ambient python everywhere
```

## Consistency Checks

These commands evaluate the `[[check]]` and `[[exists]]` rules in the config
file. See [Configuration](configuration.md) for the rule fields. Unlike every
other command, output is organised by **rule**, not by repo: each rule prints
a `[rule-name]` header followed by its verdict.

### `rsmultigit check-same`

For each enabled `[[check]]` rule, hash `path` in every selected repo and
group the repos by content. A rule passes when there is at most one group
and, for `must_have = true` rules, no selected repo lacks the file.

```text
[gitignore]
ok (212 files)
[workflow-build-yml]
34 files, 2 groups (1 missing, 3 skipped)
  group A (31 files):
    /home/me/git/alpha/.github/workflows/build.yml
    ...
  group B (3 files):
    /home/me/git/beta/.github/workflows/build.yml
    ...
  missing in:
    /home/me/git/gamma
```

Groups are ordered largest first, so group A is the presumed canonical
version. `missing` counts `must_have` violations; `skipped` counts repos
that lack the file under a rule without `must_have`.

Exit code is 1 when any rule fails, 0 otherwise. A rule that matches no
files at all is reported as `no files matched` and, after all rules have
run, turns into a hard error on stderr naming every such rule, since an
empty rule almost always means a stale `select` or `path`.

| Flag | Effect |
|------|--------|
| `--checks <NAME>...` | Run only these rules, in the given order. Named rules run even if `enabled = false`. An unknown name is an error |
| `--checks-re <REGEX>...` | Also run every rule whose name matches a regex (unanchored). A regex matching nothing is an error. Combines with `--checks` |
| `--only-failed` | Drop the `ok (N files)` lines; print failing rules only |
| `--allow-empty` | Let a rule that matches no files pass as `ok (0 files)` instead of failing |
| `--diff` | After reporting a failing rule, print a unified diff between representatives of the differing groups. With exactly two groups this is automatic; with more, it prompts for the pair and offers to diff another |
| `--copy` | After reporting a failing rule, prompt for a "from" group and a "to" group, confirm, then overwrite every file in the "to" group with the "from" representative (preserving each destination's file mode). Exit code is always 0 |
| `--fix-missing` | For rules with `must_have` violations, prompt for a group to seed from, confirm, then create the file in each violating repo (creating parent directories as needed). Exit code is always 0 |

The interactive prompts accept a group letter (`A`, `B`, ...), `s` to skip
the rule, or `q` to quit the whole run; confirmations default to no. EOF on
stdin is treated as quit, so the command never hangs when stdin is closed.

`--terse` prints only the names of failing rules, one per line, and implies
`--only-failed`. `--short-circuit` stops after the first failing rule.

```bash
rsmultigit check-same                         # Report every rule
rsmultigit --terse --no-stop check-same       # Names of broken rules only, for scripts
rsmultigit check-same --checks gitignore editorconfig
rsmultigit check-same --checks-re '^rs-'      # Every rule whose name starts with rs-
rsmultigit check-same --diff --only-failed    # Show what differs
rsmultigit check-same --copy                  # Fix drift interactively
rsmultigit check-same --fix-missing           # Create files missing from must_have repos
```

### `rsmultigit check-exists`

For each enabled `[[exists]]` rule, assert that every selected repo contains
`path`. Content is never compared, which makes this the right rule type for
files that must exist but legitimately differ per repo (a README, say).

```text
[readme-present]
ok (212 repos)
[license-present]
212 repos, 2 missing LICENSE
  missing in:
    /home/me/git/alpha
    /home/me/git/beta
```

Exit code is 1 when any rule fails. A rule that selects no repos at all
fails with `no repos selected` and becomes a hard error at the end, unless
`--allow-empty` is passed.

`--checks`, `--checks-re`, `--only-failed`, `--allow-empty`, `--terse` and
`--short-circuit` behave exactly as for `check-same`. There are no
interactive modes.

### `rsmultigit check-all [--only-failed] [--allow-empty]`

Run every enabled `[[check]]` rule and then every enabled `[[exists]]` rule.
Both halves always run, so a failure in the first never hides the state of
the second. Exit code is non-zero if either half fails.

```bash
rsmultigit check-all --only-failed
```

### `rsmultigit list-checks`

Print the name of every `[[check]]` rule, one per line, including disabled
ones. The bash and zsh completion scripts call this to complete
`check-same --checks <TAB>`.

## Build Commands

`rsmultigit build [<method>]` runs a build tool in each project directory.
Projects with a `.disable` file in their root are always skipped.

By default (the global `--venv` flag), a project that has a local `.venv`
gets it activated before the build tool runs, so the tools the build spawns
(pytest, mypy, ruff, ...) resolve from the repo's own venv, including the
build tool itself when the venv provides it. Projects without a `.venv`
build with the ambient environment. Pass `--no-venv` to disable the
activation everywhere.

| Method | What runs | Which projects |
|--------|-----------|----------------|
| `bootstrap` | `python bootstrap.py` | all |
| `make` | `make` | all |
| `rsconstruct` | `rsconstruct --quiet build` | those with `rsconstruct.toml` |
| `cargo` | `cargo build` then `cargo build --release` | those with `Cargo.toml` |
| `cargo-publish` | `cargo publish` | those with `Cargo.toml` |

### Default build method

The method is optional when the config file sets `default_build_method`:

```toml
# ~/.config/rsmultigit/config.toml
default_build_method = "rsconstruct"
```

With that in place a bare `rsmultigit build` means
`rsmultigit build rsconstruct`. The key accepts exactly the spellings the
command line does (`cargo-publish`, not `CargoPublish`), and a method given
on the command line always wins over the config file. Without the key, a
bare `rsmultigit build` is an error that says how to fix it.

```bash
rsmultigit build rsconstruct
rsmultigit build                       # same, given default_build_method = "rsconstruct"
rsmultigit build make                  # explicit method overrides the default
rsmultigit --no-venv build rsconstruct
```

## Cargo and Rust Commands

These commands operate only on repositories that have a `Cargo.toml` file
and no `.disable` file. Other repositories are skipped.

### `rsmultigit cargo update`

`cargo update` in each Rust project, refreshing `Cargo.lock`. Honours
`--venv`.

### `rsmultigit rust publish [--type <patch|minor|major>]`

Release a new version of each Rust project by running
`cargo release <type> --execute --no-confirm`, which bumps the version in
`Cargo.toml`, commits, tags, pushes, and publishes to crates.io. The default
release type is `patch`. Requires
[cargo-release](https://crates.io/crates/cargo-release) to be installed.

```bash
rsmultigit rust publish                  # Patch release (default)
rsmultigit rust publish --type minor     # Minor release
rsmultigit rust publish --type major     # Major release
```

## uv Commands

Run [uv](https://docs.astral.sh/uv/) operations across every repository that
has a `pyproject.toml` at its root; other repositories are skipped.

### `rsmultigit uv lock [--upgrade|--check]`

Run `uv lock` in each Python project, re-resolving `uv.lock` from
`pyproject.toml`. Without `--upgrade`, versions that are already locked are
kept and only added or removed dependencies change; with `--upgrade`, locked
versions may move forward to the newest allowed releases.

With `--check`, nothing is written: `uv lock --check` only asserts that the
lockfile is up to date with `pyproject.toml`, and a stale lockfile is an
error (combine with the global `--no-stop` to survey all projects instead of
stopping at the first stale one). `--check` and `--upgrade` are mutually
exclusive.

```bash
rsmultigit uv lock                    # Bring every lockfile in sync with pyproject
rsmultigit uv lock --upgrade          # Deliberately upgrade all locked versions
rsmultigit uv lock --check --no-stop  # Report which lockfiles are stale
```

### `rsmultigit uv sync`

Run `uv sync` in each Python project, syncing its environment from the
lockfile. Projects without a `.venv` get one created.

`uv` selects its own target environment from the project directory, so
rsmultigit runs it with `VIRTUAL_ENV` (and `UV_PROJECT_ENVIRONMENT`) unset
and leaves the choice to `uv`. This matters when you invoke rsmultigit from
an activated shell: the inherited `VIRTUAL_ENV` names *your* venv, never the
repo being synced, and passing it through makes `uv sync` warn
(`does not match the project environment path`, after which the value is
ignored) while the `uv pip` interface would silently target the wrong
environment. The global `--venv`/`--no-venv` flag therefore does not apply
to `uv`; `uv lock` behaves the same way.

## GitHub Commands

These commands talk to GitHub through the [gh CLI](https://cli.github.com/)
(which must be installed and authenticated) and operate only on repositories
that have a remote whose URL contains `github.com`. Other repositories are
skipped.

### `rsmultigit gh clean-all [--keep <N>]`

Clean up GitHub deployments, releases, and workflow runs for each repository.
Keeps only the `--keep` (default 4) most recent non-failed of each and
deletes the rest; failed deployments (latest status `failure`/`error`) and
failed workflow runs (`failure`, `cancelled`, `timed_out`, `startup_failure`,
`action_required`) are always deleted, even if recent.

Note that this deletes data on GitHub permanently: releases, deployment
history, and workflow run logs are gone once removed.

```bash
rsmultigit gh clean-all              # Keep the 4 most recent of each
rsmultigit gh clean-all --keep 10    # Keep the 10 most recent of each
```

## Utility Commands

These three need no config file, because they are how a fresh install
bootstraps one.

### `rsmultigit config-example`

Print a fully commented sample config to stdout.

```bash
mkdir -p ~/.config/rsmultigit
rsmultigit config-example > ~/.config/rsmultigit/config.toml
```

### `rsmultigit complete <bash|zsh|fish|elvish|powershell>`

Print a shell completion script. The bash and zsh scripts additionally
complete `check-same --checks <TAB>` with the rule names from your config,
by calling `rsmultigit list-checks` at completion time.

```bash
rsmultigit complete bash >> ~/.bash_completion
rsmultigit complete zsh > ~/.zfunc/_rsmultigit
```

### `rsmultigit version`

Print detailed version information: crate version, git describe, commit,
branch, dirty state, rustc version, edition, and build timestamp.

```bash
rsmultigit version
rsmultigit --version                 # One-line "rsmultigit x.y.z by Author"
```
