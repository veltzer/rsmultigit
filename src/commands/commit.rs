use camino::Utf8Path;

use anyhow::Result;

use crate::commands::count::has_changes;
use crate::subprocess_utils::check_call;

/// Commit all staged and unstaged changes with a message.
/// Skips repos that have no changes.
pub fn do_commit(project: &Utf8Path, message: &str) -> Result<bool> {
    let (dirty, untracked) = has_changes(project)?;
    if !dirty && !untracked {
        return Ok(false);
    }
    check_call(project, "git", &["add", "-A"])?;
    check_call(project, "git", &["commit", "-m", message])?;
    Ok(true)
}
