use camino::Utf8Path;

use anyhow::Result;

use crate::subprocess_utils::Cmd;

/// Repos rsconstruct can operate on: those with an rsconstruct.toml at the root.
pub fn check_rsconstruct(project: &Utf8Path) -> bool {
    project.join("rsconstruct.toml").exists()
}

/// Move every rsconstruct.lock entry to its latest release:
/// `rsconstruct tool lock --upgrade`. Honours the global `--venv` flag, as
/// `build rsconstruct` does, so the repo's own environment is the one seen.
pub fn tool_lock_upgrade(project: &Utf8Path, venv: bool) -> Result<()> {
    Cmd::new(project, "rsconstruct")
        .args(&["tool", "lock", "--upgrade"])
        .venv(venv)?
        .run()?;
    Ok(())
}
