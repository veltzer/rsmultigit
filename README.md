# rmg - Rust Multi Git

A fast CLI tool for managing multiple git repositories at once. Run status checks, builds, pulls, greps, and more across all your repos in a single command.

## Documentation

Full documentation: <https://veltzer.github.io/rmg/>

## Features

- **Batch operations** — pull, diff, grep, clean, and build across all repos at once
- **Smart discovery** — finds git repos via glob patterns, explicit folder lists, or automatic fallback
- **Dirty/untracked detection** — uses libgit2 for fast native repo inspection
- **Build system support** — make, pydmt, rsb, bootstrap, virtualenv workflows
- **Selective output** — only prints repos where work was done; use `-v` for all
- **Flexible filtering** — `--print-not` to invert selection, `--terse` for minimal output, `--stats` for counts
- **Shell completions** — bash, zsh, fish, elvish, powershell

## Installation

### Download pre-built binary (Linux)

Pre-built binaries are available for x86_64 and aarch64 (arm64).

```bash
# x86_64
gh release download latest --repo veltzer/rmg --pattern 'rmg-x86_64-unknown-linux-gnu' --output rmg --clobber

# aarch64 / arm64
gh release download latest --repo veltzer/rmg --pattern 'rmg-aarch64-unknown-linux-gnu' --output rmg --clobber

chmod +x rmg
sudo mv rmg /usr/local/bin/
```

Or without the GitHub CLI:

```bash
# x86_64
curl -Lo rmg https://github.com/veltzer/rmg/releases/download/latest/rmg-x86_64-unknown-linux-gnu

# aarch64 / arm64
curl -Lo rmg https://github.com/veltzer/rmg/releases/download/latest/rmg-aarch64-unknown-linux-gnu

chmod +x rmg
sudo mv rmg /usr/local/bin/
```

### Build from source

```bash
cargo build --release
```

## Quick Start

```bash
# Navigate to a directory containing git repos (e.g. ~/git/myorg)
cd ~/git/myorg

# See which repos are dirty
rmg dirty

# Pull all repos
rmg pull

# Count dirty repos with stats
rmg --stats count dirty

# Grep across all repos
rmg grep "TODO"

# Show status of all repos
rmg status

# List discovered projects
rmg list-projects

# Build all rsb projects
rmg build rsb

# Generate shell completions
rmg complete bash >> ~/.bash_completion
```

## Commands

### Inspection
| Command | Description |
|---------|-------------|
| `count dirty` | Count dirty repositories |
| `count untracked` | Count repositories with untracked files |
| `count synchronized` | Count non-synchronized repositories (ahead/behind remote) |
| `status` | Show status of repositories |
| `dirty` | Show dirty repositories |
| `list-projects` | List discovered projects |
| `age` | Show the age of the last commit per repo |
| `authors` | Show unique commit authors per repo |
| `config <key>` | Show a git config value across all repos |
| `size` | Show the size of the `.git` directory per repo |
| `last-tag` | Show the most recent tag per repo |

### Operations
| Command | Description |
|---------|-------------|
| `pull` | Pull all repositories |
| `push` | Push all repositories |
| `fetch` | Fetch from origin for all repositories |
| `stash push` | Stash working-tree changes |
| `stash pop` | Pop the most recent stash |
| `reset hard/soft/mixed` | Reset HEAD across all repositories |
| `diff` | Show diff for all repositories |
| `log` | Show recent commits (default 10) |
| `tag` | List tags |
| `remote` | Show remote URLs |
| `prune` | Prune stale remote-tracking branches |
| `gc` | Run git garbage collection |
| `checkout <branch>` | Checkout a branch across all repositories |
| `commit -m <msg>` | Commit all changes with a shared message |
| `submodule-update` | Update submodules recursively |
| `blame <file>` | Run git blame on a file (skips repos without it) |
| `grep <regexp>` | Grep across all repositories |
| `clean hard` | Hard-clean all repositories (`git clean -ffxd`) |
| `clean soft` | Remove untracked files only (`git clean -fd`) |
| `clean make` | Run `make clean` |
| `clean git` | Discard unstaged working-tree changes (`git checkout .`) |
| `clean cargo` | Run `cargo clean` (skip if no `Cargo.toml`) |
| `branch local` | Show local branches |
| `branch remote` | Show remote branches |
| `branch github` | Show GitHub default branch |

### Build
| Command | Description |
|---------|-------------|
| `build make` | Run make across all projects |
| `build rsb` | Run rsb build on projects with `rsb.toml` |
| `build pydmt` | Run pydmt build across all projects |
| `build bootstrap` | Run bootstrap across all projects |
| `build venv-make` | Run make inside a virtualenv |
| `build venv-pydmt` | Run pydmt inside a virtualenv |
| `build pydmt-build-venv` | Run pydmt build_venv |

### Other
| Command | Description |
|---------|-------------|
| `complete <shell>` | Generate shell completion scripts |
| `version` | Print version information |

## Global Options

| Option | Description |
|--------|-------------|
| `-v, --verbose` | Print all projects, even when no action is taken |
| `--terse` | Terse output (suppress project headers) |
| `--stats` | Show statistics |
| `--no-output` | Suppress command output |
| `--print-not` | Print repos that do NOT match (invert selection) |
| `--glob <pattern>` | Glob pattern for discovering projects (default: `*/*`) |
| `--no-glob` | Disable glob-based discovery |
| `--folders <list>` | Explicit comma-separated list of folders to operate on |
| `--no-sort` | Do not sort project list |
| `--no-stop` | Do not stop on errors |

## Project Discovery

By default, rmg looks for git repositories matching the `*/*` glob pattern (two levels deep, e.g. `org/repo`). If no repos are found with `*/*`, it automatically falls back to `*` (immediate subdirectories).

You can override this with:
- `--glob "myorg/*"` — custom glob pattern
- `--folders "repo1,repo2,repo3"` — explicit list
- `--no-glob` — only scan immediate subdirectories

## License

MIT
