use camino::Utf8Path;

use anyhow::Result;

use crate::subprocess_utils::check_call;

// There is no `soft` mode: `git reset --soft HEAD` moves HEAD to where it
// already is and changes nothing, and a soft reset to anywhere else (undoing
// the last commit in every repo) is not something to do fleet-wide.

/// Whether `reset hard` would discard anything: any change to a tracked
/// file, staged or not (conflicts included). Untracked files survive it.
pub fn would_reset_hard(project: &Utf8Path) -> Result<bool> {
    crate::commands::count::is_dirty(project)
}

/// Discard every change to tracked files (git reset --hard HEAD).
pub fn reset_hard(project: &Utf8Path) -> Result<()> {
    check_call(project, "git", &["reset", "--hard", "HEAD"])
}

/// What `reset hard` would discard, without discarding it.
pub fn preview_reset_hard(project: &Utf8Path) -> Result<()> {
    check_call(
        project,
        "git",
        &["status", "--short", "--untracked-files=no"],
    )
}

/// Whether `reset mixed` would unstage anything.
pub fn would_reset_mixed(project: &Utf8Path) -> Result<bool> {
    crate::commands::count::has_staged(project)
}

/// Unstage everything, keeping the changes in the working tree
/// (git reset --mixed HEAD).
pub fn reset_mixed(project: &Utf8Path) -> Result<()> {
    check_call(project, "git", &["reset", "--quiet", "--mixed", "HEAD"])
}

/// What `reset mixed` would unstage, without unstaging it.
pub fn preview_reset_mixed(project: &Utf8Path) -> Result<()> {
    check_call(project, "git", &["diff", "--cached", "--name-status"])
}
