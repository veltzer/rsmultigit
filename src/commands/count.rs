use std::path::Path;

use anyhow::{Context, Result};
use git2::Repository;

pub(crate) fn open_repo(project: &Path) -> Result<Repository> {
    Repository::open(project)
        .with_context(|| format!("failed to open repo at {}", project.display()))
}

/// Returns true if there are any dirty changes (modified, staged, or new in index)
/// OR any untracked files. One status scan serves both questions.
pub fn has_changes(project: &Path) -> Result<(bool, bool)> {
    let repo = open_repo(project)?;
    let statuses = repo
        .statuses(None)
        .with_context(|| format!("failed to get statuses for {}", project.display()))?;
    let mut dirty = false;
    let mut untracked = false;
    for entry in statuses.iter() {
        let s = entry.status();
        if s.intersects(
            git2::Status::WT_MODIFIED
                | git2::Status::WT_DELETED
                | git2::Status::WT_RENAMED
                | git2::Status::WT_TYPECHANGE
                | git2::Status::INDEX_MODIFIED
                | git2::Status::INDEX_DELETED
                | git2::Status::INDEX_RENAMED
                | git2::Status::INDEX_TYPECHANGE
                | git2::Status::INDEX_NEW,
        ) {
            dirty = true;
        }
        if s.contains(git2::Status::WT_NEW) {
            untracked = true;
        }
        if dirty && untracked {
            break;
        }
    }
    Ok((dirty, untracked))
}

/// Returns true if the repository has modified (dirty) files in its working directory.
pub fn is_dirty(project: &Path) -> Result<bool> {
    Ok(has_changes(project)?.0)
}

/// Returns true if the repository has untracked files.
pub fn has_untracked(project: &Path) -> Result<bool> {
    Ok(has_changes(project)?.1)
}

/// Returns `Some((ahead, behind))` relative to `refs/remotes/origin/<current_branch>`,
/// or `None` when the repo has no HEAD, no branch, or no upstream ref.
pub fn ahead_behind(project: &Path) -> Result<Option<(usize, usize)>> {
    let repo = open_repo(project)?;

    let head = match repo.head() {
        Ok(h) => h,
        Err(_) => return Ok(None),
    };

    let local_oid = match head.target() {
        Some(oid) => oid,
        None => return Ok(None),
    };

    let branch_name = match head.shorthand() {
        Some(name) => name.to_string(),
        None => return Ok(None),
    };

    let upstream_ref = format!("refs/remotes/origin/{branch_name}");
    let upstream_oid = match repo.refname_to_id(&upstream_ref) {
        Ok(oid) => oid,
        Err(_) => return Ok(None),
    };

    let counts = repo.graph_ahead_behind(local_oid, upstream_oid)?;
    Ok(Some(counts))
}

/// Returns true if the local branch is NOT synchronized with its upstream.
/// A repo with no upstream is considered non-synchronized.
pub fn non_synchronized(project: &Path) -> Result<bool> {
    match ahead_behind(project)? {
        Some((ahead, behind)) => Ok(ahead != 0 || behind != 0),
        None => Ok(true),
    }
}

/// Returns true if the local branch has commits ahead of its upstream.
/// Repos without an upstream have nothing to push to, so return false.
pub fn is_ahead(project: &Path) -> Result<bool> {
    match ahead_behind(project)? {
        Some((ahead, _)) => Ok(ahead != 0),
        None => Ok(false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use git2::Signature;
    use std::fs;
    use tempfile::TempDir;

    fn init_repo_with_commit(dir: &std::path::Path) -> Repository {
        let repo = Repository::init(dir).unwrap();
        let sig = Signature::now("Test", "test@test.com").unwrap();
        let tree_id = {
            let mut index = repo.index().unwrap();
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
    fn clean_repo_is_not_dirty() {
        let tmp = TempDir::new().unwrap();
        init_repo_with_commit(tmp.path());
        assert!(!is_dirty(tmp.path()).unwrap());
    }

    #[test]
    fn modified_file_is_dirty() {
        let tmp = TempDir::new().unwrap();
        let repo = init_repo_with_commit(tmp.path());

        let file_path = tmp.path().join("hello.txt");
        fs::write(&file_path, "hello").unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(std::path::Path::new("hello.txt")).unwrap();
        index.write().unwrap();
        let tree_id = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_id).unwrap();
        let head = repo.head().unwrap().peel_to_commit().unwrap();
        let sig = Signature::now("Test", "test@test.com").unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "add file", &tree, &[&head])
            .unwrap();

        fs::write(&file_path, "changed").unwrap();
        assert!(is_dirty(tmp.path()).unwrap());
    }

    #[test]
    fn staged_file_is_dirty() {
        let tmp = TempDir::new().unwrap();
        let repo = init_repo_with_commit(tmp.path());

        fs::write(tmp.path().join("new.txt"), "new").unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(std::path::Path::new("new.txt")).unwrap();
        index.write().unwrap();

        assert!(is_dirty(tmp.path()).unwrap());
    }

    #[test]
    fn clean_repo_has_no_untracked() {
        let tmp = TempDir::new().unwrap();
        init_repo_with_commit(tmp.path());
        assert!(!has_untracked(tmp.path()).unwrap());
    }

    #[test]
    fn repo_with_new_file_has_untracked() {
        let tmp = TempDir::new().unwrap();
        init_repo_with_commit(tmp.path());
        fs::write(tmp.path().join("untracked.txt"), "data").unwrap();
        assert!(has_untracked(tmp.path()).unwrap());
    }

    #[test]
    fn has_changes_detects_both() {
        let tmp = TempDir::new().unwrap();
        init_repo_with_commit(tmp.path());
        fs::write(tmp.path().join("untracked.txt"), "data").unwrap();
        let (dirty, untracked) = has_changes(tmp.path()).unwrap();
        assert!(!dirty);
        assert!(untracked);
    }

    #[test]
    fn repo_without_upstream_is_non_synchronized() {
        let tmp = TempDir::new().unwrap();
        init_repo_with_commit(tmp.path());
        assert!(non_synchronized(tmp.path()).unwrap());
    }

    #[test]
    fn repo_without_upstream_is_not_ahead() {
        let tmp = TempDir::new().unwrap();
        init_repo_with_commit(tmp.path());
        assert!(!is_ahead(tmp.path()).unwrap());
    }

    #[test]
    fn non_repo_errors() {
        let tmp = TempDir::new().unwrap();
        assert!(is_dirty(tmp.path()).is_err());
        assert!(has_untracked(tmp.path()).is_err());
    }
}
