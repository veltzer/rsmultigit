use std::path::Path;

use anyhow::Result;

use crate::subprocess_utils::{capture_output, check_call};

/// List local tags.
pub fn tag_local(_project: &Path) -> Result<bool> {
    check_call("git", &["tag"])?;
    Ok(true)
}

/// List remote tags.
pub fn tag_remote(_project: &Path) -> Result<bool> {
    check_call("git", &["ls-remote", "--tags", "origin"])?;
    Ok(true)
}

/// Check if local tags exist.
pub fn tag_has_local(_project: &Path) -> Result<bool> {
    let output = capture_output("git", &["tag"])?;
    Ok(!output.is_empty())
}

/// Check if remote tags exist.
pub fn tag_has_remote(_project: &Path) -> Result<bool> {
    let output = capture_output("git", &["ls-remote", "--tags", "origin"])?;
    Ok(!output.is_empty())
}
