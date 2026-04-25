use std::path::Path;

use anyhow::Result;

use crate::subprocess_utils::capture_output_allow_failure;

/// Show a git config value. Returns None if the key is not set.
/// `git config <key>` exits 1 when the key is missing, which is not an error here.
pub fn do_config(project: &Path, key: &str) -> Result<Option<String>> {
    let (code, stdout, stderr) = capture_output_allow_failure(project, "git", &["config", key])?;
    match code {
        0 => {
            let trimmed = stdout.trim().to_string();
            if trimmed.is_empty() {
                Ok(None)
            } else {
                Ok(Some(trimmed))
            }
        }
        1 => Ok(None),
        _ => anyhow::bail!("git config {key} failed (exit {code}): {stderr}"),
    }
}
