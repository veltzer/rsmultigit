use crate::common::{run_rsmultigit, stderr_str, stdout_str};

#[test]
fn help_flag_shows_usage() {
    let tmp = tempfile::TempDir::new().unwrap();
    let output = run_rsmultigit(tmp.path(), &["--help"]);
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
    let output = run_rsmultigit(tmp.path(), &["nonexistent"]);
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
    let output = run_rsmultigit(tmp.path(), &[]);
    assert!(!output.status.success());
}

#[test]
fn grep_requires_regexp() {
    let tmp = tempfile::TempDir::new().unwrap();
    let output = run_rsmultigit(tmp.path(), &["grep"]);
    assert!(!output.status.success());
}
