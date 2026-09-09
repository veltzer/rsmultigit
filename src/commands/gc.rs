use camino::Utf8Path;

use anyhow::Result;

use crate::subprocess_utils::check_call;

/// Run git garbage collection.
pub fn do_gc(project: &Utf8Path) -> Result<bool> {
    check_call(project, "git", &["gc"])?;
    Ok(true)
}
