# Getting Started

## Basic usage

Navigate to a directory that contains git repositories as subdirectories, then run any rsmultigit command:

```bash
cd ~/git/myorg
rsmultigit list-projects
```

RSMultiGit will automatically discover git repos by looking for directories containing a `.git` folder. It searches both immediate subdirectories (`*`) and two levels deep (`*/*`).

## Checking repository status

See which repos have uncommitted changes:

```bash
rsmultigit status
```

Count dirty repos with statistics:

```bash
rsmultigit --stats count-dirty
```

Find repos with untracked files:

```bash
rsmultigit --stats untracked
```

## Pulling all repos

```bash
rsmultigit pull
```

Or quietly:

```bash
rsmultigit pull --quiet
```

## Searching across repos

Grep for a pattern across all repositories:

```bash
rsmultigit grep "TODO"
```

Show only filenames:

```bash
rsmultigit grep --files "TODO"
```

## Building all projects

Run make across all repos:

```bash
rsmultigit build-make
```

Run rsbuild build on projects that have an `rsbuild.toml`:

```bash
rsmultigit build-rsbuild
```

## Filtering projects

Only operate on specific folders:

```bash
rsmultigit --folders projectA,projectB status
```

Use a custom glob pattern:

```bash
rsmultigit --glob "python-*" status
```

## Error handling

By default, rsmultigit stops on the first error. To continue through all projects:

```bash
rsmultigit --no-stop pull
```
