use std::path::Path;

use anyhow::Result;

use crate::subprocess_utils::{check_call, check_call_ve};

/// Check if the project has a mechanism to disable build (e.g. .disable file).
fn is_build_disabled() -> bool {
    Path::new(".disable").exists()
}

fn has_pydmt_config() -> bool {
    Path::new(".pydmt.config").exists()
}

// --- Check functions (cheap predicates: should we build this project?) ---

pub fn check_bootstrap(_project: &Path) -> Result<bool> {
    Ok(!is_build_disabled())
}

pub fn check_pydmt(_project: &Path) -> Result<bool> {
    Ok(!is_build_disabled() && has_pydmt_config())
}

pub fn check_make(_project: &Path) -> Result<bool> {
    Ok(!is_build_disabled())
}

pub fn check_venv_make(_project: &Path) -> Result<bool> {
    Ok(!is_build_disabled())
}

pub fn check_venv_pydmt(_project: &Path) -> Result<bool> {
    Ok(!is_build_disabled() && has_pydmt_config())
}

pub fn check_pydmt_build_venv(_project: &Path) -> Result<bool> {
    Ok(!is_build_disabled() && has_pydmt_config())
}

pub fn check_cargo(project: &Path) -> Result<bool> {
    Ok(!is_build_disabled() && project.join("Cargo.toml").exists())
}

pub fn check_cargo_publish(project: &Path) -> Result<bool> {
    Ok(!is_build_disabled() && project.join("Cargo.toml").exists())
}

pub fn check_rsconstruct(_project: &Path) -> Result<bool> {
    Ok(!is_build_disabled() && Path::new("rsconstruct.toml").exists())
}

// --- Action functions (do the actual build, assuming check already passed) ---

pub fn build_bootstrap(_project: &Path) -> Result<bool> {
    check_call("python", &["bootstrap.py"])?;
    Ok(true)
}

pub fn build_pydmt(_project: &Path) -> Result<bool> {
    check_call("pydmt", &["build"])?;
    Ok(true)
}

pub fn build_make(_project: &Path) -> Result<bool> {
    check_call("make", &[])?;
    Ok(true)
}

pub fn build_venv_make(_project: &Path) -> Result<bool> {
    check_call_ve(&["make"])?;
    Ok(true)
}

pub fn build_venv_pydmt(_project: &Path) -> Result<bool> {
    check_call_ve(&["pydmt", "build"])?;
    Ok(true)
}

pub fn build_pydmt_build_venv(_project: &Path) -> Result<bool> {
    check_call("pydmt", &["build_venv"])?;
    Ok(true)
}

pub fn build_cargo(_project: &Path) -> Result<bool> {
    check_call("cargo", &["build"])?;
    check_call("cargo", &["build", "--release"])?;
    Ok(true)
}

pub fn build_cargo_publish(_project: &Path) -> Result<bool> {
    check_call("cargo", &["publish"])?;
    Ok(true)
}

pub fn build_rsconstruct(_project: &Path) -> Result<bool> {
    check_call("rsconstruct", &["--quiet", "build"])?;
    Ok(true)
}
