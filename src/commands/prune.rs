use camino::Utf8Path;

use anyhow::Result;

use crate::subprocess_utils::check_call;

/// Prune stale remote-tracking branches.
pub fn do_prune(project: &Utf8Path) -> Result<bool> {
    check_call(project, "git", &["remote", "prune", "origin"])?;
    Ok(true)
}
