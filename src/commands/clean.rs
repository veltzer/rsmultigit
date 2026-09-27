use camino::Utf8Path;

use anyhow::Result;

use crate::subprocess_utils::{check_call, check_call_maybe_ve};

/// Hard-clean the repository (git clean -ffxd).
pub fn clean_hard(project: &Utf8Path) -> Result<()> {
    check_call(project, "git", &["clean", "-ffxd"])?;
    Ok(())
}

/// Soft-clean the repository: remove untracked files only (git clean -fd).
pub fn clean_soft(project: &Utf8Path) -> Result<()> {
    check_call(project, "git", &["clean", "-fd"])?;
    Ok(())
}

/// Run `make clean`. With `venv` (the global `--venv` flag, default on), an
/// existing repo `.venv` is activated first so Makefile targets that call
/// python tools resolve them from the repo's own venv.
pub fn clean_make(project: &Utf8Path, venv: bool) -> Result<()> {
    check_call_maybe_ve(project, venv, "make", &["clean"])?;
    Ok(())
}

/// Discard unstaged working-tree changes (git checkout .).
pub fn clean_git(project: &Utf8Path) -> Result<()> {
    check_call(project, "git", &["checkout", "."])?;
    Ok(())
}

/// Repos `clean cargo` applies to: those with a `Cargo.toml`.
pub fn has_cargo_toml(project: &Utf8Path) -> Result<bool> {
    Ok(project.join("Cargo.toml").exists())
}

/// Run `cargo clean` (see [`has_cargo_toml`] for the precondition).
pub fn clean_cargo(project: &Utf8Path) -> Result<()> {
    check_call(project, "cargo", &["clean"])?;
    Ok(())
}
