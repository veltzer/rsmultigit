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
        stdout.contains("1 modified"),
        "default status should summarize the situation: {stdout}"
    );
    assert!(
        !stdout.contains("file.txt"),
        "default status is a summary: should not include per-file git output: {stdout}"
    );

    let output = run_rsmultigit(tmp.path(), &["--verbose", "status"]);
    assert!(output.status.success());
    let stdout = stdout_str(&output);
    assert!(
        stdout.contains("dirty"),
        "verbose should show the dirty repo: {stdout}"
    );
    assert!(
        stdout.contains("file.txt"),
        "verbose should mention the changed file: {stdout}"
    );
}

#[test]
fn status_shows_repo_with_unpushed_commits() {
    let tmp = setup_git_repos(&["insync", "ahead"]);
    let repo_path = tmp.path().join("ahead");

    // Mark the current commit as the upstream tip, then commit past it so the
    // repo is ahead of origin with a clean working tree.
    let branch = String::from_utf8(
        std::process::Command::new("git")
            .args(["rev-parse", "--abbrev-ref", "HEAD"])
            .current_dir(&repo_path)
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap();
    let branch = branch.trim();
    std::process::Command::new("git")
        .args([
            "update-ref",
            &format!("refs/remotes/origin/{branch}"),
            "HEAD",
        ])
        .current_dir(&repo_path)
        .status()
        .unwrap();
    std::process::Command::new("git")
        .args(["commit", "--allow-empty", "-m", "local only"])
        .current_dir(&repo_path)
        .status()
        .unwrap();

    let output = run_rsmultigit(tmp.path(), &["status"]);
    assert!(output.status.success());
    let stdout = stdout_str(&output);
    assert!(
        stdout.contains("ahead 1"),
        "should summarize the repo that is ahead of origin: {stdout}"
    );
    assert!(
        !stdout.contains("insync"),
        "repo without upstream and no changes should not be listed: {stdout}"
    );

    let output = run_rsmultigit(tmp.path(), &["--verbose", "status"]);
    assert!(output.status.success());
    let stdout = stdout_str(&output);
    assert!(
        stdout.contains("ahead of origin by 1 commit"),
        "verbose should explain the unpushed commit: {stdout}"
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
