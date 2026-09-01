use std::path::Path;

use anyhow::Result;

use crate::subprocess_utils::check_call_maybe_ve;

/// Repos uv can operate on: those with a pyproject.toml at the root.
pub fn check_pyproject(project: &Path) -> Result<bool> {
    Ok(project.join("pyproject.toml").exists())
}

/// Sync the project environment from the lockfile: `uv sync`. With `venv`
/// (the global `--venv` flag, default on), a repo that already has a `.venv`
/// gets it activated (PATH + VIRTUAL_ENV) before `uv sync` runs; repos
/// without one run with the environment unchanged.
pub fn sync(project: &Path, venv: bool) -> Result<bool> {
    check_call_maybe_ve(project, venv, "uv", &["sync"])?;
    Ok(true)
}

/// Re-resolve the lockfile from pyproject.toml: `uv lock`, optionally with
/// `--upgrade` to allow moving already-locked versions forward, or with
/// `--check` to only assert the lockfile is up to date (stale lockfile =
/// non-zero exit = error). The two flags are mutually exclusive (enforced
/// by clap). `venv` activates an existing repo `.venv` first (see `sync`).
pub fn lock(project: &Path, upgrade: bool, check: bool, venv: bool) -> Result<bool> {
    let args: &[&str] = match (upgrade, check) {
        (true, _) => &["lock", "--upgrade"],
        (_, true) => &["lock", "--check"],
        _ => &["lock"],
    };
    check_call_maybe_ve(project, venv, "uv", args)?;
    Ok(true)
}
