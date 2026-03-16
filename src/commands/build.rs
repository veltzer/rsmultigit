use std::path::Path;

use anyhow::Result;

use crate::subprocess_utils::{check_call, check_call_ve};

/// Check if the project has a mechanism to disable build (e.g. .disable file).
fn is_build_disabled() -> bool {
    Path::new(".disable").exists()
}

/// Run bootstrap (python bootstrap.py or ./bootstrap).
pub fn build_bootstrap(_project: &Path) -> Result<bool> {
    if is_build_disabled() {
        return Ok(false);
    }
    check_call("python", &["bootstrap.py"])?;
    Ok(true)
}

/// Run pydmt build, but only if the project has a .pydmt.config file.
pub fn build_pydmt(_project: &Path) -> Result<bool> {
    if is_build_disabled() {
        return Ok(false);
    }
    if !Path::new(".pydmt.config").exists() {
        return Ok(false);
    }
    check_call("pydmt", &["build"])?;
    Ok(true)
}

/// Run make.
pub fn build_make(_project: &Path) -> Result<bool> {
    if is_build_disabled() {
        return Ok(false);
    }
    check_call("make", &[])?;
    Ok(true)
}

/// Run make inside a virtualenv.
pub fn build_venv_make(_project: &Path) -> Result<bool> {
    if is_build_disabled() {
        return Ok(false);
    }
    check_call_ve(&["make"])?;
    Ok(true)
}

/// Run pydmt inside a virtualenv, but only if the project has a .pydmt.config file.
pub fn build_venv_pydmt(_project: &Path) -> Result<bool> {
    if is_build_disabled() {
        return Ok(false);
    }
    if !Path::new(".pydmt.config").exists() {
        return Ok(false);
    }
    check_call_ve(&["pydmt", "build"])?;
    Ok(true)
}

/// Run pydmt build_venv, but only if the project has a .pydmt.config file.
pub fn build_pydmt_build_venv(_project: &Path) -> Result<bool> {
    if is_build_disabled() {
        return Ok(false);
    }
    if !Path::new(".pydmt.config").exists() {
        return Ok(false);
    }
    check_call("pydmt", &["build_venv"])?;
    Ok(true)
}

/// Run cargo build (debug + release), but only if the project has a Cargo.toml file.
pub fn build_cargo(project: &Path) -> Result<bool> {
    if is_build_disabled() {
        return Ok(false);
    }
    if !project.join("Cargo.toml").exists() {
        return Ok(false);
    }
    check_call("cargo", &["build"])?;
    check_call("cargo", &["build", "--release"])?;
    Ok(true)
}

/// Run cargo publish, but only if the project has a Cargo.toml file.
pub fn build_cargo_publish(project: &Path) -> Result<bool> {
    if is_build_disabled() {
        return Ok(false);
    }
    if !project.join("Cargo.toml").exists() {
        return Ok(false);
    }
    check_call("cargo", &["publish"])?;
    Ok(true)
}

/// Run rsconstruct build, but only if the project has an rsconstruct.toml file.
pub fn build_rsconstruct(_project: &Path) -> Result<bool> {
    if is_build_disabled() {
        return Ok(false);
    }
    if !Path::new("rsconstruct.toml").exists() {
        return Ok(false);
    }
    check_call("rsconstruct", &["--quiet", "build"])?;
    Ok(true)
}
