use camino::Utf8Path;

use anyhow::Result;

use crate::cli::NpmWhat;
use crate::subprocess_utils::check_call_maybe_ve;

/// Repos npm can operate on: those with a package.json at the root.
pub fn check_package_json(project: &Utf8Path) -> Result<bool> {
    Ok(project.join("package.json").exists())
}

/// The argument vector `rsmultigit npm <what>` hands to `npm`.
///
/// `fix` turns `audit` into `audit fix`. main.rs has already rejected the
/// flag/operation combinations that make no sense, so this only has to know
/// where the flag goes.
pub fn args(what: NpmWhat, fix: bool) -> Vec<&'static str> {
    let mut args: Vec<&'static str> = match what {
        NpmWhat::Install => vec!["install"],
        NpmWhat::Ci => vec!["ci"],
        NpmWhat::Update => vec!["update"],
        NpmWhat::Audit => vec!["audit"],
        NpmWhat::Outdated => vec!["outdated"],
        NpmWhat::Test => vec!["test"],
        NpmWhat::Publish => vec!["publish"],
    };
    if fix {
        args.push("fix");
    }
    args
}

/// Run one npm operation in `project`. Honours the global `--venv` flag the
/// way `cargo` does, so package.json scripts that shell out to tooling living
/// in the repo's `.venv` see it on PATH.
pub fn run(project: &Utf8Path, venv: bool, what: NpmWhat, fix: bool) -> Result<()> {
    let args = args(what, fix);
    check_call_maybe_ve(project, venv, "npm", &args)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_operations() {
        assert_eq!(args(NpmWhat::Install, false), ["install"]);
        assert_eq!(args(NpmWhat::Ci, false), ["ci"]);
        assert_eq!(args(NpmWhat::Update, false), ["update"]);
        assert_eq!(args(NpmWhat::Audit, false), ["audit"]);
        assert_eq!(args(NpmWhat::Outdated, false), ["outdated"]);
        assert_eq!(args(NpmWhat::Test, false), ["test"]);
        assert_eq!(args(NpmWhat::Publish, false), ["publish"]);
    }

    #[test]
    fn audit_fix() {
        assert_eq!(args(NpmWhat::Audit, true), ["audit", "fix"]);
    }

    #[test]
    fn check_package_json_requires_the_file() {
        let tmp = tempfile::TempDir::new().unwrap();
        let dir = Utf8Path::from_path(tmp.path()).unwrap();
        assert!(!check_package_json(dir).unwrap());
        std::fs::write(dir.join("package.json"), "{}").unwrap();
        assert!(check_package_json(dir).unwrap());
    }
}
