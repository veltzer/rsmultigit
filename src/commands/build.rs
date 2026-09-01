use std::path::Path;

use anyhow::Result;

use crate::subprocess_utils::check_call_maybe_ve;

fn is_build_disabled(project: &Path) -> bool {
    project.join(".disable").exists()
}

fn has_pydmt_config(project: &Path) -> bool {
    project.join(".pydmt.config").exists()
}

// --- Check functions (cheap predicates: should we build this project?) ---

pub fn check_not_disabled(project: &Path) -> Result<bool> {
    Ok(!is_build_disabled(project))
}

pub fn check_pydmt(project: &Path) -> Result<bool> {
    Ok(!is_build_disabled(project) && has_pydmt_config(project))
}

pub fn check_cargo(project: &Path) -> Result<bool> {
    Ok(!is_build_disabled(project) && project.join("Cargo.toml").exists())
}

pub fn check_rsconstruct(project: &Path) -> Result<bool> {
    Ok(!is_build_disabled(project) && project.join("rsconstruct.toml").exists())
}

// --- Action functions (do the actual build, assuming check already passed) ---
//
// All actions share the `(project, venv)` signature so main.rs can dispatch
// them through one fn-pointer table. `venv` comes from the global
// `--venv`/`--no-venv` flag (default on): the repo's `.venv/bin` is prepended
// to PATH and VIRTUAL_ENV is set before the tool runs, so the tool itself and
// whatever it spawns (pytest, mypy, ...) resolve from the repo's own venv.
// Repos without a `.venv` run with the environment unchanged.

pub fn build_bootstrap(project: &Path, venv: bool) -> Result<bool> {
    check_call_maybe_ve(project, venv, "python", &["bootstrap.py"])?;
    Ok(true)
}

pub fn build_pydmt(project: &Path, venv: bool) -> Result<bool> {
    check_call_maybe_ve(project, venv, "pydmt", &["build"])?;
    Ok(true)
}

pub fn build_make(project: &Path, venv: bool) -> Result<bool> {
    check_call_maybe_ve(project, venv, "make", &[])?;
    Ok(true)
}

pub fn build_pydmt_build_venv(project: &Path, venv: bool) -> Result<bool> {
    check_call_maybe_ve(project, venv, "pydmt", &["build_venv"])?;
    Ok(true)
}

pub fn build_cargo(project: &Path, venv: bool) -> Result<bool> {
    check_call_maybe_ve(project, venv, "cargo", &["build"])?;
    check_call_maybe_ve(project, venv, "cargo", &["build", "--release"])?;
    Ok(true)
}

pub fn build_cargo_publish(project: &Path, venv: bool) -> Result<bool> {
    check_call_maybe_ve(project, venv, "cargo", &["publish"])?;
    Ok(true)
}

pub fn build_rsconstruct(project: &Path, venv: bool) -> Result<bool> {
    check_call_maybe_ve(project, venv, "rsconstruct", &["--quiet", "build"])?;
    Ok(true)
}
