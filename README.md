# RSMultiGit - Rust Multi Git

A fast CLI tool for managing multiple git repositories at once. Run status
checks, pulls, builds, greps, and cross-repo consistency checks over every
repo in a single command.

## Documentation

Full documentation: <https://veltzer.github.io/rsmultigit/>

## Features

- **Batch operations** — pull, push, fetch, diff, grep, clean, commit, and build across all repos at once
- **Native git inspection** — status, dirty and sync checks use libgit2 in-process, with no `git` subprocess per repo
- **Consistency checks** — `check-same` verifies that shared files (`.gitignore`, CI workflows, lint configs, ...) are byte-identical across the fleet, `check-exists` verifies that required files are present, and `--diff`, `--copy` and `--fix-missing` repair drift interactively
- **Build orchestration** — make, rsconstruct, cargo, bootstrap, with each repo's `.venv` activated automatically
- **Tooling passthrough** — `uv lock` / `uv sync` on Python projects, `cargo build|check|clippy|fmt|test|nextest|doc|deny|fetch|update|clean|publish` on Rust projects, `gh` cleanup on GitHub repos
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

rsmultigit reads `~/.config/rsmultigit/config.toml` on every run. Bootstrap
it from the built-in example:

```bash
mkdir -p ~/.config/rsmultigit
rsmultigit config-example > ~/.config/rsmultigit/config.toml
```

The file names the repositories to operate on and, optionally, the
consistency rules to enforce:

```toml
repos = ["~/git/*"]                  # shell-expanded globs; non-git matches are ignored
default_build_method = "rsconstruct" # what a bare `rsmultigit build` runs
crates_io_pass_entry = "keys/crates.io" # where `rust publish` finds the crates.io token

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
rsmultigit status                 # one-line summary of every repo that needs attention
rsmultigit -j 8 pull              # pull all repos, 8 at a time
rsmultigit count dirty            # count repos with uncommitted changes
rsmultigit grep "TODO"            # git grep across all repos
rsmultigit check-same             # verify shared files are identical everywhere
rsmultigit check-same --diff      # ... and show what differs
rsmultigit build                  # build every repo with the configured default method
rsmultigit run "git log -1"       # run any shell command in every repo
rsmultigit complete bash >> ~/.bash_completion
```

## Commands

### Inspection
| Command | Description |
|---------|-------------|
| `status` | One-line summary per repo needing attention (`--verbose` for `git status -s`) |
| `dirty` | Show `git diff --stat` for repos with modifications |
| `count dirty` | Count repositories with uncommitted changes |
| `count untracked` | Count repositories with untracked files |
| `count synchronized` | Count repositories ahead of or behind their upstream |
| `list-repos` | Print the path of every configured repo |
| `age` | Show the age of the last commit per repo |
| `authors` | Show commit authors per repo |
| `config <key>` | Show a git config value across all repos |
| `size` | Show the size of the `.git` directory per repo |
| `last-tag` | Show the most recent tag per repo |
| `tag local` / `tag remote` | List local or remote tags |
| `tag has-local` / `tag has-remote` | Count repos that have local or remote tags |
| `branch local` / `branch remote` / `branch github` | Show local, remote, or GitHub default branches |
| `remote` | Show remote URLs |
| `log [--count N]` | Show recent commits (default 10) |
| `diff` | Show `git diff` for all repositories |
| `blame <file>` | Run `git blame` on a file (skips repos without it) |
| `grep [-l] <regexp>` | Grep across all repositories |

### Consistency checks
| Command | Description |
|---------|-------------|
| `check-same` | Verify that `[[check]]` files are byte-identical across repos |
| `check-same --diff` | Also show a unified diff between differing groups |
| `check-same --copy` | Interactively copy one group's content over another |
| `check-same --fix-missing` | Interactively create files missing from `must_have` repos |
| `check-exists` | Verify that `[[exists]]` files are present in every selected repo |
| `check-all` | Run both checks; exit non-zero if either fails |
| `list-checks [check\|exists]` | Print every rule name of one kind (used by shell completion) |

### Operations
| Command | Description |
|---------|-------------|
| `pull [--quiet]` | Pull all repositories |
| `push` | Push repositories that are ahead of their upstream |
| `fetch` | Fetch from origin |
| `commit -m <msg>` | Stage and commit all changes with a shared message |
| `checkout <branch>` | Checkout a branch across all repositories |
| `stash push` / `stash pop` | Stash or pop working-tree changes |
| `reset hard` / `reset soft` / `reset mixed` | Reset HEAD across all repositories |
| `clean hard` | `git clean -ffxd` (removes untracked and ignored files) |
| `clean soft` | `git clean -fd` (removes untracked files only) |
| `clean git` | `git checkout .` (discards unstaged changes) |
| `clean make` | `make clean` |
| `prune` | Prune stale remote-tracking branches |
| `gc` | Run git garbage collection |
| `submodule-update` | `git submodule update --init --recursive` |
| `run <cmd...>` (alias `exec`) | Run an arbitrary command in every repo |

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
| `uv lock [--upgrade\|--check]` | Run `uv lock` on repos with `pyproject.toml` |
| `uv sync\|build\|publish` | Run `uv sync`, `uv build` or `uv publish` on repos with `pyproject.toml` |
| `npm install\|ci\|update\|outdated\|test\|publish` | Run the matching npm command on repos with `package.json` |
| `npm audit [--fix]` | Run `npm audit` (or `npm audit fix`) on repos with `package.json` |
| `rust publish [--type patch\|minor\|major]` | Run `cargo release` on repos with `Cargo.toml`, crates.io token from pass(1) |
| `gh clean-all [--keep N]` | Delete old deployments, releases and workflow runs on GitHub repos |

### Other
| Command | Description |
|---------|-------------|
| `config-example` | Print a sample config file to stdout |
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
| `--no-stop` | Report errors and continue instead of stopping at the first one |
| `--short-circuit` | Stop at the first failing rule (`check-same`, `check-exists`) |
| `-j, --jobs <N>` | Run N repos in parallel (default 1; 0 means all CPUs) |
| `--venv` / `--no-venv` | Activate each repo's `.venv` before running tools (default on) |

## License

MIT
