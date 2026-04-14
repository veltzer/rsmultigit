use std::fs;

use crate::common::{run_rsmultigit, setup_git_repos, stderr_str, stdout_str};

fn write_config(dir: &std::path::Path, body: &str) {
    fs::write(dir.join(".rsmultigit.toml"), body).unwrap();
}

#[test]
fn check_same_all_identical_is_silent() {
    let tmp = setup_git_repos(&["a", "b", "c"]);
    for repo in ["a", "b", "c"] {
        fs::write(tmp.path().join(repo).join(".gitignore"), "target\n").unwrap();
    }
    write_config(
        tmp.path(),
        r#"
[[check]]
name = "gi"
select = "*"
path = ".gitignore"
"#,
    );

    let output = run_rsmultigit(tmp.path(), &["check-same"]);
    assert!(output.status.success(), "stderr: {}", stderr_str(&output));
    assert_eq!(stdout_str(&output), "");
}

#[test]
fn check_same_divergent_exits_nonzero() {
    let tmp = setup_git_repos(&["a", "b", "c"]);
    fs::write(tmp.path().join("a/.gitignore"), "x\n").unwrap();
    fs::write(tmp.path().join("b/.gitignore"), "x\n").unwrap();
    fs::write(tmp.path().join("c/.gitignore"), "y\n").unwrap();
    write_config(
        tmp.path(),
        r#"
[[check]]
name = "gi"
select = "*"
path = ".gitignore"
"#,
    );

    let output = run_rsmultigit(tmp.path(), &["check-same"]);
    assert!(!output.status.success());
    let stdout = stdout_str(&output);
    assert!(stdout.contains("[gi]"), "stdout: {stdout}");
    assert!(stdout.contains("3 files, 2 groups"), "stdout: {stdout}");
    assert!(stdout.contains("group A (2 files)"), "stdout: {stdout}");
    assert!(stdout.contains("group B (1 files)"), "stdout: {stdout}");
}

#[test]
fn check_same_terse_prints_only_rule_name() {
    let tmp = setup_git_repos(&["a", "b"]);
    fs::write(tmp.path().join("a/.gitignore"), "x\n").unwrap();
    fs::write(tmp.path().join("b/.gitignore"), "y\n").unwrap();
    write_config(
        tmp.path(),
        r#"
[[check]]
name = "gi"
select = "*"
path = ".gitignore"
"#,
    );

    let output = run_rsmultigit(tmp.path(), &["check-same", "--terse"]);
    assert!(!output.status.success());
    assert_eq!(stdout_str(&output), "gi");
}

#[test]
fn check_same_no_header_suppresses_rule_label() {
    let tmp = setup_git_repos(&["a", "b"]);
    fs::write(tmp.path().join("a/.gitignore"), "x\n").unwrap();
    fs::write(tmp.path().join("b/.gitignore"), "y\n").unwrap();
    write_config(
        tmp.path(),
        r#"
[[check]]
name = "gi"
select = "*"
path = ".gitignore"
"#,
    );

    let output = run_rsmultigit(tmp.path(), &["check-same", "--no-header"]);
    assert!(!output.status.success());
    let stdout = stdout_str(&output);
    assert!(!stdout.contains("[gi]"), "stdout should not contain [gi]: {stdout}");
    assert!(stdout.contains("2 files, 2 groups"), "stdout: {stdout}");
}

#[test]
fn check_same_only_runs_named_rule() {
    let tmp = setup_git_repos(&["a", "b"]);
    fs::write(tmp.path().join("a/.gitignore"), "x\n").unwrap();
    fs::write(tmp.path().join("b/.gitignore"), "y\n").unwrap();
    fs::write(tmp.path().join("a/README"), "same\n").unwrap();
    fs::write(tmp.path().join("b/README"), "same\n").unwrap();
    write_config(
        tmp.path(),
        r#"
[[check]]
name = "gi"
select = "*"
path = ".gitignore"

[[check]]
name = "readme"
select = "*"
path = "README"
"#,
    );

    let output = run_rsmultigit(tmp.path(), &["check-same", "--rule", "readme"]);
    assert!(output.status.success(), "stderr: {}", stderr_str(&output));
    assert_eq!(stdout_str(&output), "");
}

#[test]
fn check_same_unknown_rule_fails() {
    let tmp = setup_git_repos(&["a"]);
    fs::write(tmp.path().join("a/.gitignore"), "x\n").unwrap();
    write_config(
        tmp.path(),
        r#"
[[check]]
name = "gi"
select = "*"
path = ".gitignore"
"#,
    );

    let output = run_rsmultigit(tmp.path(), &["check-same", "--rule", "nonexistent"]);
    assert!(!output.status.success());
    let stderr = stderr_str(&output);
    assert!(stderr.contains("nonexistent"), "stderr: {stderr}");
}

#[test]
fn check_same_missing_config_fails() {
    let tmp = setup_git_repos(&["a"]);
    let output = run_rsmultigit(tmp.path(), &["check-same"]);
    assert!(!output.status.success());
}

#[test]
fn check_same_respects_select_glob() {
    let tmp = setup_git_repos(&["pyalpha", "pybeta", "go-proj"]);
    fs::write(tmp.path().join("pyalpha/Makefile"), "PY\n").unwrap();
    fs::write(tmp.path().join("pybeta/Makefile"), "PY\n").unwrap();
    fs::write(tmp.path().join("go-proj/Makefile"), "GO\n").unwrap();
    write_config(
        tmp.path(),
        r#"
[[check]]
name = "py-make"
select = "py*"
path = "Makefile"
"#,
    );

    // Only py* repos considered — they match, so should succeed even though go-proj differs.
    let output = run_rsmultigit(tmp.path(), &["check-same"]);
    assert!(output.status.success(), "stdout: {}\nstderr: {}", stdout_str(&output), stderr_str(&output));
}

#[test]
fn check_same_marker_requires_file() {
    let tmp = setup_git_repos(&["a", "b"]);
    // Only repo 'a' has the marker; its .gitignore content is "x". b has "y" but
    // is excluded by the marker filter, so no divergence.
    fs::write(tmp.path().join("a/.tag"), "").unwrap();
    fs::write(tmp.path().join("a/.gitignore"), "x\n").unwrap();
    fs::write(tmp.path().join("b/.gitignore"), "y\n").unwrap();
    write_config(
        tmp.path(),
        r#"
[[check]]
name = "gi"
select = "*"
marker = ".tag"
path = ".gitignore"
"#,
    );

    let output = run_rsmultigit(tmp.path(), &["check-same"]);
    assert!(output.status.success(), "stdout: {}\nstderr: {}", stdout_str(&output), stderr_str(&output));
}

#[test]
fn check_same_disabled_rule_is_skipped() {
    let tmp = setup_git_repos(&["a", "b"]);
    fs::write(tmp.path().join("a/.gitignore"), "x\n").unwrap();
    fs::write(tmp.path().join("b/.gitignore"), "y\n").unwrap();
    write_config(
        tmp.path(),
        r#"
[[check]]
name = "gi"
select = "*"
path = ".gitignore"
enabled = false
"#,
    );

    // Disabled rule — divergence is not flagged.
    let output = run_rsmultigit(tmp.path(), &["check-same"]);
    assert!(output.status.success(), "stdout: {}\nstderr: {}", stdout_str(&output), stderr_str(&output));
}

#[test]
fn check_same_rule_flag_overrides_enabled_false() {
    let tmp = setup_git_repos(&["a", "b"]);
    fs::write(tmp.path().join("a/.gitignore"), "x\n").unwrap();
    fs::write(tmp.path().join("b/.gitignore"), "y\n").unwrap();
    write_config(
        tmp.path(),
        r#"
[[check]]
name = "gi"
select = "*"
path = ".gitignore"
enabled = false
"#,
    );

    // --rule selects the rule explicitly and should override enabled=false.
    let output = run_rsmultigit(tmp.path(), &["check-same", "--rule", "gi"]);
    assert!(!output.status.success());
    let stdout = stdout_str(&output);
    assert!(stdout.contains("[gi]"), "stdout: {stdout}");
}

#[test]
fn check_same_verbose_reports_consistent_rules() {
    let tmp = setup_git_repos(&["a", "b"]);
    fs::write(tmp.path().join("a/.gitignore"), "same\n").unwrap();
    fs::write(tmp.path().join("b/.gitignore"), "same\n").unwrap();
    write_config(
        tmp.path(),
        r#"
[[check]]
name = "gi"
select = "*"
path = ".gitignore"
"#,
    );

    let output = run_rsmultigit(tmp.path(), &["check-same", "--verbose"]);
    assert!(output.status.success());
    let stdout = stdout_str(&output);
    assert!(stdout.contains("[gi]"), "stdout: {stdout}");
    assert!(stdout.contains("ok"), "stdout: {stdout}");
}
