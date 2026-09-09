use camino::Utf8Path;

use anyhow::Result;

use crate::subprocess_utils::capture_output_allow_failure;

/// Show the most recent tag. Returns None if no tags exist.
pub fn do_last_tag(project: &Utf8Path) -> Result<Option<String>> {
    let (code, stdout, _stderr) =
        capture_output_allow_failure(project, "git", &["describe", "--tags", "--abbrev=0"])?;
    let trimmed = stdout.trim().to_string();
    if code == 0 && !trimmed.is_empty() {
        Ok(Some(trimmed))
    } else {
        Ok(None)
    }
}
