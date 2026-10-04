//! `rsmultigit setup`, driven non-interactively: the tests have no terminal,
//! so every answer comes from a flag and the prompts are never reached.

use std::fs;

use crate::common::{run_rsmultigit_with_env, setup_git_repos, stderr_str, stdout_str, utf8};

#[test]
fn setup_writes_a_config_the_other_commands_can_use() {
    let tmp = setup_git_repos(&["a", "b"]);
    let root = utf8(&tmp);
    fs::write(root.join("a/Cargo.toml"), "").unwrap();
    fs::create_dir_all(root.join("not-a-repo")).unwrap();
    // The config path does not exist yet, nor do its parent directories.
    let cfg = root.join("home/.config/rsmultigit/config.toml");

    let output = run_rsmultigit_with_env(
        root,
        &["setup", "--repos-dir", root.as_str(), "--build", "cargo"],
        &[("RSMULTIGIT_CONFIG", cfg.as_str())],
    );
    assert!(output.status.success(), "stderr: {}", stderr_str(&output));
    let stdout = stdout_str(&output);
    assert!(stdout.contains(&format!("Wrote {cfg}")), "{stdout}");
    assert!(stdout.contains("2 git repositories"), "{stdout}");
    assert!(
        stdout.contains("cargo (1 of 2 repos have Cargo.toml)"),
        "{stdout}"
    );

    let text = fs::read_to_string(&cfg).unwrap();
    assert!(text.contains(&format!("\"{root}/*\"")), "{text}");
    assert!(text.contains("default_build_method = \"cargo\""), "{text}");

    // The written config drives the tool: list-repos sees exactly the two repos.
    let output = run_rsmultigit_with_env(
        root,
        &["list-repos"],
        &[("RSMULTIGIT_CONFIG", cfg.as_str())],
    );
    assert!(output.status.success(), "stderr: {}", stderr_str(&output));
    let stdout = stdout_str(&output);
    let listed: Vec<&str> = stdout.lines().collect();
    assert_eq!(listed.len(), 2, "{listed:?}");
    assert!(
        listed
            .iter()
            .all(|l| l.ends_with("/a") || l.ends_with("/b")),
        "{listed:?}"
    );
}

#[test]
fn setup_no_build_leaves_the_default_method_unset() {
    let tmp = setup_git_repos(&["a"]);
    let root = utf8(&tmp);
    let cfg = root.join("config.toml");
    let output = run_rsmultigit_with_env(
        root,
        &["setup", "--repos-dir", root.as_str(), "--no-build"],
        &[("RSMULTIGIT_CONFIG", cfg.as_str())],
    );
    assert!(output.status.success(), "stderr: {}", stderr_str(&output));
    assert!(stdout_str(&output).contains("default build method: none"));
    let text = fs::read_to_string(&cfg).unwrap();
    assert!(!text.contains("\ndefault_build_method ="), "{text}");

    // And a bare `build` now says so, through the normal config path.
    let output = run_rsmultigit_with_env(root, &["build"], &[("RSMULTIGIT_CONFIG", cfg.as_str())]);
    assert!(!output.status.success());
    assert!(stderr_str(&output).contains("default_build_method"));
}

#[test]
fn setup_build_and_no_build_are_exclusive() {
    let tmp = setup_git_repos(&["a"]);
    let root = utf8(&tmp);
    let cfg = root.join("config.toml");
    let output = run_rsmultigit_with_env(
        root,
        &[
            "setup",
            "--repos-dir",
            root.as_str(),
            "--build",
            "make",
            "--no-build",
        ],
        &[("RSMULTIGIT_CONFIG", cfg.as_str())],
    );
    assert!(!output.status.success());
    assert!(
        stderr_str(&output).contains("cannot be used with"),
        "{}",
        stderr_str(&output)
    );
    assert!(!cfg.exists());
}

#[test]
fn setup_without_a_terminal_names_the_missing_flag() {
    let tmp = setup_git_repos(&["a"]);
    let root = utf8(&tmp);
    let cfg = root.join("config.toml");

    // Repos dir missing: the first question would be asked.
    let output = run_rsmultigit_with_env(
        root,
        &["setup", "--no-build"],
        &[("RSMULTIGIT_CONFIG", cfg.as_str())],
    );
    assert!(!output.status.success());
    let stderr = stderr_str(&output);
    assert!(stderr.contains("--repos-dir"), "{stderr}");

    // Build method missing: the second question would be asked.
    let output = run_rsmultigit_with_env(
        root,
        &["setup", "--repos-dir", root.as_str()],
        &[("RSMULTIGIT_CONFIG", cfg.as_str())],
    );
    assert!(!output.status.success());
    let stderr = stderr_str(&output);
    assert!(stderr.contains("--build"), "{stderr}");
    assert!(stderr.contains("--no-build"), "{stderr}");
    assert!(!cfg.exists());
}

#[test]
fn setup_refuses_a_directory_without_repos() {
    let tmp = tempfile::TempDir::new().unwrap();
    let root = utf8(&tmp);
    fs::create_dir_all(root.join("plain")).unwrap();
    let cfg = root.join("config.toml");
    let output = run_rsmultigit_with_env(
        root,
        &["setup", "--repos-dir", root.as_str(), "--no-build"],
        &[("RSMULTIGIT_CONFIG", cfg.as_str())],
    );
    assert!(!output.status.success());
    assert!(
        stderr_str(&output).contains("no git repositories directly under"),
        "{}",
        stderr_str(&output)
    );
    assert!(!cfg.exists());

    let output = run_rsmultigit_with_env(
        root,
        &[
            "setup",
            "--repos-dir",
            root.join("missing").as_str(),
            "--no-build",
        ],
        &[("RSMULTIGIT_CONFIG", cfg.as_str())],
    );
    assert!(!output.status.success());
    assert!(
        stderr_str(&output).contains("is not a directory"),
        "{}",
        stderr_str(&output)
    );
}

#[test]
fn setup_keeps_an_existing_config_unless_overwrite() {
    let tmp = setup_git_repos(&["a"]);
    let root = utf8(&tmp);
    let cfg = root.join("config.toml");
    fs::write(&cfg, "repos = [\"/elsewhere/*\"]\n").unwrap();

    let output = run_rsmultigit_with_env(
        root,
        &["setup", "--repos-dir", root.as_str(), "--no-build"],
        &[("RSMULTIGIT_CONFIG", cfg.as_str())],
    );
    assert!(!output.status.success());
    assert!(
        stderr_str(&output).contains("--overwrite"),
        "{}",
        stderr_str(&output)
    );
    assert_eq!(
        fs::read_to_string(&cfg).unwrap(),
        "repos = [\"/elsewhere/*\"]\n"
    );

    let output = run_rsmultigit_with_env(
        root,
        &[
            "setup",
            "--repos-dir",
            root.as_str(),
            "--no-build",
            "--overwrite",
        ],
        &[("RSMULTIGIT_CONFIG", cfg.as_str())],
    );
    assert!(output.status.success(), "stderr: {}", stderr_str(&output));
    assert!(
        fs::read_to_string(&cfg)
            .unwrap()
            .contains(&format!("\"{root}/*\""))
    );
}

#[test]
fn setup_does_not_need_an_existing_config() {
    // Like config-example: a broken or missing config file must not stop the
    // command that exists to write one.
    let tmp = setup_git_repos(&["a"]);
    let root = utf8(&tmp);
    let cfg = root.join("config.toml");
    fs::write(&cfg, "this is = not [ toml\n").unwrap();
    let output = run_rsmultigit_with_env(
        root,
        &[
            "setup",
            "--repos-dir",
            root.as_str(),
            "--no-build",
            "--overwrite",
        ],
        &[("RSMULTIGIT_CONFIG", cfg.as_str())],
    );
    assert!(output.status.success(), "stderr: {}", stderr_str(&output));
}
