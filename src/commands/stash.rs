use camino::Utf8Path;

use anyhow::Result;

use crate::subprocess_utils::check_call;

/// Stash working-tree changes.
pub fn stash_push(project: &Utf8Path) -> Result<bool> {
    check_call(project, "git", &["stash", "push"])?;
    Ok(true)
}

/// Pop the most recent stash.
pub fn stash_pop(project: &Utf8Path) -> Result<bool> {
    check_call(project, "git", &["stash", "pop"])?;
    Ok(true)
}
