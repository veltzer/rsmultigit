use crate::common::{commit_file, git, run_rsmultigit, setup_git_repos, stdout_str, utf8};
use std::fs;

#[test]
fn count_dirty_clean_repos() {
    let tmp = setup_git_repos(&["a", "b"]);
    let output = run_rsmultigit(utf8(&tmp), &["git", "count", "dirty"]);
    assert!(output.status.success());
    let stdout = stdout_str(&output);
    assert!(
        stdout.contains("0/2"),
        "clean repos should show 0/2: {stdout}"
    );
}

#[test]
fn count_dirty_with_modified_file() {
    let tmp = setup_git_repos(&["clean", "dirty"]);
    // Create and commit a file in dirty, then modify it
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

    let output = run_rsmultigit(utf8(&tmp), &["git", "count", "dirty"]);
    assert!(output.status.success());
    let stdout = stdout_str(&output);
    assert!(
        stdout.contains("1/2"),
        "one dirty repo should show 1/2: {stdout}"
    );
}

#[test]
fn untracked_detects_new_files() {
    let tmp = setup_git_repos(&["clean", "has_new"]);
    fs::write(utf8(&tmp).join("has_new/untracked.txt"), "data").unwrap();

    let output = run_rsmultigit(utf8(&tmp), &["git", "count", "untracked"]);
    assert!(output.status.success());
    let stdout = stdout_str(&output);
    assert!(
        stdout.contains("1/2"),
        "one repo with untracked should show 1/2: {stdout}"
    );
}

#[test]
fn count_dirty_terse_suppresses_names() {
    let tmp = setup_git_repos(&["a", "b"]);
    fs::write(utf8(&tmp).join("a/untracked.txt"), "x").unwrap();

    // With --terse, project names are suppressed; only the count line remains.
    let output = run_rsmultigit(utf8(&tmp), &["--terse", "git", "count", "untracked"]);
    assert!(output.status.success());
    let stdout = stdout_str(&output);
    assert_eq!(stdout, "1/2");
}

#[test]
fn print_not_inverts_selection() {
    let tmp = setup_git_repos(&["a", "b"]);
    fs::write(utf8(&tmp).join("a/untracked.txt"), "x").unwrap();

    // --print-not should show "b" (the one WITHOUT untracked)
    let output = run_rsmultigit(utf8(&tmp), &["--print-not", "git", "count", "untracked"]);
    assert!(output.status.success());
    let stdout = stdout_str(&output);
    assert!(
        stdout.contains("b"),
        "should print the non-matching repo: {stdout}"
    );
    assert!(
        !stdout.contains("/a\n"),
        "should not print the matching repo"
    );
}

#[test]
fn count_dirty_counts_a_repo_mid_merge() {
    // A conflicted file is dirty even though nothing else is modified.
    let tmp = setup_git_repos(&["clean", "merging"]);
    let repo = utf8(&tmp).join("merging");
    commit_file(&repo, "f.txt", "base", "base");
    let main = git(&repo, &["rev-parse", "--abbrev-ref", "HEAD"]);
    git(&repo, &["checkout", "-q", "-b", "other"]);
    commit_file(&repo, "f.txt", "theirs", "theirs");
    git(&repo, &["checkout", "-q", &main]);
    commit_file(&repo, "f.txt", "ours", "ours");
    let merge = crate::common::git_command()
        .args(["merge", "-q", "other"])
        .current_dir(&repo)
        .output()
        .unwrap();
    assert!(!merge.status.success(), "the merge should conflict");

    let output = run_rsmultigit(utf8(&tmp), &["git", "count", "dirty"]);
    assert!(output.status.success());
    let stdout = stdout_str(&output);
    assert!(stdout.contains("merging"), "{stdout}");
    assert!(stdout.contains("1/2"), "{stdout}");
}
