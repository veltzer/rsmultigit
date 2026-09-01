use std::cell::RefCell;
use std::path::Path;
use std::process::{Command, Stdio};

use anyhow::{Result, bail};

// Per-thread capture buffer. When present, `check_call` and `check_call_ve_env`
// collect subprocess stdout/stderr into it instead of inheriting the parent's
// streams. This lets the parallel runner replay output in project order.
thread_local! {
    static CAPTURE_BUF: RefCell<Option<Vec<u8>>> = const { RefCell::new(None) };
}

/// Begin capturing subprocess output on this thread. Any prior buffer is replaced.
pub fn enter_capture() {
    CAPTURE_BUF.with(|cell| {
        *cell.borrow_mut() = Some(Vec::new());
    });
}

/// Stop capturing and return the collected bytes (empty if capture was not active).
pub fn leave_capture() -> Vec<u8> {
    CAPTURE_BUF.with(|cell| cell.borrow_mut().take().unwrap_or_default())
}

fn is_capturing() -> bool {
    CAPTURE_BUF.with(|cell| cell.borrow().is_some())
}

fn append_to_capture(bytes: &[u8]) {
    CAPTURE_BUF.with(|cell| {
        if let Some(buf) = cell.borrow_mut().as_mut() {
            buf.extend_from_slice(bytes);
        }
    });
}

/// Run a command in `cwd` with the local virtualenv activated: `.venv/bin` is
/// prepended to PATH and VIRTUAL_ENV points at `.venv`, so the tools the
/// command spawns (pytest, mypy, ...) resolve from the repo's own venv. The
/// command itself still comes from the ambient PATH. When `cwd` has no
/// `.venv/bin`, the command runs with the environment unchanged.
pub fn check_call_ve_env(cwd: &Path, cmd: &str, args: &[&str]) -> Result<()> {
    let venv = cwd.join(".venv");
    let venv_bin = venv.join("bin");
    let mut command = Command::new(cmd);
    command.args(args).current_dir(cwd);
    if venv_bin.is_dir() {
        let path = match std::env::var_os("PATH") {
            Some(path) => {
                let mut parts = vec![venv_bin];
                parts.extend(std::env::split_paths(&path));
                std::env::join_paths(parts)?
            }
            None => venv_bin.into_os_string(),
        };
        command.env("PATH", path).env("VIRTUAL_ENV", &venv);
    }
    run_command(command, cmd)
}

/// Run a shell command in `cwd`, inheriting stdout/stderr (or routing into the
/// per-thread capture buffer if active).
pub fn check_call(cwd: &Path, cmd: &str, args: &[&str]) -> Result<()> {
    run_inheriting_or_capturing(cwd, cmd, args)
}

/// Run a tool in `cwd`, with the local virtualenv activated first when `venv`
/// is true (see `check_call_ve_env`; a repo without a `.venv` runs with the
/// environment unchanged either way). This is the entry point for commands
/// honouring the global `--venv`/`--no-venv` flag.
pub fn check_call_maybe_ve(cwd: &Path, venv: bool, cmd: &str, args: &[&str]) -> Result<()> {
    if venv {
        check_call_ve_env(cwd, cmd, args)
    } else {
        check_call(cwd, cmd, args)
    }
}

fn run_inheriting_or_capturing(cwd: &Path, cmd: &str, args: &[&str]) -> Result<()> {
    let mut command = Command::new(cmd);
    command.args(args).current_dir(cwd);
    run_command(command, cmd)
}

fn run_command(mut command: Command, name: &str) -> Result<()> {
    if is_capturing() {
        let output = command.output()?;
        append_to_capture(&output.stdout);
        append_to_capture(&output.stderr);
        if !output.status.success() {
            bail!("{name} failed with {}", output.status);
        }
        Ok(())
    } else {
        let status = command.status()?;
        if !status.success() {
            bail!("{name} failed with {status}");
        }
        Ok(())
    }
}

/// Run a shell command in `cwd` and return its stdout as a String (trimmed).
/// Fails if the command exits non-zero.
pub fn capture_output(cwd: &Path, cmd: &str, args: &[&str]) -> Result<String> {
    let output = Command::new(cmd)
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .output()?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("{cmd} failed: {stderr}");
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Run a shell command in `cwd` and return (exit_code, stdout, stderr) without failing
/// on non-zero exit. Useful for commands where non-zero is a meaningful signal
/// (e.g. `git grep` returns 1 for "no match").
pub fn capture_output_allow_failure(
    cwd: &Path,
    cmd: &str,
    args: &[&str],
) -> Result<(i32, String, String)> {
    let output = Command::new(cmd)
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .output()?;
    let code = output.status.code().unwrap_or(-1);
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    Ok((code, stdout, stderr))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cwd() -> std::path::PathBuf {
        std::env::current_dir().unwrap()
    }

    #[test]
    fn capture_output_true() {
        let out = capture_output(&cwd(), "echo", &["hello"]).unwrap();
        assert_eq!(out, "hello");
    }

    #[test]
    fn capture_output_trims_whitespace() {
        let out = capture_output(&cwd(), "echo", &["  padded  "]).unwrap();
        assert_eq!(out, "padded");
    }

    #[test]
    fn capture_output_fails_on_bad_command() {
        let result = capture_output(&cwd(), "false", &[]);
        assert!(result.is_err());
    }

    #[test]
    fn capture_output_allow_failure_returns_nonzero() {
        let (code, _, _) = capture_output_allow_failure(&cwd(), "false", &[]).unwrap();
        assert_ne!(code, 0);
    }

    #[test]
    fn capture_output_allow_failure_returns_zero() {
        let (code, _, _) = capture_output_allow_failure(&cwd(), "true", &[]).unwrap();
        assert_eq!(code, 0);
    }

    #[test]
    fn check_call_succeeds() {
        assert!(check_call(&cwd(), "true", &[]).is_ok());
    }

    #[test]
    fn check_call_fails() {
        assert!(check_call(&cwd(), "false", &[]).is_err());
    }

    #[test]
    fn check_call_ve_env_prefers_venv_tools() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join(".venv/bin");
        std::fs::create_dir_all(&bin).unwrap();
        let tool = bin.join("ve-env-probe");
        std::fs::write(&tool, "#!/bin/sh\necho from-venv\necho \"$VIRTUAL_ENV\"\n").unwrap();
        std::fs::set_permissions(&tool, std::fs::Permissions::from_mode(0o755)).unwrap();
        enter_capture();
        check_call_ve_env(dir.path(), "ve-env-probe", &[]).unwrap();
        let captured = leave_capture();
        let text = String::from_utf8_lossy(&captured);
        assert!(text.contains("from-venv"));
        assert!(text.contains(".venv"));
    }

    #[test]
    fn check_call_ve_env_without_venv_runs_ambient() {
        let dir = tempfile::tempdir().unwrap();
        assert!(check_call_ve_env(dir.path(), "true", &[]).is_ok());
    }

    #[test]
    fn capture_mode_collects_output() {
        enter_capture();
        check_call(&cwd(), "sh", &["-c", "echo hi"]).unwrap();
        let captured = leave_capture();
        let text = String::from_utf8_lossy(&captured);
        assert!(text.contains("hi"));
    }

    #[test]
    fn leave_without_enter_returns_empty() {
        let buf = leave_capture();
        assert!(buf.is_empty());
    }
}
