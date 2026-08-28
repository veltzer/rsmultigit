use std::path::Path;

use anyhow::Result;

use crate::subprocess_utils::check_call;

/// Release a new version of a rust project: `cargo release <level>` bumps the
/// version in Cargo.toml, commits, tags, pushes, and publishes to crates.io.
/// `level` is one of "patch", "minor", "major" (see `cli::ReleaseType::as_str`).
pub fn publish(project: &Path, level: &str) -> Result<bool> {
    check_call(
        project,
        "cargo",
        &["release", level, "--execute", "--no-confirm"],
    )?;
    Ok(true)
}
