use crate::common::{run_rsmultigit, setup_git_repos, stdout_str, utf8};
use std::fs;

#[test]
fn status_clean_repos_no_output() {
    let tmp = setup_git_repos(&["a", "b"]);
    let output = run_rsmultigit(utf8(&tmp), &["git", "status"]);
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
    let dirty_path = utf8(&tmp).join("dirty");
    let file = dirty_path.join("file.txt");
    fs::write(&file, "original").unwrap();
    crate::common::git_command()
        .args(["add", "file.txt"])
        .current_dir(&dirty_path)
        .status()
        .unwrap();
    crate::common::git_command()
        .args(["commit", "-m", "add file"])
        .current_dir(&dirty_path)
        .status()
        .unwrap();
    fs::write(&file, "modified").unwrap();

    let output = run_rsmultigit(utf8(&tmp), &["git", "status"]);
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

    let output = run_rsmultigit(utf8(&tmp), &["--verbose", "git", "status"]);
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
fn top_level_status_matches_git_status() {
    let tmp = setup_git_repos(&["clean", "dirty"]);
    fs::write(utf8(&tmp).join("dirty").join("new.txt"), "x").unwrap();

    for flags in [&[][..], &["--verbose"][..]] {
        let git_args: Vec<&str> = flags.iter().copied().chain(["git", "status"]).collect();
        let top_args: Vec<&str> = flags.iter().copied().chain(["status"]).collect();
        let via_git = run_rsmultigit(utf8(&tmp), &git_args);
        let via_top = run_rsmultigit(utf8(&tmp), &top_args);
        assert!(via_git.status.success());
        assert!(via_top.status.success());
        let stdout = stdout_str(&via_top);
        assert!(
            stdout.contains("dirty"),
            "should show the dirty repo: {stdout}"
        );
        assert_eq!(
            stdout_str(&via_git),
            stdout,
            "`status` should be exactly `git status` (flags {flags:?})"
        );
    }
}

#[test]
fn status_shows_repo_with_unpushed_commits() {
    let tmp = setup_git_repos(&["insync", "ahead"]);
    let repo_path = utf8(&tmp).join("ahead");

    // Mark the current commit as the upstream tip, then commit past it so the
    // repo is ahead of origin with a clean working tree.
    let branch = String::from_utf8(
        crate::common::git_command()
            .args(["rev-parse", "--abbrev-ref", "HEAD"])
            .current_dir(&repo_path)
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap();
    let branch = branch.trim();
    crate::common::git_command()
        .args([
            "update-ref",
            &format!("refs/remotes/origin/{branch}"),
            "HEAD",
        ])
        .current_dir(&repo_path)
        .status()
        .unwrap();
    crate::common::git_command()
        .args(["commit", "--allow-empty", "-m", "local only"])
        .current_dir(&repo_path)
        .status()
        .unwrap();

    let output = run_rsmultigit(utf8(&tmp), &["git", "status"]);
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

    let output = run_rsmultigit(utf8(&tmp), &["--verbose", "git", "status"]);
    assert!(output.status.success());
    let stdout = stdout_str(&output);
    assert!(
        stdout.contains("ahead of upstream by 1 commit"),
        "verbose should explain the unpushed commit: {stdout}"
    );
}

#[test]
fn dirty_subcommand_shows_diff_stat() {
    let tmp = setup_git_repos(&["repo"]);
    let repo_path = utf8(&tmp).join("repo");
    let file = repo_path.join("hello.txt");
    fs::write(&file, "hello").unwrap();
    crate::common::git_command()
        .args(["add", "hello.txt"])
        .current_dir(&repo_path)
        .status()
        .unwrap();
    crate::common::git_command()
        .args(["commit", "-m", "add hello"])
        .current_dir(&repo_path)
        .status()
        .unwrap();
    fs::write(&file, "changed").unwrap();

    let output = run_rsmultigit(utf8(&tmp), &["git", "dirty"]);
    assert!(output.status.success());
    let stdout = stdout_str(&output);
    assert!(
        stdout.contains("repo"),
        "should show the repo name: {stdout}"
    );
}
