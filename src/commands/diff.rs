use std::path::Path;

use anyhow::Result;

use crate::subprocess_utils::check_call;

/// Show diff for the repository.
pub fn do_diff(_project: &Path) -> Result<bool> {
    check_call("git", &["diff"])?;
    Ok(true)
}
