use crate::common::{run_rsmultigit, setup_git_repos, stderr_str, stdout_str, utf8};

#[test]
fn run_executes_command_across_all_repos() {
    let tmp = setup_git_repos(&["repo1", "repo2"]);
    let output = run_rsmultigit(utf8(&tmp), &["run", "touch", "marker.txt"]);
    assert!(
        output.status.success(),
        "run command should succeed: {}",
        stderr_str(&output)
    );

    assert!(utf8(&tmp).join("repo1/marker.txt").exists());
    assert!(utf8(&tmp).join("repo2/marker.txt").exists());
}

#[test]
fn run_executes_single_string_shell_command() {
    let tmp = setup_git_repos(&["repo1", "repo2"]);
    let output = run_rsmultigit(utf8(&tmp), &["run", "echo hello > greeting.txt"]);
    assert!(
        output.status.success(),
        "run command should succeed: {}",
        stderr_str(&output)
    );

    assert!(utf8(&tmp).join("repo1/greeting.txt").exists());
    assert!(utf8(&tmp).join("repo2/greeting.txt").exists());
    assert_eq!(
        std::fs::read_to_string(utf8(&tmp).join("repo1/greeting.txt"))
            .unwrap()
            .trim(),
        "hello"
    );
}

#[test]
fn run_exec_alias_works() {
    let tmp = setup_git_repos(&["repo1", "repo2"]);
    let output = run_rsmultigit(utf8(&tmp), &["exec", "touch", "alias_marker.txt"]);
    assert!(
        output.status.success(),
        "exec command should succeed: {}",
        stderr_str(&output)
    );

    assert!(utf8(&tmp).join("repo1/alias_marker.txt").exists());
    assert!(utf8(&tmp).join("repo2/alias_marker.txt").exists());
}

#[test]
fn run_failing_command_stops_by_default() {
    let tmp = setup_git_repos(&["repo1", "repo2"]);
    let output = run_rsmultigit(utf8(&tmp), &["run", "false"]);
    assert!(!output.status.success());
}

#[test]
fn run_failing_command_continues_with_no_stop_but_exits_non_zero() {
    for jobs in ["1", "2"] {
        let tmp = setup_git_repos(&["repo1", "repo2"]);
        let dir = utf8(&tmp);
        let output = run_rsmultigit(
            dir,
            &["-j", jobs, "--no-stop", "run", "touch", "x", "missing/y"],
        );
        // Both repos ran despite the first failing...
        assert!(dir.join("repo1/x").exists(), "jobs={jobs}");
        assert!(dir.join("repo2/x").exists(), "jobs={jobs}");
        // ...and the run still reports the failures.
        assert!(!output.status.success(), "jobs={jobs}");
        assert!(
            stderr_str(&output).contains("2 of 2 repos failed"),
            "jobs={jobs}: {}",
            stderr_str(&output)
        );
    }
}

#[test]
fn run_no_output_keeps_headers_and_drops_command_output() {
    for jobs in ["1", "2"] {
        let tmp = setup_git_repos(&["repo1", "repo2"]);
        let dir = utf8(&tmp);
        let output = run_rsmultigit(dir, &["-j", jobs, "--no-output", "run", "echo", "VISIBLE"]);
        assert!(output.status.success(), "{}", stderr_str(&output));
        let stdout = stdout_str(&output);
        assert_eq!(
            stdout,
            format!("[{}]\n[{}]", dir.join("repo1"), dir.join("repo2")),
            "jobs={jobs}"
        );
    }
}

#[test]
fn run_no_output_error_still_carries_the_command_output() {
    let tmp = setup_git_repos(&["repo1"]);
    let dir = utf8(&tmp);
    let output = run_rsmultigit(dir, &["--no-output", "run", "sh -c 'echo WHY; exit 3'"]);
    assert!(!output.status.success());
    assert_eq!(stdout_str(&output), format!("[{}]", dir.join("repo1")));
    let stderr = stderr_str(&output);
    assert!(stderr.contains("WHY"), "{stderr}");
}

/// Commands that format their own lines (grep) must land under their repo's
/// header, in repo order, when workers run concurrently.
#[test]
fn parallel_grep_output_stays_in_repo_order() {
    let names = ["r1", "r2", "r3", "r4", "r5", "r6"];
    let tmp = setup_git_repos(&names);
    let dir = utf8(&tmp);
    for name in names {
        let repo = dir.join(name);
        std::fs::write(repo.join("f.txt"), "needle\n").unwrap();
        for args in [vec!["add", "f.txt"], vec!["commit", "-q", "-m", "add"]] {
            let ok = std::process::Command::new("git")
                .args(&args)
                .current_dir(&repo)
                .status()
                .unwrap()
                .success();
            assert!(ok);
        }
    }
    let output = run_rsmultigit(dir, &["-j", "4", "git", "grep", "needle"]);
    assert!(output.status.success(), "{}", stderr_str(&output));
    let expected: Vec<String> = names
        .iter()
        .map(|n| format!("[{}]\n{n}: f.txt:1:needle", dir.join(n)))
        .collect();
    assert_eq!(stdout_str(&output), expected.join("\n"));
}

/// A pattern starting with `-` is a pattern, not a `git grep` option
/// (`-foo` would otherwise be `-f oo`, "read patterns from file oo").
#[test]
fn grep_pattern_starting_with_dash_is_searched_for() {
    let tmp = setup_git_repos(&["r1"]);
    let dir = utf8(&tmp);
    let repo = dir.join("r1");
    std::fs::write(repo.join("f.txt"), "x -foo y\n").unwrap();
    let ok = std::process::Command::new("git")
        .args(["add", "f.txt"])
        .current_dir(&repo)
        .status()
        .unwrap()
        .success();
    assert!(ok);
    let output = run_rsmultigit(dir, &["git", "grep", "--", "-foo"]);
    assert!(output.status.success(), "{}", stderr_str(&output));
    assert_eq!(
        stdout_str(&output),
        format!("[{repo}]\nr1: f.txt:1:x -foo y")
    );
}

#[test]
fn a_failing_repo_prints_the_same_under_j_as_serially() {
    // Header, the repo's own output on its own streams, then the error:
    // byte for byte the same whether the repos ran one by one or in parallel.
    // The sleep keeps the two writes apart: written back to back, two pipes
    // may deliver them in either order, which capture cannot know.
    let tmp = setup_git_repos(&["a", "b"]);
    let dir = utf8(&tmp);
    for flags in [&["--no-stop"][..], &["--no-stop", "--no-output"], &[]] {
        let run = |jobs: &str| {
            let mut args = vec!["-j", jobs];
            args.extend_from_slice(flags);
            args.extend(["run", "echo OUT; sleep 0.1; echo ERR >&2; exit 1"]);
            run_rsmultigit(dir, &args)
        };
        let serial = run("1");
        let parallel = run("2");
        assert!(!serial.status.success(), "{flags:?}");
        assert_eq!(serial.status.code(), parallel.status.code(), "{flags:?}");
        assert_eq!(
            String::from_utf8_lossy(&serial.stdout),
            String::from_utf8_lossy(&parallel.stdout),
            "stdout differs for {flags:?}"
        );
        assert_eq!(
            String::from_utf8_lossy(&serial.stderr),
            String::from_utf8_lossy(&parallel.stderr),
            "stderr differs for {flags:?}"
        );
    }
    // The repo's stderr stays on stderr and its stdout under its header.
    let output = run_rsmultigit(
        dir,
        &[
            "-j",
            "2",
            "--no-stop",
            "run",
            "echo OUT; echo ERR >&2; exit 1",
        ],
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("a]\nOUT\n"), "{stdout}");
    assert!(!stdout.contains("ERR"), "{stdout}");
    assert!(stderr_str(&output).contains("ERR"));
}
