use camino::Utf8Path;

use anyhow::Result;

use crate::subprocess_utils::capture_output_allow_failure;

/// Grep across the repository. Prefix output lines with the project name.
/// `git grep` exit codes: 0 = match, 1 = no match, >=2 = error.
pub fn do_grep(project: &Utf8Path, regexp: &str, files_only: bool) -> Result<bool> {
    let mut args = vec!["grep", "-n"];
    if files_only {
        args.push("-l");
    }
    args.push(regexp);

    let (code, stdout, stderr) = capture_output_allow_failure(project, "git", &args)?;

    match code {
        0 => {
            let project_name = project
                .file_name()
                .map(|n| n.to_string())
                .unwrap_or_default();

            for line in stdout.lines() {
                if files_only {
                    println!("{project_name}/{line}");
                } else {
                    println!("{project_name}: {line}");
                }
            }
            Ok(true)
        }
        1 => Ok(false),
        _ => anyhow::bail!("git grep failed (exit {code}): {stderr}"),
    }
}
