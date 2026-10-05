use camino::Utf8Path;

use anyhow::Result;

use crate::subprocess_utils::{capture_output, check_call};

// libgit2 has no clean, so these run git. The precondition asks git itself
// (`git clean -n`, the same flags) whether anything would go, so a repo is
// selected exactly when the real run would remove something.

const HARD: &[&str] = &["clean", "-ffxd"];
const HARD_DRY: &[&str] = &["clean", "-nxd"];
const SOFT: &[&str] = &["clean", "-fd"];
const SOFT_DRY: &[&str] = &["clean", "-nd"];

/// Whether `clean hard` would remove anything (untracked or ignored).
pub fn would_clean_hard(project: &Utf8Path) -> Result<bool> {
    Ok(!capture_output(project, "git", HARD_DRY)?.is_empty())
}

/// Hard-clean the repository: untracked and ignored files (git clean -ffxd).
pub fn clean_hard(project: &Utf8Path) -> Result<()> {
    check_call(project, "git", HARD)
}

/// List what `clean hard` would remove (git clean -nxd).
pub fn preview_clean_hard(project: &Utf8Path) -> Result<()> {
    check_call(project, "git", HARD_DRY)
}

/// Whether `clean soft` would remove anything (untracked, not ignored).
pub fn would_clean_soft(project: &Utf8Path) -> Result<bool> {
    Ok(!capture_output(project, "git", SOFT_DRY)?.is_empty())
}

/// Soft-clean the repository: remove untracked files only (git clean -fd).
pub fn clean_soft(project: &Utf8Path) -> Result<()> {
    check_call(project, "git", SOFT)
}

/// List what `clean soft` would remove (git clean -nd).
pub fn preview_clean_soft(project: &Utf8Path) -> Result<()> {
    check_call(project, "git", SOFT_DRY)
}
