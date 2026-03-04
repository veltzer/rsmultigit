use std::path::Path;
use std::process::Command;

use anyhow::Result;

/// Show a git config value. Returns None if the key is not set.
pub fn do_config(_project: &Path, key: &str) -> Result<Option<String>> {
    let output = Command::new("git").args(["config", key]).output()?;
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if stdout.is_empty() {
        Ok(None)
    } else {
        Ok(Some(stdout))
    }
}
