use std::path::Path;

use anyhow::Result;

use crate::subprocess_utils::check_call;

/// Run git blame on a file. Skips repos where the file does not exist.
pub fn do_blame(_project: &Path, file: &str) -> Result<bool> {
    if !Path::new(file).exists() {
        return Ok(false);
    }
    check_call("git", &["blame", file])?;
    Ok(true)
}
