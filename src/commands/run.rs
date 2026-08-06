use std::path::Path;

use anyhow::{Result, bail};

use crate::subprocess_utils::check_call;

/// Execute an arbitrary command in a project directory.
///
/// If `command` has a single item, it is executed via the system shell (`sh -c` on
/// Unix, `cmd /C` on Windows) so shell string expressions (with spaces, pipes,
/// redirects, etc.) work as expected. Otherwise, `command[0]` is executed directly
/// with `command[1..]` as arguments.
pub fn do_run(project: &Path, command: &[String]) -> Result<bool> {
    if command.is_empty() {
        bail!("no command specified to run");
    }

    if command.len() == 1 {
        let cmd_str = &command[0];
        if cmd_str.trim().is_empty() {
            bail!("empty command specified to run");
        }
        #[cfg(windows)]
        let shell = "cmd";
        #[cfg(windows)]
        let shell_arg = "/C";

        #[cfg(not(windows))]
        let shell = "sh";
        #[cfg(not(windows))]
        let shell_arg = "-c";

        check_call(project, shell, &[shell_arg, cmd_str])?;
        return Ok(true);
    }

    let args: Vec<&str> = command.iter().map(|s| s.as_str()).collect();
    check_call(project, args[0], &args[1..])?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn cwd() -> TempDir {
        TempDir::new().unwrap()
    }

    #[test]
    fn do_run_empty_fails() {
        let tmp = cwd();
        assert!(do_run(tmp.path(), &[]).is_err());
    }

    #[test]
    fn do_run_empty_string_fails() {
        let tmp = cwd();
        assert!(do_run(tmp.path(), &[String::from("  ")]).is_err());
    }

    #[test]
    fn do_run_multiple_args() {
        let tmp = cwd();
        let cmd = vec!["echo".to_string(), "hello".to_string()];
        assert!(do_run(tmp.path(), &cmd).is_ok());
    }

    #[test]
    fn do_run_single_arg_shell() {
        let tmp = cwd();
        let cmd = vec!["echo hello".to_string()];
        assert!(do_run(tmp.path(), &cmd).is_ok());
    }

    #[test]
    fn do_run_failing_command() {
        let tmp = cwd();
        let cmd = vec!["false".to_string()];
        assert!(do_run(tmp.path(), &cmd).is_err());
    }
}
