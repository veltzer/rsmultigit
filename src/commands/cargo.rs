use std::path::Path;

use anyhow::Result;

use crate::subprocess_utils::check_call_maybe_ve;

/// Update dependencies via `cargo update`.
pub fn update(project: &Path, venv: bool) -> Result<bool> {
    check_call_maybe_ve(project, venv, "cargo", &["update"])?;
    Ok(true)
}
