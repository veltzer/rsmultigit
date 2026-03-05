# Command Reference

## Global Flags

These flags can be used with any subcommand and must appear before the subcommand name:

| Flag | Description |
|------|-------------|
| `--terse` | Terse output — suppress project headers |
| `--stats` | Print match count as `N/total` after count commands |
| `--no-output` | Suppress command output (only print project names) |
| `--print-not` | Invert selection — print repos that do NOT match |
| `--git-verbose` | Pass `--verbose` to git commands |
| `--git-quiet` | Pass `--quiet` to git commands |
| `--no-sort` | Do not sort the project list |
| `--glob <PATTERN>` | Glob pattern for project discovery (default: `*/*`) |
| `--no-glob` | Disable glob — check immediate subdirectories only |
| `--folders <LIST>` | Comma-separated list of folders to operate on |
| `--no-stop` | Do not stop on errors — continue to next project |
| `--no-print-no-projects` | Suppress the "no projects found" message |

Example:

```bash
rsmultigit --stats --terse count-dirty       # Just print "3/50"
rsmultigit --no-stop pull                    # Pull all, skip failures
rsmultigit --glob "python-*" status          # Only match python-* dirs
rsmultigit --folders a,b,c list-projects     # Operate on specific folders
```

## Count Commands

These commands test each discovered repo and print matching projects.

### `rsmultigit count-dirty`

Count repositories with dirty working trees (modified, deleted, or staged files). Uses libgit2 for fast native inspection.

```bash
rsmultigit count-dirty
rsmultigit --stats count-dirty               # Print count as N/total
rsmultigit --terse --stats count-dirty       # Print only the count line
```

### `rsmultigit untracked`

Count repositories that have untracked files.

```bash
rsmultigit untracked
rsmultigit --stats untracked
```

### `rsmultigit synchronized`

Count repositories that are not synchronized with their upstream (ahead or behind `origin/<branch>`).

```bash
rsmultigit synchronized
rsmultigit --stats synchronized
rsmultigit --print-not synchronized          # Show repos that ARE synchronized
```

## Status Commands

These commands inspect each repo and print output only for repos that have data.

### `rsmultigit status`

Show `git status -s` output for repositories that are not clean.

```bash
rsmultigit status
```

### `rsmultigit dirty`

Show `git diff --stat` output for repositories with modifications.

```bash
rsmultigit dirty
```

### `rsmultigit list-projects`

List all discovered projects.

```bash
rsmultigit list-projects
```

### `rsmultigit age`

Show the age of the last commit for each repo as a human-readable relative date.

```bash
rsmultigit age
```

### `rsmultigit authors`

Show unique commit authors for each repo, sorted by number of commits.

```bash
rsmultigit authors
```

### `rsmultigit config <KEY>`

Show a git config value across all repos. Repos where the key is not set are skipped.

```bash
rsmultigit config user.email
rsmultigit config remote.origin.url
```

### `rsmultigit size`

Show the size of the `.git` directory for each repo. Useful for finding bloated repos.

```bash
rsmultigit size
```

### `rsmultigit last-tag`

Show the most recent tag for each repo. Repos without tags are skipped.

```bash
rsmultigit last-tag
```

## Action Commands

These commands run an action in each project directory.

### `rsmultigit branch-local`

Show local branches for each repo.

```bash
rsmultigit branch-local
```

### `rsmultigit branch-remote`

Show remote branches for each repo.

```bash
rsmultigit branch-remote
```

### `rsmultigit branch-github`

Show the GitHub default branch for each repo (requires `gh` CLI).

```bash
rsmultigit branch-github
```

### `rsmultigit pull`

Pull the current branch from origin.

```bash
rsmultigit pull
rsmultigit pull --quiet
```

### `rsmultigit push`

Push the current branch to origin.

```bash
rsmultigit push
```

### `rsmultigit fetch`

Fetch from origin without merging.

```bash
rsmultigit fetch
```

### `rsmultigit stash push`

Stash working-tree changes in each repo.

```bash
rsmultigit stash push
```

### `rsmultigit stash pop`

Pop the most recent stash in each repo.

```bash
rsmultigit stash pop
```

### `rsmultigit reset hard|soft|mixed`

Reset HEAD across all repos.

```bash
rsmultigit reset hard       # Discard all changes
rsmultigit reset soft       # Keep changes staged
rsmultigit reset mixed      # Unstage changes (default git behavior)
```

### `rsmultigit log`

Show recent commits for each repo.

```bash
rsmultigit log              # Show last 10 commits
rsmultigit log -n 5         # Show last 5 commits
```

### `rsmultigit tag local`

List local tags for each repo.

```bash
rsmultigit tag local
```

### `rsmultigit tag remote`

List remote tags for each repo.

```bash
rsmultigit tag remote
```

### `rsmultigit tag has-local`

Show repos that have local tags (prints only the project header).

```bash
rsmultigit tag has-local
```

### `rsmultigit tag has-remote`

Show repos that have remote tags (prints only the project header).

```bash
rsmultigit tag has-remote
```

### `rsmultigit remote`

Show remote URLs for each repo.

```bash
rsmultigit remote
```

### `rsmultigit prune`

Prune stale remote-tracking branches (`git remote prune origin`).

```bash
rsmultigit prune
```

### `rsmultigit gc`

Run git garbage collection on each repo.

```bash
rsmultigit gc
```

### `rsmultigit checkout <BRANCH>`

Checkout a branch across all repos.

```bash
rsmultigit checkout main
rsmultigit checkout develop
```

### `rsmultigit commit -m <MESSAGE>`

Stage all changes and commit across all repos with a shared message.

```bash
rsmultigit commit -m "bump version"
```

### `rsmultigit submodule-update`

Update submodules recursively (`git submodule update --init --recursive`).

```bash
rsmultigit submodule-update
```

### `rsmultigit blame <FILE>`

Run `git blame` on a specific file across all repos. Repos where the file does not exist are skipped.

```bash
rsmultigit blame README.md
rsmultigit blame Makefile
```

### `rsmultigit clean-hard`

Hard-clean each repository with `git clean -ffxd`. **Warning:** this removes all untracked and ignored files.

```bash
rsmultigit clean-hard
```

### `rsmultigit diff`

Show `git diff` for each repository.

```bash
rsmultigit diff
```

### `rsmultigit grep <REGEXP>`

Grep across all repositories. Output lines are prefixed with the project name.

```bash
rsmultigit grep "TODO"
rsmultigit grep --files "TODO"               # Only show filenames
```

## Build Commands

These commands run build tools in each project directory. Projects with a `.disable` file are skipped.

### `rsmultigit build-bootstrap`

Run `python bootstrap.py` in each project.

### `rsmultigit build-pydmt`

Run `pydmt build` in each project.

### `rsmultigit build-make`

Run `make` in each project.

### `rsmultigit build-venv-make`

Run `make` inside the project's virtualenv (`.venv/bin/make`).

### `rsmultigit build-venv-pydmt`

Run `pydmt build` inside the project's virtualenv.

### `rsmultigit build-pydmt-build-venv`

Run `pydmt build_venv` in each project.

### `rsmultigit build-rsbuild`

Run `rsbuild build` on projects that have an `rsbuild.toml` file. Projects without `rsbuild.toml` are skipped.

```bash
rsmultigit build-rsbuild
```

## Utility Commands

### `rsmultigit version`

Print detailed version information including git commit, branch, dirty status, and Rust compiler version.

```bash
rsmultigit version
```

Short version via flag:

```bash
rsmultigit --version
```
