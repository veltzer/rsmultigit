# TOFIX

Findings from a code scan on 2026-10-04.

## Low

- `src/subprocess_utils.rs:65-67` - when building the venv-prefixed PATH, every non-UTF-8 PATH entry is silently dropped by `filter_map(Utf8PathBuf::from_path_buf(..).ok())`. There is no need for UTF-8 here; build the list from `std::path::PathBuf` (`std::env::split_paths` + `venv_bin.into_std_path_buf()`) so the ambient PATH is preserved exactly.
- `src/subprocess_utils.rs:55-56` - the doc comment says "The command itself still comes from the ambient PATH", but Rust resolves the program through the child's modified PATH, and both `check_call_ve_env_prefers_venv_tools` (`src/subprocess_utils.rs:242`) and `do_run_venv_activates_local_venv` (`src/commands/run.rs:102`) rely on the venv copy being found. Fix the comment to match `src/commands/run.rs:14-16`.
- `src/subprocess_utils.rs:141-143` - in capture mode a failing command's stdout and stderr are appended one after the other, so the replayed output loses interleaving and stderr ends up on stdout (`src/runner.rs:316`). Either keep the two streams separate and replay stderr to stderr, or document that `-j`/`--no-output` merge them.
- `CLAUDE.md:30` - documents `make test` ("Runs nextest in both release and debug"), but the repo has no Makefile. Drop the line or point at the real command.
- `CLAUDE.md:58` - says `do_count` is "Boolean test per repo using git2 (no subprocess)", but `tag has-local` / `tag has-remote` (`src/main.rs:316-322`) go through `do_count` and spawn `git tag` / `git ls-remote` (`src/commands/tag.rs:21`, `src/commands/tag.rs:27`). Correct the description (and the list of users of `do_count`).
