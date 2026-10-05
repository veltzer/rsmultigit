use std::ffi::{OsStr, OsString};
use std::process::{Command, Stdio};

use camino::{Utf8Path, Utf8PathBuf};

use anyhow::{Context, Result, bail};

use crate::subprocess_utils::check_call_with_env;

/// Should `cargo release` release this project? Only a repo whose root
/// `Cargo.toml` declares a `[package]` that cargo would publish. That is
/// cargo's own vocabulary for "this is a crate meant for a registry", so no
/// rsmultigit-specific marker is needed:
///
/// - A virtual workspace (a `[workspace]` of members with no root `[package]`)
///   has nothing to version or publish and is skipped.
/// - A package with `publish = false` (or `publish = []`, cargo's spelling of
///   the same thing) is a scratch crate, skipped here just as `cargo publish`
///   itself would refuse it.
/// - `publish.workspace = true` defers to `[workspace.package].publish` in
///   the same manifest, as cargo does.
///
/// A repo with no `Cargo.toml` is not a rust project at all, hence skipped.
pub fn check_publishable(project: &Utf8Path) -> Result<bool> {
    let manifest = project.join("Cargo.toml");
    if !manifest.exists() {
        return Ok(false);
    }
    let text = std::fs::read_to_string(&manifest).with_context(|| format!("reading {manifest}"))?;
    let doc: toml::Table = toml::from_str(&text).with_context(|| format!("parsing {manifest}"))?;
    Ok(manifest_is_publishable(&doc))
}

/// The decision behind [`check_publishable`], on an already-parsed manifest.
fn manifest_is_publishable(doc: &toml::Table) -> bool {
    let Some(package) = doc.get("package") else {
        return false;
    };
    let mut publish = package.get("publish");
    if let Some(table) = publish.and_then(toml::Value::as_table)
        && table.get("workspace").and_then(toml::Value::as_bool) == Some(true)
    {
        publish = doc
            .get("workspace")
            .and_then(|ws| ws.get("package"))
            .and_then(|pkg| pkg.get("publish"));
    }
    match publish {
        None => true,
        Some(toml::Value::Boolean(allowed)) => *allowed,
        Some(toml::Value::Array(registries)) => !registries.is_empty(),
        // Anything else is not a shape cargo accepts; leave it to cargo to
        // complain rather than silently skipping a real crate.
        Some(_) => true,
    }
}

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

/// Everything `cargo release` needs before touching the first repo: the level
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

    fn publishable(manifest: &str) -> bool {
        manifest_is_publishable(&toml::from_str(manifest).unwrap())
    }

    #[test]
    fn a_plain_package_is_publishable() {
        assert!(publishable(
            "[package]\nname = \"x\"\nversion = \"0.1.0\"\n"
        ));
        assert!(publishable("[package]\nname = \"x\"\npublish = true\n"));
        assert!(publishable(
            "[package]\nname = \"x\"\npublish = [\"my-registry\"]\n"
        ));
    }

    #[test]
    fn a_virtual_workspace_is_not_publishable() {
        assert!(!publishable(
            "[workspace]\nresolver = \"2\"\nmembers = [\"a\", \"b\"]\n"
        ));
    }

    #[test]
    fn publish_false_opts_out() {
        assert!(!publishable("[package]\nname = \"x\"\npublish = false\n"));
        assert!(!publishable("[package]\nname = \"x\"\npublish = []\n"));
    }

    #[test]
    fn publish_inherited_from_the_workspace_is_honoured() {
        let inherited_false = "[workspace]\nmembers = [\".\"]\n\
            [workspace.package]\npublish = false\n\
            [package]\nname = \"x\"\npublish.workspace = true\n";
        assert!(!publishable(inherited_false));
        let inherited_unset = "[workspace]\nmembers = [\".\"]\n\
            [package]\nname = \"x\"\npublish.workspace = true\n";
        assert!(publishable(inherited_unset));
    }

    #[test]
    fn check_publishable_reads_the_root_manifest() {
        let tmp = tempfile::tempdir().unwrap();
        let project = Utf8Path::from_path(tmp.path()).unwrap();
        assert!(!check_publishable(project).unwrap(), "no Cargo.toml");
        std::fs::write(
            project.join("Cargo.toml"),
            "[workspace]\nmembers = [\"examples/hello\"]\n",
        )
        .unwrap();
        assert!(!check_publishable(project).unwrap(), "virtual workspace");
        std::fs::write(
            project.join("Cargo.toml"),
            "[package]\nname = \"x\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        assert!(check_publishable(project).unwrap(), "real crate");
        std::fs::write(project.join("Cargo.toml"), "[package\n").unwrap();
        assert!(
            check_publishable(project).is_err(),
            "broken manifest is an error"
        );
    }

    #[test]
    fn token_from_prefers_a_non_empty_environment_value() {
        let token = token_from(Some(OsString::from("from-env")), "keys/crates.io").unwrap();
        assert_eq!(token, "from-env");
    }
}
