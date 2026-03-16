use std::path::Path;

use anyhow::{Context, Result};
use git2::Repository;

/// Returns true if the repository has modified (dirty) files in its working directory.
pub fn is_dirty(project: &Path) -> Result<bool> {
    let repo = Repository::open(project)
        .with_context(|| format!("failed to open repo at {}", project.display()))?;
    let statuses = repo
        .statuses(None)
        .with_context(|| format!("failed to get statuses for {}", project.display()))?;
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
            return Ok(true);
        }
    }
    Ok(false)
}

/// Returns true if the repository has untracked files.
pub fn has_untracked(project: &Path) -> Result<bool> {
    let repo = Repository::open(project)
        .with_context(|| format!("failed to open repo at {}", project.display()))?;
    let statuses = repo
        .statuses(None)
        .with_context(|| format!("failed to get statuses for {}", project.display()))?;
    for entry in statuses.iter() {
        if entry.status().contains(git2::Status::WT_NEW) {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Returns true if the local branch is NOT synchronized with its upstream
/// (i.e. there are commits ahead or behind).
pub fn non_synchronized(project: &Path) -> Result<bool> {
    let repo = Repository::open(project)
        .with_context(|| format!("failed to open repo at {}", project.display()))?;

    let head = match repo.head() {
        Ok(h) => h,
        Err(_) => return Ok(false), // no HEAD, skip
    };

    let local_oid = match head.target() {
        Some(oid) => oid,
        None => return Ok(false),
    };

    let branch_name = match head.shorthand() {
        Some(name) => name.to_string(),
        None => return Ok(false),
    };

    let upstream_ref = format!("refs/remotes/origin/{branch_name}");
    let upstream_oid = match repo.refname_to_id(&upstream_ref) {
        Ok(oid) => oid,
        Err(_) => return Ok(true), // no upstream → not synchronized
    };

    let (ahead, behind) = repo.graph_ahead_behind(local_oid, upstream_oid)?;
    Ok(ahead != 0 || behind != 0)
}

/// Returns true if the local branch has commits ahead of its upstream.
pub fn is_ahead(project: &Path) -> Result<bool> {
    let repo = Repository::open(project)
        .with_context(|| format!("failed to open repo at {}", project.display()))?;

    let head = match repo.head() {
        Ok(h) => h,
        Err(_) => return Ok(false),
    };

    let local_oid = match head.target() {
        Some(oid) => oid,
        None => return Ok(false),
    };

    let branch_name = match head.shorthand() {
        Some(name) => name.to_string(),
        None => return Ok(false),
    };

    let upstream_ref = format!("refs/remotes/origin/{branch_name}");
    let upstream_oid = match repo.refname_to_id(&upstream_ref) {
        Ok(oid) => oid,
        Err(_) => return Ok(false), // no upstream → nothing to push to
    };

    let (ahead, _behind) = repo.graph_ahead_behind(local_oid, upstream_oid)?;
    Ok(ahead != 0)
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

        // Create and commit a file
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

        // Modify the committed file
        fs::write(&file_path, "changed").unwrap();
        assert!(is_dirty(tmp.path()).unwrap());
    }

    #[test]
    fn staged_file_is_dirty() {
        let tmp = TempDir::new().unwrap();
        let repo = init_repo_with_commit(tmp.path());

        // Create a new file and stage it (INDEX_NEW)
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
    fn repo_without_upstream_is_non_synchronized() {
        let tmp = TempDir::new().unwrap();
        init_repo_with_commit(tmp.path());
        // No remote set up, so non_synchronized should return true
        assert!(non_synchronized(tmp.path()).unwrap());
    }

    #[test]
    fn non_repo_errors() {
        let tmp = TempDir::new().unwrap();
        assert!(is_dirty(tmp.path()).is_err());
        assert!(has_untracked(tmp.path()).is_err());
    }
}
