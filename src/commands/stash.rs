use camino::Utf8Path;

use anyhow::{Context, Result};

use crate::commands::count::open_repo;
use crate::subprocess_utils::check_call;

/// The message every stash rsmultigit creates carries. `pop` restores only a
/// stash with this message, so a stash the user made by hand — or one left
/// over from long ago — is never popped by a fleet-wide `stash pop`.
const STASH_MESSAGE: &str = "rsmultigit stash";

/// Whether `push` has anything to stash: changes to tracked files, the only
/// thing a plain `git stash push` saves. A clean repo is skipped instead of
/// running a push that stashes nothing.
pub fn has_changes_to_stash(project: &Utf8Path) -> Result<bool> {
    crate::commands::count::is_dirty(project)
}

/// Stash working-tree changes under rsmultigit's own message.
pub fn stash_push(project: &Utf8Path) -> Result<()> {
    check_call(project, "git", &["stash", "push", "-m", STASH_MESSAGE])
}

/// Index of the most recent stash rsmultigit made, if any. git records the
/// message as `On <branch>: <message>` (`On (no branch): ...` when detached).
fn own_stash(project: &Utf8Path) -> Result<Option<usize>> {
    let mut repo = open_repo(project)?;
    let suffix = format!(": {STASH_MESSAGE}");
    let mut found = None;
    repo.stash_foreach(|index, message, _| {
        if message.ends_with(&suffix) {
            found = Some(index);
            false
        } else {
            true
        }
    })
    .with_context(|| format!("failed to list stashes in {project}"))?;
    Ok(found)
}

/// Whether `pop` has a stash of rsmultigit's own to restore.
pub fn has_own_stash(project: &Utf8Path) -> Result<bool> {
    Ok(own_stash(project)?.is_some())
}

/// Pop the most recent stash rsmultigit made (not necessarily `stash@{0}`).
pub fn stash_pop(project: &Utf8Path) -> Result<()> {
    let index =
        own_stash(project)?.with_context(|| format!("no `{STASH_MESSAGE}` stash in {project}"))?;
    check_call(
        project,
        "git",
        &["stash", "pop", &format!("stash@{{{index}}}")],
    )
}
