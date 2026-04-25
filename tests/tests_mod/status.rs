use crate::common::{run_rsmultigit, setup_git_repos, stdout_str};
use std::fs;

#[test]
fn status_clean_repos_no_output() {
    let tmp = setup_git_repos(&["a", "b"]);
    let output = run_rsmultigit(tmp.path(), &["status"]);
    assert!(output.status.success());
    // Clean repos have no status output, so nothing should be printed
    let stdout = stdout_str(&output);
    assert!(
        stdout.is_empty(),
        "clean repos should produce no status output: {stdout}"
    );
}

#[test]
fn status_shows_dirty_repo() {
    let tmp = setup_git_repos(&["clean", "dirty"]);
    let dirty_path = tmp.path().join("dirty");
    let file = dirty_path.join("file.txt");
    fs::write(&file, "original").unwrap();
    std::process::Command::new("git")
        .args(["add", "file.txt"])
        .current_dir(&dirty_path)
        .status()
        .unwrap();
    std::process::Command::new("git")
        .args(["commit", "-m", "add file"])
        .current_dir(&dirty_path)
        .status()
        .unwrap();
    fs::write(&file, "modified").unwrap();

    let output = run_rsmultigit(tmp.path(), &["status"]);
    assert!(output.status.success());
    let stdout = stdout_str(&output);
    assert!(
        stdout.contains("dirty"),
        "should show the dirty repo: {stdout}"
    );
    assert!(
        stdout.contains("file.txt"),
        "should mention the changed file: {stdout}"
    );
}

#[test]
fn dirty_subcommand_shows_diff_stat() {
    let tmp = setup_git_repos(&["repo"]);
    let repo_path = tmp.path().join("repo");
    let file = repo_path.join("hello.txt");
    fs::write(&file, "hello").unwrap();
    std::process::Command::new("git")
        .args(["add", "hello.txt"])
        .current_dir(&repo_path)
        .status()
        .unwrap();
    std::process::Command::new("git")
        .args(["commit", "-m", "add hello"])
        .current_dir(&repo_path)
        .status()
        .unwrap();
    fs::write(&file, "changed").unwrap();

    let output = run_rsmultigit(tmp.path(), &["dirty"]);
    assert!(output.status.success());
    let stdout = stdout_str(&output);
    assert!(
        stdout.contains("repo"),
        "should show the repo name: {stdout}"
    );
}
