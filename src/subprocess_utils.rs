use std::cell::RefCell;
use std::path::Path;
use std::process::{Command, Stdio};

use anyhow::{Result, bail};

// Per-thread capture buffer. When present, `check_call` and `check_call_ve`
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

/// Run a command inside a Python virtualenv (.venv/bin/{cmd}) located in `cwd`.
pub fn check_call_ve(cwd: &Path, args: &[&str]) -> Result<()> {
    if args.is_empty() {
        bail!("check_call_ve requires at least one argument");
    }
    let venv_cmd = cwd.join(".venv/bin").join(args[0]);
    run_inheriting_or_capturing(cwd, venv_cmd.to_string_lossy().as_ref(), &args[1..])
}

/// Run a shell command in `cwd`, inheriting stdout/stderr (or routing into the
/// per-thread capture buffer if active).
pub fn check_call(cwd: &Path, cmd: &str, args: &[&str]) -> Result<()> {
    run_inheriting_or_capturing(cwd, cmd, args)
}

fn run_inheriting_or_capturing(cwd: &Path, cmd: &str, args: &[&str]) -> Result<()> {
    if is_capturing() {
        let output = Command::new(cmd).args(args).current_dir(cwd).output()?;
        append_to_capture(&output.stdout);
        append_to_capture(&output.stderr);
        if !output.status.success() {
            bail!("{cmd} failed with {}", output.status);
        }
        Ok(())
    } else {
        let status = Command::new(cmd).args(args).current_dir(cwd).status()?;
        if !status.success() {
            bail!("{cmd} failed with {status}");
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
    fn check_call_ve_empty_args() {
        assert!(check_call_ve(&cwd(), &[]).is_err());
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
