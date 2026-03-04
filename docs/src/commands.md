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
rmg --stats --terse count-dirty       # Just print "3/50"
rmg --no-stop pull                    # Pull all, skip failures
rmg --glob "python-*" status          # Only match python-* dirs
rmg --folders a,b,c list-projects     # Operate on specific folders
```

## Count Commands

These commands test each discovered repo and print matching projects.

### `rmg count-dirty`

Count repositories with dirty working trees (modified, deleted, or staged files). Uses libgit2 for fast native inspection.

```bash
rmg count-dirty
rmg --stats count-dirty               # Print count as N/total
rmg --terse --stats count-dirty       # Print only the count line
```

### `rmg untracked`

Count repositories that have untracked files.

```bash
rmg untracked
rmg --stats untracked
```

### `rmg synchronized`

Count repositories that are not synchronized with their upstream (ahead or behind `origin/<branch>`).

```bash
rmg synchronized
rmg --stats synchronized
rmg --print-not synchronized          # Show repos that ARE synchronized
```

## Status Commands

These commands inspect each repo and print output only for repos that have data.

### `rmg status`

Show `git status -s` output for repositories that are not clean.

```bash
rmg status
```

### `rmg dirty`

Show `git diff --stat` output for repositories with modifications.

```bash
rmg dirty
```

### `rmg list-projects`

List all discovered projects.

```bash
rmg list-projects
```

### `rmg age`

Show the age of the last commit for each repo as a human-readable relative date.

```bash
rmg age
```

### `rmg authors`

Show unique commit authors for each repo, sorted by number of commits.

```bash
rmg authors
```

### `rmg config <KEY>`

Show a git config value across all repos. Repos where the key is not set are skipped.

```bash
rmg config user.email
rmg config remote.origin.url
```

### `rmg size`

Show the size of the `.git` directory for each repo. Useful for finding bloated repos.

```bash
rmg size
```

### `rmg last-tag`

Show the most recent tag for each repo. Repos without tags are skipped.

```bash
rmg last-tag
```

## Action Commands

These commands run an action in each project directory.

### `rmg branch-local`

Show local branches for each repo.

```bash
rmg branch-local
```

### `rmg branch-remote`

Show remote branches for each repo.

```bash
rmg branch-remote
```

### `rmg branch-github`

Show the GitHub default branch for each repo (requires `gh` CLI).

```bash
rmg branch-github
```

### `rmg pull`

Pull the current branch from origin.

```bash
rmg pull
rmg pull --quiet
```

### `rmg push`

Push the current branch to origin.

```bash
rmg push
```

### `rmg fetch`

Fetch from origin without merging.

```bash
rmg fetch
```

### `rmg stash push`

Stash working-tree changes in each repo.

```bash
rmg stash push
```

### `rmg stash pop`

Pop the most recent stash in each repo.

```bash
rmg stash pop
```

### `rmg reset hard|soft|mixed`

Reset HEAD across all repos.

```bash
rmg reset hard       # Discard all changes
rmg reset soft       # Keep changes staged
rmg reset mixed      # Unstage changes (default git behavior)
```

### `rmg log`

Show recent commits for each repo.

```bash
rmg log              # Show last 10 commits
rmg log -n 5         # Show last 5 commits
```

### `rmg tag`

List tags for each repo.

```bash
rmg tag
```

### `rmg remote`

Show remote URLs for each repo.

```bash
rmg remote
```

### `rmg prune`

Prune stale remote-tracking branches (`git remote prune origin`).

```bash
rmg prune
```

### `rmg gc`

Run git garbage collection on each repo.

```bash
rmg gc
```

### `rmg checkout <BRANCH>`

Checkout a branch across all repos.

```bash
rmg checkout main
rmg checkout develop
```

### `rmg commit -m <MESSAGE>`

Stage all changes and commit across all repos with a shared message.

```bash
rmg commit -m "bump version"
```

### `rmg submodule-update`

Update submodules recursively (`git submodule update --init --recursive`).

```bash
rmg submodule-update
```

### `rmg blame <FILE>`

Run `git blame` on a specific file across all repos. Repos where the file does not exist are skipped.

```bash
rmg blame README.md
rmg blame Makefile
```

### `rmg clean-hard`

Hard-clean each repository with `git clean -ffxd`. **Warning:** this removes all untracked and ignored files.

```bash
rmg clean-hard
```

### `rmg diff`

Show `git diff` for each repository.

```bash
rmg diff
```

### `rmg grep <REGEXP>`

Grep across all repositories. Output lines are prefixed with the project name.

```bash
rmg grep "TODO"
rmg grep --files "TODO"               # Only show filenames
```

## Build Commands

These commands run build tools in each project directory. Projects with a `.disable` file are skipped.

### `rmg build-bootstrap`

Run `python bootstrap.py` in each project.

### `rmg build-pydmt`

Run `pydmt build` in each project.

### `rmg build-make`

Run `make` in each project.

### `rmg build-venv-make`

Run `make` inside the project's virtualenv (`.venv/bin/make`).

### `rmg build-venv-pydmt`

Run `pydmt build` inside the project's virtualenv.

### `rmg build-pydmt-build-venv`

Run `pydmt build_venv` in each project.

### `rmg build-rsb`

Run `rsb build` on projects that have an `rsb.toml` file. Projects without `rsb.toml` are skipped.

```bash
rmg build-rsb
```

## Utility Commands

### `rmg version`

Print detailed version information including git commit, branch, dirty status, and Rust compiler version.

```bash
rmg version
```

Short version via flag:

```bash
rmg --version
```
