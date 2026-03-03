use std::path::Path;

use anyhow::Result;

use crate::subprocess_utils::check_call;

/// Fetch from origin.
pub fn do_fetch(_project: &Path) -> Result<bool> {
    check_call("git", &["fetch"])?;
    Ok(true)
}
