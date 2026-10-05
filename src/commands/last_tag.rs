use camino::Utf8Path;

use anyhow::Result;

use crate::commands::count::open_repo;

/// The most recent tag reachable from HEAD, as `git describe --tags
/// --abbrev=0` names it, read through libgit2. None when there is no tag to
/// describe from (or no commit at all).
pub fn do_last_tag(project: &Utf8Path) -> Result<Option<String>> {
    let repo = open_repo(project)?;
    if repo.head().is_err() {
        return Ok(None);
    }
    let mut opts = git2::DescribeOptions::new();
    opts.describe_tags();
    let Ok(describe) = repo.describe(&opts) else {
        return Ok(None);
    };
    let mut format = git2::DescribeFormatOptions::new();
    format.abbreviated_size(0);
    Ok(Some(describe.format(Some(&format))?))
}
