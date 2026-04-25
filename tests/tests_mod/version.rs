use crate::common::{run_rsmultigit, stdout_str};
use tempfile::TempDir;

#[test]
fn version_subcommand_prints_info() {
    let tmp = TempDir::new().unwrap();
    let output = run_rsmultigit(tmp.path(), &["version"]);
    assert!(output.status.success());
    let stdout = stdout_str(&output);
    assert!(
        stdout.contains("rsmultigit"),
        "version output should contain 'rsmultigit'"
    );
    assert!(
        stdout.contains("GIT_SHA:"),
        "version output should contain git sha"
    );
    assert!(
        stdout.contains("GIT_BRANCH:"),
        "version output should contain git branch"
    );
    assert!(
        stdout.contains("RUSTC_SEMVER:"),
        "version output should contain rustc version"
    );
}

#[test]
fn version_flag_prints_short_version() {
    let tmp = TempDir::new().unwrap();
    let output = run_rsmultigit(tmp.path(), &["--version"]);
    assert!(output.status.success());
    let stdout = stdout_str(&output);
    assert!(
        stdout.starts_with("rsmultigit "),
        "expected 'rsmultigit ...' but got: {stdout}"
    );
}
