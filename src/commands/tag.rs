use camino::Utf8Path;

use anyhow::Result;

use crate::commands::count::open_repo;
use crate::subprocess_utils::{capture_output, check_call, out_line};

/// The repo's local tag names, sorted as `git tag` lists them.
fn local_tags(project: &Utf8Path) -> Result<Vec<String>> {
    let repo = open_repo(project)?;
    let tags = repo.tag_names(None)?;
    // Names that are not valid UTF-8 come back as None and are left out.
    let mut names: Vec<String> = tags
        .iter()
        .filter_map(|name| name.ok().flatten().map(String::from))
        .collect();
    names.sort();
    Ok(names)
}

/// List local tags.
pub fn tag_local(project: &Utf8Path) -> Result<()> {
    for name in local_tags(project)? {
        out_line(&name);
    }
    Ok(())
}

/// List remote tags. A network operation, so it stays a git call.
pub fn tag_remote(project: &Utf8Path) -> Result<()> {
    check_call(project, "git", &["ls-remote", "--tags", "origin"])
}

/// Check if local tags exist.
pub fn tag_has_local(project: &Utf8Path) -> Result<bool> {
    Ok(!local_tags(project)?.is_empty())
}

/// Check if remote tags exist.
pub fn tag_has_remote(project: &Utf8Path) -> Result<bool> {
    let output = capture_output(project, "git", &["ls-remote", "--tags", "origin"])?;
    Ok(!output.is_empty())
}
