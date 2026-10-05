use camino::Utf8Path;

use anyhow::{Context, Result};

use crate::commands::count::open_repo;

/// Show a git config value, as `git config <key>` would: the repo's config
/// merged over the global and system ones. Returns None if the key is not
/// set. A malformed key (`foo`, no section) is an error, not "unset": git
/// exits 1 for both, which is why this reads the config through libgit2,
/// whose error codes tell the two apart.
pub fn do_config(project: &Utf8Path, key: &str) -> Result<Option<String>> {
    let config = open_repo(project)?
        .config()
        .with_context(|| format!("failed to read the git config of {project}"))?;
    match config.get_string(key) {
        Ok(value) if value.trim().is_empty() => Ok(None),
        Ok(value) => Ok(Some(value.trim().to_string())),
        Err(e) if e.code() == git2::ErrorCode::NotFound => Ok(None),
        Err(e) => Err(e).with_context(|| format!("invalid git config key `{key}`")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo() -> tempfile::TempDir {
        let tmp = tempfile::tempdir().unwrap();
        let repo = git2::Repository::init(tmp.path()).unwrap();
        repo.config()
            .unwrap()
            .set_str("rsmultigit.probe", "value")
            .unwrap();
        tmp
    }

    #[test]
    fn set_unset_and_malformed_keys() {
        let tmp = repo();
        let dir = Utf8Path::from_path(tmp.path()).unwrap();
        assert_eq!(
            do_config(dir, "rsmultigit.probe").unwrap().as_deref(),
            Some("value")
        );
        assert_eq!(do_config(dir, "rsmultigit.absent").unwrap(), None);
        let err = do_config(dir, "nosection").unwrap_err();
        assert!(
            format!("{err:#}").contains("invalid git config key"),
            "{err:#}"
        );
    }
}
