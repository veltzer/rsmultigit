use std::path::Path;

use anyhow::Result;

use crate::subprocess_utils::{check_call, check_call_ve, check_call_ve_env};

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

pub fn build_bootstrap(project: &Path) -> Result<bool> {
    check_call(project, "python", &["bootstrap.py"])?;
    Ok(true)
}

pub fn build_pydmt(project: &Path) -> Result<bool> {
    check_call(project, "pydmt", &["build"])?;
    Ok(true)
}

pub fn build_make(project: &Path) -> Result<bool> {
    check_call(project, "make", &[])?;
    Ok(true)
}

pub fn build_venv_make(project: &Path) -> Result<bool> {
    check_call_ve(project, &["make"])?;
    Ok(true)
}

pub fn build_venv_pydmt(project: &Path) -> Result<bool> {
    check_call_ve(project, &["pydmt", "build"])?;
    Ok(true)
}

pub fn build_pydmt_build_venv(project: &Path) -> Result<bool> {
    check_call(project, "pydmt", &["build_venv"])?;
    Ok(true)
}

pub fn build_cargo(project: &Path) -> Result<bool> {
    check_call(project, "cargo", &["build"])?;
    check_call(project, "cargo", &["build", "--release"])?;
    Ok(true)
}

pub fn build_cargo_publish(project: &Path) -> Result<bool> {
    check_call(project, "cargo", &["publish"])?;
    Ok(true)
}

pub fn build_rsconstruct(project: &Path) -> Result<bool> {
    check_call(project, "rsconstruct", &["--quiet", "build"])?;
    Ok(true)
}

pub fn build_venv_rsconstruct(project: &Path) -> Result<bool> {
    check_call_ve_env(project, "rsconstruct", &["--quiet", "build"])?;
    Ok(true)
}
