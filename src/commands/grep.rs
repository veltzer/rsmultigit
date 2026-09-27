use camino::Utf8Path;

use anyhow::Result;

use crate::subprocess_utils::capture_output_allow_failure;

/// Grep across the repository. Returns the matching lines, each prefixed
/// with the repo name, or `None` when nothing matched, so the runner prints
/// nothing at all for repos without a match.
/// `git grep` exit codes: 0 = match, 1 = no match, >=2 = error.
pub fn do_grep(project: &Utf8Path, regexp: &str, files_only: bool) -> Result<Option<String>> {
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
            let separator = if files_only { "/" } else { ": " };
            let lines: Vec<String> = stdout
                .lines()
                .map(|line| format!("{project_name}{separator}{line}"))
                .collect();
            Ok(Some(lines.join("\n")))
        }
        1 => Ok(None),
        _ => anyhow::bail!("git grep failed (exit {code}): {stderr}"),
    }
}
