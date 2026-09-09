use camino::Utf8Path;

use anyhow::Result;

use crate::commands::count::is_ahead;
use crate::subprocess_utils::check_call;

/// Push the current branch to origin. Skips repos not ahead of remote.
pub fn do_push(project: &Utf8Path) -> Result<bool> {
    if !is_ahead(project)? {
        return Ok(false);
    }
    check_call(project, "git", &["push"])?;
    Ok(true)
}
