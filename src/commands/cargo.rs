use camino::Utf8Path;

use anyhow::Result;

use crate::cli::CargoWhat;
use crate::subprocess_utils::check_call_maybe_ve;

/// The argument vector `rsmultigit cargo <what>` hands to `cargo`.
///
/// `release` appends `--release` to the operations that compile under a
/// profile; `check` turns `fmt` into a verification-only run. main.rs has
/// already rejected the flag/operation combinations that make no sense, so
/// this only has to know where each flag goes (`clippy` takes it before the
/// `--` that separates cargo's arguments from clippy's own).
pub fn args(what: CargoWhat, release: bool, check: bool) -> Vec<&'static str> {
    let mut args: Vec<&'static str> = match what {
        CargoWhat::Build => vec!["build"],
        CargoWhat::Check => vec!["check"],
        CargoWhat::Clippy => vec!["clippy", "--all-targets"],
        CargoWhat::Fmt => vec!["fmt", "--all"],
        CargoWhat::Test => vec!["test"],
        CargoWhat::Nextest => vec!["nextest", "run"],
        CargoWhat::Doc => vec!["doc", "--no-deps"],
        CargoWhat::Deny => vec!["deny", "check"],
        CargoWhat::Fetch => vec!["fetch"],
        CargoWhat::Update => vec!["update"],
        CargoWhat::Clean => vec!["clean"],
        CargoWhat::Publish => vec!["publish"],
    };
    if release {
        args.push("--release");
    }
    if check {
        args.push("--check");
    }
    if what == CargoWhat::Clippy {
        // Same invocation as the fleet's ci.yml: warnings fail the run.
        args.extend(["--", "-D", "warnings"]);
    }
    args
}

/// Run one cargo operation in `project`. Honours the global `--venv` flag the
/// way `build cargo` does, so a repo whose `.venv` carries tooling cargo
/// shells out to (e.g. python for build scripts) sees it on PATH.
pub fn run(
    project: &Utf8Path,
    venv: bool,
    what: CargoWhat,
    release: bool,
    check: bool,
) -> Result<()> {
    let args = args(what, release, check);
    check_call_maybe_ve(project, venv, "cargo", &args)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_operations() {
        assert_eq!(args(CargoWhat::Build, false, false), ["build"]);
        assert_eq!(args(CargoWhat::Check, false, false), ["check"]);
        assert_eq!(args(CargoWhat::Fmt, false, false), ["fmt", "--all"]);
        assert_eq!(args(CargoWhat::Test, false, false), ["test"]);
        assert_eq!(args(CargoWhat::Nextest, false, false), ["nextest", "run"]);
        assert_eq!(args(CargoWhat::Doc, false, false), ["doc", "--no-deps"]);
        assert_eq!(args(CargoWhat::Deny, false, false), ["deny", "check"]);
        assert_eq!(args(CargoWhat::Fetch, false, false), ["fetch"]);
        assert_eq!(args(CargoWhat::Update, false, false), ["update"]);
        assert_eq!(args(CargoWhat::Clean, false, false), ["clean"]);
        assert_eq!(args(CargoWhat::Publish, false, false), ["publish"]);
    }

    #[test]
    fn clippy_matches_ci_and_keeps_release_before_the_separator() {
        assert_eq!(
            args(CargoWhat::Clippy, false, false),
            ["clippy", "--all-targets", "--", "-D", "warnings"]
        );
        assert_eq!(
            args(CargoWhat::Clippy, true, false),
            [
                "clippy",
                "--all-targets",
                "--release",
                "--",
                "-D",
                "warnings"
            ]
        );
    }

    #[test]
    fn release_and_check_flags() {
        assert_eq!(args(CargoWhat::Build, true, false), ["build", "--release"]);
        assert_eq!(
            args(CargoWhat::Nextest, true, false),
            ["nextest", "run", "--release"]
        );
        assert_eq!(
            args(CargoWhat::Fmt, false, true),
            ["fmt", "--all", "--check"]
        );
    }

    #[test]
    fn flag_applicability() {
        for what in [
            CargoWhat::Build,
            CargoWhat::Check,
            CargoWhat::Clippy,
            CargoWhat::Test,
            CargoWhat::Nextest,
            CargoWhat::Doc,
        ] {
            assert!(what.takes_release(), "{what:?} should take --release");
            assert!(!what.takes_check(), "{what:?} should not take --check");
        }
        for what in [
            CargoWhat::Deny,
            CargoWhat::Fetch,
            CargoWhat::Update,
            CargoWhat::Clean,
            CargoWhat::Publish,
        ] {
            assert!(!what.takes_release(), "{what:?} should not take --release");
            assert!(!what.takes_check(), "{what:?} should not take --check");
        }
        assert!(CargoWhat::Fmt.takes_check());
        assert!(!CargoWhat::Fmt.takes_release());
    }
}
