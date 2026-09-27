use camino::Utf8Path;

use anyhow::Result;

use crate::subprocess_utils::check_call;

/// Repos `blame <file>` applies to: those that have the file.
pub fn has_file(project: &Utf8Path, file: &str) -> Result<bool> {
    Ok(project.join(file).is_file())
}

/// Run git blame on a file (see [`has_file`] for the precondition).
pub fn do_blame(project: &Utf8Path, file: &str) -> Result<()> {
    check_call(project, "git", &["blame", file])?;
    Ok(())
}
