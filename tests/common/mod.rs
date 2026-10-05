#![allow(dead_code)]

use camino::{Utf8Path, Utf8PathBuf};
use std::fs;
use std::io::Write;
use std::process::{Command, Output, Stdio};
use tempfile::TempDir;

/// Write a minimal config file at <dir>/config.toml with `repos = ["<dir>/*"]`
/// and the given `extra` body appended (for `[[check]]` blocks, etc).
/// Returns the config path, to be fed into `RSMULTIGIT_CONFIG`.
pub fn write_config(dir: &Utf8Path, extra: &str) -> Utf8PathBuf {
    let path = dir.join("config.toml");
    let body = format!("repos = [\"{}/*\"]\n\n{}", dir, extra);
    fs::write(&path, body).unwrap();
    path
}

/// Run rsmultigit against a tempdir of git repos, pointing the tool at
/// a config written by `write_config`.
pub fn run_rsmultigit(dir: &Utf8Path, args: &[&str]) -> Output {
    let cfg = write_config(dir, "");
    let cfg_str = cfg.to_string();
    run_rsmultigit_with_env(dir, args, &[("RSMULTIGIT_CONFIG", &cfg_str)])
}

/// Shield a child process — git itself, or rsmultigit, which spawns git —
/// from the developer's git setup: no global or system config (a global
/// `commit.gpgsign`, `push.default` or hook must not change what a test
/// sees), and no inherited `GIT_DIR` / `GIT_WORK_TREE` / `GIT_INDEX_FILE`
/// (set when the suite runs from inside a git hook) pointing git at the
/// wrong repository.
pub fn isolate_git(cmd: &mut Command) -> &mut Command {
    cmd.env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
}

/// A `git` command with the isolation of [`isolate_git`] applied.
pub fn git_command() -> Command {
    let mut cmd = Command::new("git");
    isolate_git(&mut cmd);
    cmd
}

/// Run the rsmultigit binary with extra env vars layered on top of the parent env.
pub fn run_rsmultigit_with_env(dir: &Utf8Path, args: &[&str], env: &[(&str, &str)]) -> Output {
    let bin_path = env!("CARGO_BIN_EXE_rsmultigit");
    let mut cmd = Command::new(bin_path);
    isolate_git(&mut cmd);
    cmd.current_dir(dir).args(args);
    for (k, v) in env {
        cmd.env(k, v);
    }
    cmd.output().expect("Failed to execute rsmultigit")
}

/// Run the rsmultigit binary with piped stdin so tests can feed interactive input.
/// Writes `stdin_bytes` to the child's stdin, then reads the output.
pub fn run_rsmultigit_with_stdin(
    dir: &Utf8Path,
    args: &[&str],
    env: &[(&str, &str)],
    stdin_bytes: &[u8],
) -> Output {
    let bin_path = env!("CARGO_BIN_EXE_rsmultigit");
    let mut cmd = Command::new(bin_path);
    isolate_git(&mut cmd);
    cmd.current_dir(dir)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (k, v) in env {
        cmd.env(k, v);
    }
    let mut child = cmd.spawn().expect("Failed to spawn rsmultigit");
    {
        let stdin = child.stdin.as_mut().expect("stdin must be piped");
        stdin.write_all(stdin_bytes).expect("failed writing stdin");
    }
    child
        .wait_with_output()
        .expect("Failed to wait on rsmultigit")
}

/// The temp directory as a UTF-8 path, which every helper and the binary's
/// output speak. Test temp dirs are always UTF-8, so the conversion never
/// fails in practice.
pub fn utf8(tmp: &TempDir) -> &Utf8Path {
    Utf8Path::from_path(tmp.path()).expect("temp dir path is UTF-8")
}

/// Get stdout from an Output as a trimmed String.
pub fn stdout_str(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

/// Get stderr from an Output as a trimmed String.
pub fn stderr_str(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).trim().to_string()
}

/// Run `git <args>` in `dir`, panicking on failure, and return trimmed stdout.
pub fn git(dir: &Utf8Path, args: &[&str]) -> String {
    let out = git_command()
        .args(args)
        .current_dir(dir)
        .output()
        .expect("git runs");
    assert!(
        out.status.success(),
        "git {args:?} in {dir} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// The checked-out branch name of `dir`.
pub fn current_branch(dir: &Utf8Path) -> String {
    git(dir, &["rev-parse", "--abbrev-ref", "HEAD"])
}

/// Give `repo` an `origin` remote backed by a bare repository at `bare`
/// (created here), and push the current branch to it with tracking set, the
/// way a fresh clone would have it. Returns the bare path.
pub fn add_bare_origin(repo: &Utf8Path, bare: &Utf8Path) {
    let status = git_command()
        .args(["init", "-q", "--bare", bare.as_str()])
        .status()
        .unwrap();
    assert!(status.success(), "git init --bare failed");
    git(repo, &["remote", "add", "origin", bare.as_str()]);
    git(repo, &["push", "-q", "-u", "origin", "HEAD"]);
}

/// A second working clone of `bare` at `path`, with a user configured, for
/// producing commits "somewhere else" that a fetch or pull must bring in.
pub fn clone_of(bare: &Utf8Path, path: &Utf8Path) {
    let status = git_command()
        .args(["clone", "-q", bare.as_str(), path.as_str()])
        .status()
        .unwrap();
    assert!(status.success(), "git clone failed");
    git(path, &["config", "user.email", "test@test.com"]);
    git(path, &["config", "user.name", "Test"]);
}

/// Write `content` to `name` in `repo` and commit it.
pub fn commit_file(repo: &Utf8Path, name: &str, content: &str, message: &str) {
    fs::write(repo.join(name), content).unwrap();
    git(repo, &["add", name]);
    git(repo, &["commit", "-q", "-m", message]);
}

/// Create a temp directory containing `n` fake git repos as immediate subdirectories.
/// Returns the TempDir (caller must hold it to keep the directory alive).
pub fn setup_git_repos(names: &[&str]) -> TempDir {
    let tmp = TempDir::new().expect("Failed to create temp dir");
    for name in names {
        let repo_path = utf8(&tmp).join(name);
        init_git_repo(&repo_path);
    }
    tmp
}

/// Initialise a minimal git repo at `path` with one commit, on `master`
/// whatever git's own default branch is.
pub fn init_git_repo(path: &Utf8Path) {
    fs::create_dir_all(path).unwrap();
    git(path, &["init", "-q", "--initial-branch=master"]);
    git(path, &["config", "user.email", "test@test.com"]);
    git(path, &["config", "user.name", "Test"]);
    // An initial commit so HEAD exists.
    git(path, &["commit", "-q", "--allow-empty", "-m", "initial"]);
}
