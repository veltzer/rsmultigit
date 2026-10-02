use std::ffi::{OsStr, OsString};
use std::process::{Command, Stdio};

use camino::{Utf8Path, Utf8PathBuf};

use anyhow::{Context, Result, bail};

use crate::subprocess_utils::check_call_with_env;

/// The environment variable `cargo publish` reads its crates.io token from.
pub const TOKEN_VAR: &str = "CARGO_REGISTRY_TOKEN";

/// The crates.io token lives in pass(1) only, so there is no
/// `~/.cargo/credentials` file for cargo to fall back on. `cargo release` runs
/// `cargo publish` as a child process, which reads the token from
/// `CARGO_REGISTRY_TOKEN`; fetching it once and handing it over through the
/// environment of each `cargo release` process keeps it off the disk.
pub fn default_pass_entry() -> String {
    "keys/crates.io".to_string()
}

/// Everything `rust publish` needs before touching the first repo: the level
/// to bump and the crates.io token. Built once per run, not once per repo, so
/// a missing cargo-release is reported before any version is bumped and
/// pass(1) is asked (and gpg prompts) at most once.
pub struct Release {
    level: &'static str,
    token: String,
}

impl Release {
    /// `level` is one of "patch", "minor", "major" (see
    /// `cli::ReleaseType::as_str`); `pass_entry` is the pass(1) entry holding
    /// the crates.io token, consulted only when `CARGO_REGISTRY_TOKEN` is not
    /// already set in the environment.
    pub fn prepare(level: &'static str, pass_entry: &str) -> Result<Self> {
        let path = std::env::var_os("PATH").unwrap_or_default();
        ensure_cargo_release_installed(&path)?;
        let token = token_from(std::env::var_os(TOKEN_VAR), pass_entry)?;
        Ok(Self { level, token })
    }

    /// Release a new version of the rust project in `project`:
    /// `cargo release <level> --execute --no-confirm` bumps the version in
    /// Cargo.toml, commits, tags, pushes, and publishes to crates.io (see
    /// release.toml in the repo for the rest of the policy).
    pub fn run(&self, project: &Utf8Path) -> Result<()> {
        check_call_with_env(
            project,
            "cargo",
            &["release", self.level, "--execute", "--no-confirm"],
            &[(TOKEN_VAR, &self.token)],
        )
    }
}

/// cargo-release is a separate crate, not part of the toolchain, so a rebuilt
/// CARGO_HOME loses it. Check up front: cargo's own message for a missing
/// subcommand, "no such command: release", does not say which crate is
/// missing, and by the time it appears the first repo is already being
/// processed.
fn ensure_cargo_release_installed(path: &OsStr) -> Result<()> {
    if find_in_path("cargo-release", path).is_none() {
        bail!("cargo-release is not installed, run: cargo install cargo-release");
    }
    Ok(())
}

/// The first executable file named `name` in the `PATH`-style list `path`,
/// like `command -v`.
pub fn find_in_path(name: &str, path: &OsStr) -> Option<Utf8PathBuf> {
    std::env::split_paths(path)
        .filter_map(|dir| Utf8PathBuf::from_path_buf(dir).ok())
        .map(|dir| dir.join(name))
        .find(|candidate| is_executable_file(candidate))
}

fn is_executable_file(path: &Utf8Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    match path.metadata() {
        Ok(meta) => meta.is_file() && meta.permissions().mode() & 0o111 != 0,
        Err(_) => false,
    }
}

/// The token to hand to cargo: `CARGO_REGISTRY_TOKEN` from the environment
/// when it is set and non-empty (the interactive shell wrapper does the same
/// thing), otherwise the first line of `pass show <pass_entry>`.
fn token_from(env: Option<OsString>, pass_entry: &str) -> Result<String> {
    if let Some(token) = env {
        let token = token
            .into_string()
            .map_err(|_| anyhow::anyhow!("{TOKEN_VAR} is not valid UTF-8"))?;
        if !token.is_empty() {
            return Ok(token);
        }
    }
    pass_show(pass_entry)
}

/// The first line of `pass show <entry>`. Only stdout is captured; stdin and
/// stderr stay attached so gpg can prompt for a passphrase.
fn pass_show(entry: &str) -> Result<String> {
    let output = Command::new("pass")
        .args(["show", entry])
        .stdout(Stdio::piped())
        .output()
        .context("running `pass`; is pass(1) installed?")?;
    if !output.status.success() {
        bail!("pass show {entry} failed with {}", output.status);
    }
    let stdout = String::from_utf8(output.stdout).context("pass output is not valid UTF-8")?;
    let token = stdout.lines().next().unwrap_or_default().trim().to_string();
    if token.is_empty() {
        bail!("pass show {entry} returned an empty token");
    }
    Ok(token)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn tool(dir: &Utf8Path, name: &str, mode: u32) -> Utf8PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, "#!/bin/sh\n").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).unwrap();
        path
    }

    #[test]
    fn find_in_path_finds_the_first_executable_match() {
        let tmp = tempfile::tempdir().unwrap();
        let tmp = Utf8Path::from_path(tmp.path()).unwrap();
        let first = tmp.join("first");
        let second = tmp.join("second");
        std::fs::create_dir_all(&first).unwrap();
        std::fs::create_dir_all(&second).unwrap();
        // Not executable in `first`, executable in `second`: `second` wins.
        tool(&first, "cargo-release", 0o644);
        let hit = tool(&second, "cargo-release", 0o755);
        let path = std::env::join_paths([first.as_std_path(), second.as_std_path()]).unwrap();
        assert_eq!(find_in_path("cargo-release", &path), Some(hit));
        assert_eq!(find_in_path("cargo-nothing", &path), None);
        assert!(ensure_cargo_release_installed(&path).is_ok());
    }

    #[test]
    fn missing_cargo_release_names_the_fix() {
        let tmp = tempfile::tempdir().unwrap();
        let err = ensure_cargo_release_installed(tmp.path().as_os_str()).unwrap_err();
        assert!(
            err.to_string().contains("cargo install cargo-release"),
            "{err}"
        );
    }

    #[test]
    fn token_from_prefers_a_non_empty_environment_value() {
        let token = token_from(Some(OsString::from("from-env")), "keys/crates.io").unwrap();
        assert_eq!(token, "from-env");
    }
}
