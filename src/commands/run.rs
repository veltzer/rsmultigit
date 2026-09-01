use std::path::Path;

use anyhow::{Result, bail};

use crate::subprocess_utils::check_call_maybe_ve;

/// Execute an arbitrary command in a project directory.
///
/// If `command` has a single item, it is executed via the system shell (`sh -c` on
/// Unix, `cmd /C` on Windows) so shell string expressions (with spaces, pipes,
/// redirects, etc.) work as expected. Otherwise, `command[0]` is executed directly
/// with `command[1..]` as arguments.
///
/// With `venv` (the global `--venv` flag, default on), an existing repo `.venv`
/// is activated (PATH + VIRTUAL_ENV) before the command runs, so both the
/// command itself and anything it spawns resolve from the repo's own venv.
pub fn do_run(project: &Path, command: &[String], venv: bool) -> Result<bool> {
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

        check_call_maybe_ve(project, venv, shell, &[shell_arg, cmd_str])?;
        return Ok(true);
    }

    let args: Vec<&str> = command.iter().map(|s| s.as_str()).collect();
    check_call_maybe_ve(project, venv, args[0], &args[1..])?;
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
        assert!(do_run(tmp.path(), &[], true).is_err());
    }

    #[test]
    fn do_run_empty_string_fails() {
        let tmp = cwd();
        assert!(do_run(tmp.path(), &[String::from("  ")], true).is_err());
    }

    #[test]
    fn do_run_multiple_args() {
        let tmp = cwd();
        let cmd = vec!["echo".to_string(), "hello".to_string()];
        assert!(do_run(tmp.path(), &cmd, true).is_ok());
    }

    #[test]
    fn do_run_single_arg_shell() {
        let tmp = cwd();
        let cmd = vec!["echo hello".to_string()];
        assert!(do_run(tmp.path(), &cmd, true).is_ok());
    }

    #[test]
    fn do_run_without_venv() {
        let tmp = cwd();
        let cmd = vec!["echo".to_string(), "hello".to_string()];
        assert!(do_run(tmp.path(), &cmd, false).is_ok());
    }

    #[test]
    fn do_run_venv_activates_local_venv() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = cwd();
        let bin = tmp.path().join(".venv/bin");
        std::fs::create_dir_all(&bin).unwrap();
        let tool = bin.join("run-venv-probe");
        std::fs::write(&tool, "#!/bin/sh\nexit 0\n").unwrap();
        std::fs::set_permissions(&tool, std::fs::Permissions::from_mode(0o755)).unwrap();
        // Resolvable only via the venv's PATH entry: works with venv on...
        let cmd = vec!["run-venv-probe".to_string()];
        assert!(do_run(tmp.path(), &cmd, true).is_ok());
        // ...and not with --no-venv.
        assert!(do_run(tmp.path(), &cmd, false).is_err());
    }

    #[test]
    fn do_run_failing_command() {
        let tmp = cwd();
        let cmd = vec!["false".to_string()];
        assert!(do_run(tmp.path(), &cmd, true).is_err());
    }
}
