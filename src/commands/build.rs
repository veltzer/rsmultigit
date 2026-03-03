use std::path::Path;

use anyhow::Result;

use crate::subprocess_utils::{check_call, check_call_ve};

/// Check if the project has a mechanism to disable build (e.g. .disable file).
fn is_build_disabled() -> bool {
    Path::new(".disable").exists()
}

/// Run bootstrap (python bootstrap.py or ./bootstrap).
pub fn build_bootstrap(_project: &Path) -> Result<()> {
    if is_build_disabled() {
        return Ok(());
    }
    check_call("python", &["bootstrap.py"])
}

/// Run pydmt build.
pub fn build_pydmt(_project: &Path) -> Result<()> {
    if is_build_disabled() {
        return Ok(());
    }
    check_call("pydmt", &["build"])
}

/// Run make.
pub fn build_make(_project: &Path) -> Result<()> {
    if is_build_disabled() {
        return Ok(());
    }
    check_call("make", &[])
}

/// Run make inside a virtualenv.
pub fn build_venv_make(_project: &Path) -> Result<()> {
    if is_build_disabled() {
        return Ok(());
    }
    check_call_ve(&["make"])
}

/// Run pydmt inside a virtualenv.
pub fn build_venv_pydmt(_project: &Path) -> Result<()> {
    if is_build_disabled() {
        return Ok(());
    }
    check_call_ve(&["pydmt", "build"])
}

/// Run pydmt build_venv.
pub fn build_pydmt_build_venv(_project: &Path) -> Result<()> {
    if is_build_disabled() {
        return Ok(());
    }
    check_call("pydmt", &["build_venv"])
}

/// Run rsb build, but only if the project has an rsb.toml file.
pub fn build_rsb(_project: &Path) -> Result<()> {
    if is_build_disabled() {
        return Ok(());
    }
    if !Path::new("rsb.toml").exists() {
        return Ok(());
    }
    check_call("rsb", &["build"])
}
