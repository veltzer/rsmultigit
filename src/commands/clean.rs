use std::path::Path;

use anyhow::Result;

use crate::subprocess_utils::check_call;

/// Hard-clean the repository (git clean -ffxd).
pub fn do_clean(_project: &Path) -> Result<bool> {
    check_call("git", &["clean", "-ffxd"])?;
    Ok(true)
}
