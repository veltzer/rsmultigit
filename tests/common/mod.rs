#![allow(dead_code)]

use std::fs;
use std::path::Path;
use std::process::{Command, Output};
use tempfile::TempDir;

/// Run the rsmultigit binary with the given arguments, using `dir` as the working directory.
pub fn run_rmg(dir: &Path, args: &[&str]) -> Output {
    let rmg_path = env!("CARGO_BIN_EXE_rsmultigit");
    Command::new(rmg_path)
        .current_dir(dir)
        .args(args)
        .output()
        .expect("Failed to execute rsmultigit")
}

/// Get stdout from an Output as a trimmed String.
pub fn stdout_str(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

/// Get stderr from an Output as a trimmed String.
pub fn stderr_str(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).trim().to_string()
}

/// Create a temp directory containing `n` fake git repos as immediate subdirectories.
/// Returns the TempDir (caller must hold it to keep the directory alive).
pub fn setup_git_repos(names: &[&str]) -> TempDir {
    let tmp = TempDir::new().expect("Failed to create temp dir");
    for name in names {
        let repo_path = tmp.path().join(name);
        init_git_repo(&repo_path);
    }
    tmp
}

/// Initialise a minimal git repo at `path` with one commit.
pub fn init_git_repo(path: &Path) {
    fs::create_dir_all(path).unwrap();
    let status = Command::new("git")
        .args(["init", "-q"])
        .current_dir(path)
        .status()
        .unwrap();
    assert!(status.success(), "git init failed");

    // Configure user for the repo so commits work
    Command::new("git")
        .args(["config", "user.email", "test@test.com"])
        .current_dir(path)
        .status()
        .unwrap();
    Command::new("git")
        .args(["config", "user.name", "Test"])
        .current_dir(path)
        .status()
        .unwrap();

    // Create an initial commit so HEAD exists
    Command::new("git")
        .args(["commit", "--allow-empty", "-m", "initial"])
        .current_dir(path)
        .status()
        .unwrap();
}
