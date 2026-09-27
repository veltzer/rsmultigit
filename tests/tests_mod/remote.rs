//! Commands that talk to a remote, exercised against a local bare
//! repository standing in for origin, so nothing touches the network.

use std::fs;

use crate::common::{
    add_bare_origin, clone_of, commit_file, current_branch, git, run_rsmultigit, setup_git_repos,
    stderr_str, stdout_str, utf8,
};

#[test]
fn push_pushes_only_repos_that_are_ahead() {
    let tmp = setup_git_repos(&["ahead", "insync"]);
    let dir = utf8(&tmp);
    let bares = tempfile::TempDir::new().unwrap();
    for name in ["ahead", "insync"] {
        add_bare_origin(&dir.join(name), &utf8(&bares).join(name));
    }
    commit_file(&dir.join("ahead"), "new.txt", "x", "local only");
    let local_tip = git(&dir.join("ahead"), &["rev-parse", "HEAD"]);

    let output = run_rsmultigit(dir, &["push"]);
    assert!(output.status.success(), "{}", stderr_str(&output));
    let stdout = stdout_str(&output);
    assert!(
        stdout.contains("[") && stdout.contains("ahead]"),
        "{stdout}"
    );
    assert!(
        !stdout.contains("insync]"),
        "in-sync repo must be skipped: {stdout}"
    );

    let remote_tip = git(&utf8(&bares).join("ahead"), &["rev-parse", "HEAD"]);
    assert_eq!(
        remote_tip, local_tip,
        "the commit must have arrived on origin"
    );
}

#[test]
fn fetch_updates_tracking_ref_and_status_reports_behind() {
    let tmp = setup_git_repos(&["repo"]);
    let dir = utf8(&tmp);
    let repo = dir.join("repo");
    let bares = tempfile::TempDir::new().unwrap();
    let bare = utf8(&bares).join("repo");
    add_bare_origin(&repo, &bare);
    let clone = utf8(&bares).join("clone");
    clone_of(&bare, &clone);
    commit_file(&clone, "elsewhere.txt", "x", "made elsewhere");
    git(&clone, &["push", "-q", "origin", "HEAD"]);
    let head_before = git(&repo, &["rev-parse", "HEAD"]);

    let output = run_rsmultigit(dir, &["fetch"]);
    assert!(output.status.success(), "{}", stderr_str(&output));
    assert_eq!(
        git(&repo, &["rev-parse", "HEAD"]),
        head_before,
        "fetch must not move HEAD"
    );
    let branch = current_branch(&repo);
    let tracking = git(&repo, &["rev-parse", &format!("origin/{branch}")]);
    assert_eq!(tracking, git(&clone, &["rev-parse", "HEAD"]));

    let output = run_rsmultigit(dir, &["status"]);
    assert!(
        stdout_str(&output).contains("behind 1"),
        "{}",
        stdout_str(&output)
    );
}

#[test]
fn pull_fast_forwards_to_the_remote() {
    let tmp = setup_git_repos(&["repo"]);
    let dir = utf8(&tmp);
    let repo = dir.join("repo");
    let bares = tempfile::TempDir::new().unwrap();
    let bare = utf8(&bares).join("repo");
    add_bare_origin(&repo, &bare);
    let clone = utf8(&bares).join("clone");
    clone_of(&bare, &clone);
    commit_file(&clone, "elsewhere.txt", "hello", "made elsewhere");
    git(&clone, &["push", "-q", "origin", "HEAD"]);

    let output = run_rsmultigit(dir, &["pull", "--quiet"]);
    assert!(output.status.success(), "{}", stderr_str(&output));
    assert_eq!(
        git(&repo, &["rev-parse", "HEAD"]),
        git(&clone, &["rev-parse", "HEAD"])
    );
    assert_eq!(
        fs::read_to_string(repo.join("elsewhere.txt")).unwrap(),
        "hello"
    );
}

#[test]
fn prune_drops_remote_tracking_branches_deleted_upstream() {
    let tmp = setup_git_repos(&["repo"]);
    let dir = utf8(&tmp);
    let repo = dir.join("repo");
    let bares = tempfile::TempDir::new().unwrap();
    let bare = utf8(&bares).join("repo");
    add_bare_origin(&repo, &bare);
    let clone = utf8(&bares).join("clone");
    clone_of(&bare, &clone);
    git(&clone, &["checkout", "-q", "-b", "feature"]);
    git(&clone, &["push", "-q", "-u", "origin", "feature"]);
    git(&repo, &["fetch", "-q"]);
    assert!(git(&repo, &["branch", "-r"]).contains("origin/feature"));
    git(&clone, &["push", "-q", "origin", "--delete", "feature"]);

    let output = run_rsmultigit(dir, &["prune"]);
    assert!(output.status.success(), "{}", stderr_str(&output));
    assert!(!git(&repo, &["branch", "-r"]).contains("origin/feature"));
}

#[test]
fn remote_and_branch_remote_show_origin() {
    let tmp = setup_git_repos(&["repo"]);
    let dir = utf8(&tmp);
    let bares = tempfile::TempDir::new().unwrap();
    let bare = utf8(&bares).join("repo");
    add_bare_origin(&dir.join("repo"), &bare);

    let output = run_rsmultigit(dir, &["remote"]);
    assert!(output.status.success());
    let stdout = stdout_str(&output);
    assert!(
        stdout.contains(&format!("origin\t{bare} (fetch)")),
        "{stdout}"
    );

    let output = run_rsmultigit(dir, &["branch", "remote"]);
    assert!(output.status.success());
    let branch = current_branch(&dir.join("repo"));
    assert!(stdout_str(&output).contains(&format!("origin/{branch}")));
}

#[test]
fn tag_remote_and_has_remote_see_pushed_tags() {
    let tmp = setup_git_repos(&["tagged", "bare-only"]);
    let dir = utf8(&tmp);
    let bares = tempfile::TempDir::new().unwrap();
    for name in ["tagged", "bare-only"] {
        add_bare_origin(&dir.join(name), &utf8(&bares).join(name));
    }
    git(&dir.join("tagged"), &["tag", "v1.0"]);
    git(&dir.join("tagged"), &["push", "-q", "origin", "v1.0"]);

    let output = run_rsmultigit(dir, &["tag", "remote"]);
    assert!(output.status.success(), "{}", stderr_str(&output));
    assert!(stdout_str(&output).contains("refs/tags/v1.0"));

    let output = run_rsmultigit(dir, &["tag", "has-remote"]);
    assert!(output.status.success(), "{}", stderr_str(&output));
    let stdout = stdout_str(&output);
    assert!(stdout.contains("tagged"), "{stdout}");
    assert!(!stdout.contains("bare-only"), "{stdout}");
    assert!(stdout.ends_with("1/2"), "{stdout}");
}
