use crate::common::{run_rsmultigit, setup_git_repos, stderr_str};

#[test]
fn run_executes_command_across_all_repos() {
    let tmp = setup_git_repos(&["repo1", "repo2"]);
    let output = run_rsmultigit(tmp.path(), &["run", "touch", "marker.txt"]);
    assert!(
        output.status.success(),
        "run command should succeed: {}",
        stderr_str(&output)
    );

    assert!(tmp.path().join("repo1/marker.txt").exists());
    assert!(tmp.path().join("repo2/marker.txt").exists());
}

#[test]
fn run_executes_single_string_shell_command() {
    let tmp = setup_git_repos(&["repo1", "repo2"]);
    let output = run_rsmultigit(tmp.path(), &["run", "echo hello > greeting.txt"]);
    assert!(
        output.status.success(),
        "run command should succeed: {}",
        stderr_str(&output)
    );

    assert!(tmp.path().join("repo1/greeting.txt").exists());
    assert!(tmp.path().join("repo2/greeting.txt").exists());
    assert_eq!(
        std::fs::read_to_string(tmp.path().join("repo1/greeting.txt"))
            .unwrap()
            .trim(),
        "hello"
    );
}

#[test]
fn run_exec_alias_works() {
    let tmp = setup_git_repos(&["repo1", "repo2"]);
    let output = run_rsmultigit(tmp.path(), &["exec", "touch", "alias_marker.txt"]);
    assert!(
        output.status.success(),
        "exec command should succeed: {}",
        stderr_str(&output)
    );

    assert!(tmp.path().join("repo1/alias_marker.txt").exists());
    assert!(tmp.path().join("repo2/alias_marker.txt").exists());
}

#[test]
fn run_failing_command_stops_by_default() {
    let tmp = setup_git_repos(&["repo1", "repo2"]);
    let output = run_rsmultigit(tmp.path(), &["run", "false"]);
    assert!(!output.status.success());
}

#[test]
fn run_failing_command_continues_with_no_stop() {
    let tmp = setup_git_repos(&["repo1", "repo2"]);
    let output = run_rsmultigit(tmp.path(), &["--no-stop", "run", "false"]);
    assert!(output.status.success());
}
