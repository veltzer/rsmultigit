use camino::Utf8Path;

use anyhow::Result;

use crate::commands::count::has_changes;
use crate::subprocess_utils::check_call;

/// Repos `commit` applies to: those with anything to commit, staged,
/// unstaged or untracked.
pub fn has_anything_to_commit(project: &Utf8Path) -> Result<bool> {
    let (dirty, untracked) = has_changes(project)?;
    Ok(dirty || untracked)
}

/// Commit all staged and unstaged changes with a message (see
/// [`has_anything_to_commit`] for the precondition).
pub fn do_commit(project: &Utf8Path, message: &str) -> Result<()> {
    check_call(project, "git", &["add", "-A"])?;
    check_call(project, "git", &["commit", "-m", message])?;
    Ok(())
}
