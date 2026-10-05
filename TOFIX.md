# TOFIX

Findings from a code scan on 2026-10-05, each verified against the code
(most reproduced on throwaway repos). The four most urgent ones (stash
popping foreign stashes, worktrees left out, conflicts not counted as dirty,
garbled `-j` failure output) are fixed; these remain.

## Medium

- `src/commands/reset.rs:14-16` - `git reset soft` runs `git reset --soft HEAD`, a no-op, while `docs/src/commands.md` says it keeps changes staged. Drop the mode or give it a real target (e.g. `HEAD~1`) behind a confirmation.
- `src/commands/config.rs:20` - git exits 1 both for an unset key and for an invalid one, so `rsmultigit git config <typo>` looks like "unset everywhere". Use git2 (`repo.config()?.get_string`): `NotFound` vs `InvalidSpec` tells them apart and drops a subprocess per repo.
- `src/commands/check.rs:310` with `src/commands/check_run.rs:251` - one unreadable file (`chmod 000`) aborts the whole `check same` run with a bare I/O error: later rules are never reported and the error does not name the rule. Report it per rule and carry on.
- `src/commands/check_run.rs:286-292, 314, 364` - `check same --copy` / `--fix-missing` under `--terse` or `--no-output` do nothing and still exit 0, which scripts read as a pass. Reject the combination, or exit 0 only when the interactive flow actually ran.
- `src/commands/check_run.rs:329-334`, `src/commands/interactive.rs:68` - on a `must_have` rule where every repo that has the file agrees (one group) and some lack it, `--diff` / `--copy` loop on a prompt with no valid answer until `s`. Only run them with two or more groups.
- `src/commands/clean.rs:9`, `reset.rs:9`, `restore.rs:9`, `checkout.rs:9` - destructive fleet-wide commands have no dry-run or confirmation; `git clean hard` (`-ffxd`) deletes every `.venv` and `target/` in the fleet unasked. Add `--dry-run` (as `gh sync-metadata` has) and/or a confirmation, and skip clean repos via a check.
- `src/subprocess_utils.rs` (`run_command`, `run_capturing`, `capture_output*`) - a program that fails to start yields a bare "No such file or directory" that does not name it. Add `.with_context(|| format!("failed to run {name}"))`.
- `src/commands/count.rs:110` with `src/commands/push.rs:9` - `is_ahead` falls back to `origin/<branch>` without an upstream, but plain `git push` refuses such a branch unless the global config sets `push.default current`. Push with `git push origin HEAD` when the fallback applied, or drop the fallback; `docs/src/commands.md` says repos with no upstream are skipped.
- `src/runner.rs:44` - a `--no-stop` failure reads `error in <repo>: error in project <repo>: ...`, naming the repo twice (serial and `-j` alike). Drop one of the two prefixes.

## Low

- `src/commands/age.rs:9`, `authors.rs:9` - both fail on an empty repo (unborn branch) and abort the run. Return `None` when there is no HEAD.
- `src/commands/interactive.rs:85-87` - input is lowercased and matched against `s`/`q` before being parsed as a group label, so groups `Q` (17th) and `S` (19th) can never be picked. Use skip/quit words that cannot be labels.
- `src/commands/check_run.rs:601-608` - `--copy` follows destination symlinks and writes into their target, possibly outside every repo; `hash_file` also groups a symlink by its target's content. Check `symlink_metadata` and refuse or warn.
- `src/commands/check.rs:300, 344` - rule `path` is never validated: an absolute path makes every repo hash the same file (the rule always passes) and `..` escapes the repo. Reject both in `load_config`.
- `src/cli.rs:154`, `src/main.rs:141` - `gh <op> --keep` is silently accepted on every operation but `clean-all`, unlike `--dry-run`, `--fix`, `--upgrade` and `--type`, which are rejected. Make it `Option<usize>` and bail.
- `src/cli.rs:630` - `git count synchronized` counts the repos that are *not* synchronized (`list unsynchronized` has the accurate name), and `count` lacks `ahead` / `behind`, which `list` has. Rename (keep an alias) and add them.
- `src/commands/tag.rs:21`, `last_tag.rs:10`, `age.rs`, `status.rs` (`git dirty`'s two `git diff --stat`) - per-repo git subprocesses where git2 would do, against CLAUDE.md's rule.
- `docs/src/commands.md:194` says `rsmultigit git cargo clean`; it is `rsmultigit cargo clean`. `src/main.rs` carries a doc comment about the `git` group on `run_list_command`.
- `tests/common/mod.rs:150-177` - `init_git_repo` ignores git's exit status and inherits the user's global git config (`commit.gpgsign`, hooks, `GIT_DIR`). Assert success and set `GIT_CONFIG_GLOBAL=/dev/null`, `GIT_CONFIG_NOSYSTEM=1`.

## Test gaps

- Runner unit tests for `do_count` / `print_if_data` assert only `is_ok()`, never the `--print-not` / `--terse` / `--no-header` output; no integration test for `--verbose` on a `_with_check` command.
- No tests for empty (unborn) repos, `git config` with an invalid key, `check same` with one group plus missing repos, symlinked check targets, interactive flags combined with `--terse` / `--no-output`, or `exclude` / `marker_absent` end to end.

## Fleet-shared (fix in rsconstruct's canonical copy)

- `build.rs:90-109` re-runs only on `Cargo.toml`, `.git/HEAD` or ref changes, so `GIT_DIRTY` and `BUILD_TIMESTAMP` go stale after source edits; reading `.git/HEAD` as a file breaks inside a worktree.
