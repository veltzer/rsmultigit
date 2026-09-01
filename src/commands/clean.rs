use std::path::Path;

use anyhow::Result;

use crate::subprocess_utils::{check_call, check_call_maybe_ve};

/// Hard-clean the repository (git clean -ffxd).
pub fn clean_hard(project: &Path) -> Result<bool> {
    check_call(project, "git", &["clean", "-ffxd"])?;
    Ok(true)
}

/// Soft-clean the repository: remove untracked files only (git clean -fd).
pub fn clean_soft(project: &Path) -> Result<bool> {
    check_call(project, "git", &["clean", "-fd"])?;
    Ok(true)
}

/// Run `make clean`. With `venv` (the global `--venv` flag, default on), an
/// existing repo `.venv` is activated first so Makefile targets that call
/// python tools resolve them from the repo's own venv.
pub fn clean_make(project: &Path, venv: bool) -> Result<bool> {
    check_call_maybe_ve(project, venv, "make", &["clean"])?;
    Ok(true)
}

/// Discard unstaged working-tree changes (git checkout .).
pub fn clean_git(project: &Path) -> Result<bool> {
    check_call(project, "git", &["checkout", "."])?;
    Ok(true)
}

/// Run `cargo clean` if `Cargo.toml` exists, otherwise skip.
pub fn clean_cargo(project: &Path) -> Result<bool> {
    if !project.join("Cargo.toml").exists() {
        return Ok(false);
    }
    check_call(project, "cargo", &["clean"])?;
    Ok(true)
}
