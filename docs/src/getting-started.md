# Getting Started

## Create the config file

Every rsmultigit command reads `~/.config/rsmultigit/config.toml` to learn
which repositories to operate on. There is no `--config` flag and no
directory scanning: the file is the single source of truth.

The quickest start is the interactive setup, which asks where your
repositories are (with tab completion), checks that git repos are really
there, and lets you pick the default build tool from a menu:

```bash
rsmultigit setup
```

Or bootstrap the file from the fully commented built-in example and edit the
`repos` list by hand:

```bash
mkdir -p ~/.config/rsmultigit
rsmultigit config-example > ~/.config/rsmultigit/config.toml
```

The minimum useful config is one glob:

```toml
repos = ["~/git/*"]
```

Patterns are shell-expanded, and matches that are not git repositories are
ignored. Check what was picked up:

```bash
rsmultigit list-repos
```

## Checking repository status

See which repos need attention, one line per repo:

```bash
rsmultigit status
# [/home/me/git/myrepo]
# 2 modified, 1 untracked, ahead 1
```

Clean, in-sync repos print nothing. Pass `--verbose` for the full
`git status -s` per-file output instead of the summary.

Count repos with uncommitted changes, untracked files, or unpushed commits:

```bash
rsmultigit count dirty
rsmultigit count untracked
rsmultigit count synchronized      # repos ahead of or behind their upstream
```

Each prints the matching repos followed by a `matched/total` line.

## Pulling all repos

```bash
rsmultigit git pull
rsmultigit git pull --quiet
rsmultigit -j 8 git pull           # eight repos at a time
```

## Searching across repos

```bash
rsmultigit git grep "TODO"
rsmultigit git grep -l "TODO"      # filenames only
```

## Running any command

A single argument is run through the shell; several arguments are executed
directly:

```bash
rsmultigit run "git log -1 --oneline"
rsmultigit run cargo check
```

Repos with a `.venv` get it activated first, so `rsmultigit run pytest` runs
each repo's own pytest.

## Building all projects

```bash
rsmultigit build make
rsmultigit build rsconstruct       # only repos with rsconstruct.toml
rsmultigit build                   # uses default_build_method from the config
```

## Keeping shared files identical

Declare the files that must not drift between repos, then check them:

```toml
[[check]]
name = "gitignore"
select = "*"
path = ".gitignore"
```

```bash
rsmultigit check-same              # report every rule
rsmultigit check-same --diff       # show what differs
rsmultigit check-same --copy       # interactively copy one version over the others
```

See [Configuration](configuration.md) for `[[check]]` and `[[exists]]` rules.

## Error handling

By default, rsmultigit stops on the first error. To report errors and keep
going:

```bash
rsmultigit --no-stop git pull
```
