# Testing

RSMultiGit has unit tests inside the source files and integration tests
that drive the compiled binary.

## Running tests

The test runner is [cargo-nextest](https://nexte.st/); it is what CI runs
and what the shared `.config/nextest.toml` profile is written for.

```bash
cargo nextest run               # Debug
cargo nextest run --release     # Release
cargo nt                        # Alias defined in .cargo/config.toml
make test                       # Both
```

CI additionally gates on `cargo fmt --all --check`,
`cargo clippy --all-targets -- -D warnings` and `cargo deny check`; run all
four before pushing.

## Unit tests

Unit tests live in `#[cfg(test)]` modules inside each source file:

| Module | What's tested |
|--------|---------------|
| `cli` | Every subcommand and value enum parses; global flags before and after the subcommand; `--venv`/`--no-venv` precedence; `default_build_method` accepts exactly the CLI spellings |
| `commands::check` | Config parsing, `repos` resolution (git filter, dedupe, empty-list errors), rule selection filters, SHA-256 grouping, `must_have`, `matched_nothing`, `[[exists]]` evaluation |
| `commands::check_run` | `--checks` / `--checks-re` resolution; whole `check same` and `check exists` runs against in-memory stdin/stdout: output format, exit codes, `--terse`, `--only-failed`, `--short-circuit`, `--allow-empty`, `--diff`, `--copy`, `--fix-missing` |
| `commands::count` | `is_dirty`, `has_untracked`, `ahead_behind` (configured upstream, origin fallback, detached HEAD) against temp git repos built with git2 |
| `commands::status` | The one-line status summary |
| `commands::run` | Shell vs direct execution, venv activation |
| `commands::gh` | Deletion selection and failed-conclusion classification (pure functions, no network) |
| `commands::interactive` | Group labels (`A`..`Z`, `AA`...), prompt parsing, skip/quit/EOF handling via `Cursor` buffers |
| `runner` | All three runner patterns, serial and parallel, with mock closures, including stop-on-first-error under `-j` |
| `subprocess_utils` | Capture mode, venv activation, clean-environment execution |

No test changes the process working directory: every helper takes an
explicit repo path, so tests run in parallel without coordination.

## Integration tests

Integration tests are in `tests/` and run the compiled `rsmultigit` binary
as a subprocess against temporary git repositories:

```
tests/
  main.rs              Test entry point, loads modules
  common/mod.rs        Shared helpers
  tests_mod/
    build.rs           build with and without default_build_method
    check_same.rs      check same: grouping, must_have, --checks/--checks-re,
                       --only-failed, --allow-empty, --diff, --copy, --fix-missing,
                       --terse, --short-circuit, exit codes
    cli.rs             Help, unknown subcommand, missing args
    count.rs           count dirty/untracked, --terse, --print-not
    docs.rs            Every subcommand and global flag is in commands.md
    inspect.rs         list-repos, log, blame, tags, size, age, authors, config,
                       check exists, check all, complete
    remote.rs          push, fetch, pull, prune, remote, tag remote, against a
                       local bare repository standing in for origin
    run.rs             run/exec, shell vs direct, --no-stop, --no-output,
                       parallel output ordering
    rust.rs            cargo release against a fake cargo/cargo-release/pass on
                       PATH: level, token from pass or environment, preflight
    status.rs          status summary, --verbose, dirty
    version.rs         version subcommand and --version flag
    worktree.rs        commit, checkout, stash, reset, diff, clean, gc,
                       submodule-update
```

Every run points the binary at a config written into the temp directory via
the `RSMULTIGIT_CONFIG` environment variable, so tests never touch
`~/.config/rsmultigit/config.toml`.

### Test helpers (`tests/common/mod.rs`)

| Function | Description |
|----------|-------------|
| `setup_git_repos(names)` | Create a temp dir with one initialised git repo (one empty commit) per name |
| `init_git_repo(path)` | Initialise a single git repo with one commit |
| `utf8(&tmp)` | The temp dir as a UTF-8 path |
| `git(dir, args)` | Run git in `dir` with signing off, panic on failure, return trimmed stdout |
| `current_branch(dir)` | The checked-out branch name |
| `commit_file(repo, name, content, message)` | Write a file and commit it |
| `add_bare_origin(repo, bare)` | Create a bare repo at `bare`, add it as `origin`, push with tracking |
| `clone_of(bare, path)` | A second working clone of `bare`, for commits made "elsewhere" |
| `write_config(dir, extra)` | Write `<dir>/config.toml` with `repos = ["<dir>/*"]` plus `extra` (for `[[check]]` blocks); returns its path |
| `run_rsmultigit(dir, args)` | Run the binary in `dir` against a default config written by `write_config` |
| `run_rsmultigit_with_env(dir, args, env)` | Run the binary with extra environment variables (used to pass a custom `RSMULTIGIT_CONFIG`) |
| `run_rsmultigit_with_stdin(dir, args, env, bytes)` | Same, with `bytes` fed to stdin, for the interactive check same flows |
| `stdout_str(output)` / `stderr_str(output)` | Trimmed stdout / stderr of a finished command |

### Writing new tests

1. Create a new file in `tests/tests_mod/`
2. Add a `#[path]` module entry in `tests/main.rs`
3. Use `setup_git_repos()` to create fixtures and `write_config()` if the
   test needs check rules
4. Use `run_rsmultigit()` or `run_rsmultigit_with_env()` and assert on the
   output and exit status

Example:

```rust
use camino::Utf8Path;
use crate::common::{run_rsmultigit, setup_git_repos, stdout_str};

#[test]
fn my_new_test() {
    let tmp = setup_git_repos(&["repo1", "repo2"]);
    let dir = Utf8Path::from_path(tmp.path()).unwrap();
    let output = run_rsmultigit(dir, &["list-repos"]);
    assert!(output.status.success());
    let stdout = stdout_str(&output);
    assert!(stdout.contains("repo1"));
}
```
