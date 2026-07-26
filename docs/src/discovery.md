# Project Discovery

RSMultiGit discovers git repositories by searching for directories that contain a `.git` subdirectory.

## Discovery modes

There are three ways RSMultiGit finds projects, checked in this order:

### 1. Explicit folders (`--folders`)

When `--folders` is provided, only those directories are considered. Non-git directories are silently skipped.

```bash
rsmultigit --folders /path/to/repoA,/path/to/repoB status
```

### 2. No-glob mode (`--no-glob`)

When `--no-glob` is set, RSMultiGit scans immediate subdirectories of the current directory:

```bash
rsmultigit --no-glob list-repos
```

### 3. Glob-based discovery (default)

By default, RSMultiGit uses the glob pattern `*/*` to find projects two levels deep (e.g., `org/repo`). If no projects are found with `*/*`, it automatically falls back to `*` to handle the common case where immediate subdirectories are git repos.

```bash
# Works from a directory whose repos are nested (repos are at */*)
cd ~/src
rsmultigit list-repos

# Also works from ~/git (repos are at *)
cd ~/git
rsmultigit list-repos
```

A custom glob can be provided:

```bash
rsmultigit --glob "python-*" list-repos
rsmultigit --glob "org/team-*" list-repos
```

## Sorting

By default, discovered projects are sorted alphabetically. Use `--no-sort` to preserve the filesystem discovery order.

## How it works

1. Collect candidate paths using the selected mode
2. Filter to directories that contain `.git/`
3. Sort alphabetically (unless `--no-sort`)
4. Pass the list to the selected subcommand's runner
