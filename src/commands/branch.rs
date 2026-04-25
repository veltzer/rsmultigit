use std::path::Path;

use anyhow::Result;

use crate::subprocess_utils::{capture_output, check_call};

/// Show local branches.
pub fn branch_local(project: &Path) -> Result<bool> {
    check_call(project, "git", &["branch"])?;
    Ok(true)
}

/// Show remote branches.
pub fn branch_remote(project: &Path) -> Result<bool> {
    check_call(project, "git", &["branch", "-r"])?;
    Ok(true)
}

/// Show the GitHub default branch (via `gh repo view`).
pub fn branch_github(project: &Path) -> Result<bool> {
    let output = capture_output(
        project,
        "gh",
        &[
            "repo",
            "view",
            "--json",
            "defaultBranchRef",
            "-q",
            ".defaultBranchRef.name",
        ],
    )?;
    println!("{output}");
    Ok(true)
}
