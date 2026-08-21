use std::path::Path;

use anyhow::{Context, Result};

use crate::commands::count::{ahead_behind, open_repo};
use crate::subprocess_utils::capture_output;

/// Returns `Some(output)` when the repo needs attention: `git status -s` shows
/// working-tree changes, or the branch is ahead of / behind its upstream
/// (commits not yet pushed or not yet merged). A repo with no upstream is
/// reported by working-tree state only.
pub fn do_status(project: &Path) -> Result<Option<String>> {
    let mut output = capture_output(project, "git", &["status", "-s"])?;
    if let Some((ahead, behind)) = ahead_behind(project)? {
        for (count, direction) in [(ahead, "ahead of"), (behind, "behind")] {
            if count > 0 {
                if !output.is_empty() {
                    output.push('\n');
                }
                let plural = if count == 1 { "" } else { "s" };
                output.push_str(&format!("{direction} origin by {count} commit{plural}"));
            }
        }
    }
    if output.is_empty() {
        Ok(None)
    } else {
        Ok(Some(output))
    }
}

/// Returns `Some(summary)` when the repo needs attention, where `summary` is a
/// single line describing the situation, e.g. `2 modified, 1 untracked, ahead 3`.
/// Counted via git2 (no subprocess). Returns `None` for a clean, in-sync repo;
/// a repo with no upstream is reported by working-tree state only.
pub fn do_status_summary(project: &Path) -> Result<Option<String>> {
    let repo = open_repo(project)?;
    let statuses = repo
        .statuses(None)
        .with_context(|| format!("failed to get statuses for {}", project.display()))?;

    let mut staged = 0u32;
    let mut modified = 0u32;
    let mut deleted = 0u32;
    let mut untracked = 0u32;
    let mut conflicted = 0u32;
    for entry in statuses.iter() {
        let s = entry.status();
        if s.intersects(
            git2::Status::INDEX_NEW
                | git2::Status::INDEX_MODIFIED
                | git2::Status::INDEX_DELETED
                | git2::Status::INDEX_RENAMED
                | git2::Status::INDEX_TYPECHANGE,
        ) {
            staged += 1;
        }
        if s.intersects(
            git2::Status::WT_MODIFIED | git2::Status::WT_RENAMED | git2::Status::WT_TYPECHANGE,
        ) {
            modified += 1;
        }
        if s.contains(git2::Status::WT_DELETED) {
            deleted += 1;
        }
        if s.contains(git2::Status::WT_NEW) {
            untracked += 1;
        }
        if s.contains(git2::Status::CONFLICTED) {
            conflicted += 1;
        }
    }

    let mut parts: Vec<String> = Vec::new();
    for (count, label) in [
        (conflicted, "conflicted"),
        (staged, "staged"),
        (modified, "modified"),
        (deleted, "deleted"),
        (untracked, "untracked"),
    ] {
        if count > 0 {
            parts.push(format!("{count} {label}"));
        }
    }
    if let Some((ahead, behind)) = ahead_behind(project)? {
        for (count, direction) in [(ahead, "ahead"), (behind, "behind")] {
            if count > 0 {
                parts.push(format!("{direction} {count}"));
            }
        }
    }

    if parts.is_empty() {
        Ok(None)
    } else {
        Ok(Some(parts.join(", ")))
    }
}

/// Returns `Some(output)` if there are dirty (modified/staged) changes.
/// Uses `git diff --stat` to detect modifications.
pub fn do_dirty(project: &Path) -> Result<Option<String>> {
    let output = capture_output(project, "git", &["diff", "--stat"])?;
    if output.is_empty() {
        let staged = capture_output(project, "git", &["diff", "--cached", "--stat"])?;
        if staged.is_empty() {
            Ok(None)
        } else {
            Ok(Some(staged))
        }
    } else {
        Ok(Some(output))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use git2::{Repository, Signature};
    use std::fs;
    use tempfile::TempDir;

    fn init_repo_with_commit(dir: &Path) -> Repository {
        let repo = Repository::init(dir).unwrap();
        let sig = Signature::now("Test", "test@test.com").unwrap();
        fs::write(dir.join("tracked.txt"), "original").unwrap();
        let tree_id = {
            let mut index = repo.index().unwrap();
            index.add_path(Path::new("tracked.txt")).unwrap();
            index.write().unwrap();
            index.write_tree().unwrap()
        };
        {
            let tree = repo.find_tree(tree_id).unwrap();
            repo.commit(Some("HEAD"), &sig, &sig, "initial", &tree, &[])
                .unwrap();
        }
        repo
    }

    #[test]
    fn clean_repo_has_no_summary() {
        let tmp = TempDir::new().unwrap();
        init_repo_with_commit(tmp.path());
        assert_eq!(do_status_summary(tmp.path()).unwrap(), None);
    }

    #[test]
    fn summary_counts_modified_and_untracked() {
        let tmp = TempDir::new().unwrap();
        init_repo_with_commit(tmp.path());
        fs::write(tmp.path().join("tracked.txt"), "changed").unwrap();
        fs::write(tmp.path().join("new.txt"), "new").unwrap();
        let summary = do_status_summary(tmp.path()).unwrap().unwrap();
        assert_eq!(summary, "1 modified, 1 untracked");
    }

    #[test]
    fn summary_counts_staged_and_deleted() {
        let tmp = TempDir::new().unwrap();
        let repo = init_repo_with_commit(tmp.path());
        fs::write(tmp.path().join("added.txt"), "added").unwrap();
        {
            let mut index = repo.index().unwrap();
            index.add_path(Path::new("added.txt")).unwrap();
            index.write().unwrap();
        }
        fs::remove_file(tmp.path().join("tracked.txt")).unwrap();
        let summary = do_status_summary(tmp.path()).unwrap().unwrap();
        assert_eq!(summary, "1 staged, 1 deleted");
    }

    #[test]
    fn summary_reports_ahead_of_upstream() {
        let tmp = TempDir::new().unwrap();
        let repo = init_repo_with_commit(tmp.path());

        // Mark the current commit as the upstream tip, then commit past it.
        let head = repo.head().unwrap();
        let branch = head.shorthand().unwrap().to_string();
        let oid = head.target().unwrap();
        repo.reference(
            &format!("refs/remotes/origin/{branch}"),
            oid,
            true,
            "set upstream",
        )
        .unwrap();
        let sig = Signature::now("Test", "test@test.com").unwrap();
        let tree = repo.find_commit(oid).unwrap().tree().unwrap();
        let parent = repo.find_commit(oid).unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "local only", &tree, &[&parent])
            .unwrap();

        let summary = do_status_summary(tmp.path()).unwrap().unwrap();
        assert_eq!(summary, "ahead 1");
    }
}
