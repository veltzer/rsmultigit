# Configuration

RMG does not use a configuration file. All behavior is controlled via CLI flags passed before the subcommand.

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

## Error handling

| Flag | Default | Description |
|------|---------|-------------|
| `--no-stop` | `false` | Continue on errors instead of stopping |
| `--no-print-no-projects` | `false` | Suppress "no projects found" message |

## Build command skipping

Build commands (`build-*`) automatically skip projects that contain a `.disable` file in their root directory. The `build-rsbuild` command additionally skips projects that do not have an `rsbuild.toml` file.
