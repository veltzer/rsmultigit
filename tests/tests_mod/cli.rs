use crate::common::{run_rsmultigit, stderr_str, stdout_str, utf8};

#[test]
fn help_flag_shows_usage() {
    let tmp = tempfile::TempDir::new().unwrap();
    let output = run_rsmultigit(utf8(&tmp), &["--help"]);
    assert!(output.status.success());
    let stdout = stdout_str(&output);
    assert!(
        stdout.contains("Usage:"),
        "help should contain Usage: {stdout}"
    );
    assert!(
        stdout.contains("count"),
        "help should list subcommands: {stdout}"
    );
}

#[test]
fn unknown_subcommand_fails() {
    let tmp = tempfile::TempDir::new().unwrap();
    let output = run_rsmultigit(utf8(&tmp), &["nonexistent"]);
    assert!(!output.status.success());
    let stderr = stderr_str(&output);
    assert!(
        stderr.contains("unrecognized") || stderr.contains("invalid"),
        "should report error: {stderr}"
    );
}

#[test]
fn no_subcommand_fails() {
    let tmp = tempfile::TempDir::new().unwrap();
    let output = run_rsmultigit(utf8(&tmp), &[]);
    assert!(!output.status.success());
}

#[test]
fn grep_requires_regexp() {
    let tmp = tempfile::TempDir::new().unwrap();
    let output = run_rsmultigit(utf8(&tmp), &["git", "grep"]);
    assert!(!output.status.success());
}

#[test]
fn bare_gh_lists_its_operations_with_descriptions() {
    // A command whose positional picks the operation, invoked bare, must
    // tell the user what the choices are - not just that `<WHAT>` is missing.
    let tmp = tempfile::TempDir::new().unwrap();
    let output = run_rsmultigit(utf8(&tmp), &["gh"]);
    assert_eq!(output.status.code(), Some(2));
    let stderr = stderr_str(&output);
    assert!(
        !stderr.contains("required arguments were not provided"),
        "bare gh should print help, not the missing-argument error: {stderr}"
    );
    assert!(stderr.contains("Usage: rsmultigit gh"), "{stderr}");
    for name in [
        "clean-all",
        "artifacts",
        "last-workflow-state",
        "open-site",
        "sync-metadata",
    ] {
        assert!(
            stderr.contains(name),
            "bare gh should list `{name}`: {stderr}"
        );
    }
    assert!(
        stderr.contains("Print the conclusion of the most recent workflow run"),
        "each choice should come with its description: {stderr}"
    );
}

#[test]
fn bare_count_lists_its_choices() {
    let tmp = tempfile::TempDir::new().unwrap();
    let output = run_rsmultigit(utf8(&tmp), &["count"]);
    assert_eq!(output.status.code(), Some(2));
    let stderr = stderr_str(&output);
    for name in ["dirty", "untracked", "synchronized"] {
        assert!(
            stderr.contains(name),
            "bare count should list `{name}`: {stderr}"
        );
    }
}

#[test]
fn bare_git_lists_its_subcommands_and_no_flags() {
    // `git` is a group of subcommands rather than a `<WHAT>` positional;
    // invoked bare, or with options only, it lists them all the same.
    let tmp = tempfile::TempDir::new().unwrap();
    for args in [&["git"][..], &["git", "--no-stop"][..]] {
        let output = run_rsmultigit(utf8(&tmp), args);
        assert_eq!(output.status.code(), Some(2), "{args:?}");
        let stderr = stderr_str(&output);
        assert!(
            stderr.contains("Usage: rsmultigit git"),
            "{args:?}: {stderr}"
        );
        assert!(
            !stderr.contains("requires a subcommand"),
            "{args:?}: {stderr}"
        );
        for name in ["pull", "restore", "remote-prune", "submodule-update"] {
            assert!(
                stderr.contains(name),
                "{args:?} should list `{name}`: {stderr}"
            );
        }
        assert!(
            stderr.contains("Discard unstaged changes to tracked files"),
            "each subcommand should come with its description: {stderr}"
        );
        assert!(!stderr.contains("Options:"), "{args:?}: {stderr}");
        assert!(
            stderr.contains("Run `rsmultigit git --help` for the options."),
            "{stderr}"
        );
    }
}

#[test]
fn bare_git_operation_lists_its_choices() {
    let tmp = tempfile::TempDir::new().unwrap();
    let output = run_rsmultigit(utf8(&tmp), &["git", "reset"]);
    assert_eq!(output.status.code(), Some(2));
    let stderr = stderr_str(&output);
    assert!(stderr.contains("Usage: rsmultigit git reset"), "{stderr}");
    for name in ["hard", "soft", "mixed"] {
        assert!(
            stderr.contains(name),
            "bare git reset should list `{name}`: {stderr}"
        );
    }
}

#[test]
fn gh_with_options_but_no_operation_lists_its_operations() {
    // Options alone do not satisfy the command either; the choices are shown
    // just as for the bare form, instead of a "<WHAT> not provided" error.
    let tmp = tempfile::TempDir::new().unwrap();
    let output = run_rsmultigit(utf8(&tmp), &["gh", "--keep", "3"]);
    assert_eq!(output.status.code(), Some(2));
    let stderr = stderr_str(&output);
    assert!(
        !stderr.contains("required arguments were not provided"),
        "{stderr}"
    );
    assert!(stderr.contains("Usage: rsmultigit gh"), "{stderr}");
    assert!(stderr.contains("last-workflow-state"), "{stderr}");
}

#[test]
fn missing_plain_positional_keeps_clap_error() {
    // Only the operation-selecting positional gets the help treatment; a
    // missing free-form argument (blame's file) still gets clap's error.
    let tmp = tempfile::TempDir::new().unwrap();
    let output = run_rsmultigit(utf8(&tmp), &["git", "blame"]);
    assert_eq!(output.status.code(), Some(2));
    let stderr = stderr_str(&output);
    assert!(
        stderr.contains("required arguments were not provided") && stderr.contains("<FILE>"),
        "{stderr}"
    );
}
