use camino::Utf8Path;

use anyhow::Result;

use crate::subprocess_utils::check_call;

/// Push the current branch to its upstream. The runner's precondition is
/// `count::is_ahead`, so repos with nothing to push are never reached.
pub fn do_push(project: &Utf8Path) -> Result<()> {
    check_call(project, "git", &["push"])?;
    Ok(())
}
