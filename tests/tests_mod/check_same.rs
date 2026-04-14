use std::fs;
use std::path::Path;
use std::process::Output;

use crate::common::{run_rsmultigit_with_env, setup_git_repos, stderr_str, stdout_str, write_config};

fn run(tmp: &Path, config: &Path, args: &[&str]) -> Output {
    let cfg_str = config.to_string_lossy().to_string();
    run_rsmultigit_with_env(tmp, args, &[("RSMULTIGIT_CONFIG", &cfg_str)])
}

#[test]
fn check_same_all_identical_is_silent() {
    let tmp = setup_git_repos(&["a", "b", "c"]);
    for repo in ["a", "b", "c"] {
        fs::write(tmp.path().join(repo).join(".gitignore"), "target\n").unwrap();
    }
    let cfg = write_config(
        tmp.path(),
        r#"
[[check]]
name = "gi"
select = "*"
path = ".gitignore"
"#,
    );

    let output = run(tmp.path(), &cfg, &["check-same"]);
    assert!(output.status.success(), "stderr: {}", stderr_str(&output));
    assert_eq!(stdout_str(&output), "");
}

#[test]
fn check_same_divergent_exits_nonzero() {
    let tmp = setup_git_repos(&["a", "b", "c"]);
    fs::write(tmp.path().join("a/.gitignore"), "x\n").unwrap();
    fs::write(tmp.path().join("b/.gitignore"), "x\n").unwrap();
    fs::write(tmp.path().join("c/.gitignore"), "y\n").unwrap();
    let cfg = write_config(
        tmp.path(),
        r#"
[[check]]
name = "gi"
select = "*"
path = ".gitignore"
"#,
    );

    let output = run(tmp.path(), &cfg, &["check-same"]);
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
    let cfg = write_config(
        tmp.path(),
        r#"
[[check]]
name = "gi"
select = "*"
path = ".gitignore"
"#,
    );

    let output = run(tmp.path(), &cfg, &["check-same", "--terse"]);
    assert!(!output.status.success());
    assert_eq!(stdout_str(&output), "gi");
}

#[test]
fn check_same_no_header_suppresses_rule_label() {
    let tmp = setup_git_repos(&["a", "b"]);
    fs::write(tmp.path().join("a/.gitignore"), "x\n").unwrap();
    fs::write(tmp.path().join("b/.gitignore"), "y\n").unwrap();
    let cfg = write_config(
        tmp.path(),
        r#"
[[check]]
name = "gi"
select = "*"
path = ".gitignore"
"#,
    );

    let output = run(tmp.path(), &cfg, &["check-same", "--no-header"]);
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
    let cfg = write_config(
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

    let output = run(tmp.path(), &cfg, &["check-same", "--rule", "readme"]);
    assert!(output.status.success(), "stderr: {}", stderr_str(&output));
    assert_eq!(stdout_str(&output), "");
}

#[test]
fn check_same_unknown_rule_fails() {
    let tmp = setup_git_repos(&["a"]);
    fs::write(tmp.path().join("a/.gitignore"), "x\n").unwrap();
    let cfg = write_config(
        tmp.path(),
        r#"
[[check]]
name = "gi"
select = "*"
path = ".gitignore"
"#,
    );

    let output = run(tmp.path(), &cfg, &["check-same", "--rule", "nonexistent"]);
    assert!(!output.status.success());
    let stderr = stderr_str(&output);
    assert!(stderr.contains("nonexistent"), "stderr: {stderr}");
}

#[test]
fn check_same_missing_config_fails() {
    let tmp = setup_git_repos(&["a"]);
    let nonexistent = tmp.path().join("nope.toml");
    let output = run(tmp.path(), &nonexistent, &["check-same"]);
    assert!(!output.status.success());
}

#[test]
fn check_same_respects_select_glob() {
    let tmp = setup_git_repos(&["pyalpha", "pybeta", "go-proj"]);
    fs::write(tmp.path().join("pyalpha/Makefile"), "PY\n").unwrap();
    fs::write(tmp.path().join("pybeta/Makefile"), "PY\n").unwrap();
    fs::write(tmp.path().join("go-proj/Makefile"), "GO\n").unwrap();
    let cfg = write_config(
        tmp.path(),
        r#"
[[check]]
name = "py-make"
select = "py*"
path = "Makefile"
"#,
    );

    // Only py* repos considered — they match, so should succeed even though go-proj differs.
    let output = run(tmp.path(), &cfg, &["check-same"]);
    assert!(output.status.success(), "stdout: {}\nstderr: {}", stdout_str(&output), stderr_str(&output));
}

#[test]
fn check_same_marker_requires_file() {
    let tmp = setup_git_repos(&["a", "b"]);
    fs::write(tmp.path().join("a/.tag"), "").unwrap();
    fs::write(tmp.path().join("a/.gitignore"), "x\n").unwrap();
    fs::write(tmp.path().join("b/.gitignore"), "y\n").unwrap();
    let cfg = write_config(
        tmp.path(),
        r#"
[[check]]
name = "gi"
select = "*"
marker = ".tag"
path = ".gitignore"
"#,
    );

    let output = run(tmp.path(), &cfg, &["check-same"]);
    assert!(output.status.success(), "stdout: {}\nstderr: {}", stdout_str(&output), stderr_str(&output));
}

#[test]
fn check_same_disabled_rule_is_skipped() {
    let tmp = setup_git_repos(&["a", "b"]);
    fs::write(tmp.path().join("a/.gitignore"), "x\n").unwrap();
    fs::write(tmp.path().join("b/.gitignore"), "y\n").unwrap();
    let cfg = write_config(
        tmp.path(),
        r#"
[[check]]
name = "gi"
select = "*"
path = ".gitignore"
enabled = false
"#,
    );

    let output = run(tmp.path(), &cfg, &["check-same"]);
    assert!(output.status.success(), "stdout: {}\nstderr: {}", stdout_str(&output), stderr_str(&output));
}

#[test]
fn check_same_rule_flag_overrides_enabled_false() {
    let tmp = setup_git_repos(&["a", "b"]);
    fs::write(tmp.path().join("a/.gitignore"), "x\n").unwrap();
    fs::write(tmp.path().join("b/.gitignore"), "y\n").unwrap();
    let cfg = write_config(
        tmp.path(),
        r#"
[[check]]
name = "gi"
select = "*"
path = ".gitignore"
enabled = false
"#,
    );

    let output = run(tmp.path(), &cfg, &["check-same", "--rule", "gi"]);
    assert!(!output.status.success());
    let stdout = stdout_str(&output);
    assert!(stdout.contains("[gi]"), "stdout: {stdout}");
}

#[test]
fn check_same_verbose_reports_consistent_rules() {
    let tmp = setup_git_repos(&["a", "b"]);
    fs::write(tmp.path().join("a/.gitignore"), "same\n").unwrap();
    fs::write(tmp.path().join("b/.gitignore"), "same\n").unwrap();
    let cfg = write_config(
        tmp.path(),
        r#"
[[check]]
name = "gi"
select = "*"
path = ".gitignore"
"#,
    );

    let output = run(tmp.path(), &cfg, &["check-same", "--verbose"]);
    assert!(output.status.success());
    let stdout = stdout_str(&output);
    assert!(stdout.contains("[gi]"), "stdout: {stdout}");
    assert!(stdout.contains("ok"), "stdout: {stdout}");
}

#[test]
fn check_same_config_without_repos_fails() {
    let tmp = setup_git_repos(&["a"]);
    // Config missing the `repos` key.
    let cfg_path = tmp.path().join("no-repos.toml");
    fs::write(
        &cfg_path,
        r#"
[[check]]
name = "gi"
select = "*"
path = ".gitignore"
"#,
    )
    .unwrap();

    let output = run(tmp.path(), &cfg_path, &["check-same"]);
    assert!(!output.status.success());
    let stderr = stderr_str(&output);
    assert!(stderr.contains("repos"), "stderr should mention repos: {stderr}");
}
