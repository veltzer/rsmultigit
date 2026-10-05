use camino::Utf8Path;

use anyhow::Result;

use crate::subprocess_utils::check_call;

/// Hard-clean the repository (git clean -ffxd).
pub fn clean_hard(project: &Utf8Path) -> Result<()> {
    check_call(project, "git", &["clean", "-ffxd"])?;
    Ok(())
}

/// Soft-clean the repository: remove untracked files only (git clean -fd).
pub fn clean_soft(project: &Utf8Path) -> Result<()> {
    check_call(project, "git", &["clean", "-fd"])?;
    Ok(())
}
