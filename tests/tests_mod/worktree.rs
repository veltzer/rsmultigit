//! Commands that change the working tree or index of each repo.

use std::fs;

use crate::common::{
    commit_file, current_branch, git, run_rsmultigit, setup_git_repos, stderr_str, stdout_str, utf8,
};

#[test]
fn commit_commits_dirty_repos_and_skips_clean_ones() {
    let tmp = setup_git_repos(&["dirty", "clean"]);
    let dir = utf8(&tmp);
    fs::write(dir.join("dirty/new.txt"), "x").unwrap();

    let output = run_rsmultigit(dir, &["git", "commit", "-m", "shared message"]);
    assert!(output.status.success(), "{}", stderr_str(&output));
    let stdout = stdout_str(&output);
    assert!(stdout.contains("dirty]"), "{stdout}");
    assert!(
        !stdout.contains("clean]"),
        "clean repo must be skipped: {stdout}"
    );
    assert_eq!(
        git(&dir.join("dirty"), &["log", "-1", "--format=%s"]),
        "shared message"
    );
    assert_eq!(git(&dir.join("dirty"), &["status", "--short"]), "");
    assert_eq!(
        git(&dir.join("clean"), &["log", "-1", "--format=%s"]),
        "initial"
    );
}

#[test]
fn checkout_switches_every_repo_to_the_branch() {
    let tmp = setup_git_repos(&["a", "b"]);
    let dir = utf8(&tmp);
    for name in ["a", "b"] {
        git(&dir.join(name), &["branch", "topic"]);
    }
    let output = run_rsmultigit(dir, &["git", "checkout", "topic"]);
    assert!(output.status.success(), "{}", stderr_str(&output));
    for name in ["a", "b"] {
        assert_eq!(current_branch(&dir.join(name)), "topic");
    }
}

#[test]
fn stash_push_then_pop_round_trips_changes() {
    let tmp = setup_git_repos(&["repo"]);
    let dir = utf8(&tmp);
    let repo = dir.join("repo");
    commit_file(&repo, "f.txt", "original", "add f");
    fs::write(repo.join("f.txt"), "changed").unwrap();

    let output = run_rsmultigit(dir, &["git", "stash", "push"]);
    assert!(output.status.success(), "{}", stderr_str(&output));
    assert_eq!(fs::read_to_string(repo.join("f.txt")).unwrap(), "original");

    let output = run_rsmultigit(dir, &["git", "stash", "pop"]);
    assert!(output.status.success(), "{}", stderr_str(&output));
    assert_eq!(fs::read_to_string(repo.join("f.txt")).unwrap(), "changed");
}

#[test]
fn reset_hard_discards_and_reset_mixed_unstages() {
    let tmp = setup_git_repos(&["repo"]);
    let dir = utf8(&tmp);
    let repo = dir.join("repo");
    commit_file(&repo, "f.txt", "original", "add f");

    fs::write(repo.join("f.txt"), "changed").unwrap();
    let output = run_rsmultigit(dir, &["git", "reset", "hard"]);
    assert!(output.status.success(), "{}", stderr_str(&output));
    assert_eq!(fs::read_to_string(repo.join("f.txt")).unwrap(), "original");

    fs::write(repo.join("f.txt"), "staged").unwrap();
    git(&repo, &["add", "f.txt"]);
    assert_eq!(git(&repo, &["diff", "--cached", "--name-only"]), "f.txt");
    let output = run_rsmultigit(dir, &["git", "reset", "mixed"]);
    assert!(output.status.success(), "{}", stderr_str(&output));
    assert_eq!(git(&repo, &["diff", "--cached", "--name-only"]), "");
    assert_eq!(git(&repo, &["diff", "--name-only"]), "f.txt");
    assert_eq!(fs::read_to_string(repo.join("f.txt")).unwrap(), "staged");
}

#[test]
fn restore_discards_unstaged_but_keeps_staged_and_untracked() {
    let tmp = setup_git_repos(&["repo"]);
    let dir = utf8(&tmp);
    let repo = dir.join("repo");
    commit_file(&repo, "f.txt", "original", "add f");
    commit_file(&repo, "g.txt", "original", "add g");

    fs::write(repo.join("g.txt"), "staged").unwrap();
    git(&repo, &["add", "g.txt"]);
    fs::write(repo.join("f.txt"), "changed").unwrap();
    fs::write(repo.join("new.txt"), "untracked").unwrap();

    let output = run_rsmultigit(dir, &["git", "restore"]);
    assert!(output.status.success(), "{}", stderr_str(&output));
    assert_eq!(fs::read_to_string(repo.join("f.txt")).unwrap(), "original");
    assert_eq!(fs::read_to_string(repo.join("g.txt")).unwrap(), "staged");
    assert!(repo.join("new.txt").exists());
}

#[test]
fn diff_shows_the_change_under_the_repo_header() {
    let tmp = setup_git_repos(&["repo"]);
    let dir = utf8(&tmp);
    let repo = dir.join("repo");
    commit_file(&repo, "f.txt", "original\n", "add f");
    fs::write(repo.join("f.txt"), "modified\n").unwrap();

    let output = run_rsmultigit(dir, &["git", "diff"]);
    assert!(output.status.success(), "{}", stderr_str(&output));
    let stdout = stdout_str(&output);
    assert!(stdout.starts_with(&format!("[{repo}]")), "{stdout}");
    assert!(
        stdout.contains("-original") && stdout.contains("+modified"),
        "{stdout}"
    );
}

#[test]
fn gc_and_submodule_update_succeed_on_plain_repos() {
    let tmp = setup_git_repos(&["a", "b"]);
    let dir = utf8(&tmp);
    for cmd in [["git", "gc"], ["git", "submodule-update"]] {
        let output = run_rsmultigit(dir, &cmd);
        assert!(output.status.success(), "{cmd:?}: {}", stderr_str(&output));
        let stdout = stdout_str(&output);
        assert!(
            stdout.contains("[") && stdout.contains("a]") && stdout.contains("b]"),
            "{stdout}"
        );
    }
}

#[test]
fn clean_soft_removes_untracked_but_keeps_ignored() {
    let tmp = setup_git_repos(&["repo"]);
    let dir = utf8(&tmp);
    let repo = dir.join("repo");
    commit_file(&repo, ".gitignore", "ignored.txt\n", "ignore");
    fs::write(repo.join("untracked.txt"), "x").unwrap();
    fs::write(repo.join("ignored.txt"), "x").unwrap();

    let output = run_rsmultigit(dir, &["git", "clean", "soft"]);
    assert!(output.status.success(), "{}", stderr_str(&output));
    assert!(!repo.join("untracked.txt").exists());
    assert!(repo.join("ignored.txt").exists());

    let output = run_rsmultigit(dir, &["git", "clean", "hard"]);
    assert!(output.status.success(), "{}", stderr_str(&output));
    assert!(!repo.join("ignored.txt").exists());
}
