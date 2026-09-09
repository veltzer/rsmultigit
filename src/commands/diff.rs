use camino::Utf8Path;

use anyhow::Result;

use crate::subprocess_utils::check_call;

/// Show diff for the repository.
pub fn do_diff(project: &Utf8Path) -> Result<bool> {
    check_call(project, "git", &["diff"])?;
    Ok(true)
}
