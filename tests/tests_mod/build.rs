use camino::Utf8Path;
use std::fs;
use std::process::Output;

use crate::common::{run_rsmultigit_with_env, setup_git_repos, stderr_str, write_config};

fn run(tmp: &Utf8Path, config: &Utf8Path, args: &[&str]) -> Output {
    let cfg_str = config.to_string();
    run_rsmultigit_with_env(tmp, args, &[("RSMULTIGIT_CONFIG", &cfg_str)])
}

/// Give each repo a Makefile whose default target drops a marker file, so a
/// `make` build leaves visible evidence without needing any real toolchain.
fn add_marker_makefiles(tmp: &Utf8Path, repos: &[&str]) {
    for repo in repos {
        fs::write(
            tmp.join(repo).join("Makefile"),
            "all:\n\ttouch built.marker\n",
        )
        .unwrap();
    }
}

#[test]
fn build_without_method_and_without_default_is_an_error() {
    let tmp = setup_git_repos(&["a"]);
    let tmp = Utf8Path::from_path(tmp.path()).unwrap();
    let cfg = write_config(tmp, "");
    let out = run(tmp, &cfg, &["build"]);
    assert!(!out.status.success());
    let err = stderr_str(&out);
    assert!(err.contains("default_build_method"), "stderr: {err}");
    assert!(err.contains("usage: rsmultigit build"), "stderr: {err}");
}

#[test]
fn build_without_method_uses_default_build_method_from_config() {
    let tmp = setup_git_repos(&["a", "b"]);
    let tmp = Utf8Path::from_path(tmp.path()).unwrap();
    add_marker_makefiles(tmp, &["a", "b"]);
    let cfg = write_config(tmp, "default_build_method = \"make\"\n");
    let out = run(tmp, &cfg, &["build"]);
    assert!(out.status.success(), "stderr: {}", stderr_str(&out));
    assert!(tmp.join("a/built.marker").exists());
    assert!(tmp.join("b/built.marker").exists());
}

#[test]
fn build_explicit_method_overrides_default_build_method() {
    let tmp = setup_git_repos(&["a"]);
    let tmp = Utf8Path::from_path(tmp.path()).unwrap();
    add_marker_makefiles(tmp, &["a"]);
    // Default is rsconstruct, but no repo has rsconstruct.toml, so that would
    // skip everything. Explicit `make` must win and produce the marker.
    let cfg = write_config(tmp, "default_build_method = \"rsconstruct\"\n");
    let out = run(tmp, &cfg, &["build", "make"]);
    assert!(out.status.success(), "stderr: {}", stderr_str(&out));
    assert!(tmp.join("a/built.marker").exists());
}

#[test]
fn build_rejects_unknown_default_build_method_in_config() {
    let tmp = setup_git_repos(&["a"]);
    let tmp = Utf8Path::from_path(tmp.path()).unwrap();
    let cfg = write_config(tmp, "default_build_method = \"ninja\"\n");
    let out = run(tmp, &cfg, &["build"]);
    assert!(!out.status.success());
    let err = stderr_str(&out);
    assert!(err.contains("ninja"), "stderr: {err}");
}
