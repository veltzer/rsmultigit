use std::fs;

use camino::Utf8Path;

use crate::common::{
    commit_file, git, run_rsmultigit, run_rsmultigit_with_env, setup_git_repos, stderr_str,
    stdout_str, utf8, write_config,
};

/// Add `lib` as the submodule `shared/lib` of `repo` and commit it. The file
/// protocol is off by default in modern git, so it is allowed per command.
fn add_submodule(repo: &Utf8Path, lib: &Utf8Path) {
    git(
        repo,
        &[
            "-c",
            "protocol.file.allow=always",
            "submodule",
            "add",
            lib.as_str(),
            "shared/lib",
        ],
    );
    git(repo, &["commit", "-q", "-m", "add submodule"]);
    // The clone inherits no identity (the tests blank the global config), and
    // one test commits inside it.
    let checkout = repo.join("shared/lib");
    git(&checkout, &["config", "user.email", "test@test.com"]);
    git(&checkout, &["config", "user.name", "Test"]);
}

/// Two consumers of one library, pinned to the same commit.
fn fleet_with_shared_submodule() -> tempfile::TempDir {
    let tmp = setup_git_repos(&["a", "b", "lib"]);
    let dir = utf8(&tmp);
    commit_file(&dir.join("lib"), "lib.txt", "v1\n", "lib v1");
    add_submodule(&dir.join("a"), &dir.join("lib"));
    add_submodule(&dir.join("b"), &dir.join("lib"));
    tmp
}

#[test]
fn check_submodules_reports_ok_when_every_repo_pins_the_same_commit() {
    let tmp = fleet_with_shared_submodule();
    let dir = utf8(&tmp);
    let pin = git(
        &dir.join("a/shared/lib"),
        &["rev-parse", "--short=7", "HEAD"],
    );

    let output = run_rsmultigit(dir, &["check", "submodules"]);
    assert!(output.status.success(), "{}", stderr_str(&output));
    let stdout = stdout_str(&output);
    assert!(stdout.starts_with("[lib] url="), "{stdout}");
    assert!(
        stdout.ends_with(&format!("\nok (2 repos @ {pin})")),
        "{stdout}"
    );

    let output = run_rsmultigit(dir, &["check", "submodules", "--only-failed"]);
    assert!(output.status.success());
    assert_eq!(stdout_str(&output), "");
}

#[test]
fn check_submodules_fails_when_one_repo_moves_its_pin() {
    let tmp = fleet_with_shared_submodule();
    let dir = utf8(&tmp);
    let old = git(
        &dir.join("a/shared/lib"),
        &["rev-parse", "--short=7", "HEAD"],
    );
    // b bumps its checkout of lib; a stays behind.
    commit_file(&dir.join("b/shared/lib"), "lib.txt", "v2\n", "lib v2");
    let new = git(
        &dir.join("b/shared/lib"),
        &["rev-parse", "--short=7", "HEAD"],
    );
    assert_ne!(old, new);

    let output = run_rsmultigit(dir, &["check", "submodules"]);
    assert_eq!(output.status.code(), Some(1));
    let stdout = stdout_str(&output);
    let lines: Vec<&str> = stdout.lines().collect();
    assert!(lines[0].starts_with("[lib] url="), "{stdout}");
    assert_eq!(lines[1], "2 repos, 2 commits", "{stdout}");
    // Groups are the same size, so their order is the commit order; find each.
    for (pin, repo) in [(&old, "a"), (&new, "b")] {
        let head = format!("  {pin} (1 repo):");
        let member = format!("    {} (shared/lib)", dir.join(repo));
        let at = lines
            .iter()
            .position(|l| *l == head)
            .unwrap_or_else(|| panic!("{stdout}"));
        assert_eq!(lines[at + 1], member, "{stdout}");
    }

    let output = run_rsmultigit(dir, &["--terse", "check", "submodules"]);
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(stdout_str(&output), "lib");

    let output = run_rsmultigit(dir, &["--no-output", "check", "submodules"]);
    assert_eq!(
        stdout_str(&output).lines().count(),
        2,
        "{}",
        stdout_str(&output)
    );
}

#[test]
fn check_all_includes_submodules() {
    let tmp = fleet_with_shared_submodule();
    let dir = utf8(&tmp);
    for name in ["a", "b"] {
        fs::write(dir.join(name).join(".gitignore"), "same\n").unwrap();
    }
    let cfg = write_config(
        dir,
        "[[check]]\nname = \"gi\"\nselect = \"*\"\npath = \".gitignore\"\n",
    );
    let env = [("RSMULTIGIT_CONFIG", cfg.as_str())];

    let output = run_rsmultigit_with_env(dir, &["check", "all", "--only-failed"], &env);
    assert!(output.status.success(), "{}", stderr_str(&output));
    assert_eq!(stdout_str(&output), "");

    commit_file(&dir.join("b/shared/lib"), "lib.txt", "v2\n", "lib v2");
    let output = run_rsmultigit_with_env(dir, &["check", "all", "--only-failed"], &env);
    assert_eq!(output.status.code(), Some(1));
    let stdout = stdout_str(&output);
    assert!(stdout.starts_with("[lib] url="), "{stdout}");
    assert!(!stdout.contains("[gi]"), "{stdout}");
}

#[test]
fn a_fleet_without_submodules_passes() {
    let tmp = setup_git_repos(&["a", "b"]);
    let dir = utf8(&tmp);
    let output = run_rsmultigit(dir, &["check", "submodules"]);
    assert!(output.status.success(), "{}", stderr_str(&output));
    assert_eq!(stdout_str(&output), "");
    let output = run_rsmultigit(dir, &["--verbose", "check", "submodules"]);
    assert_eq!(stdout_str(&output), "no submodules found");
}
