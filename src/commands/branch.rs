use camino::Utf8Path;

use anyhow::Result;

use crate::subprocess_utils::{capture_output, check_call, out_line};

/// Show local branches.
pub fn branch_local(project: &Utf8Path) -> Result<()> {
    check_call(project, "git", &["branch"])?;
    Ok(())
}

/// Show remote branches.
pub fn branch_remote(project: &Utf8Path) -> Result<()> {
    check_call(project, "git", &["branch", "-r"])?;
    Ok(())
}

/// Show the GitHub default branch (via `gh repo view`).
pub fn branch_github(project: &Utf8Path) -> Result<()> {
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
    out_line(&output);
    Ok(())
}
