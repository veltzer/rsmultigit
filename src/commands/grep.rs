use std::path::Path;
use std::process::Command;

use anyhow::Result;

/// Grep across the repository. Prefix output lines with the project name.
pub fn do_grep(project: &Path, regexp: &str, files_only: bool) -> Result<bool> {
    let mut args = vec!["grep", "-n"];
    if files_only {
        args.push("-l");
    }
    args.push(regexp);

    let output = Command::new("git").args(&args).output()?;
    let stdout = String::from_utf8_lossy(&output.stdout);

    if stdout.is_empty() {
        return Ok(false);
    }

    let project_name = project
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();

    for line in stdout.lines() {
        println!("{project_name}: {line}");
    }

    Ok(true)
}
