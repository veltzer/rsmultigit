use std::path::Path;

use anyhow::Result;

use crate::subprocess_utils::{check_call, check_call_ve_env};

/// Repos uv can operate on: those with a pyproject.toml at the root.
pub fn check_pyproject(project: &Path) -> Result<bool> {
    Ok(project.join("pyproject.toml").exists())
}

/// Repos venv-sync operates on: pyproject repos that already have a `.venv`.
pub fn check_pyproject_venv(project: &Path) -> Result<bool> {
    Ok(project.join("pyproject.toml").exists() && project.join(".venv").is_dir())
}

/// Sync the project environment from the lockfile: `uv sync`.
pub fn sync(project: &Path) -> Result<bool> {
    check_call(project, "uv", &["sync"])?;
    Ok(true)
}

/// Sync the project environment with the local `.venv` activated (PATH +
/// VIRTUAL_ENV) before `uv sync` runs. Only called for repos where
/// `check_pyproject_venv` passed, so the venv is known to exist.
pub fn venv_sync(project: &Path) -> Result<bool> {
    check_call_ve_env(project, "uv", &["sync"])?;
    Ok(true)
}

/// Re-resolve the lockfile from pyproject.toml: `uv lock`, optionally with
/// `--upgrade` to allow moving already-locked versions forward, or with
/// `--check` to only assert the lockfile is up to date (stale lockfile =
/// non-zero exit = error). The two flags are mutually exclusive (enforced
/// by clap).
pub fn lock(project: &Path, upgrade: bool, check: bool) -> Result<bool> {
    let args: &[&str] = match (upgrade, check) {
        (true, _) => &["lock", "--upgrade"],
        (_, true) => &["lock", "--check"],
        _ => &["lock"],
    };
    check_call(project, "uv", args)?;
    Ok(true)
}
