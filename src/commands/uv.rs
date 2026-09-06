use std::path::Path;

use anyhow::Result;

use crate::subprocess_utils::check_call_clean_env;

/// Repos uv can operate on: those with a pyproject.toml at the root.
pub fn check_pyproject(project: &Path) -> Result<bool> {
    Ok(project.join("pyproject.toml").exists())
}

/// Sync the project environment from the lockfile: `uv sync`.
///
/// Runs with a clean environment (see `check_call_clean_env`): uv resolves the
/// project and its `.venv` from the repo dir itself, so the global
/// `--venv`/`--no-venv` flag does not apply to uv.
pub fn sync(project: &Path) -> Result<bool> {
    check_call_clean_env(project, "uv", &["sync"])?;
    Ok(true)
}

/// Re-resolve the lockfile from pyproject.toml: `uv lock`, optionally with
/// `--upgrade` to allow moving already-locked versions forward, or with
/// `--check` to only assert the lockfile is up to date (stale lockfile =
/// non-zero exit = error). The two flags are mutually exclusive (enforced
/// by clap). Runs with a clean environment, as `sync` does.
pub fn lock(project: &Path, upgrade: bool, check: bool) -> Result<bool> {
    let args: &[&str] = match (upgrade, check) {
        (true, _) => &["lock", "--upgrade"],
        (_, true) => &["lock", "--check"],
        _ => &["lock"],
    };
    check_call_clean_env(project, "uv", args)?;
    Ok(true)
}
