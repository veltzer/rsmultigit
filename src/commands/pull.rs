use std::path::Path;

use anyhow::Result;

use crate::subprocess_utils::check_call;

/// Pull the current branch from origin.
pub fn do_pull(_project: &Path, quiet: bool) -> Result<bool> {
    let mut args = vec!["pull"];
    if quiet {
        args.push("--quiet");
    }
    check_call("git", &args)?;
    Ok(true)
}
