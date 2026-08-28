use std::path::Path;

use anyhow::Result;

use crate::subprocess_utils::check_call;

/// Repos uv can operate on: those with a pyproject.toml at the root.
pub fn check_pyproject(project: &Path) -> Result<bool> {
    Ok(project.join("pyproject.toml").exists())
}

/// Sync the project environment from the lockfile: `uv sync`.
pub fn sync(project: &Path) -> Result<bool> {
    check_call(project, "uv", &["sync"])?;
    Ok(true)
}

/// Re-resolve the lockfile from pyproject.toml: `uv lock`, optionally with
/// `--upgrade` to allow moving already-locked versions forward.
pub fn lock(project: &Path, upgrade: bool) -> Result<bool> {
    let args: &[&str] = if upgrade {
        &["lock", "--upgrade"]
    } else {
        &["lock"]
    };
    check_call(project, "uv", args)?;
    Ok(true)
}
