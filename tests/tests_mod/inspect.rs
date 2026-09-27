//! Read-only commands: data commands, list-repos, log, blame, tags, and the
//! check-exists / check-all / complete surfaces.

use std::fs;

use crate::common::{
    commit_file, current_branch, git, run_rsmultigit, run_rsmultigit_with_env, setup_git_repos,
    stderr_str, stdout_str, utf8, write_config,
};

#[test]
fn list_repos_prints_paths_and_verbose_adds_headers() {
    let tmp = setup_git_repos(&["a", "b"]);
    let dir = utf8(&tmp);
    let output = run_rsmultigit(dir, &["list-repos"]);
    assert!(output.status.success());
    assert_eq!(
        stdout_str(&output),
        format!("{}\n{}", dir.join("a"), dir.join("b"))
    );

    let output = run_rsmultigit(dir, &["--verbose", "list-repos"]);
    assert_eq!(
        stdout_str(&output),
        format!("[{0}]\n{0}\n[{1}]\n{1}", dir.join("a"), dir.join("b"))
    );
}

#[test]
fn log_count_limits_the_lines_per_repo() {
    let tmp = setup_git_repos(&["repo"]);
    let dir = utf8(&tmp);
    let repo = dir.join("repo");
    for i in 1..=3 {
        commit_file(&repo, "f.txt", &i.to_string(), &format!("commit {i}"));
    }
    let output = run_rsmultigit(dir, &["log", "--count", "2"]);
    assert!(output.status.success(), "{}", stderr_str(&output));
    let stdout = stdout_str(&output);
    let lines: Vec<&str> = stdout.lines().skip(1).collect();
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert!(lines[0].ends_with("commit 3"));
    assert!(lines[1].ends_with("commit 2"));
}

#[test]
fn blame_skips_repos_without_the_file() {
    let tmp = setup_git_repos(&["has", "lacks"]);
    let dir = utf8(&tmp);
    commit_file(&dir.join("has"), "f.txt", "line one\n", "add f");

    let output = run_rsmultigit(dir, &["blame", "f.txt"]);
    assert!(output.status.success(), "{}", stderr_str(&output));
    let stdout = stdout_str(&output);
    assert!(stdout.contains("has]"), "{stdout}");
    assert!(!stdout.contains("lacks]"), "{stdout}");
    assert!(
        stdout.contains("Test") && stdout.contains("line one"),
        "{stdout}"
    );
}

#[test]
fn last_tag_and_tag_has_local_see_only_tagged_repos() {
    let tmp = setup_git_repos(&["tagged", "plain"]);
    let dir = utf8(&tmp);
    let tagged = dir.join("tagged");
    git(&tagged, &["tag", "v0.1"]);
    commit_file(&tagged, "f.txt", "x", "later");
    git(&tagged, &["tag", "v0.2"]);

    let output = run_rsmultigit(dir, &["last-tag"]);
    assert!(output.status.success(), "{}", stderr_str(&output));
    assert_eq!(stdout_str(&output), format!("[{tagged}]\nv0.2"));

    let output = run_rsmultigit(dir, &["tag", "local"]);
    assert!(output.status.success());
    let stdout = stdout_str(&output);
    assert!(stdout.contains("v0.1\nv0.2"), "{stdout}");

    let output = run_rsmultigit(dir, &["tag", "has-local"]);
    assert!(output.status.success());
    assert_eq!(stdout_str(&output), format!("{tagged}\n1/2"));
}

#[test]
fn size_reports_a_human_readable_size_per_repo() {
    let tmp = setup_git_repos(&["repo"]);
    let dir = utf8(&tmp);
    let output = run_rsmultigit(dir, &["size"]);
    assert!(output.status.success(), "{}", stderr_str(&output));
    let stdout = stdout_str(&output);
    let value = stdout.lines().nth(1).unwrap_or("");
    let (number, unit) = value.split_once(' ').unwrap_or(("", ""));
    assert!(number.parse::<f64>().is_ok(), "{stdout}");
    assert!(["B", "KB", "MB", "GB"].contains(&unit), "{stdout}");
}

#[test]
fn age_authors_and_branch_local_report_per_repo() {
    let tmp = setup_git_repos(&["repo"]);
    let dir = utf8(&tmp);
    let repo = dir.join("repo");
    let branch = current_branch(&repo);

    let output = run_rsmultigit(dir, &["age"]);
    assert!(
        stdout_str(&output).contains("ago"),
        "{}",
        stdout_str(&output)
    );

    let output = run_rsmultigit(dir, &["authors"]);
    assert!(
        stdout_str(&output).contains("Test <test@test.com>"),
        "{}",
        stdout_str(&output)
    );

    let output = run_rsmultigit(dir, &["branch", "local"]);
    assert!(
        stdout_str(&output).contains(&format!("* {branch}")),
        "{}",
        stdout_str(&output)
    );

    let output = run_rsmultigit(dir, &["config", "user.name"]);
    assert_eq!(stdout_str(&output), format!("[{repo}]\nTest"));
}

#[test]
fn check_exists_and_check_all_end_to_end() {
    let tmp = setup_git_repos(&["a", "b"]);
    let dir = utf8(&tmp);
    fs::write(dir.join("a/README.md"), "a").unwrap();
    for name in ["a", "b"] {
        fs::write(dir.join(name).join(".gitignore"), "same\n").unwrap();
    }
    let cfg = write_config(
        dir,
        "[[check]]\nname = \"gi\"\nselect = \"*\"\npath = \".gitignore\"\n\n[[exists]]\nname = \"rd\"\nselect = \"*\"\npath = \"README.md\"\n",
    );
    let env = [("RSMULTIGIT_CONFIG", cfg.as_str())];

    let output = run_rsmultigit_with_env(dir, &["check-exists"], &env);
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(
        stdout_str(&output),
        format!(
            "[rd]\n2 repos, 1 missing README.md\n  missing in:\n    {}",
            dir.join("b")
        )
    );

    let output = run_rsmultigit_with_env(dir, &["--terse", "check-exists"], &env);
    assert_eq!(stdout_str(&output), "rd");

    let output = run_rsmultigit_with_env(dir, &["list-checks", "exists"], &env);
    assert_eq!(stdout_str(&output), "rd");

    // check-all: the [[check]] half passes, the [[exists]] half fails, so
    // both are reported and the exit is non-zero.
    let output = run_rsmultigit_with_env(dir, &["check-all"], &env);
    assert_eq!(output.status.code(), Some(1));
    let stdout = stdout_str(&output);
    assert!(stdout.starts_with("[gi]\nok (2 files)\n[rd]\n"), "{stdout}");

    fs::write(dir.join("b/README.md"), "b").unwrap();
    let output = run_rsmultigit_with_env(dir, &["check-all", "--only-failed"], &env);
    assert!(output.status.success(), "{}", stderr_str(&output));
    assert_eq!(stdout_str(&output), "");
}

#[test]
fn complete_emits_a_script_with_the_dynamic_checks_snippet() {
    let tmp = tempfile::TempDir::new().unwrap();
    for shell in ["bash", "zsh"] {
        let output = run_rsmultigit(utf8(&tmp), &["complete", shell]);
        assert!(output.status.success());
        let stdout = stdout_str(&output);
        assert!(stdout.contains("_rsmultigit"), "{shell}");
        assert!(stdout.contains("list-checks \"$kind\""), "{shell}");
        assert!(stdout.contains("check-exists)"), "{shell}");
    }
    // Only bash and zsh get the dynamic snippet.
    let output = run_rsmultigit(utf8(&tmp), &["complete", "fish"]);
    assert!(output.status.success());
    assert!(!stdout_str(&output).contains("list-checks \"$kind\""));
}
