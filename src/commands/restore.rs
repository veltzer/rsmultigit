use camino::Utf8Path;

use anyhow::Result;

use crate::subprocess_utils::check_call;

/// Discard unstaged working-tree changes to tracked files (git restore .).
pub fn restore(project: &Utf8Path) -> Result<()> {
    check_call(project, "git", &["restore", "."])?;
    Ok(())
}
