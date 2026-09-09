use camino::Utf8Path;

use anyhow::Result;

use crate::subprocess_utils::check_call;

/// Show recent commits (git log --oneline -n N).
pub fn do_log(project: &Utf8Path, count: u32) -> Result<bool> {
    let n = count.to_string();
    check_call(project, "git", &["log", "--oneline", "-n", &n])?;
    Ok(true)
}
