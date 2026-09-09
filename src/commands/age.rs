use camino::Utf8Path;

use anyhow::Result;

use crate::subprocess_utils::capture_output;

/// Show the age of the last commit as a human-readable relative date.
pub fn do_age(project: &Utf8Path) -> Result<Option<String>> {
    let output = capture_output(project, "git", &["log", "-1", "--format=%cr"])?;
    if output.is_empty() {
        Ok(None)
    } else {
        Ok(Some(output))
    }
}
