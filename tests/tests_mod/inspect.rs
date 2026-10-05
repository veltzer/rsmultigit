//! Read-only commands: data commands, list, log, blame, tags, and the
//! check exists / check all / complete surfaces.

use std::fs;

use crate::common::{
    add_bare_origin, clone_of, commit_file, current_branch, git, run_rsmultigit,
    run_rsmultigit_with_env, setup_git_repos, stderr_str, stdout_str, utf8, write_config,
};

#[test]
fn list_repos_prints_paths_and_verbose_adds_headers() {
    let tmp = setup_git_repos(&["a", "b"]);
    let dir = utf8(&tmp);
    let output = run_rsmultigit(dir, &["list", "repos"]);
    assert!(output.status.success());
    assert_eq!(
        stdout_str(&output),
        format!("{}\n{}", dir.join("a"), dir.join("b"))
    );

    let output = run_rsmultigit(dir, &["--verbose", "list", "repos"]);
    assert_eq!(
        stdout_str(&output),
        format!("[{0}]\n{0}\n[{1}]\n{1}", dir.join("a"), dir.join("b"))
    );
}

#[test]
fn list_filters_print_only_the_matching_paths() {
    let names = ["ahead", "behind", "clean", "dirty", "untracked"];
    let tmp = setup_git_repos(&names);
    let dir = utf8(&tmp);
    let bares = tempfile::TempDir::new().unwrap();
    for name in ["ahead", "behind"] {
        add_bare_origin(&dir.join(name), &utf8(&bares).join(name));
    }
    commit_file(&dir.join("ahead"), "new.txt", "x", "local only");
    // `behind` gets a commit from another clone, then fetches it.
    let clone = utf8(&bares).join("clone");
    clone_of(&utf8(&bares).join("behind"), &clone);
    commit_file(&clone, "elsewhere.txt", "x", "made elsewhere");
    git(&clone, &["push", "-q", "origin", "HEAD"]);
    git(&dir.join("behind"), &["fetch", "-q"]);
    commit_file(&dir.join("dirty"), "f.txt", "1", "add f");
    fs::write(dir.join("dirty/f.txt"), "2").unwrap();
    fs::write(dir.join("untracked/new.txt"), "x").unwrap();

    let paths = |names: &[&str]| -> String {
        names
            .iter()
            .map(|n| dir.join(n).to_string())
            .collect::<Vec<_>>()
            .join("\n")
    };
    for (what, expected) in [
        ("dirty", &["dirty"][..]),
        ("untracked", &["untracked"]),
        ("unsynchronized", &["ahead", "behind"]),
        ("ahead", &["ahead"]),
        ("behind", &["behind"]),
    ] {
        for jobs in ["1", "4"] {
            let output = run_rsmultigit(dir, &["-j", jobs, "list", what]);
            assert!(output.status.success(), "{}", stderr_str(&output));
            assert_eq!(
                stdout_str(&output),
                paths(expected),
                "list {what} -j {jobs}"
            );
        }
    }

    // --print-not inverts the selection.
    let output = run_rsmultigit(dir, &["list", "dirty", "--print-not"]);
    assert!(output.status.success(), "{}", stderr_str(&output));
    assert_eq!(
        stdout_str(&output),
        paths(&["ahead", "behind", "clean", "untracked"])
    );
}

#[test]
fn bare_list_names_its_operations() {
    let tmp = setup_git_repos(&["a"]);
    let output = run_rsmultigit(utf8(&tmp), &["list"]);
    assert_eq!(output.status.code(), Some(2));
    let stderr = stderr_str(&output);
    for op in [
        "repos",
        "dirty",
        "untracked",
        "unsynchronized",
        "ahead",
        "behind",
    ] {
        assert!(stderr.contains(&format!("  {op} ")), "{stderr}");
    }
    assert!(!stderr.contains("--jobs"), "{stderr}");
}

#[test]
fn log_count_limits_the_lines_per_repo() {
    let tmp = setup_git_repos(&["repo"]);
    let dir = utf8(&tmp);
    let repo = dir.join("repo");
    for i in 1..=3 {
        commit_file(&repo, "f.txt", &i.to_string(), &format!("commit {i}"));
    }
    let output = run_rsmultigit(dir, &["git", "log", "--count", "2"]);
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

    let output = run_rsmultigit(dir, &["git", "blame", "f.txt"]);
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

    let output = run_rsmultigit(dir, &["git", "last-tag"]);
    assert!(output.status.success(), "{}", stderr_str(&output));
    assert_eq!(stdout_str(&output), format!("[{tagged}]\nv0.2"));

    let output = run_rsmultigit(dir, &["git", "tag", "local"]);
    assert!(output.status.success());
    let stdout = stdout_str(&output);
    assert!(stdout.contains("v0.1\nv0.2"), "{stdout}");

    let output = run_rsmultigit(dir, &["git", "tag", "has-local"]);
    assert!(output.status.success());
    assert_eq!(stdout_str(&output), format!("{tagged}\n1/2"));
}

#[test]
fn size_reports_a_human_readable_size_per_repo() {
    let tmp = setup_git_repos(&["repo"]);
    let dir = utf8(&tmp);
    let output = run_rsmultigit(dir, &["git", "size"]);
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

    let output = run_rsmultigit(dir, &["git", "age"]);
    assert!(
        stdout_str(&output).contains("ago"),
        "{}",
        stdout_str(&output)
    );

    let output = run_rsmultigit(dir, &["git", "authors"]);
    assert!(
        stdout_str(&output).contains("Test <test@test.com>"),
        "{}",
        stdout_str(&output)
    );

    let output = run_rsmultigit(dir, &["git", "branch", "local"]);
    assert!(
        stdout_str(&output).contains(&format!("* {branch}")),
        "{}",
        stdout_str(&output)
    );

    let output = run_rsmultigit(dir, &["git", "config", "user.name"]);
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

    let output = run_rsmultigit_with_env(dir, &["check", "exists"], &env);
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(
        stdout_str(&output),
        format!(
            "[rd] path=README.md select=*\n2 repos, 1 missing README.md\n  missing in:\n    {}",
            dir.join("b")
        )
    );

    let output = run_rsmultigit_with_env(dir, &["--terse", "check", "exists"], &env);
    assert_eq!(stdout_str(&output), "rd");

    let output = run_rsmultigit_with_env(dir, &["check", "list", "exists"], &env);
    assert_eq!(stdout_str(&output), "rd");

    // check all: the [[check]] half passes, the [[exists]] half fails, so
    // both are reported and the exit is non-zero.
    let output = run_rsmultigit_with_env(dir, &["check", "all"], &env);
    assert_eq!(output.status.code(), Some(1));
    let stdout = stdout_str(&output);
    assert!(
        stdout.starts_with(
            "[gi] path=.gitignore select=*\nok (2 files)\n[rd] path=README.md select=*\n"
        ),
        "{stdout}"
    );

    fs::write(dir.join("b/README.md"), "b").unwrap();
    let output = run_rsmultigit_with_env(dir, &["check", "all", "--only-failed"], &env);
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
        assert!(stdout.contains("check list \"$kind\""), "{shell}");
        assert!(stdout.contains("exists)"), "{shell}");
    }
    // Only bash and zsh get the dynamic snippet.
    let output = run_rsmultigit(utf8(&tmp), &["complete", "fish"]);
    assert!(output.status.success());
    assert!(!stdout_str(&output).contains("check list \"$kind\""));
}

#[test]
fn data_and_count_runners_honour_the_output_flags() {
    // `tagged` has a tag, `plain` has none: `git last-tag` reports one repo.
    let tmp = setup_git_repos(&["plain", "tagged"]);
    let dir = utf8(&tmp);
    git(&dir.join("tagged"), &["tag", "v1"]);
    let p = |name: &str| dir.join(name).to_string();

    for (args, want) in [
        (&["git", "last-tag"][..], format!("[{}]\nv1", p("tagged"))),
        (&["--terse", "git", "last-tag"], p("tagged")),
        (&["--no-header", "git", "last-tag"], "v1".to_string()),
        (
            &["--no-output", "git", "last-tag"],
            format!("[{}]", p("tagged")),
        ),
        (
            &["--print-not", "git", "last-tag"],
            format!("[{}]", p("plain")),
        ),
        (&["--print-not", "--terse", "git", "last-tag"], p("plain")),
        (
            &["--verbose", "git", "last-tag"],
            format!("[{}]\n[{}]\nv1", p("plain"), p("tagged")),
        ),
        (
            &["git", "tag", "has-local"],
            format!("{}\n1/2", p("tagged")),
        ),
        (&["--terse", "git", "tag", "has-local"], "1/2".to_string()),
        (
            &["--print-not", "git", "tag", "has-local"],
            format!("{}\n1/2", p("plain")),
        ),
    ] {
        for jobs in ["1", "2"] {
            let mut full = vec!["-j", jobs];
            full.extend_from_slice(args);
            let output = run_rsmultigit(dir, &full);
            assert!(output.status.success(), "{full:?}: {}", stderr_str(&output));
            assert_eq!(stdout_str(&output), want, "{full:?}");
        }
    }
}

#[test]
fn verbose_shows_skipped_repos_on_a_checked_action_serial_and_parallel() {
    // `git restore --dry-run` acts only on repos with unstaged changes and
    // changes nothing. Without --verbose only the acted-on repo gets a
    // header; with it, the skipped one does too. Same with and without -j.
    let tmp = setup_git_repos(&["changed", "idle"]);
    let dir = utf8(&tmp);
    commit_file(&dir.join("changed"), "f.txt", "a", "add f");
    fs::write(dir.join("changed/f.txt"), "b").unwrap();
    let changed = format!("[{}]\nM\tf.txt", dir.join("changed"));
    let idle = format!("[{}]", dir.join("idle"));

    for jobs in ["1", "2"] {
        let output = run_rsmultigit(dir, &["-j", jobs, "git", "restore", "--dry-run"]);
        assert!(output.status.success(), "{}", stderr_str(&output));
        assert_eq!(stdout_str(&output), changed, "-j {jobs}");

        let output = run_rsmultigit(
            dir,
            &["--verbose", "-j", jobs, "git", "restore", "--dry-run"],
        );
        assert!(output.status.success(), "{}", stderr_str(&output));
        assert_eq!(
            stdout_str(&output),
            format!("{changed}\n{idle}"),
            "-j {jobs}"
        );
    }
}

#[test]
fn reports_skip_an_empty_repo_instead_of_failing() {
    // A repo with no commit yet (unborn branch) has no age, authors or tag;
    // it must not abort the run for the repos that do.
    let tmp = setup_git_repos(&["full"]);
    let dir = utf8(&tmp);
    let empty = dir.join("empty");
    fs::create_dir(&empty).unwrap();
    git(&empty, &["init", "-q"]);
    fs::write(empty.join("new.txt"), "x").unwrap();
    git(&empty, &["add", "new.txt"]);

    for args in [
        &["git", "age"][..],
        &["git", "authors"],
        &["git", "last-tag"],
        &["git", "status"],
        &["git", "count", "dirty"],
    ] {
        let output = run_rsmultigit(dir, args);
        assert!(output.status.success(), "{args:?}: {}", stderr_str(&output));
    }
    let output = run_rsmultigit(dir, &["git", "age"]);
    let stdout = stdout_str(&output);
    assert!(
        stdout.contains("full]") && !stdout.contains("empty]"),
        "{stdout}"
    );
    // The staged file in the unborn repo shows up in `git dirty`.
    let output = run_rsmultigit(dir, &["git", "dirty"]);
    assert!(output.status.success(), "{}", stderr_str(&output));
    assert!(
        stdout_str(&output).contains("new.txt"),
        "{}",
        stdout_str(&output)
    );
}

#[test]
fn git_config_reports_values_and_rejects_a_malformed_key() {
    let tmp = setup_git_repos(&["a", "b"]);
    let dir = utf8(&tmp);
    git(&dir.join("a"), &["config", "rsmultigit.probe", "here"]);

    let output = run_rsmultigit(dir, &["git", "config", "rsmultigit.probe"]);
    assert!(output.status.success(), "{}", stderr_str(&output));
    assert_eq!(stdout_str(&output), format!("[{}]\nhere", dir.join("a")));

    let output = run_rsmultigit(dir, &["git", "config", "nosection"]);
    assert!(!output.status.success());
    assert!(
        stderr_str(&output).contains("invalid git config key"),
        "{}",
        stderr_str(&output)
    );
}
