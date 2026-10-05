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
fn stash_skips_clean_repos_and_never_pops_a_hand_made_stash() {
    // `clean` is clean but holds an old stash of its own; `dirty` has a
    // change; `bare` is clean with no stash at all. push must stash only in
    // `dirty`, and pop must restore only that, leaving the old stash alone.
    let tmp = setup_git_repos(&["bare", "clean", "dirty"]);
    let dir = utf8(&tmp);
    for name in ["clean", "dirty"] {
        commit_file(&dir.join(name), "f.txt", "original", "add f");
    }
    fs::write(dir.join("clean/f.txt"), "old").unwrap();
    git(&dir.join("clean"), &["stash", "push", "-m", "by hand"]);
    fs::write(dir.join("dirty/f.txt"), "changed").unwrap();

    let output = run_rsmultigit(dir, &["git", "stash", "push"]);
    assert!(output.status.success(), "{}", stderr_str(&output));
    let stdout = stdout_str(&output);
    assert!(stdout.contains("dirty]"), "{stdout}");
    assert!(
        !stdout.contains("clean]") && !stdout.contains("bare]"),
        "{stdout}"
    );
    assert_eq!(
        fs::read_to_string(dir.join("dirty/f.txt")).unwrap(),
        "original"
    );

    let output = run_rsmultigit(dir, &["git", "stash", "pop"]);
    assert!(output.status.success(), "{}", stderr_str(&output));
    let stdout = stdout_str(&output);
    assert!(stdout.contains("dirty]"), "{stdout}");
    assert!(
        !stdout.contains("clean]") && !stdout.contains("bare]"),
        "{stdout}"
    );
    assert_eq!(
        fs::read_to_string(dir.join("dirty/f.txt")).unwrap(),
        "changed"
    );
    assert_eq!(
        fs::read_to_string(dir.join("clean/f.txt")).unwrap(),
        "original"
    );
    let list = git(&dir.join("clean"), &["stash", "list", "--format=%s"]);
    assert!(
        list.ends_with(": by hand") && !list.contains('\n'),
        "{list}"
    );
}

#[test]
fn linked_worktrees_are_discovered_as_repos() {
    // A linked worktree's `.git` is a file (`gitdir: ...`), not a directory.
    let tmp = setup_git_repos(&["main"]);
    let dir = utf8(&tmp);
    git(&dir.join("main"), &["worktree", "add", "-q", "../linked"]);
    assert!(dir.join("linked/.git").is_file());
    // A stray `.git` file that is not a gitdir pointer is still no repo.
    fs::create_dir(dir.join("fake")).unwrap();
    fs::write(dir.join("fake/.git"), "not a pointer").unwrap();

    let output = run_rsmultigit(dir, &["list", "repos"]);
    assert!(output.status.success(), "{}", stderr_str(&output));
    let stdout = stdout_str(&output);
    assert!(stdout.contains("/linked"), "{stdout}");
    assert!(stdout.contains("/main"), "{stdout}");
    assert!(!stdout.contains("/fake"), "{stdout}");
}

#[test]
fn stash_pop_finds_its_stash_below_a_newer_hand_made_one() {
    let tmp = setup_git_repos(&["repo"]);
    let dir = utf8(&tmp);
    let repo = dir.join("repo");
    commit_file(&repo, "f.txt", "original", "add f");
    commit_file(&repo, "g.txt", "original", "add g");
    fs::write(repo.join("f.txt"), "ours").unwrap();
    let output = run_rsmultigit(dir, &["git", "stash", "push"]);
    assert!(output.status.success(), "{}", stderr_str(&output));
    fs::write(repo.join("g.txt"), "theirs").unwrap();
    git(&repo, &["stash", "push", "-m", "by hand"]);

    let output = run_rsmultigit(dir, &["git", "stash", "pop"]);
    assert!(output.status.success(), "{}", stderr_str(&output));
    assert_eq!(fs::read_to_string(repo.join("f.txt")).unwrap(), "ours");
    assert_eq!(fs::read_to_string(repo.join("g.txt")).unwrap(), "original");
    let list = git(&repo, &["stash", "list", "--format=%s"]);
    assert!(
        list.ends_with(": by hand") && !list.contains('\n'),
        "{list}"
    );
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

#[test]
fn destructive_commands_skip_untouched_repos_and_dry_run_changes_nothing() {
    // `work` has a staged change, an unstaged change, an untracked file and
    // an ignored file; `idle` has nothing for any of the commands to do.
    let tmp = setup_git_repos(&["idle", "work"]);
    let dir = utf8(&tmp);
    let work = dir.join("work");
    commit_file(&work, ".gitignore", "ignored.txt\n", "ignore");
    commit_file(&work, "staged.txt", "a", "add staged");
    commit_file(&work, "unstaged.txt", "a", "add unstaged");
    fs::write(work.join("staged.txt"), "b").unwrap();
    git(&work, &["add", "staged.txt"]);
    fs::write(work.join("unstaged.txt"), "b").unwrap();
    fs::write(work.join("untracked.txt"), "x").unwrap();
    fs::write(work.join("ignored.txt"), "x").unwrap();
    let before = git(&work, &["status", "--short", "--ignored"]);

    for (args, listed) in [
        (&["git", "clean", "hard", "--dry-run"][..], "ignored.txt"),
        (&["git", "clean", "soft", "--dry-run"], "untracked.txt"),
        (&["git", "reset", "hard", "--dry-run"], "unstaged.txt"),
        (&["git", "reset", "mixed", "--dry-run"], "staged.txt"),
        (&["git", "restore", "--dry-run"], "unstaged.txt"),
    ] {
        let output = run_rsmultigit(dir, args);
        assert!(output.status.success(), "{args:?}: {}", stderr_str(&output));
        let stdout = stdout_str(&output);
        assert!(stdout.contains("work]"), "{args:?}: {stdout}");
        assert!(
            stdout.contains(listed),
            "{args:?} should list {listed}: {stdout}"
        );
        assert!(
            !stdout.contains("idle]"),
            "{args:?} must skip idle: {stdout}"
        );
        assert_eq!(
            git(&work, &["status", "--short", "--ignored"]),
            before,
            "{args:?} must change nothing"
        );
    }
    // soft leaves ignored files alone, so its dry run does not list them.
    let output = run_rsmultigit(dir, &["git", "clean", "soft", "--dry-run"]);
    assert!(!stdout_str(&output).contains("ignored.txt"));

    // The real thing, still skipping the idle repo.
    let output = run_rsmultigit(dir, &["git", "clean", "soft"]);
    assert!(output.status.success(), "{}", stderr_str(&output));
    assert!(!stdout_str(&output).contains("idle]"));
    assert!(!work.join("untracked.txt").exists());
    assert!(work.join("ignored.txt").exists());
}

#[test]
fn reset_soft_is_gone() {
    let tmp = setup_git_repos(&["a"]);
    let output = run_rsmultigit(utf8(&tmp), &["git", "reset", "soft"]);
    assert!(!output.status.success());
}
