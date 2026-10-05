# RSMultiGit - Rust Multi Git

A fast CLI tool for managing multiple git repositories at once. Run status
checks, pulls, builds, greps, and cross-repo consistency checks over every
repo in a single command.

## Documentation

Full documentation: <https://veltzer.github.io/rsmultigit/>

## Features

- **Batch operations** — pull, push, fetch, diff, grep, clean, commit, and build across all repos at once
- **Native git inspection** — status, dirty and sync checks use libgit2 in-process, with no `git` subprocess per repo
- **Consistency checks** — `check same` verifies that shared files (`.gitignore`, CI workflows, lint configs, ...) are byte-identical across the fleet, `check exists` verifies that required files are present, and `--diff`, `--copy` and `--fix-missing` repair drift interactively
- **Build orchestration** — make, rsconstruct, cargo, bootstrap, with each repo's `.venv` activated automatically
- **Tooling passthrough** — `uv lock` / `uv sync` on Python projects, `cargo build|check|clippy|fmt|test|nextest|doc|deny|fetch|update|clean|publish` on Rust projects, `gh` cleanup, release/workflow inspection and metadata sync on GitHub repos
- **Parallel execution** — `-j N` runs repos concurrently while keeping output in repo order
- **Selective output** — only prints repos where something happened; `--verbose`, `--terse`, `--print-not` and `--no-header` control the rest
- **Shell completions** — bash, zsh, fish, elvish, powershell, including dynamic completion of check names

## Installation

### From crates.io

```bash
cargo install rsmultigit
```

### Pre-built binaries

Every release ships binaries for Linux (x86_64, aarch64) and macOS (x86_64,
aarch64). The asset names are:

| Platform | Asset |
|----------|-------|
| Linux x86_64 | `rsmultigit-linux-x86_64` |
| Linux aarch64 | `rsmultigit-linux-aarch64` |
| macOS x86_64 | `rsmultigit-macos-x86_64` |
| macOS aarch64 | `rsmultigit-macos-aarch64` |

```bash
gh release download --repo veltzer/rsmultigit --pattern rsmultigit-linux-x86_64 --output rsmultigit --clobber
chmod +x rsmultigit
sudo mv rsmultigit /usr/local/bin/
```

Or without the GitHub CLI:

```bash
curl -Lo rsmultigit https://github.com/veltzer/rsmultigit/releases/latest/download/rsmultigit-linux-x86_64
chmod +x rsmultigit
sudo mv rsmultigit /usr/local/bin/
```

### Build from source

```bash
cargo build --release
```

## Configuration

rsmultigit reads `~/.config/rsmultigit/config.toml` on every run. Create it
interactively (it asks where your repositories are and which build tool you
use, with tab completion and a menu):

```bash
rsmultigit setup interactive
```

or bootstrap it from the fully commented built-in example:

```bash
mkdir -p ~/.config/rsmultigit
rsmultigit setup config-sample > ~/.config/rsmultigit/config.toml
```

The file names the repositories to operate on and, optionally, the
consistency rules to enforce:

```toml
repos = ["~/git/*"]                  # shell-expanded globs; non-git matches are ignored
default_build_method = "rsconstruct" # what a bare `rsmultigit build` runs
crates_io_pass_entry = "keys/crates.io" # where `cargo release` finds the crates.io token

[[check]]                            # files that must be byte-identical
name = "gitignore"
select = "*"
path = ".gitignore"

[[exists]]                           # files that must be present
name = "readme-present"
select = "*"
path = "README.md"
```

See the [configuration docs](https://veltzer.github.io/rsmultigit/configuration.html)
for every field.

## Quick Start

```bash
rsmultigit git status             # one-line summary of every repo that needs attention
rsmultigit -j 8 git pull          # pull all repos, 8 at a time
rsmultigit git count dirty        # count repos with uncommitted changes
rsmultigit git grep "TODO"        # git grep across all repos
rsmultigit check same             # verify shared files are identical everywhere
rsmultigit check same --diff      # ... and show what differs
rsmultigit build                  # build every repo with the configured default method
rsmultigit run "git log -1"       # run any shell command in every repo
rsmultigit complete bash >> ~/.bash_completion
```

## Commands

### Git
Everything that inspects or operates on the repos as git repositories;
`rsmultigit git` alone lists it. The reports come first:

| Command | Description |
|---------|-------------|
| `git status` | One-line summary per repo needing attention (`--verbose` for `git status -s`) |
| `git dirty` | Show `git diff --stat` for repos with modifications |
| `git count dirty` | Count repositories with uncommitted changes |
| `git count untracked` | Count repositories with untracked files |
| `git count synchronized` | Count repositories ahead of or behind their upstream |
| `git age` | Show the age of the last commit per repo |
| `git authors` | Show commit authors per repo |
| `git size` | Show the size of the `.git` directory per repo |
| `git last-tag` | Show the most recent tag per repo |

The rest each run the git command of the same name in every repo:

| Command | Description |
|---------|-------------|
| `git pull [--quiet]` | Pull all repositories |
| `git push` | Push repositories that are ahead of their upstream |
| `git fetch` | Fetch from origin |
| `git commit -m <msg>` | Stage and commit all changes with a shared message |
| `git checkout <branch>` | Checkout a branch across all repositories |
| `git stash push` / `git stash pop` | Stash or pop working-tree changes |
| `git reset hard` / `git reset soft` / `git reset mixed` | Reset HEAD across all repositories |
| `git restore` | `git restore .` (discards unstaged changes to tracked files) |
| `git clean hard` | `git clean -ffxd` (removes untracked and ignored files) |
| `git clean soft` | `git clean -fd` (removes untracked files only) |
| `git diff` | Show the diff of every repo |
| `git log [--count N]` | Show recent commits (default 10) |
| `git blame <file>` | `git blame` in every repo that has the file |
| `git grep [-l] <regexp>` | `git grep` across all repositories |
| `git config <key>` | Show a git config value across all repos |
| `git branch local` / `git branch remote` / `git branch github` | Show local, remote, or GitHub default branches |
| `git tag local` / `git tag remote` | List local or remote tags |
| `git tag has-local` / `git tag has-remote` | Count repos that have local or remote tags |
| `git remote` | Show remote URLs |
| `git remote-prune` | Prune stale remote-tracking branches (`git remote prune origin`) |
| `git gc` | Run git garbage collection |
| `git submodule-update` | `git submodule update --init --recursive` |

### Consistency checks
`rsmultigit check` alone lists these.

| Command | Description |
|---------|-------------|
| `check same [--diff] [--copy] [--fix-missing]` | Verify the `[[check]]` files are byte-identical across the repos they select |
| `check exists` | Verify the `[[exists]]` files are present in every repo they select |
| `check all` | Run both; non-zero if either fails |
| `check list [exists]` | Print the rule names (for shell completion) |

### Running commands
| Command | Description |
|---------|-------------|
| `run <cmd...>` (alias `exec`) | Run an arbitrary command in every repo (`run make clean` for `make clean`) |

### Build and tooling
| Command | Description |
|---------|-------------|
| `build` | Build with the config file's `default_build_method` |
| `build make` / `build bootstrap` | Run `make` or `python bootstrap.py` |
| `build rsconstruct` | Run `rsconstruct --quiet build` on repos with `rsconstruct.toml` |
| `build cargo` | Run `cargo build` (debug and release) on repos with `Cargo.toml` |
| `cargo build [--release\|--profile <name>]` | Run `cargo build` on repos with `Cargo.toml`: every profile (dev, then release) by default, or just the one named |
| `cargo check\|clippy\|test\|nextest\|doc [--release\|--profile <name>]` | Run the matching cargo command on repos with `Cargo.toml` (`clippy` as CI: `--all-targets -- -D warnings`) |
| `cargo fmt [--check]` | Run `cargo fmt --all` on repos with `Cargo.toml` |
| `cargo deny\|fetch\|update\|clean\|publish` | Run `cargo deny check`, `cargo fetch`, `cargo update`, `cargo clean` or `cargo publish` on repos with `Cargo.toml` |
| `cargo release [--type patch\|minor\|major]` | Run `cargo release` on repos whose `Cargo.toml` has a publishable `[package]`, crates.io token from pass(1) |
| `uv lock [--upgrade\|--check]` | Run `uv lock` on repos with `pyproject.toml` |
| `uv sync\|build\|publish` | Run `uv sync`, `uv build` or `uv publish` on repos with `pyproject.toml` |
| `npm install\|ci\|update\|outdated\|test\|publish` | Run the matching npm command on repos with `package.json` |
| `npm audit [--fix]` | Run `npm audit` (or `npm audit fix`) on repos with `package.json` |
| `gh clean-all [--keep N]` | Delete old deployments, releases and workflow runs on GitHub repos |
| `gh artifacts` | List the assets of the latest release (name, size, downloads) on GitHub repos |
| `gh last-workflow-state` | Print the conclusion of the most recent workflow run on GitHub repos |
| `gh open-site` | Open the GitHub Pages site of each repo in the browser |
| `gh sync-metadata [--dry-run]` | Sync GitHub description, topics and feature policy from `config/project.lua`, printing only what differs |

### Other
| Command | Description |
|---------|-------------|
| `setup interactive [--repos-dir <DIR>] [--build <METHOD>\|--no-build] [--overwrite]` | Interactively write a first config file: pick the repositories directory and the default build tool |
| `setup config-sample` | Print a sample config file to stdout |
| `list-repos` | Print the path of every configured repo |
| `complete <shell>` | Generate shell completion scripts |
| `version` | Print detailed version information |

## Global Options

All global options work before or after the subcommand.

| Option | Description |
|--------|-------------|
| `-v, --verbose` | Print all repos, even when no action is taken |
| `--terse` | Minimal output: repo names only, or failing rule names for the check commands |
| `--no-header` | Suppress the `[repo]` header line before per-repo output |
| `--no-output` | Suppress command output, keep the `[repo]` headers |
| `--print-not` | Invert selection: print repos that do NOT match |
| `--no-stop` | Report errors and continue instead of stopping at the first one; still exits non-zero if any repo failed |
| `--short-circuit` | Stop at the first failing rule (`check same`, `check exists`) |
| `-j, --jobs <N>` | Run N repos in parallel (default 1; 0 means all CPUs) |
| `--venv` / `--no-venv` | Activate each repo's `.venv` before running tools (default on) |

## License

MIT
