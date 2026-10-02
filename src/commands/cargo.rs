use camino::Utf8Path;

use anyhow::Result;

use crate::cli::CargoWhat;
use crate::subprocess_utils::check_call_maybe_ve;

/// Which cargo profile(s) an operation runs under, as chosen by
/// `--release` / `--profile <name>` on the command line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Profile {
    /// No profile flag given. For `build` this means every profile the
    /// fleet ships (dev, then release, like `cargo_build.sh`); every other
    /// operation keeps cargo's own default.
    All,
    /// `--release`: cargo's `release` profile only.
    Release,
    /// `--profile <name>`: one named profile (`dev`, `release`, or any custom
    /// profile the crate defines).
    Named(String),
}

impl Profile {
    /// Build the selection from the raw CLI flags. main.rs has already
    /// rejected the flag/operation combinations that make no sense, and clap
    /// has rejected `--release --profile`.
    pub fn from_flags(release: bool, profile: Option<&str>) -> Self {
        match (release, profile) {
            (true, _) => Profile::Release,
            (false, Some(name)) => Profile::Named(name.to_string()),
            (false, None) => Profile::All,
        }
    }

    /// The flags cargo takes for this selection; empty for `All`.
    fn flags(&self) -> Vec<String> {
        match self {
            Profile::All => vec![],
            Profile::Release => vec!["--release".to_string()],
            Profile::Named(name) => vec!["--profile".to_string(), name.clone()],
        }
    }
}

/// The argument vectors `rsmultigit cargo <what>` hands to `cargo`, one per
/// invocation, in order.
///
/// Most operations are a single invocation. `build` with no profile chosen
/// is two, `cargo build` then `cargo build --release`, matching what
/// `build cargo` and `cargo_build.sh` do; a `--release` or `--profile` pins
/// it to that one profile. `check` turns `fmt` into a verification-only run.
/// `clippy` takes the profile before the `--` that separates cargo's
/// arguments from clippy's own.
pub fn invocations(what: CargoWhat, profile: &Profile, check: bool) -> Vec<Vec<String>> {
    if what == CargoWhat::Build && *profile == Profile::All {
        return vec![
            args(what, &Profile::Named("dev".to_string()), check),
            args(what, &Profile::Release, check),
        ];
    }
    vec![args(what, profile, check)]
}

/// One cargo invocation for `what` under `profile`.
fn args(what: CargoWhat, profile: &Profile, check: bool) -> Vec<String> {
    let base: &[&str] = match what {
        CargoWhat::Build => &["build"],
        CargoWhat::Check => &["check"],
        CargoWhat::Clippy => &["clippy", "--all-targets"],
        CargoWhat::Fmt => &["fmt", "--all"],
        CargoWhat::Test => &["test"],
        CargoWhat::Nextest => &["nextest", "run"],
        CargoWhat::Doc => &["doc", "--no-deps"],
        CargoWhat::Deny => &["deny", "check"],
        CargoWhat::Fetch => &["fetch"],
        CargoWhat::Update => &["update"],
        CargoWhat::Clean => &["clean"],
        CargoWhat::Publish => &["publish"],
    };
    let mut args: Vec<String> = base.iter().map(|s| s.to_string()).collect();
    args.extend(profile.flags());
    if check {
        args.push("--check".to_string());
    }
    if what == CargoWhat::Clippy {
        // Same invocation as the fleet's ci.yml: warnings fail the run.
        args.extend(["--", "-D", "warnings"].map(str::to_string));
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
    profile: &Profile,
    check: bool,
) -> Result<()> {
    for args in invocations(what, profile, check) {
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        check_call_maybe_ve(project, venv, "cargo", &args)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn one(what: CargoWhat, profile: &Profile, check: bool) -> Vec<String> {
        let mut invs = invocations(what, profile, check);
        assert_eq!(
            invs.len(),
            1,
            "{what:?} under {profile:?} is one invocation"
        );
        invs.pop().unwrap()
    }

    #[test]
    fn plain_operations() {
        let all = Profile::All;
        assert_eq!(one(CargoWhat::Check, &all, false), ["check"]);
        assert_eq!(one(CargoWhat::Fmt, &all, false), ["fmt", "--all"]);
        assert_eq!(one(CargoWhat::Test, &all, false), ["test"]);
        assert_eq!(one(CargoWhat::Nextest, &all, false), ["nextest", "run"]);
        assert_eq!(one(CargoWhat::Doc, &all, false), ["doc", "--no-deps"]);
        assert_eq!(one(CargoWhat::Deny, &all, false), ["deny", "check"]);
        assert_eq!(one(CargoWhat::Fetch, &all, false), ["fetch"]);
        assert_eq!(one(CargoWhat::Update, &all, false), ["update"]);
        assert_eq!(one(CargoWhat::Clean, &all, false), ["clean"]);
        assert_eq!(one(CargoWhat::Publish, &all, false), ["publish"]);
    }

    #[test]
    fn bare_build_runs_dev_then_release_like_cargo_build_sh() {
        assert_eq!(
            invocations(CargoWhat::Build, &Profile::All, false),
            [
                vec!["build", "--profile", "dev"],
                vec!["build", "--release"]
            ]
        );
    }

    #[test]
    fn build_with_a_profile_runs_only_that_profile() {
        assert_eq!(
            one(CargoWhat::Build, &Profile::Release, false),
            ["build", "--release"]
        );
        assert_eq!(
            one(CargoWhat::Build, &Profile::Named("dev".into()), false),
            ["build", "--profile", "dev"]
        );
        assert_eq!(
            one(CargoWhat::Build, &Profile::Named("bench".into()), false),
            ["build", "--profile", "bench"]
        );
    }

    #[test]
    fn clippy_matches_ci_and_keeps_profile_before_the_separator() {
        assert_eq!(
            one(CargoWhat::Clippy, &Profile::All, false),
            ["clippy", "--all-targets", "--", "-D", "warnings"]
        );
        assert_eq!(
            one(CargoWhat::Clippy, &Profile::Release, false),
            [
                "clippy",
                "--all-targets",
                "--release",
                "--",
                "-D",
                "warnings"
            ]
        );
        assert_eq!(
            one(CargoWhat::Clippy, &Profile::Named("dev".into()), false),
            [
                "clippy",
                "--all-targets",
                "--profile",
                "dev",
                "--",
                "-D",
                "warnings"
            ]
        );
    }

    #[test]
    fn release_profile_and_check_flags() {
        assert_eq!(
            one(CargoWhat::Nextest, &Profile::Release, false),
            ["nextest", "run", "--release"]
        );
        assert_eq!(
            one(CargoWhat::Test, &Profile::Named("release".into()), false),
            ["test", "--profile", "release"]
        );
        assert_eq!(
            one(CargoWhat::Fmt, &Profile::All, true),
            ["fmt", "--all", "--check"]
        );
    }

    #[test]
    fn profile_from_flags() {
        assert_eq!(Profile::from_flags(false, None), Profile::All);
        assert_eq!(Profile::from_flags(true, None), Profile::Release);
        assert_eq!(
            Profile::from_flags(false, Some("dev")),
            Profile::Named("dev".into())
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
