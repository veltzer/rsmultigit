use std::path::Path;

use anyhow::Result;

use crate::subprocess_utils::check_call_maybe_ve;

/// Upgrade dependencies via `cargo upgrade`.
pub fn upgrade(project: &Path, venv: bool) -> Result<bool> {
    check_call_maybe_ve(project, venv, "cargo", &["upgrade"])?;
    Ok(true)
}
