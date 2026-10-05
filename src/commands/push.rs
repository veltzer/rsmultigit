use camino::Utf8Path;

use anyhow::Result;

use crate::commands::count::open_repo;
use crate::subprocess_utils::check_call;

/// Push the current branch to where `count::is_ahead` measured it against.
/// The runner's precondition is `is_ahead`, so repos with nothing to push
/// are never reached. With a configured upstream that is plain `git push`.
/// Without one, `is_ahead` compared against `origin/<branch>`, and plain
/// `git push` would refuse the branch ("has no upstream branch") unless the
/// user's config happens to set `push.default = current`; so the push names
/// that same target explicitly instead.
pub fn do_push(project: &Utf8Path) -> Result<()> {
    if has_configured_upstream(project)? {
        check_call(project, "git", &["push"])
    } else {
        check_call(project, "git", &["push", "origin", "HEAD"])
    }
}

/// Whether the checked-out branch has `branch.<name>.remote`/`.merge` set.
fn has_configured_upstream(project: &Utf8Path) -> Result<bool> {
    let repo = open_repo(project)?;
    let head = repo.head()?;
    let Ok(name) = head.shorthand() else {
        return Ok(false);
    };
    Ok(repo
        .find_branch(name, git2::BranchType::Local)
        .is_ok_and(|branch| branch.upstream().is_ok()))
}
