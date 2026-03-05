# Getting Started

## Basic usage

Navigate to a directory that contains git repositories as subdirectories, then run any rmg command:

```bash
cd ~/git/myorg
rmg list-projects
```

RMG will automatically discover git repos by looking for directories containing a `.git` folder. It searches both immediate subdirectories (`*`) and two levels deep (`*/*`).

## Checking repository status

See which repos have uncommitted changes:

```bash
rmg status
```

Count dirty repos with statistics:

```bash
rmg --stats count-dirty
```

Find repos with untracked files:

```bash
rmg --stats untracked
```

## Pulling all repos

```bash
rmg pull
```

Or quietly:

```bash
rmg pull --quiet
```

## Searching across repos

Grep for a pattern across all repositories:

```bash
rmg grep "TODO"
```

Show only filenames:

```bash
rmg grep --files "TODO"
```

## Building all projects

Run make across all repos:

```bash
rmg build-make
```

Run rsbuild build on projects that have an `rsbuild.toml`:

```bash
rmg build-rsbuild
```

## Filtering projects

Only operate on specific folders:

```bash
rmg --folders projectA,projectB status
```

Use a custom glob pattern:

```bash
rmg --glob "python-*" status
```

## Error handling

By default, rmg stops on the first error. To continue through all projects:

```bash
rmg --no-stop pull
```
