use std::path::Path;

use anyhow::Result;

use crate::subprocess_utils::check_call;

/// Stash working-tree changes.
pub fn stash_push(_project: &Path) -> Result<bool> {
    check_call("git", &["stash", "push"])?;
    Ok(true)
}

/// Pop the most recent stash.
pub fn stash_pop(_project: &Path) -> Result<bool> {
    check_call("git", &["stash", "pop"])?;
    Ok(true)
}
