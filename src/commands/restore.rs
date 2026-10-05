use camino::Utf8Path;

use anyhow::Result;

use crate::subprocess_utils::check_call;

/// Whether `restore` would discard anything: unstaged changes to tracked files.
pub fn would_restore(project: &Utf8Path) -> Result<bool> {
    crate::commands::count::has_unstaged(project)
}

/// Discard unstaged working-tree changes to tracked files (git restore .).
pub fn restore(project: &Utf8Path) -> Result<()> {
    check_call(project, "git", &["restore", "."])
}

/// What `restore` would discard, without discarding it.
pub fn preview_restore(project: &Utf8Path) -> Result<()> {
    check_call(project, "git", &["diff", "--name-status"])
}
