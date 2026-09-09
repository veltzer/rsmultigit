use camino::Utf8Path;

use anyhow::Result;

use crate::subprocess_utils::check_call;

/// Fetch from origin.
pub fn do_fetch(project: &Utf8Path) -> Result<bool> {
    check_call(project, "git", &["fetch"])?;
    Ok(true)
}
