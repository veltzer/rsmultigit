# Repository Discovery

RSMultiGit does not scan the current directory. The set of repositories it
operates on comes from the `repos` list in `~/.config/rsmultigit/config.toml`,
so every command sees the same fleet no matter where it is run from.

## The `repos` list

```toml
repos = [
    "~/git/*",
    "~/work/team-*/repo",
    "/srv/checkouts/special",
]
```

Each entry is a glob pattern. Resolution works as follows:

1. **Shell expansion.** `~` and `$VAR` are expanded in every entry.
2. **Glob matching.** The expanded pattern is matched against the filesystem.
   A pattern with no glob characters is simply a path.
3. **Git filter.** Only matches that are directories containing a `.git`
   directory are kept. Plain directories, files, and worktrees whose `.git`
   is a file are silently dropped.
4. **Dedupe and sort.** Matches from all patterns are merged, duplicates
   removed, and the list sorted alphabetically. Output order is always this
   sorted order, even when running in parallel.

An empty `repos` list, or a list that matches no git repositories at all, is
an error: rsmultigit refuses to run a command over nothing.

## Inspecting the result

```bash
rsmultigit list-repos              # one absolute path per line
rsmultigit --verbose list-repos    # with the usual [repo] header per entry
```

## Per-rule selection

The `[[check]]` and `[[exists]]` rules narrow the discovered list further
with `select`, `exclude`, `marker` and `marker_absent`. Those filters apply
only to the consistency checks; every other command runs over the whole
`repos` list. See [Configuration](configuration.md).

## Overriding the config path

The location is fixed at `~/.config/rsmultigit/config.toml` and there is no
`--config` flag. The `RSMULTIGIT_CONFIG` environment variable overrides the
path; the integration tests use it to point the binary at a temporary config.

Four commands need no config at all, because they are how a fresh install
bootstraps one: `setup`, `config-example`, `complete`, and `version`.
