use std::path::Path;

use anyhow::Result;

use crate::commands::count::ahead_behind;
use crate::subprocess_utils::capture_output;

/// Returns `Some(output)` when the repo needs attention: `git status -s` shows
/// working-tree changes, or the branch is ahead of / behind its upstream
/// (commits not yet pushed or not yet merged). A repo with no upstream is
/// reported by working-tree state only.
pub fn do_status(project: &Path) -> Result<Option<String>> {
    let mut output = capture_output(project, "git", &["status", "-s"])?;
    if let Some((ahead, behind)) = ahead_behind(project)? {
        for (count, direction) in [(ahead, "ahead of"), (behind, "behind")] {
            if count > 0 {
                if !output.is_empty() {
                    output.push('\n');
                }
                let plural = if count == 1 { "" } else { "s" };
                output.push_str(&format!("{direction} origin by {count} commit{plural}"));
            }
        }
    }
    if output.is_empty() {
        Ok(None)
    } else {
        Ok(Some(output))
    }
}

/// Returns `Some(output)` if there are dirty (modified/staged) changes.
/// Uses `git diff --stat` to detect modifications.
pub fn do_dirty(project: &Path) -> Result<Option<String>> {
    let output = capture_output(project, "git", &["diff", "--stat"])?;
    if output.is_empty() {
        let staged = capture_output(project, "git", &["diff", "--cached", "--stat"])?;
        if staged.is_empty() {
            Ok(None)
        } else {
            Ok(Some(staged))
        }
    } else {
        Ok(Some(output))
    }
}
