use std::path::Path;

use anyhow::Result;

use crate::subprocess_utils::check_call;

/// Update submodules recursively.
pub fn submodule_update(project: &Path) -> Result<bool> {
    check_call(
        project,
        "git",
        &["submodule", "update", "--init", "--recursive"],
    )?;
    Ok(true)
}
