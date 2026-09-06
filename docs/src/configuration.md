# Configuration

RSMultiGit does not use a configuration file. All behavior is controlled via CLI flags passed before the subcommand.

## Output control

| Flag | Default | Description |
|------|---------|-------------|
| `--terse` | `false` | Suppress project headers (`=== name ===`) |
| `--stats` | `false` | Print match count (`N/total`) for count commands |
| `--no-output` | `false` | Suppress command output in print-if-data commands |
| `--print-not` | `false` | Invert selection — print non-matching repos |

## Debug

| Flag | Default | Description |
|------|---------|-------------|
| `--git-verbose` | `false` | Pass `--verbose` to git commands |
| `--git-quiet` | `false` | Pass `--quiet` to git commands |

## Project discovery

| Flag | Default | Description |
|------|---------|-------------|
| `--glob <PATTERN>` | `*/*` | Glob pattern for finding projects |
| `--no-glob` | `false` | Disable glob, scan immediate subdirectories only |
| `--folders <LIST>` | (none) | Comma-separated explicit folder list |
| `--no-sort` | `false` | Preserve discovery order instead of sorting |

## Tool environment

| Flag | Default | Description |
|------|---------|-------------|
| `--venv` | `true` | Activate each repo's local `.venv` (prepend `.venv/bin` to `PATH`, set `VIRTUAL_ENV`) before running tool subprocesses. Honoured by `run`, `build`, and `clean make`; repos without a `.venv` run unchanged. Not honoured by `uv`, which selects its own environment from the repo directory |
| `--no-venv` | `false` | Turn the `.venv` activation off |

## Error handling

| Flag | Default | Description |
|------|---------|-------------|
| `--no-stop` | `false` | Continue on errors instead of stopping |
| `--short-circuit` | `false` | Stop at the first negative result instead of evaluating everything |
| `--no-print-no-projects` | `false` | Suppress "no projects found" message |

## Short-circuiting

`--short-circuit` is a global flag, off by default. It tells a command to stop
at the first negative result rather than working through everything.

Today only `check-same` acts on it: with the flag set, evaluation stops as soon
as one rule is found broken — the remaining rules are neither evaluated nor
reported. Rules that already passed before the failure are still reported as
usual, and the exit code is unchanged (non-zero when a rule is broken). Without
the flag, every rule is evaluated and every failure reported.

```bash
rsmultigit check-same                        # report every broken rule
rsmultigit --short-circuit check-same        # report the first broken rule and stop
rsmultigit --terse --short-circuit check-same # print just that rule's name
```

Other commands accept the flag (it is global) but currently ignore it.

## Build command skipping

Build commands (`build-*`) automatically skip projects that contain a `.disable` file in their root directory. The `build-rsconstruct` command additionally skips projects that do not have an `rsconstruct.toml` file.
