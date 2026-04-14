use std::fs;
use std::path::Path;
use std::process::Output;

use crate::common::{
    run_rsmultigit_with_env, run_rsmultigit_with_stdin, setup_git_repos, stderr_str, stdout_str,
    write_config,
};

fn run(tmp: &Path, config: &Path, args: &[&str]) -> Output {
    let cfg_str = config.to_string_lossy().to_string();
    run_rsmultigit_with_env(tmp, args, &[("RSMULTIGIT_CONFIG", &cfg_str)])
}

fn run_with_stdin(tmp: &Path, config: &Path, args: &[&str], stdin_bytes: &[u8]) -> Output {
    let cfg_str = config.to_string_lossy().to_string();
    run_rsmultigit_with_stdin(tmp, args, &[("RSMULTIGIT_CONFIG", &cfg_str)], stdin_bytes)
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

    let output = run(tmp.path(), &cfg, &["check-same", "--checks", "readme"]);
    assert!(output.status.success(), "stderr: {}", stderr_str(&output));
    assert_eq!(stdout_str(&output), "");
}

#[test]
fn check_same_checks_accepts_multiple_names() {
    let tmp = setup_git_repos(&["a", "b"]);
    // Two mismatching checks; a third consistent one we want to skip.
    fs::write(tmp.path().join("a/.gitignore"), "x\n").unwrap();
    fs::write(tmp.path().join("b/.gitignore"), "y\n").unwrap();
    fs::write(tmp.path().join("a/README"), "one\n").unwrap();
    fs::write(tmp.path().join("b/README"), "two\n").unwrap();
    fs::write(tmp.path().join("a/LICENSE"), "MIT\n").unwrap();
    fs::write(tmp.path().join("b/LICENSE"), "BSD\n").unwrap();
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

[[check]]
name = "license"
select = "*"
path = "LICENSE"
"#,
    );

    // Run only gi + license; readme's divergence must not appear.
    let output = run(tmp.path(), &cfg, &["check-same", "--checks", "gi", "license"]);
    assert!(!output.status.success());
    let stdout = stdout_str(&output);
    assert!(stdout.contains("[gi]"), "stdout: {stdout}");
    assert!(stdout.contains("[license]"), "stdout: {stdout}");
    assert!(!stdout.contains("[readme]"), "readme should be skipped: {stdout}");
}

#[test]
fn check_same_checks_preserves_request_order() {
    let tmp = setup_git_repos(&["a", "b"]);
    fs::write(tmp.path().join("a/.gitignore"), "x\n").unwrap();
    fs::write(tmp.path().join("b/.gitignore"), "y\n").unwrap();
    fs::write(tmp.path().join("a/README"), "one\n").unwrap();
    fs::write(tmp.path().join("b/README"), "two\n").unwrap();
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

    // Request in reverse of config order.
    let output = run(tmp.path(), &cfg, &["check-same", "--checks", "readme", "gi"]);
    assert!(!output.status.success());
    let stdout = stdout_str(&output);
    let readme_pos = stdout.find("[readme]").expect("missing [readme]");
    let gi_pos = stdout.find("[gi]").expect("missing [gi]");
    assert!(readme_pos < gi_pos, "expected readme before gi: {stdout}");
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

    let output = run(tmp.path(), &cfg, &["check-same", "--checks", "nonexistent"]);
    assert!(!output.status.success());
    let stderr = stderr_str(&output);
    assert!(
        stderr.contains("nonexistent") && stderr.contains("unknown check name"),
        "stderr: {stderr}"
    );
}

#[test]
fn check_same_mixed_known_and_unknown_names_fails() {
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

    // Even if one name is known, an unknown one must hard-error.
    let output = run(tmp.path(), &cfg, &["check-same", "--checks", "gi", "bogus"]);
    assert!(!output.status.success());
    let stderr = stderr_str(&output);
    assert!(stderr.contains("bogus"), "stderr should mention bogus: {stderr}");
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
fn check_same_checks_override_enabled_false() {
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

    let output = run(tmp.path(), &cfg, &["check-same", "--checks", "gi"]);
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
fn check_same_diff_emits_unified_diff_for_two_groups() {
    let tmp = setup_git_repos(&["a", "b"]);
    fs::write(tmp.path().join("a/.gitignore"), "target\nnode_modules\n.env\n").unwrap();
    fs::write(tmp.path().join("b/.gitignore"), "target\ndist\n.env\n").unwrap();
    let cfg = write_config(
        tmp.path(),
        r#"
[[check]]
name = "gi"
select = "*"
path = ".gitignore"
"#,
    );

    let output = run(tmp.path(), &cfg, &["check-same", "--diff"]);
    assert!(!output.status.success());
    let stdout = stdout_str(&output);
    // Unified diff markers must appear.
    assert!(stdout.contains("--- "), "stdout should contain unified diff header: {stdout}");
    assert!(stdout.contains("+++ "), "stdout should contain unified diff header: {stdout}");
    assert!(stdout.contains("-node_modules"), "should show removed line: {stdout}");
    assert!(stdout.contains("+dist"), "should show added line: {stdout}");
}

#[test]
fn check_same_diff_three_groups_prompts_for_pair() {
    let tmp = setup_git_repos(&["a", "b", "c"]);
    fs::write(tmp.path().join("a/.gitignore"), "one\n").unwrap();
    fs::write(tmp.path().join("b/.gitignore"), "two\n").unwrap();
    fs::write(tmp.path().join("c/.gitignore"), "three\n").unwrap();
    let cfg = write_config(
        tmp.path(),
        r#"
[[check]]
name = "gi"
select = "*"
path = ".gitignore"
"#,
    );

    // stdin is the default (closed → EOF → treated as Quit).
    // The prompt for "diff from group" should still be visible in stdout.
    let output = run(tmp.path(), &cfg, &["check-same", "--diff"]);
    assert!(!output.status.success());
    let stdout = stdout_str(&output);
    assert!(
        stdout.contains("diff from group"),
        "should prompt for a pair: {stdout}"
    );
    // With closed stdin, no diff body should be emitted.
    assert!(!stdout.contains("--- "), "stdout should not contain diff header: {stdout}");
}

#[test]
fn check_same_diff_off_by_default() {
    let tmp = setup_git_repos(&["a", "b"]);
    fs::write(tmp.path().join("a/.gitignore"), "target\n").unwrap();
    fs::write(tmp.path().join("b/.gitignore"), "dist\n").unwrap();
    let cfg = write_config(
        tmp.path(),
        r#"
[[check]]
name = "gi"
select = "*"
path = ".gitignore"
"#,
    );

    // Without --diff, no unified-diff output.
    let output = run(tmp.path(), &cfg, &["check-same"]);
    assert!(!output.status.success());
    let stdout = stdout_str(&output);
    assert!(!stdout.contains("+++ "), "no diff without --diff: {stdout}");
}

#[test]
fn check_same_diff_binary_files() {
    let tmp = setup_git_repos(&["a", "b"]);
    // Non-UTF-8 payloads.
    fs::write(tmp.path().join("a/blob.bin"), [0xffu8, 0xfe, 0x00, 0x01]).unwrap();
    fs::write(tmp.path().join("b/blob.bin"), [0xffu8, 0xfe, 0x00, 0x02]).unwrap();
    let cfg = write_config(
        tmp.path(),
        r#"
[[check]]
name = "bin"
select = "*"
path = "blob.bin"
"#,
    );

    let output = run(tmp.path(), &cfg, &["check-same", "--diff"]);
    assert!(!output.status.success());
    let stdout = stdout_str(&output);
    assert!(
        stdout.contains("binary files differ"),
        "should note binary files: {stdout}"
    );
}

#[test]
fn check_same_diff_suppressed_by_terse() {
    let tmp = setup_git_repos(&["a", "b"]);
    fs::write(tmp.path().join("a/.gitignore"), "target\n").unwrap();
    fs::write(tmp.path().join("b/.gitignore"), "dist\n").unwrap();
    let cfg = write_config(
        tmp.path(),
        r#"
[[check]]
name = "gi"
select = "*"
path = ".gitignore"
"#,
    );

    // --terse takes precedence — only the rule name, no grouping output, no diff.
    let output = run(tmp.path(), &cfg, &["check-same", "--terse", "--diff"]);
    assert!(!output.status.success());
    assert_eq!(stdout_str(&output), "gi");
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

// ── list-checks ──────────────────────────────────────────────────────────────

#[test]
fn list_checks_prints_every_rule_name() {
    let tmp = setup_git_repos(&["a"]);
    let cfg = write_config(
        tmp.path(),
        r#"
[[check]]
name = "alpha"
select = "*"
path = "a"

[[check]]
name = "beta"
select = "*"
path = "b"
enabled = false

[[check]]
name = "gamma"
select = "*"
path = "c"
"#,
    );
    let output = run(tmp.path(), &cfg, &["list-checks"]);
    assert!(output.status.success(), "stderr: {}", stderr_str(&output));
    // Order should match config order, and disabled rules must still appear
    // (completion needs to offer them so they can be forced on via --checks).
    assert_eq!(stdout_str(&output), "alpha\nbeta\ngamma");
}

#[test]
fn list_checks_empty_when_no_rules() {
    let tmp = setup_git_repos(&["a"]);
    let cfg = write_config(tmp.path(), "");
    let output = run(tmp.path(), &cfg, &["list-checks"]);
    assert!(output.status.success(), "stderr: {}", stderr_str(&output));
    assert_eq!(stdout_str(&output), "");
}

// ── interactive --diff (3+ groups) ──────────────────────────────────────────

#[test]
fn check_same_diff_three_groups_feeds_pair_and_exits() {
    // Use distinct group sizes so A/B/C are deterministic:
    //   A = 3 repos ("one"), B = 2 repos ("two"), C = 1 repo ("three").
    let tmp = setup_git_repos(&["a1", "a2", "a3", "b1", "b2", "c1"]);
    for r in ["a1", "a2", "a3"] {
        fs::write(tmp.path().join(r).join(".gitignore"), "one\n").unwrap();
    }
    for r in ["b1", "b2"] {
        fs::write(tmp.path().join(r).join(".gitignore"), "two\n").unwrap();
    }
    fs::write(tmp.path().join("c1/.gitignore"), "three\n").unwrap();
    let cfg = write_config(
        tmp.path(),
        r#"
[[check]]
name = "gi"
select = "*"
path = ".gitignore"
"#,
    );

    let output = run_with_stdin(tmp.path(), &cfg, &["check-same", "--diff"], b"A\nB\nn\n");
    assert!(!output.status.success(), "non-copy invocation: mismatch → exit 1");
    let stdout = stdout_str(&output);
    assert!(stdout.contains("--- "), "should emit a diff header: {stdout}");
    assert!(stdout.contains("-one"), "should show group A content: {stdout}");
    assert!(stdout.contains("+two"), "should show group B content: {stdout}");
}

#[test]
fn check_same_diff_three_groups_loops_for_more_pairs() {
    // Distinct sizes for deterministic A/B/C ordering.
    let tmp = setup_git_repos(&["a1", "a2", "a3", "b1", "b2", "c1"]);
    for r in ["a1", "a2", "a3"] {
        fs::write(tmp.path().join(r).join(".gitignore"), "one\n").unwrap();
    }
    for r in ["b1", "b2"] {
        fs::write(tmp.path().join(r).join(".gitignore"), "two\n").unwrap();
    }
    fs::write(tmp.path().join("c1/.gitignore"), "three\n").unwrap();
    let cfg = write_config(
        tmp.path(),
        r#"
[[check]]
name = "gi"
select = "*"
path = ".gitignore"
"#,
    );

    // First diff A→B, then y (another pair), then A→C, then n.
    let output = run_with_stdin(
        tmp.path(),
        &cfg,
        &["check-same", "--diff"],
        b"A\nB\ny\nA\nC\nn\n",
    );
    let stdout = stdout_str(&output);
    let plus_count = stdout.matches("+++ ").count();
    assert_eq!(plus_count, 2, "expected 2 diff bodies, got {plus_count}: {stdout}");
}

// ── --copy ──────────────────────────────────────────────────────────────────

#[test]
fn check_same_copy_overwrites_destination_group() {
    let tmp = setup_git_repos(&["a", "b", "c"]);
    fs::write(tmp.path().join("a/.gitignore"), "canonical\n").unwrap();
    fs::write(tmp.path().join("b/.gitignore"), "canonical\n").unwrap();
    fs::write(tmp.path().join("c/.gitignore"), "stale\n").unwrap();
    let cfg = write_config(
        tmp.path(),
        r#"
[[check]]
name = "gi"
select = "*"
path = ".gitignore"
"#,
    );

    // 2 groups: A = {a, b} (canonical, larger), B = {c} (stale).
    // Copy from A to B, confirm.
    let output = run_with_stdin(
        tmp.path(),
        &cfg,
        &["check-same", "--copy"],
        b"A\nB\ny\n",
    );
    assert!(
        output.status.success(),
        "--copy always exits 0: stderr={}",
        stderr_str(&output)
    );
    // c's .gitignore should now equal a's.
    let c_content = fs::read_to_string(tmp.path().join("c/.gitignore")).unwrap();
    assert_eq!(c_content, "canonical\n");
    // a and b unchanged.
    let a_content = fs::read_to_string(tmp.path().join("a/.gitignore")).unwrap();
    assert_eq!(a_content, "canonical\n");
}

#[test]
fn check_same_copy_declined_leaves_files_alone() {
    let tmp = setup_git_repos(&["a", "b"]);
    fs::write(tmp.path().join("a/.gitignore"), "keep-a\n").unwrap();
    fs::write(tmp.path().join("b/.gitignore"), "keep-b\n").unwrap();
    let cfg = write_config(
        tmp.path(),
        r#"
[[check]]
name = "gi"
select = "*"
path = ".gitignore"
"#,
    );

    // Pick A from, B to, but decline the confirmation.
    let output = run_with_stdin(
        tmp.path(),
        &cfg,
        &["check-same", "--copy"],
        b"A\nB\nn\n",
    );
    assert!(output.status.success());
    let a = fs::read_to_string(tmp.path().join("a/.gitignore")).unwrap();
    let b = fs::read_to_string(tmp.path().join("b/.gitignore")).unwrap();
    assert_eq!(a, "keep-a\n");
    assert_eq!(b, "keep-b\n");
}

#[test]
fn check_same_copy_always_exits_zero() {
    // Even when there's a mismatch and the user quits, --copy returns 0.
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

    // Quit immediately.
    let output = run_with_stdin(tmp.path(), &cfg, &["check-same", "--copy"], b"q\n");
    assert!(output.status.success(), "stderr: {}", stderr_str(&output));
}

#[test]
fn check_same_copy_preserves_destination_mode() {
    // Group A = {a1, a2} (2 files, "from-a"), Group B = {b1} (1 file, "old-b").
    let tmp = setup_git_repos(&["a1", "a2", "b1"]);
    fs::write(tmp.path().join("a1/.gitignore"), "from-a\n").unwrap();
    fs::write(tmp.path().join("a2/.gitignore"), "from-a\n").unwrap();
    fs::write(tmp.path().join("b1/.gitignore"), "old-b\n").unwrap();

    // Set A reps to 0644 and B to 0600 — copy A→B should keep B's 0600.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        for r in ["a1", "a2"] {
            let p = tmp.path().join(r).join(".gitignore");
            let mut perm = fs::metadata(&p).unwrap().permissions();
            perm.set_mode(0o644);
            fs::set_permissions(&p, perm).unwrap();
        }
        let p = tmp.path().join("b1/.gitignore");
        let mut perm = fs::metadata(&p).unwrap().permissions();
        perm.set_mode(0o600);
        fs::set_permissions(&p, perm).unwrap();
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
    let output = run_with_stdin(
        tmp.path(),
        &cfg,
        &["check-same", "--copy"],
        b"A\nB\ny\n",
    );
    assert!(output.status.success(), "stderr: {}", stderr_str(&output));

    let content = fs::read_to_string(tmp.path().join("b1/.gitignore")).unwrap();
    assert_eq!(content, "from-a\n");

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(tmp.path().join("b1/.gitignore"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600, "destination mode should be preserved");
    }
}

#[test]
fn check_same_copy_three_groups_picks_from_and_to() {
    // Use different-sized groups so sort-by-group-size-desc gives deterministic
    // group labels:
    //   group A = {a1, a2, a3} (content "canonical")
    //   group B = {b1, b2}     (content "stale-1")
    //   group C = {c1}         (content "stale-2")
    let tmp = setup_git_repos(&["a1", "a2", "a3", "b1", "b2", "c1"]);
    for r in ["a1", "a2", "a3"] {
        fs::write(tmp.path().join(r).join(".gitignore"), "canonical\n").unwrap();
    }
    for r in ["b1", "b2"] {
        fs::write(tmp.path().join(r).join(".gitignore"), "stale-1\n").unwrap();
    }
    fs::write(tmp.path().join("c1/.gitignore"), "stale-2\n").unwrap();
    let cfg = write_config(
        tmp.path(),
        r#"
[[check]]
name = "gi"
select = "*"
path = ".gitignore"
"#,
    );

    // Copy A → C, leave B alone.
    let output = run_with_stdin(
        tmp.path(),
        &cfg,
        &["check-same", "--copy"],
        b"A\nC\ny\n",
    );
    assert!(output.status.success(), "stderr: {}", stderr_str(&output));
    assert_eq!(
        fs::read_to_string(tmp.path().join("c1/.gitignore")).unwrap(),
        "canonical\n"
    );
    // B repos unchanged.
    assert_eq!(
        fs::read_to_string(tmp.path().join("b1/.gitignore")).unwrap(),
        "stale-1\n"
    );
    assert_eq!(
        fs::read_to_string(tmp.path().join("b2/.gitignore")).unwrap(),
        "stale-1\n"
    );
}

#[test]
fn check_same_copy_rejects_same_group_as_from_and_to() {
    // Group sizes: A = 2 files, B = 1 file — deterministic labels.
    let tmp = setup_git_repos(&["a1", "a2", "b1"]);
    fs::write(tmp.path().join("a1/.gitignore"), "from-a\n").unwrap();
    fs::write(tmp.path().join("a2/.gitignore"), "from-a\n").unwrap();
    fs::write(tmp.path().join("b1/.gitignore"), "bb\n").unwrap();
    let cfg = write_config(
        tmp.path(),
        r#"
[[check]]
name = "gi"
select = "*"
path = ".gitignore"
"#,
    );

    // User picks A for "from", then tries A again for "to" — should be rejected
    // and re-prompted. After re-prompt, pick B and confirm.
    let output = run_with_stdin(
        tmp.path(),
        &cfg,
        &["check-same", "--copy"],
        b"A\nA\nB\ny\n",
    );
    assert!(output.status.success(), "stderr: {}", stderr_str(&output));
    let stdout = stdout_str(&output);
    assert!(stdout.contains("invalid choice"), "should reject same group: {stdout}");
    assert_eq!(
        fs::read_to_string(tmp.path().join("b1/.gitignore")).unwrap(),
        "from-a\n"
    );
}

// ── must_have + --fix-missing ───────────────────────────────────────────────

#[test]
fn check_same_must_have_false_ignores_missing_files() {
    // a has the file, b doesn't. must_have defaults to false → no violation.
    let tmp = setup_git_repos(&["a", "b"]);
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

    let output = run(tmp.path(), &cfg, &["check-same"]);
    assert!(output.status.success(), "stdout: {}\nstderr: {}", stdout_str(&output), stderr_str(&output));
}

#[test]
fn check_same_must_have_true_flags_missing_files() {
    let tmp = setup_git_repos(&["a", "b"]);
    fs::write(tmp.path().join("a/.gitignore"), "x\n").unwrap();
    let cfg = write_config(
        tmp.path(),
        r#"
[[check]]
name = "gi"
select = "*"
path = ".gitignore"
must_have = true
"#,
    );

    let output = run(tmp.path(), &cfg, &["check-same"]);
    assert!(!output.status.success(), "must_have violation should exit 1");
    let stdout = stdout_str(&output);
    assert!(stdout.contains("1 missing"), "summary should mention missing count: {stdout}");
    assert!(stdout.contains("missing in:"), "should emit missing-in block: {stdout}");
    assert!(
        stdout.contains(tmp.path().join("b").to_string_lossy().as_ref()),
        "should list the violating repo: {stdout}"
    );
}

#[test]
fn check_same_fix_missing_creates_files() {
    // a has the file ("canonical"), b and c don't. With must_have=true and
    // --fix-missing, we seed from group A.
    let tmp = setup_git_repos(&["a", "b", "c"]);
    fs::write(tmp.path().join("a/.gitignore"), "canonical\n").unwrap();
    let cfg = write_config(
        tmp.path(),
        r#"
[[check]]
name = "gi"
select = "*"
path = ".gitignore"
must_have = true
"#,
    );

    // Only one content group (A), pick it, confirm.
    let output = run_with_stdin(
        tmp.path(),
        &cfg,
        &["check-same", "--fix-missing"],
        b"A\ny\n",
    );
    assert!(
        output.status.success(),
        "--fix-missing always exits 0: stderr={}",
        stderr_str(&output)
    );
    assert_eq!(
        fs::read_to_string(tmp.path().join("b/.gitignore")).unwrap(),
        "canonical\n"
    );
    assert_eq!(
        fs::read_to_string(tmp.path().join("c/.gitignore")).unwrap(),
        "canonical\n"
    );
    // a unchanged.
    assert_eq!(
        fs::read_to_string(tmp.path().join("a/.gitignore")).unwrap(),
        "canonical\n"
    );
}

#[test]
fn check_same_fix_missing_creates_parent_directories() {
    // Path with nested subdirectories that don't exist in the violator.
    let tmp = setup_git_repos(&["a", "b"]);
    let nested = tmp.path().join("a/.github/workflows/build.yml");
    fs::create_dir_all(nested.parent().unwrap()).unwrap();
    fs::write(&nested, "on: push\n").unwrap();
    let cfg = write_config(
        tmp.path(),
        r#"
[[check]]
name = "wf"
select = "*"
path = ".github/workflows/build.yml"
must_have = true
"#,
    );

    let output = run_with_stdin(
        tmp.path(),
        &cfg,
        &["check-same", "--fix-missing"],
        b"A\ny\n",
    );
    assert!(output.status.success(), "stderr: {}", stderr_str(&output));
    let dst = tmp.path().join("b/.github/workflows/build.yml");
    assert!(dst.exists(), "nested file should have been created: {}", dst.display());
    assert_eq!(fs::read_to_string(&dst).unwrap(), "on: push\n");
}

#[test]
fn check_same_fix_missing_declined_leaves_alone() {
    let tmp = setup_git_repos(&["a", "b"]);
    fs::write(tmp.path().join("a/.gitignore"), "canonical\n").unwrap();
    let cfg = write_config(
        tmp.path(),
        r#"
[[check]]
name = "gi"
select = "*"
path = ".gitignore"
must_have = true
"#,
    );

    // Pick A, then decline the confirmation.
    let output = run_with_stdin(
        tmp.path(),
        &cfg,
        &["check-same", "--fix-missing"],
        b"A\nn\n",
    );
    assert!(output.status.success());
    assert!(!tmp.path().join("b/.gitignore").exists(), "file should not have been created");
}

#[test]
fn check_same_fix_missing_with_no_groups_skips() {
    // Nobody has the file, so there's nothing to seed from.
    let tmp = setup_git_repos(&["a", "b"]);
    let cfg = write_config(
        tmp.path(),
        r#"
[[check]]
name = "gi"
select = "*"
path = ".gitignore"
must_have = true
"#,
    );

    let output = run_with_stdin(tmp.path(), &cfg, &["check-same", "--fix-missing"], b"");
    assert!(output.status.success(), "should still exit 0");
    let stdout = stdout_str(&output);
    assert!(
        stdout.contains("nothing to seed from"),
        "should note no seed available: {stdout}"
    );
    // Neither file should have been created.
    assert!(!tmp.path().join("a/.gitignore").exists());
    assert!(!tmp.path().join("b/.gitignore").exists());
}

#[test]
fn check_same_fix_missing_always_exits_zero() {
    let tmp = setup_git_repos(&["a", "b"]);
    fs::write(tmp.path().join("a/.gitignore"), "x\n").unwrap();
    let cfg = write_config(
        tmp.path(),
        r#"
[[check]]
name = "gi"
select = "*"
path = ".gitignore"
must_have = true
"#,
    );
    // Quit immediately.
    let output = run_with_stdin(tmp.path(), &cfg, &["check-same", "--fix-missing"], b"q\n");
    assert!(output.status.success());
}
