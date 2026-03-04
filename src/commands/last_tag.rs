use std::path::Path;
use std::process::Command;

use anyhow::Result;

/// Show the most recent tag. Returns None if no tags exist.
pub fn do_last_tag(_project: &Path) -> Result<Option<String>> {
    let output = Command::new("git")
        .args(["describe", "--tags", "--abbrev=0"])
        .output()?;
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if output.status.success() && !stdout.is_empty() {
        Ok(Some(stdout))
    } else {
        Ok(None)
    }
}
