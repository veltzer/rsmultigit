use camino::Utf8Path;

use anyhow::Result;

use crate::subprocess_utils::check_call;

/// Run git blame on a file. Skips repos where the file does not exist.
pub fn do_blame(project: &Utf8Path, file: &str) -> Result<bool> {
    if !project.join(file).exists() {
        return Ok(false);
    }
    check_call(project, "git", &["blame", file])?;
    Ok(true)
}
