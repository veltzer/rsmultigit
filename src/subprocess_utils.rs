use camino::Utf8Path;
use std::cell::RefCell;
use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::sync::mpsc;

use anyhow::{Context, Result, bail};

/// Which of the child's output streams a captured chunk came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stream {
    Stdout,
    Stderr,
}

/// Output collected under capture: chunks in the order they arrived, each
/// tagged with its stream, so a replay puts stderr back on stderr and keeps
/// the two interleaved as closely as two pipes allow (exactly per read; a
/// child writing to both within one scheduling slice can still be reordered).
#[derive(Debug, Default)]
pub struct Captured {
    chunks: Vec<(Stream, Vec<u8>)>,
}

impl Captured {
    pub fn is_empty(&self) -> bool {
        self.chunks.iter().all(|(_, bytes)| bytes.is_empty())
    }

    fn push(&mut self, stream: Stream, bytes: &[u8]) {
        match self.chunks.last_mut() {
            Some((last, buf)) if *last == stream => buf.extend_from_slice(bytes),
            _ => self.chunks.push((stream, bytes.to_vec())),
        }
    }

    /// Write every chunk to the stream it came from, in arrival order.
    pub fn replay(&self) {
        let stdout = std::io::stdout();
        let stderr = std::io::stderr();
        let mut out = stdout.lock();
        let mut err = stderr.lock();
        for (stream, bytes) in &self.chunks {
            // Flush the other stream first so the terminal sees the order
            // the chunks arrived in.
            match stream {
                Stream::Stdout => {
                    err.flush().ok();
                    out.write_all(bytes).ok();
                }
                Stream::Stderr => {
                    out.flush().ok();
                    err.write_all(bytes).ok();
                }
            }
        }
        out.flush().ok();
        err.flush().ok();
    }

    /// Both streams as one text, in arrival order, for error messages.
    pub fn to_text_lossy(&self) -> String {
        let bytes: Vec<u8> = self
            .chunks
            .iter()
            .flat_map(|(_, b)| b.iter().copied())
            .collect();
        String::from_utf8_lossy(&bytes).into_owned()
    }
}

// Per-thread capture buffer. When present, every child run through `Cmd`
// has its stdout/stderr collected into it instead of inheriting the parent's
// streams. This lets the parallel runner replay output in project order.
thread_local! {
    static CAPTURE_BUF: RefCell<Option<Captured>> = const { RefCell::new(None) };
}

/// Output capture on this thread for as long as the guard lives. `finish`
/// ends it and hands back what was collected; a guard dropped without
/// `finish` (an early return, a panic) still ends it, discarding the output,
/// so capture can never leak into whatever the thread runs next.
#[must_use = "capture ends when the guard is dropped"]
pub struct CaptureGuard {
    active: bool,
}

/// Begin capturing subprocess output (and `out_line`) on this thread. Any
/// prior buffer is replaced.
pub fn capture() -> CaptureGuard {
    CAPTURE_BUF.with(|cell| *cell.borrow_mut() = Some(Captured::default()));
    CaptureGuard { active: true }
}

impl CaptureGuard {
    /// Stop capturing and return what was collected.
    pub fn finish(mut self) -> Captured {
        self.active = false;
        take_capture()
    }
}

impl Drop for CaptureGuard {
    fn drop(&mut self) {
        if self.active {
            take_capture();
        }
    }
}

fn take_capture() -> Captured {
    CAPTURE_BUF.with(|cell| cell.borrow_mut().take().unwrap_or_default())
}

/// Print one line of a command's own output. It goes into the per-thread
/// capture buffer when one is active (the parallel runner, `--no-output`)
/// and straight to stdout otherwise, so command modules that format their
/// own output (`grep`, `gh`, ...) stay in repo order under `-j` and are
/// silenced by `--no-output` just like subprocess output.
pub fn out_line(line: &str) {
    if is_capturing() {
        append_to_capture(Stream::Stdout, line.as_bytes());
        append_to_capture(Stream::Stdout, b"\n");
    } else {
        println!("{line}");
    }
}

fn is_capturing() -> bool {
    CAPTURE_BUF.with(|cell| cell.borrow().is_some())
}

fn append_to_capture(stream: Stream, bytes: &[u8]) {
    CAPTURE_BUF.with(|cell| {
        if let Some(buf) = cell.borrow_mut().as_mut() {
            buf.push(stream, bytes);
        }
    });
}

/// What a child printed and how it exited, for `Cmd::output`.
#[derive(Debug)]
pub struct CmdOutput {
    /// The exit code; -1 when the child was killed by a signal.
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

/// One child process, run in a repo directory. Built up with the
/// environment changes it needs, then finished with one of `run` (output
/// shown live or captured), `stdout` (output returned, non-zero exit is an
/// error) or `output` (everything returned, any exit accepted).
pub struct Cmd {
    command: Command,
    name: String,
}

impl Cmd {
    pub fn new(cwd: &Utf8Path, program: &str) -> Self {
        let mut command = Command::new(program);
        command.current_dir(cwd);
        Self {
            command,
            name: program.to_string(),
        }
    }

    pub fn args(mut self, args: &[&str]) -> Self {
        self.command.args(args);
        self
    }

    /// Set a variable for this one child (and whatever it spawns). This is
    /// how a secret fetched at run time reaches a tool without touching
    /// disk: `cargo release` gets the crates.io token this way.
    pub fn env(mut self, key: &str, value: &str) -> Self {
        self.command.env(key, value);
        self
    }

    /// Activate the repo's local virtualenv when `on`: `.venv/bin` is
    /// prepended to PATH and VIRTUAL_ENV points at `.venv`, so the tools the
    /// command spawns (pytest, mypy, ...) resolve from the repo's own venv.
    /// The program itself is resolved through that same PATH, so a copy of
    /// it in `.venv/bin` wins over the ambient one. A repo without a
    /// `.venv/bin` runs with the environment unchanged. This is how the
    /// global `--venv`/`--no-venv` flag is honoured.
    pub fn venv(mut self, on: bool) -> Result<Self> {
        let Some(cwd) = self.command.get_current_dir().map(|d| d.to_path_buf()) else {
            return Ok(self);
        };
        let venv = cwd.join(".venv");
        let venv_bin = venv.join("bin");
        if on && venv_bin.is_dir() {
            let path = match std::env::var_os("PATH") {
                Some(path) => {
                    // Plain PathBufs: PATH entries need not be UTF-8, and
                    // every one of them must survive the round trip.
                    let mut parts = vec![venv_bin];
                    parts.extend(std::env::split_paths(&path));
                    std::env::join_paths(parts)?
                }
                None => venv_bin.into_os_string(),
            };
            self.command.env("PATH", path).env("VIRTUAL_ENV", &venv);
        }
        Ok(self)
    }

    /// Remove any inherited virtualenv selection: `VIRTUAL_ENV` and
    /// `UV_PROJECT_ENVIRONMENT` are unset, so a tool that picks its target
    /// environment itself sees only the repo directory.
    ///
    /// This is what `uv` wants. uv locates the environment from the working
    /// directory (the project's `.venv` for `uv sync`/`uv lock`, `./.venv`
    /// for the `uv pip` interface), and an inherited `VIRTUAL_ENV` only
    /// overrides that with the venv the *caller's shell* happened to be in.
    /// For `uv sync` that earns a "does not match the project environment"
    /// warning; for `uv pip` it silently redirects the install into the
    /// wrong venv. Neither is wanted across a fleet of repos.
    pub fn without_venv_selection(mut self) -> Self {
        self.command
            .env_remove("VIRTUAL_ENV")
            .env_remove("UV_PROJECT_ENVIRONMENT");
        self
    }

    /// Run the child with its output shown live, or collected into the
    /// thread's capture buffer when one is active. A non-zero exit is an
    /// error.
    pub fn run(self) -> Result<()> {
        let Self { mut command, name } = self;
        let status = if is_capturing() {
            run_capturing(command, &name)?
        } else {
            command
                .status()
                .with_context(|| format!("failed to run {name}"))?
        };
        if !status.success() {
            bail!("{name} failed with {status}");
        }
        Ok(())
    }

    /// Run the child and return what it printed and how it exited; a
    /// non-zero exit is not an error here. For commands where non-zero is a
    /// meaningful answer (`git grep` exits 1 for "no match").
    pub fn output(self) -> Result<CmdOutput> {
        let Self { mut command, name } = self;
        let output = command
            .stdin(Stdio::null())
            .output()
            .with_context(|| format!("failed to run {name}"))?;
        Ok(CmdOutput {
            code: output.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        })
    }

    /// Run the child and return its stdout, trimmed. A non-zero exit is an
    /// error carrying the child's stderr.
    pub fn stdout(self) -> Result<String> {
        let name = self.name.clone();
        let out = self.output()?;
        if out.code != 0 {
            bail!("{name} failed: {}", out.stderr);
        }
        Ok(out.stdout.trim().to_string())
    }
}

/// `Cmd::new(cwd, cmd).args(args).run()`: the common case.
pub fn check_call(cwd: &Utf8Path, cmd: &str, args: &[&str]) -> Result<()> {
    Cmd::new(cwd, cmd).args(args).run()
}

/// `Cmd::new(cwd, cmd).args(args).stdout()`: the common case.
pub fn capture_output(cwd: &Utf8Path, cmd: &str, args: &[&str]) -> Result<String> {
    Cmd::new(cwd, cmd).args(args).stdout()
}

/// Run `command` with stdout and stderr piped, appending each chunk to the
/// capture buffer, tagged with its stream, as it arrives. One reader thread
/// per pipe feeds a channel, so neither pipe can fill up and stall the child
/// while the other is being drained.
fn run_capturing(mut command: Command, name: &str) -> Result<std::process::ExitStatus> {
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("failed to run {name}"))?;
    let (tx, rx) = mpsc::channel::<(Stream, Vec<u8>)>();
    let pipes: [(Stream, Box<dyn Read + Send>); 2] = [
        (
            Stream::Stdout,
            Box::new(child.stdout.take().context("child stdout")?),
        ),
        (
            Stream::Stderr,
            Box::new(child.stderr.take().context("child stderr")?),
        ),
    ];
    let readers: Vec<_> = pipes
        .into_iter()
        .map(|(stream, mut pipe)| {
            let tx = tx.clone();
            std::thread::spawn(move || {
                let mut buf = [0u8; 8192];
                while let Ok(n) = pipe.read(&mut buf) {
                    if n == 0 || tx.send((stream, buf[..n].to_vec())).is_err() {
                        break;
                    }
                }
            })
        })
        .collect();
    drop(tx);
    for (stream, bytes) in rx {
        append_to_capture(stream, &bytes);
    }
    for reader in readers {
        reader.join().ok();
    }
    Ok(child.wait()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cwd() -> camino::Utf8PathBuf {
        camino::Utf8PathBuf::from_path_buf(std::env::current_dir().unwrap()).unwrap()
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
    fn output_returns_nonzero() {
        let out = Cmd::new(&cwd(), "false").output().unwrap();
        assert_ne!(out.code, 0);
    }

    #[test]
    fn output_returns_zero() {
        let out = Cmd::new(&cwd(), "true").output().unwrap();
        assert_eq!(out.code, 0);
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
    fn venv_prefers_venv_tools() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let bin = camino::Utf8Path::from_path(dir.path())
            .unwrap()
            .join(".venv/bin");
        std::fs::create_dir_all(&bin).unwrap();
        let tool = bin.join("ve-env-probe");
        std::fs::write(&tool, "#!/bin/sh\necho from-venv\necho \"$VIRTUAL_ENV\"\n").unwrap();
        std::fs::set_permissions(&tool, std::fs::Permissions::from_mode(0o755)).unwrap();
        let guard = capture();
        Cmd::new(
            camino::Utf8Path::from_path(dir.path()).unwrap(),
            "ve-env-probe",
        )
        .venv(true)
        .unwrap()
        .run()
        .unwrap();
        let text = guard.finish().to_text_lossy();
        assert!(text.contains("from-venv"));
        assert!(text.contains(".venv"));
    }

    #[test]
    fn without_venv_selection_unsets_virtual_env() {
        // SAFETY: single-threaded test process (nextest runs each test in its
        // own process), and the var is removed again before returning.
        unsafe {
            std::env::set_var("VIRTUAL_ENV", "/somewhere/else/.venv");
            std::env::set_var("UV_PROJECT_ENVIRONMENT", "/somewhere/else/env");
        }
        let guard = capture();
        check_call(&cwd(), "sh", &["-c", "echo ambient=[${VIRTUAL_ENV-unset}]"]).unwrap();
        let ambient = guard.finish().to_text_lossy();
        let guard = capture();
        Cmd::new(&cwd(), "sh")
            .args(&[
                "-c",
                "echo ve=[${VIRTUAL_ENV-unset}] upe=[${UV_PROJECT_ENVIRONMENT-unset}]",
            ])
            .without_venv_selection()
            .run()
            .unwrap();
        let cleaned = guard.finish().to_text_lossy();
        unsafe {
            std::env::remove_var("VIRTUAL_ENV");
            std::env::remove_var("UV_PROJECT_ENVIRONMENT");
        }
        // check_call passes the ambient value through; without_venv_selection does not.
        assert!(
            ambient.contains("ambient=[/somewhere/else/.venv]"),
            "expected ambient VIRTUAL_ENV to be inherited, got {ambient:?}"
        );
        assert!(
            cleaned.contains("ve=[unset]"),
            "expected VIRTUAL_ENV to be unset, got {cleaned:?}"
        );
        assert!(
            cleaned.contains("upe=[unset]"),
            "expected UV_PROJECT_ENVIRONMENT to be unset, got {cleaned:?}"
        );
    }

    #[test]
    fn without_venv_selection_propagates_failure() {
        let cmd = Cmd::new(&cwd(), "false").without_venv_selection();
        assert!(cmd.run().is_err());
    }

    #[test]
    fn venv_without_a_venv_runs_ambient() {
        let dir = tempfile::tempdir().unwrap();
        let cmd = Cmd::new(camino::Utf8Path::from_path(dir.path()).unwrap(), "true");
        assert!(cmd.venv(true).unwrap().run().is_ok());
    }

    #[test]
    fn a_program_that_cannot_start_is_named_in_the_error() {
        let missing = "rsmultigit-no-such-program";
        let serial = check_call(&cwd(), missing, &[]).unwrap_err();
        assert!(format!("{serial:#}").contains(&format!("failed to run {missing}")));
        let guard = capture();
        let captured = check_call(&cwd(), missing, &[]).unwrap_err();
        drop(guard);
        assert!(format!("{captured:#}").contains(&format!("failed to run {missing}")));
        let output = capture_output(&cwd(), missing, &[]).unwrap_err();
        assert!(format!("{output:#}").contains(&format!("failed to run {missing}")));
    }

    #[test]
    fn capture_mode_collects_output() {
        let guard = capture();
        check_call(&cwd(), "sh", &["-c", "echo hi"]).unwrap();
        let text = guard.finish().to_text_lossy();
        assert!(text.contains("hi"));
    }

    #[test]
    fn capture_keeps_streams_apart_and_in_order() {
        // Sleeps between the writes give each read its own chunk, so the
        // arrival order is deterministic.
        let guard = capture();
        let result = check_call(
            &cwd(),
            "sh",
            &[
                "-c",
                "echo out1; sleep 0.1; echo err1 >&2; sleep 0.1; echo out2; exit 3",
            ],
        );
        let captured = guard.finish();
        assert!(result.is_err());
        assert_eq!(
            captured.chunks,
            [
                (Stream::Stdout, b"out1\n".to_vec()),
                (Stream::Stderr, b"err1\n".to_vec()),
                (Stream::Stdout, b"out2\n".to_vec()),
            ]
        );
        assert_eq!(captured.to_text_lossy(), "out1\nerr1\nout2\n");
    }

    #[test]
    fn venv_keeps_non_utf8_path_entries() {
        use std::os::unix::ffi::OsStrExt;
        let dir = tempfile::tempdir().unwrap();
        let root = camino::Utf8Path::from_path(dir.path()).unwrap();
        std::fs::create_dir_all(root.join(".venv/bin")).unwrap();
        let odd = std::ffi::OsStr::from_bytes(b"/not/utf8/\xff");
        let ambient = std::env::var_os("PATH").unwrap();
        let mut parts = vec![std::path::PathBuf::from(odd)];
        parts.extend(std::env::split_paths(&ambient));
        // SAFETY: nextest runs each test in its own process.
        unsafe { std::env::set_var("PATH", std::env::join_paths(parts).unwrap()) };
        let guard = capture();
        Cmd::new(root, "sh")
            .args(&["-c", "printf %s \"$PATH\""])
            .venv(true)
            .unwrap()
            .run()
            .unwrap();
        let captured = guard.finish();
        unsafe { std::env::set_var("PATH", ambient) };
        let bytes: Vec<u8> = captured.chunks.into_iter().flat_map(|(_, b)| b).collect();
        assert!(
            bytes.windows(odd.len()).any(|w| w == odd.as_bytes()),
            "non-UTF-8 entry was dropped: {}",
            String::from_utf8_lossy(&bytes)
        );
    }

    #[test]
    fn out_line_goes_to_capture_when_active() {
        let guard = capture();
        out_line("first");
        out_line("second");
        assert_eq!(guard.finish().to_text_lossy(), "first\nsecond\n");
    }

    #[test]
    fn a_dropped_guard_ends_capture() {
        {
            let _guard = capture();
            assert!(is_capturing());
        }
        assert!(!is_capturing());
    }

    #[test]
    fn leave_without_enter_returns_empty() {
        let buf = take_capture();
        assert!(buf.is_empty());
    }
}
