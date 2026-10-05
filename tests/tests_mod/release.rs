//! `cargo release` end to end, against a fake toolchain on PATH: a `cargo`
//! that records how it was called, a `cargo-release` that merely exists, and
//! a `pass` that hands out a known token. Nothing here talks to crates.io.

use camino::{Utf8Path, Utf8PathBuf};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::process::Output;

use crate::common::{run_rsmultigit_with_env, setup_git_repos, stderr_str, utf8, write_config};

/// Write an executable shell script `name` into `bin`.
fn script(bin: &Utf8Path, name: &str, body: &str) {
    let path = bin.join(name);
    fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
}

/// A bin dir with the three tools `cargo release` needs. The fake `cargo`
/// appends its arguments and the token it was given to `cargo.log` in its
/// working directory (the repo); the fake `pass` logs the entry it was asked
/// for to `pass.log` next to the scripts and prints `token-from-pass`.
fn fake_bin(tmp: &Utf8Path) -> Utf8PathBuf {
    let bin = tmp.join("bin");
    fs::create_dir_all(&bin).unwrap();
    script(
        &bin,
        "cargo",
        "echo \"$* token=$CARGO_REGISTRY_TOKEN\" >> cargo.log",
    );
    script(&bin, "cargo-release", "exit 0");
    script(
        &bin,
        "pass",
        &format!(
            "echo \"$*\" >> {}/pass.log\necho token-from-pass\necho second-line-ignored",
            bin
        ),
    );
    bin
}

/// Run `rsmultigit <args>` with `bin` first on PATH. `CARGO_REGISTRY_TOKEN`
/// is cleared unless the test sets it, so a token in the developer's own
/// environment cannot leak into the run.
fn run(tmp: &Utf8Path, bin: &Utf8Path, extra_config: &str, args: &[&str], token: &str) -> Output {
    let cfg = write_config(tmp, extra_config).to_string();
    let path = format!("{bin}:{}", std::env::var("PATH").unwrap_or_default());
    run_rsmultigit_with_env(
        tmp,
        args,
        &[
            ("RSMULTIGIT_CONFIG", &cfg),
            ("PATH", &path),
            ("CARGO_REGISTRY_TOKEN", token),
        ],
    )
}

fn read(path: Utf8PathBuf) -> String {
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

#[test]
fn publish_runs_cargo_release_in_cargo_repos_with_the_token_from_pass() {
    let tmp = setup_git_repos(&["crate-a", "plain"]);
    let tmp = utf8(&tmp);
    fs::write(tmp.join("crate-a/Cargo.toml"), "[package]\nname = \"a\"\n").unwrap();
    let bin = fake_bin(tmp);

    let out = run(tmp, &bin, "", &["cargo", "release"], "");
    assert!(out.status.success(), "stderr: {}", stderr_str(&out));
    assert_eq!(
        read(tmp.join("crate-a/cargo.log")),
        "release patch --execute --no-confirm token=token-from-pass\n"
    );
    assert!(
        !tmp.join("plain/cargo.log").exists(),
        "non-cargo repo was skipped"
    );
    assert_eq!(read(bin.join("pass.log")), "show keys/crates.io\n");
}

#[test]
fn publish_skips_virtual_workspaces_and_crates_marked_publish_false() {
    let tmp = setup_git_repos(&["crate-a", "demos", "scratch"]);
    let tmp = utf8(&tmp);
    fs::write(tmp.join("crate-a/Cargo.toml"), "[package]\nname = \"a\"\n").unwrap();
    // A repo of examples: a root [workspace] of members, no [package].
    fs::write(
        tmp.join("demos/Cargo.toml"),
        "[workspace]\nresolver = \"2\"\nmembers = [\"examples/hello\"]\n",
    )
    .unwrap();
    // A real crate that says, in cargo's own words, that it never goes out.
    fs::write(
        tmp.join("scratch/Cargo.toml"),
        "[package]\nname = \"bake-off\"\npublish = false\n",
    )
    .unwrap();
    let bin = fake_bin(tmp);

    let out = run(tmp, &bin, "", &["cargo", "release"], "");
    assert!(out.status.success(), "stderr: {}", stderr_str(&out));
    assert_eq!(
        read(tmp.join("crate-a/cargo.log")),
        "release patch --execute --no-confirm token=token-from-pass\n"
    );
    assert!(
        !tmp.join("demos/cargo.log").exists(),
        "virtual workspace was skipped"
    );
    assert!(
        !tmp.join("scratch/cargo.log").exists(),
        "publish = false crate was skipped"
    );
}

#[test]
fn publish_type_selects_the_level_and_pass_is_asked_once_for_the_fleet() {
    let tmp = setup_git_repos(&["crate-a", "crate-b"]);
    let tmp = utf8(&tmp);
    for repo in ["crate-a", "crate-b"] {
        fs::write(tmp.join(repo).join("Cargo.toml"), "[package]\n").unwrap();
    }
    let bin = fake_bin(tmp);

    let out = run(tmp, &bin, "", &["cargo", "release", "--type", "minor"], "");
    assert!(out.status.success(), "stderr: {}", stderr_str(&out));
    for repo in ["crate-a", "crate-b"] {
        assert_eq!(
            read(tmp.join(repo).join("cargo.log")),
            "release minor --execute --no-confirm token=token-from-pass\n"
        );
    }
    assert_eq!(read(bin.join("pass.log")), "show keys/crates.io\n");
}

#[test]
fn publish_reads_the_pass_entry_from_the_config() {
    let tmp = setup_git_repos(&["crate-a"]);
    let tmp = utf8(&tmp);
    fs::write(tmp.join("crate-a/Cargo.toml"), "[package]\n").unwrap();
    let bin = fake_bin(tmp);

    let config = "crates_io_pass_entry = \"work/crates\"\n";
    let out = run(tmp, &bin, config, &["cargo", "release"], "");
    assert!(out.status.success(), "stderr: {}", stderr_str(&out));
    assert_eq!(read(bin.join("pass.log")), "show work/crates\n");
}

#[test]
fn publish_prefers_a_token_already_in_the_environment() {
    let tmp = setup_git_repos(&["crate-a"]);
    let tmp = utf8(&tmp);
    fs::write(tmp.join("crate-a/Cargo.toml"), "[package]\n").unwrap();
    let bin = fake_bin(tmp);
    // A pass that fails proves it was never consulted.
    script(&bin, "pass", "echo 'pass must not run' >&2; exit 1");

    let out = run(tmp, &bin, "", &["cargo", "release"], "token-from-env");
    assert!(out.status.success(), "stderr: {}", stderr_str(&out));
    assert_eq!(
        read(tmp.join("crate-a/cargo.log")),
        "release patch --execute --no-confirm token=token-from-env\n"
    );
}

#[test]
fn publish_without_cargo_release_fails_before_touching_any_repo() {
    let tmp = setup_git_repos(&["crate-a"]);
    let tmp = utf8(&tmp);
    fs::write(tmp.join("crate-a/Cargo.toml"), "[package]\n").unwrap();
    let bin = fake_bin(tmp);
    fs::remove_file(bin.join("cargo-release")).unwrap();
    // Hide any real cargo-release: PATH is just the fake bin.
    let cfg = write_config(tmp, "").to_string();
    let out = run_rsmultigit_with_env(
        tmp,
        &["cargo", "release"],
        &[
            ("RSMULTIGIT_CONFIG", &cfg),
            ("PATH", bin.as_str()),
            ("CARGO_REGISTRY_TOKEN", ""),
        ],
    );
    assert!(!out.status.success());
    let err = stderr_str(&out);
    assert!(err.contains("cargo install cargo-release"), "stderr: {err}");
    assert!(
        !tmp.join("crate-a/cargo.log").exists(),
        "cargo was never run"
    );
    assert!(!bin.join("pass.log").exists(), "pass was never asked");
}

#[test]
fn publish_fails_when_pass_has_no_token() {
    let tmp = setup_git_repos(&["crate-a"]);
    let tmp = utf8(&tmp);
    fs::write(tmp.join("crate-a/Cargo.toml"), "[package]\n").unwrap();
    let bin = fake_bin(tmp);
    script(&bin, "pass", "exit 1");

    let out = run(tmp, &bin, "", &["cargo", "release"], "");
    assert!(!out.status.success());
    let err = stderr_str(&out);
    assert!(
        err.contains("pass show keys/crates.io failed"),
        "stderr: {err}"
    );
    assert!(
        !tmp.join("crate-a/cargo.log").exists(),
        "cargo was never run"
    );
}
