# TOFIX

Findings from a code scan on 2026-10-04.

## Medium

- `src/runner.rs:182`, `src/runner.rs:268`, `src/runner.rs:323`, `src/runner.rs:390` - under `--no-stop` every per-repo error is printed to stderr and swallowed, and `main` then returns `Ok(())`, so `rsmultigit --no-stop build` (or `pull`, `push`, `run ...`) exits 0 even when repos failed; scripts and agents cannot tell a clean run from a broken one. Count the errors the runners report and exit non-zero at the end when any occurred; `tests/tests_mod/run.rs:59` (`run_failing_command_continues_with_no_stop`) currently asserts the exit-0 behaviour and must be updated with it.
- `src/commands/check_run.rs:514-519` and `src/commands/check_run.rs:582-596` - `check-same --copy` / `--fix-missing` write `error: ...` when a copy or mkdir fails but carry on and the command exits 0 (`src/commands/check_run.rs:292`); a failed write is not "success". Propagate a failure flag from `run_copy`/`run_fix_missing` and exit non-zero when any file could not be written.
- `src/commands/grep.rs:15` - the user's pattern is pushed straight into the `git grep` argv, so a pattern starting with `-` (`rsmultigit grep -- -foo`) is parsed by git as options (`-f oo` reads patterns from a file named `oo`). Pass it as `-e <regexp>`.
- `src/commands/check.rs:12`, `src/commands/check.rs:43`, `src/commands/check.rs:85` - `CheckConfig`, `ExistsRule` and `Rule` lack `#[serde(deny_unknown_fields)]`, so a typo in `~/.config/rsmultigit/config.toml` (`must_hav = true`, `exlude = ...`, `marker_abesnt = ...`) is silently ignored and the rule quietly checks something other than intended. Add `deny_unknown_fields` to all three structs plus a unit test for a misspelled key.

## Low

- `src/subprocess_utils.rs:65-67` - when building the venv-prefixed PATH, every non-UTF-8 PATH entry is silently dropped by `filter_map(Utf8PathBuf::from_path_buf(..).ok())`. There is no need for UTF-8 here; build the list from `std::path::PathBuf` (`std::env::split_paths` + `venv_bin.into_std_path_buf()`) so the ambient PATH is preserved exactly.
- `src/subprocess_utils.rs:55-56` - the doc comment says "The command itself still comes from the ambient PATH", but Rust resolves the program through the child's modified PATH, and both `check_call_ve_env_prefers_venv_tools` (`src/subprocess_utils.rs:242`) and `do_run_venv_activates_local_venv` (`src/commands/run.rs:102`) rely on the venv copy being found. Fix the comment to match `src/commands/run.rs:14-16`.
- `src/subprocess_utils.rs:141-143` - in capture mode a failing command's stdout and stderr are appended one after the other, so the replayed output loses interleaving and stderr ends up on stdout (`src/runner.rs:316`). Either keep the two streams separate and replay stderr to stderr, or document that `-j`/`--no-output` merge them.
- `CLAUDE.md:30` - documents `make test` ("Runs nextest in both release and debug"), but the repo has no Makefile. Drop the line or point at the real command.
- `CLAUDE.md:58` - says `do_count` is "Boolean test per repo using git2 (no subprocess)", but `tag has-local` / `tag has-remote` (`src/main.rs:316-322`) go through `do_count` and spawn `git tag` / `git ls-remote` (`src/commands/tag.rs:21`, `src/commands/tag.rs:27`). Correct the description (and the list of users of `do_count`).
