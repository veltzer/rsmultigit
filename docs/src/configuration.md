# Configuration

rsmultigit is driven by one file, `~/.config/rsmultigit/config.toml`, plus a
handful of global command-line flags. The file says *which repositories* to
operate on and *which invariants* to check; the flags say *how* to run and
*how much* to print.

The location is fixed and there is no `--config` flag. `RSMULTIGIT_CONFIG`
overrides the path (the test suite uses it). A missing or unparsable file is
an error for every command except `setup`, `config-example`, `complete` and
`version`.

Write a first config interactively with `rsmultigit setup`, or print a fully
commented starting point with:

```bash
rsmultigit config-example
```

## Top-level keys

```toml
repos = ["~/git/*"]
default_build_method = "rsconstruct"
crates_io_pass_entry = "keys/crates.io"
```

| Key | Required | Meaning |
|-----|----------|---------|
| `repos` | yes | List of shell-expanded glob patterns. Matches that are not git repositories are dropped; the rest are deduplicated and sorted. See [Repository Discovery](discovery.md) |
| `default_build_method` | no | What a bare `rsmultigit build` runs: `bootstrap`, `make`, `rsconstruct` or `cargo`, spelled as on the command line. A method given on the command line always wins. Without the key, `rsmultigit build` with no method is an error |
| `crates_io_pass_entry` | no | The pass(1) entry `rsmultigit rust publish` reads the crates.io token from when `CARGO_REGISTRY_TOKEN` is not set in the environment. Default `keys/crates.io` |

## `[[check]]` rules: files that must be identical

Each `[[check]]` block names one file that must have byte-identical content
in every repo it applies to. `rsmultigit check-same` evaluates them.

```toml
[[check]]
name = "workflow-build-yml"
select = "*"
marker = "rsconstruct.toml"
marker_absent = ".noci"
path = ".github/workflows/build.yml"
must_have = true
```

| Field | Required | Default | Meaning |
|-------|----------|---------|---------|
| `name` | yes | | Rule name, used in output and for `--checks` / `--checks-re` |
| `select` | yes | | Glob over repo directory names (not paths). `*` selects every repo |
| `exclude` | no | none | Glob over repo names to drop from the selection |
| `marker` | no | none | Only repos containing this file (relative to the repo root) stay selected |
| `marker_absent` | no | none | Repos containing this file are dropped. This is the in-repo opt-out: a repo declares itself exempt (with a `.noci` file, say) instead of being named in an `exclude` glob far away from it |
| `path` | yes | | The file to compare, relative to each repo root |
| `enabled` | no | `true` | Disabled rules are skipped by default but can still be forced with `--checks` or `--checks-re` |
| `must_have` | no | `false` | When true, every selected repo must contain `path`; missing files are violations. When false, repos without the file are silently skipped |

A rule passes when every surviving repo's `path` hashes the same and, with
`must_have = true`, no selected repo lacks the file. A rule that ends up
matching no files at all fails, because that almost always means a stale
`select` or `path`; `--allow-empty` turns that into a pass.

Selection filters are applied in the order listed: `select`, then
`exclude`, then `marker`, then `marker_absent`.

## `[[exists]]` rules: files that must be present

Each `[[exists]]` block asserts that a file is present in every repo it
applies to, without ever comparing content. This is the rule type for files
that must exist but legitimately differ per repo, such as a README.
`rsmultigit check-exists` evaluates them.

```toml
[[exists]]
name = "readme-present"
select = "*"
path = "README.md"
```

`name`, `select`, `exclude`, `marker`, `marker_absent`, `path` and `enabled`
mean exactly what they mean for `[[check]]`. There is no `must_have`,
because requiring the file *is* the whole rule. A directory at `path` does
not satisfy the rule; it must be a file.

`rsmultigit check-all` runs the `[[check]]` and `[[exists]]` rules together.

## Global flags

All flags are global: they may appear before or after the subcommand.

### Output control

| Flag | Default | Description |
|------|---------|-------------|
| `-v`, `--verbose` | off | Print every repo, even when nothing happened; `status` switches to per-file output |
| `--terse` | off | Repo names only for data commands; only the `N/total` line for count commands; failing rule names only for the check commands |
| `--no-header` | off | Suppress the `[repo]` (or `[rule]`) header line |
| `--no-output` | off | Suppress command output, keep the `[repo]` headers |
| `--print-not` | off | Invert selection: print the repos that do NOT match |

### Execution

| Flag | Default | Description |
|------|---------|-------------|
| `--no-stop` | off | Report errors on stderr and continue instead of stopping at the first one |
| `--short-circuit` | off | Stop at the first negative result. Honoured by `check-same` and `check-exists`; other commands accept it and ignore it |
| `-j`, `--jobs <N>` | 1 | Number of repos to process concurrently; 0 means one per CPU. Output is buffered per repo and printed in repo order |

### Tool environment

| Flag | Default | Description |
|------|---------|-------------|
| `--venv` | on | Activate each repo's local `.venv` (prepend `.venv/bin` to `PATH`, set `VIRTUAL_ENV`) before running tool subprocesses. Honoured by `run`, `build`, `cargo` and `clean make`; repos without a `.venv` run unchanged. Not honoured by `uv`, which selects its own environment from the repo directory |
| `--no-venv` | off | Turn the `.venv` activation off |

## Short-circuiting

`--short-circuit` tells a check command to stop at the first broken rule
rather than working through everything. Rules that already passed are still
reported, and the exit code is unchanged (non-zero when a rule is broken).
Without the flag, every rule is evaluated and every failure reported.

```bash
rsmultigit check-same                        # report every broken rule
rsmultigit --short-circuit check-same        # report the first broken rule and stop
rsmultigit --terse --short-circuit check-same # print just that rule's name
rsmultigit --short-circuit check-exists      # same, for presence rules
```

## Build command skipping

`rsmultigit build <method>` skips projects that contain a `.disable` file in
their root. `build rsconstruct` additionally skips projects without an
`rsconstruct.toml`, and `build cargo`, the `cargo` commands and
`rust publish` skip projects without a `Cargo.toml`. The `uv` commands
skip projects without a `pyproject.toml`, and `gh` commands skip repos with
no github.com remote.
