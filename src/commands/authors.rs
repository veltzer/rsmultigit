use camino::Utf8Path;

use anyhow::Result;

use crate::commands::count::open_repo;
use crate::subprocess_utils::capture_output;

/// Show unique commit authors sorted by number of commits. This stays a
/// `git shortlog` call: its value is git's own grouping and formatting
/// (mailmap included). An empty repo (unborn branch) has no authors and
/// reports nothing instead of failing on the missing HEAD.
pub fn do_authors(project: &Utf8Path) -> Result<Option<String>> {
    if open_repo(project)?.head().is_err() {
        return Ok(None);
    }
    let output = capture_output(project, "git", &["shortlog", "-sne", "HEAD"])?;
    if output.is_empty() {
        Ok(None)
    } else {
        Ok(Some(output))
    }
}
