# Testing

RSMultiGit has both unit tests and integration tests.

## Running tests

```bash
cargo test
```

## Unit tests

Unit tests are defined as `#[cfg(test)]` modules inside each source file:

| Module | Tests | What's tested |
|--------|-------|---------------|
| `cli` | 11 | Subcommand parsing, global flags, argument validation |
| `commands::count` | 7 | `is_dirty`, `has_untracked`, `non_synchronized` with temp git repos |
| `discovery` | 6 | Folder, glob, and no-glob discovery modes |
| `runner` | 11 | All three runner patterns with mock closures |
| `subprocess_utils` | 6 | `capture_output`, `check_call`, `check_call_ve` |

Tests that change the working directory use `serial_test::serial` to avoid conflicts with parallel test execution.

## Integration tests

Integration tests are in `tests/` and run the compiled `rsmultigit` binary as a subprocess against temporary git repositories:

```
tests/
  main.rs              Test entry point, loads modules
  common/mod.rs        Shared helpers
  tests_mod/
    cli.rs             Help, unknown subcommand, missing args
    count.rs           Count-dirty, untracked, terse, print-not
    discovery.rs       Immediate subdirs, nested, glob, folders
    status.rs          Status and dirty output
    version.rs         Version subcommand and --version flag
```

### Test helpers (`tests/common/mod.rs`)

| Function | Description |
|----------|-------------|
| `run_rmg(dir, args)` | Run the rsmultigit binary with given args in a directory |
| `stdout_str(output)` | Extract trimmed stdout from command output |
| `stderr_str(output)` | Extract trimmed stderr from command output |
| `setup_git_repos(names)` | Create a temp dir with initialized git repos |
| `init_git_repo(path)` | Initialize a single git repo with one commit |

### Writing new tests

1. Create a new file in `tests/tests_mod/`
2. Add a `#[path]` module entry in `tests/main.rs`
3. Use `setup_git_repos()` to create test fixtures
4. Use `run_rmg()` to execute commands and assert on output

Example:

```rust
use crate::common::{run_rmg, stdout_str, setup_git_repos};

#[test]
fn my_new_test() {
    let tmp = setup_git_repos(&["repo1", "repo2"]);
    let output = run_rmg(tmp.path(), &["list-projects"]);
    assert!(output.status.success());
    let stdout = stdout_str(&output);
    assert!(stdout.contains("repo1"));
}
```
