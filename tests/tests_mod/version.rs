use tempfile::TempDir;
use crate::common::{run_rmg, stdout_str};

#[test]
fn version_subcommand_prints_info() {
    let tmp = TempDir::new().unwrap();
    let output = run_rmg(tmp.path(), &["version"]);
    assert!(output.status.success());
    let stdout = stdout_str(&output);
    assert!(stdout.contains("RSMultiGit"), "version output should contain 'RSMultiGit'");
    assert!(stdout.contains("RSMULTIGIT_GIT_SHA:"), "version output should contain git sha");
    assert!(stdout.contains("RSMULTIGIT_GIT_BRANCH:"), "version output should contain git branch");
    assert!(stdout.contains("RSMULTIGIT_RUSTC_SEMVER:"), "version output should contain rustc version");
}

#[test]
fn version_flag_prints_short_version() {
    let tmp = TempDir::new().unwrap();
    let output = run_rmg(tmp.path(), &["--version"]);
    assert!(output.status.success());
    let stdout = stdout_str(&output);
    assert!(stdout.starts_with("rsmultigit "), "expected 'rsmultigit ...' but got: {stdout}");
}
