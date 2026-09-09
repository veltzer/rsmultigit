use camino::Utf8Path;

use anyhow::Result;

use crate::subprocess_utils::capture_output;

/// Show unique commit authors sorted by number of commits.
pub fn do_authors(project: &Utf8Path) -> Result<Option<String>> {
    let output = capture_output(project, "git", &["shortlog", "-sne", "HEAD"])?;
    if output.is_empty() {
        Ok(None)
    } else {
        Ok(Some(output))
    }
}
