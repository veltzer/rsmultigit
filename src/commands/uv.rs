use camino::Utf8Path;

use anyhow::Result;

use crate::subprocess_utils::Cmd;

/// Repos uv can operate on: those with a pyproject.toml at the root.
pub fn check_pyproject(project: &Utf8Path) -> Result<bool> {
    Ok(project.join("pyproject.toml").exists())
}

/// Sync the project environment from the lockfile: `uv sync`.
///
/// Runs with a clean environment (see `Cmd::without_venv_selection`): uv resolves the
/// project and its `.venv` from the repo dir itself, so the global
/// `--venv`/`--no-venv` flag does not apply to uv.
pub fn sync(project: &Utf8Path) -> Result<()> {
    Cmd::new(project, "uv")
        .args(&["sync"])
        .without_venv_selection()
        .run()?;
    Ok(())
}

/// Re-resolve the lockfile from pyproject.toml: `uv lock`, optionally with
/// `--upgrade` to allow moving already-locked versions forward, or with
/// `--check` to only assert the lockfile is up to date (stale lockfile =
/// non-zero exit = error). The two flags are mutually exclusive (enforced
/// by clap). Runs with a clean environment, as `sync` does.
pub fn lock(project: &Utf8Path, upgrade: bool, check: bool) -> Result<()> {
    let args: &[&str] = match (upgrade, check) {
        (true, _) => &["lock", "--upgrade"],
        (_, true) => &["lock", "--check"],
        _ => &["lock"],
    };
    Cmd::new(project, "uv")
        .args(args)
        .without_venv_selection()
        .run()?;
    Ok(())
}

/// Build the sdist and wheel into `dist/`: `uv build`. Runs with a clean
/// environment, as `sync` does.
pub fn build(project: &Utf8Path) -> Result<()> {
    Cmd::new(project, "uv")
        .args(&["build"])
        .without_venv_selection()
        .run()?;
    Ok(())
}

/// Upload whatever `uv build` left in `dist/` to the package index:
/// `uv publish`. Credentials come from uv's own configuration (`UV_PUBLISH_*`,
/// trusted publishing, keyring); rsmultigit passes nothing through. Runs with
/// a clean environment, as `sync` does.
pub fn publish(project: &Utf8Path) -> Result<()> {
    Cmd::new(project, "uv")
        .args(&["publish"])
        .without_venv_selection()
        .run()?;
    Ok(())
}
