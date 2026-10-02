// CLI parser limitations we've deliberately left unaddressed are documented in
// docs/src/cli-parser-notes.md — notably, clap 4 has no per-subcommand
// `help_heading`, so the flat alphabetical subcommand listing in `--help` is
// intentional. Read those notes before attempting to "categorize" subcommands.

use std::io;

use clap::{CommandFactory, Parser, Subcommand, ValueEnum};
use clap_complete::{Shell, generate};
use serde::Deserialize;

#[derive(Parser)]
#[command(name = "rsmultigit")]
#[command(version = concat!(env!("CARGO_PKG_VERSION"), " by ", env!("CARGO_PKG_AUTHORS")))]
#[command(about = "Manage multiple git repositories at once")]
#[command(help_template = "\
{about}

Usage: {usage}

Commands:
{subcommands}

Options:
  -h, --help     Print help
  -V, --version  Print version

Use `rsmultigit <command> --help` for more options.")]
pub struct Cli {
    // Output control
    /// Terse output
    #[arg(long, global = true, default_value_t = false)]
    pub terse: bool,

    /// Suppress the [project] header line printed before per-project output
    #[arg(long, global = true, default_value_t = false)]
    pub no_header: bool,

    /// Suppress command output
    #[arg(long, global = true, default_value_t = false)]
    pub no_output: bool,

    /// Verbose output (print all projects, even when no action is taken)
    #[arg(short, long, global = true, default_value_t = false)]
    pub verbose: bool,

    /// Print repos that do NOT match (invert selection)
    #[arg(long, global = true, default_value_t = false)]
    pub print_not: bool,

    /// Do not stop on errors
    #[arg(long, global = true, default_value_t = false)]
    pub no_stop: bool,

    /// Stop at the first negative result instead of processing everything.
    /// Off by default. Honoured by `check-same` and `check-exists`, which stop
    /// at the first broken rule instead of evaluating the remaining ones.
    #[arg(long, global = true, default_value_t = false)]
    pub short_circuit: bool,

    /// Number of parallel workers (default: 1; use 0 for num_cpus)
    #[arg(short = 'j', long, global = true, default_value_t = 1)]
    pub jobs: usize,

    /// Activate each repo's local .venv (prepend .venv/bin to PATH, set
    /// VIRTUAL_ENV) before running tool subprocesses. On by default; honoured
    /// by `run`, `build`, `cargo`, `npm` and `clean make`. Repos without a
    /// .venv run with the environment unchanged. Negate with --no-venv.
    ///
    /// Not honoured by `uv`, which selects its own target environment from
    /// the repo directory and is always run with VIRTUAL_ENV unset.
    #[arg(
        long,
        global = true,
        default_value_t = true,
        overrides_with = "no_venv"
    )]
    pub venv: bool,

    /// Do not activate repos' local .venv before running tool subprocesses
    #[arg(long, global = true, default_value_t = false)]
    pub no_venv: bool,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Show the age of the last commit per repo
    Age,
    /// Show unique commit authors per repo
    Authors,
    /// Run git blame on a file across all repositories
    Blame {
        /// File path to blame
        file: String,
    },
    /// Branch operations
    Branch {
        /// What branch info to show
        #[arg(value_enum)]
        what: BranchWhat,
    },
    /// Build projects
    Build {
        /// What build system to use. Optional when the config file sets
        /// `default_build_method`; a value given here always wins over it.
        #[arg(value_enum)]
        what: Option<BuildWhat>,
    },
    /// Checkout a branch across all repositories
    Checkout {
        /// Branch name to checkout
        branch: String,
    },
    /// Check that files declared in ~/.config/rsmultigit/config.toml are identical across repos
    CheckSame {
        /// Run only the listed check rules (space- or repeat-separated).
        /// Listed names override `enabled = false`. Unknown names are a hard error.
        #[arg(long, num_args = 1.., value_delimiter = ' ')]
        checks: Vec<String>,
        /// Run only check rules whose name matches one of the given regular
        /// expressions (unanchored, like grep; use ^...$ to match a full name).
        /// Combines with --checks. Matched rules override `enabled = false`.
        /// A pattern matching no check name is a hard error.
        #[arg(long, num_args = 1.., value_delimiter = ' ')]
        checks_re: Vec<String>,
        /// Print only failing checks, suppressing the `ok` lines that passing
        /// checks print by default. (--terse implies this: its output is a
        /// machine-readable list of failing rule names.)
        #[arg(long, default_value_t = false)]
        only_failed: bool,
        /// Show a unified diff between representative files of mismatching groups.
        /// With 2 groups this runs automatically; with 3+ groups it prompts interactively
        /// and offers to diff further pairs.
        #[arg(long, default_value_t = false)]
        diff: bool,
        /// Interactively copy one group's content over another's.
        /// Prompts for the "from" and "to" groups, then overwrites every file in the
        /// "to" group with the content of a representative file from the "from" group.
        /// Always exits 0 on success, regardless of whether mismatches remain.
        #[arg(long, default_value_t = false)]
        copy: bool,
        /// Treat a rule that matches no files at all as passing.
        /// By default such a rule fails: catching 0 files almost always means
        /// a stale `select`/`path`, not a healthy check.
        #[arg(long, default_value_t = false)]
        allow_empty: bool,
        /// Interactively create missing files in repos that violate a rule's
        /// `must_have = true` requirement. Prompts for which content group to
        /// seed from, then writes the file (creating parent directories as needed).
        /// Always exits 0 on success.
        #[arg(long, default_value_t = false)]
        fix_missing: bool,
    },
    /// Check that files declared as `[[exists]]` in ~/.config/rsmultigit/config.toml
    /// are present in every repo they apply to. Content is never compared, so this
    /// is the rule type for files that must exist but legitimately differ per repo.
    CheckExists {
        /// Run only the listed exists rules (space- or repeat-separated).
        /// Listed names override `enabled = false`. Unknown names are a hard error.
        #[arg(long, num_args = 1.., value_delimiter = ' ')]
        checks: Vec<String>,
        /// Run only exists rules whose name matches one of the given regular
        /// expressions (unanchored, like grep; use ^...$ to match a full name).
        /// Combines with --checks. Matched rules override `enabled = false`.
        /// A pattern matching no rule name is a hard error.
        #[arg(long, num_args = 1.., value_delimiter = ' ')]
        checks_re: Vec<String>,
        /// Print only failing rules, suppressing the `ok` lines that passing
        /// rules print by default. (--terse implies this.)
        #[arg(long, default_value_t = false)]
        only_failed: bool,
        /// Treat a rule that selects no repos at all as passing.
        /// By default such a rule fails: selecting 0 repos almost always means
        /// a stale `select`, not a healthy check.
        #[arg(long, default_value_t = false)]
        allow_empty: bool,
    },
    /// Run both check-same and check-exists. Exits non-zero if either fails.
    CheckAll {
        /// Print only failing rules from both checks.
        #[arg(long, default_value_t = false)]
        only_failed: bool,
        /// Treat rules that match nothing as passing, in both checks.
        #[arg(long, default_value_t = false)]
        allow_empty: bool,
    },
    /// Clean repositories
    Clean {
        /// What kind of clean to perform
        #[arg(value_enum)]
        what: CleanWhat,
    },
    /// Commit all changes across all repositories
    Commit {
        /// Commit message
        #[arg(short, long)]
        message: String,
    },
    /// Generate shell completion scripts
    Complete {
        /// Shell to generate completions for
        #[arg(value_enum)]
        shell: Shell,
    },
    /// Show a git config value across all repos
    Config {
        /// Git config key to show
        key: String,
    },
    /// Print a sample rsmultigit config.toml to stdout.
    /// Redirect to ~/.config/rsmultigit/config.toml to bootstrap a new install.
    ConfigExample,
    /// Count repositories matching a condition
    Count {
        /// What to count
        #[arg(value_enum)]
        what: CountWhat,
    },
    /// Show diff for all repositories
    Diff,
    /// Show dirty repositories
    Dirty,
    /// Fetch from origin for all repositories
    Fetch,
    /// Run git garbage collection
    Gc,
    /// GitHub operations (via the `gh` CLI) on repos with a github.com remote
    Gh {
        /// What GitHub operation to perform
        #[arg(value_enum)]
        what: GhWhat,
        /// How many recent non-failed deployments/releases/workflow runs to keep
        #[arg(long, default_value_t = 4)]
        keep: usize,
    },
    /// Grep across all repositories
    Grep {
        /// Regular expression to search for
        regexp: String,
        /// Only show filenames
        #[arg(short = 'l', long, default_value_t = false)]
        files: bool,
    },
    /// Show the most recent tag per repo
    LastTag,
    /// Print the path of every configured repo, one per line (no header by default).
    /// Pass --verbose to also emit the [project] header for each entry.
    ListRepos,
    /// Print the name of every rule of one kind defined in the config, one
    /// per line: the `[[check]]` rules by default, the `[[exists]]` rules with
    /// `exists`. Intended for use in shell-completion scripts. All rules are
    /// listed, including those with `enabled = false`.
    ListChecks {
        /// Which rule list to print
        #[arg(value_enum, default_value_t = RuleKind::Check)]
        kind: RuleKind,
    },
    /// Show recent commits
    Log {
        /// Number of commits to show
        #[arg(long, default_value_t = 10)]
        count: u32,
    },
    /// Prune stale remote-tracking branches
    Prune,
    /// Pull all repositories
    Pull {
        /// Pass --quiet to git pull
        #[arg(long, default_value_t = false)]
        quiet: bool,
    },
    /// Push all repositories
    Push,
    /// Show remote URLs
    Remote,
    /// Reset operations
    Reset {
        /// What kind of reset to perform
        #[arg(value_enum)]
        what: ResetWhat,
    },
    /// Run an arbitrary command across all repositories
    #[command(alias = "exec")]
    Run {
        /// Command and arguments to execute
        #[arg(required = true, num_args = 1.., trailing_var_arg = true, allow_hyphen_values = true)]
        command: Vec<String>,
    },
    /// Rust operations on projects that have a Cargo.toml file
    Rust {
        /// What rust operation to perform
        #[arg(value_enum)]
        what: RustWhat,
        /// Release type
        #[arg(long = "type", value_enum, default_value_t = ReleaseType::Patch)]
        release_type: ReleaseType,
    },
    /// Show the size of the .git directory per repo
    Size,
    /// Stash operations
    Stash {
        /// What stash operation to perform
        #[arg(value_enum)]
        what: StashWhat,
    },
    /// Show status of repositories
    Status,
    /// Update submodules recursively
    SubmoduleUpdate,
    /// List tags
    Tag {
        /// What tags to show
        #[arg(value_enum)]
        what: TagWhat,
    },
    /// Run uv operations on projects that have a pyproject.toml file
    Uv {
        /// What uv operation to perform
        #[arg(value_enum)]
        what: UvWhat,
        /// Allow upgrading locked versions (`uv lock --upgrade`).
        /// Only meaningful with `lock`; combining it with any other
        /// operation is an error.
        #[arg(long, default_value_t = false)]
        upgrade: bool,
        /// Assert the lockfile is up to date without writing it
        /// (`uv lock --check`; a stale lockfile is an error).
        /// Only meaningful with `lock`; combining it with any other
        /// operation is an error.
        #[arg(long, default_value_t = false, conflicts_with = "upgrade")]
        check: bool,
    },
    /// Run cargo operations on projects that have a Cargo.toml file
    Cargo {
        /// What cargo operation to perform
        #[arg(value_enum)]
        what: CargoWhat,
        /// Build optimized artifacts (`--release`). Only meaningful with
        /// `build`, `check`, `clippy`, `test`, `nextest` and `doc`;
        /// combining it with any other operation is an error. Without this
        /// or `--profile`, `build` compiles every profile: dev, then release.
        #[arg(long, default_value_t = false, conflicts_with = "profile")]
        release: bool,
        /// Build under one named profile (`--profile <NAME>`: `dev`,
        /// `release`, or a custom profile from Cargo.toml). Same operations
        /// as `--release`; `cargo build --profile dev` builds only dev.
        #[arg(long, value_name = "NAME")]
        profile: Option<String>,
        /// Only check formatting without rewriting files (`cargo fmt --check`;
        /// unformatted code is an error). Only meaningful with `fmt`;
        /// combining it with any other operation is an error.
        #[arg(long, default_value_t = false, conflicts_with_all = ["release", "profile"])]
        check: bool,
    },
    /// Run npm operations on projects that have a package.json file
    Npm {
        /// What npm operation to perform
        #[arg(value_enum)]
        what: NpmWhat,
        /// Also apply the fixes (`npm audit fix`; rewrites package.json and
        /// package-lock.json). Only meaningful with `audit`; combining it
        /// with any other operation is an error.
        #[arg(long, default_value_t = false)]
        fix: bool,
    },
    /// Print version information
    Version,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum RuleKind {
    /// The `[[check]]` rules, consumed by `check-same`
    Check,
    /// The `[[exists]]` rules, consumed by `check-exists`
    Exists,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum GhWhat {
    /// Delete old GitHub deployments, releases, and workflow runs, keeping
    /// only the --keep most recent non-failed of each (failed ones are
    /// always deleted)
    CleanAll,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum UvWhat {
    /// Re-resolve the lockfile from pyproject.toml (`uv lock`; without
    /// --upgrade, already-locked versions are kept)
    Lock,
    /// Sync the project environment from the lockfile (`uv sync`)
    Sync,
    /// Build the sdist and wheel into `dist/` (`uv build`)
    Build,
    /// Upload the distributions in `dist/` to the package index (`uv publish`)
    Publish,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum NpmWhat {
    /// Install dependencies from package.json, writing package-lock.json
    /// (`npm install`)
    Install,
    /// Clean install exactly what package-lock.json says (`npm ci`)
    Ci,
    /// Update dependencies to the newest versions their declared ranges
    /// allow, rewriting package-lock.json (`npm update`)
    Update,
    /// Report known vulnerabilities (`npm audit`; `--fix` also applies
    /// the fixes)
    Audit,
    /// List dependencies with newer releases (`npm outdated`; exits
    /// non-zero when any are outdated)
    Outdated,
    /// Run the test script (`npm test`)
    Test,
    /// Publish the package to the registry (`npm publish`)
    Publish,
}

impl NpmWhat {
    /// Whether `--fix` makes sense for this operation (only `audit`).
    pub fn takes_fix(self) -> bool {
        matches!(self, NpmWhat::Audit)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum CargoWhat {
    /// Compile the project (`cargo build`)
    Build,
    /// Type-check without producing artifacts (`cargo check`)
    Check,
    /// Lint as CI does (`cargo clippy --all-targets -- -D warnings`)
    Clippy,
    /// Format the code (`cargo fmt --all`; `--check` only verifies)
    Fmt,
    /// Run the tests with cargo's built-in runner (`cargo test`)
    Test,
    /// Run the tests with nextest (`cargo nextest run`)
    Nextest,
    /// Build the API docs for the crate itself (`cargo doc --no-deps`)
    Doc,
    /// Check licenses, bans and advisories (`cargo deny check`)
    Deny,
    /// Download dependencies without building (`cargo fetch`)
    Fetch,
    /// Update dependencies (`cargo update`)
    Update,
    /// Remove the target directory (`cargo clean`)
    Clean,
    /// Upload the crate to crates.io (`cargo publish`)
    Publish,
}

impl CargoWhat {
    /// Whether `--release` makes sense for this operation: the ones that
    /// compile the crate under a profile. Mirrors cargo's own flag set.
    pub fn takes_release(self) -> bool {
        matches!(
            self,
            CargoWhat::Build
                | CargoWhat::Check
                | CargoWhat::Clippy
                | CargoWhat::Test
                | CargoWhat::Nextest
                | CargoWhat::Doc
        )
    }

    /// Whether `--check` makes sense for this operation (only `fmt`).
    pub fn takes_check(self) -> bool {
        matches!(self, CargoWhat::Fmt)
    }
}

#[derive(Clone, ValueEnum)]
pub enum CleanWhat {
    /// Hard-clean: remove all untracked and ignored files (git clean -ffxd)
    Hard,
    /// Soft-clean: remove untracked files only (git clean -fd)
    Soft,
    /// Run make clean
    Make,
    /// Discard unstaged working-tree changes (git checkout .)
    Git,
}

#[derive(Clone, ValueEnum)]
pub enum CountWhat {
    /// Count dirty repositories
    Dirty,
    /// Count repositories with untracked files
    Untracked,
    /// Count non-synchronized repositories (ahead of or behind their upstream)
    Synchronized,
}

#[derive(Clone, ValueEnum)]
pub enum BranchWhat {
    /// Show local branches
    Local,
    /// Show remote branches
    Remote,
    /// Show GitHub default branch
    Github,
}

#[derive(Clone, ValueEnum)]
pub enum StashWhat {
    /// Stash working-tree changes (git stash push)
    Push,
    /// Pop the most recent stash (git stash pop)
    Pop,
}

#[derive(Clone, ValueEnum)]
pub enum TagWhat {
    /// Show local tags
    Local,
    /// Show remote tags
    Remote,
    /// Show repos that have local tags
    HasLocal,
    /// Show repos that have remote tags
    HasRemote,
}

#[derive(Clone, ValueEnum)]
pub enum ResetWhat {
    /// Hard reset: discard all changes (git reset --hard HEAD)
    Hard,
    /// Soft reset: keep changes staged (git reset --soft HEAD)
    Soft,
    /// Mixed reset: unstage changes (git reset --mixed HEAD)
    Mixed,
}

#[derive(Clone, ValueEnum)]
pub enum RustWhat {
    /// Release a new version via `cargo release` (bump, commit, tag, push, publish)
    Publish,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum ReleaseType {
    /// Bump the patch version (x.y.Z)
    Patch,
    /// Bump the minor version (x.Y.0)
    Minor,
    /// Bump the major version (X.0.0)
    Major,
}

impl ReleaseType {
    /// The level argument `cargo release` expects.
    pub fn as_str(self) -> &'static str {
        match self {
            ReleaseType::Patch => "patch",
            ReleaseType::Minor => "minor",
            ReleaseType::Major => "major",
        }
    }
}

/// Also deserializable so `default_build_method = "rsconstruct"` in
/// ~/.config/rsmultigit/config.toml accepts exactly the spellings the
/// command line does (kebab-case, as `rsconstruct` and friends are).
#[derive(Clone, Debug, PartialEq, Eq, ValueEnum, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum BuildWhat {
    /// Run bootstrap across all projects
    Bootstrap,
    /// Run make across all projects
    Make,
    /// Run rsconstruct build on projects that have an rsconstruct.toml file
    Rsconstruct,
    /// Run cargo build on projects that have a Cargo.toml file
    Cargo,
}

impl BuildWhat {
    /// The command-line spellings of every method, in declaration order,
    /// for usage messages. Derived from the enum so it cannot drift.
    pub fn names() -> Vec<String> {
        Self::value_variants()
            .iter()
            .filter_map(|v| v.to_possible_value())
            .map(|p| p.get_name().to_string())
            .collect()
    }
}

/// Generate shell completions and print to stdout.
pub fn print_completions(shell: Shell) {
    let mut cmd = Cli::command();
    generate(shell, &mut cmd, "rsmultigit", &mut io::stdout());

    // Append a dynamic extension that completes `check-same --checks <names>`
    // and `check-exists --checks <names>` against `rsmultigit list-checks`
    // (`list-checks exists` for the latter), which reads the user's config
    // file. clap_complete only knows about static ValueEnum choices, so
    // --checks (free-form names from the config) needs runtime help.
    match shell {
        Shell::Bash => print!("{}", CHECKS_COMPLETION_BASH),
        Shell::Zsh => print!("{}", CHECKS_COMPLETION_ZSH),
        _ => {}
    }
}

/// Bash snippet appended to `rsmultigit complete bash`. Wraps clap's generated
/// `_rsmultigit` function so that tabbing after `check-same --checks` or
/// `check-exists --checks` completes the rule names returned by
/// `rsmultigit list-checks` / `rsmultigit list-checks exists`.
const CHECKS_COMPLETION_BASH: &str = r#"
# rsmultigit: dynamic --checks completion (appended by `rsmultigit complete bash`)
if declare -F _rsmultigit >/dev/null; then
    eval "$(declare -f _rsmultigit | sed '1 s/^_rsmultigit /_rsmultigit_clap /')"

    _rsmultigit() {
        local i cur prev
        cur="${COMP_WORDS[COMP_CWORD]}"
        prev="${COMP_WORDS[COMP_CWORD-1]}"

        local in_checks=0
        local kind=""
        for ((i=1; i<COMP_CWORD; i++)); do
            local w="${COMP_WORDS[i]}"
            case "$w" in
                check-same)   kind=check ;;
                check-exists) kind=exists ;;
                --checks)     in_checks=1 ;;
                --*)          in_checks=0 ;;
            esac
        done
        if [[ "$prev" == "--checks" ]]; then
            in_checks=1
        fi

        if [[ -n "$kind" ]] && (( in_checks )); then
            local names
            names=$(rsmultigit list-checks "$kind" 2>/dev/null)
            if [[ -n "$names" ]]; then
                # shellcheck disable=SC2207
                COMPREPLY=($(compgen -W "$names" -- "$cur"))
                return 0
            fi
        fi
        # Bash passes (cmd_name, current_word, previous_word) as "$@" to the
        # completion function; clap's generated code reads them via $2/$3, so
        # we must forward all args intact.
        _rsmultigit_clap "$@"
    }
fi
"#;

/// Zsh equivalent of the bash snippet above.
const CHECKS_COMPLETION_ZSH: &str = r#"
# rsmultigit: dynamic --checks completion (appended by `rsmultigit complete zsh`)
if (( ${+functions[_rsmultigit]} )); then
    functions[_rsmultigit_clap]="${functions[_rsmultigit]}"

    _rsmultigit() {
        local prev=${words[$CURRENT-1]}
        local seen_checks=0
        local kind=""
        local i
        for ((i=1; i<CURRENT; i++)); do
            case "${words[i]}" in
                check-same)   kind=check ;;
                check-exists) kind=exists ;;
                --checks)     seen_checks=1 ;;
                --*)          seen_checks=0 ;;
            esac
        done
        if [[ "$prev" == "--checks" ]]; then
            seen_checks=1
        fi

        if [[ -n "$kind" ]] && (( seen_checks )); then
            local -a names
            names=(${(f)"$(rsmultigit list-checks "$kind" 2>/dev/null)"})
            if (( ${#names} )); then
                _describe 'check name' names
                return 0
            fi
        fi
        _rsmultigit_clap "$@"
    }
fi
"#;

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Cli {
        Cli::parse_from(args)
    }

    #[test]
    fn parse_count_dirty() {
        let cli = parse(&["rsmultigit", "count", "dirty"]);
        assert!(matches!(
            cli.command,
            Commands::Count {
                what: CountWhat::Dirty
            }
        ));
    }

    #[test]
    fn parse_all_subcommands() {
        let subcommands = [
            "status",
            "dirty",
            "list-repos",
            "age",
            "authors",
            "size",
            "last-tag",
            "pull",
            "push",
            "fetch",
            "diff",
            "remote",
            "prune",
            "gc",
            "submodule-update",
            "version",
        ];
        for sub in subcommands {
            let result = Cli::try_parse_from(["rsmultigit", sub]);
            assert!(result.is_ok(), "subcommand {sub} should parse");
        }

        // clean requires a what argument
        let clean_whats = ["hard", "soft", "make", "git"];
        for what in clean_whats {
            let result = Cli::try_parse_from(["rsmultigit", "clean", what]);
            assert!(result.is_ok(), "clean {what} should parse");
        }

        // count requires a what argument
        let count_whats = ["dirty", "untracked", "synchronized"];
        for what in count_whats {
            let result = Cli::try_parse_from(["rsmultigit", "count", what]);
            assert!(result.is_ok(), "count {what} should parse");
        }

        // branch requires a what argument
        let branch_whats = ["local", "remote", "github"];
        for what in branch_whats {
            let result = Cli::try_parse_from(["rsmultigit", "branch", what]);
            assert!(result.is_ok(), "branch {what} should parse");
        }

        // tag requires a what argument
        let tag_whats = ["local", "remote", "has-local", "has-remote"];
        for what in tag_whats {
            let result = Cli::try_parse_from(["rsmultigit", "tag", what]);
            assert!(result.is_ok(), "tag {what} should parse");
        }

        // reset requires a what argument
        let reset_whats = ["hard", "soft", "mixed"];
        for what in reset_whats {
            let result = Cli::try_parse_from(["rsmultigit", "reset", what]);
            assert!(result.is_ok(), "reset {what} should parse");
        }

        // log accepts optional --count
        let result = Cli::try_parse_from(["rsmultigit", "log"]);
        assert!(result.is_ok(), "log should parse without args");
        let result = Cli::try_parse_from(["rsmultigit", "log", "--count", "5"]);
        assert!(result.is_ok(), "log --count 5 should parse");

        // checkout requires a branch
        let result = Cli::try_parse_from(["rsmultigit", "checkout", "main"]);
        assert!(result.is_ok(), "checkout main should parse");

        // commit requires -m
        let result = Cli::try_parse_from(["rsmultigit", "commit", "-m", "test"]);
        assert!(result.is_ok(), "commit -m test should parse");

        // config requires a key
        let result = Cli::try_parse_from(["rsmultigit", "config", "user.email"]);
        assert!(result.is_ok(), "config user.email should parse");

        // blame requires a file
        let result = Cli::try_parse_from(["rsmultigit", "blame", "README.md"]);
        assert!(result.is_ok(), "blame README.md should parse");

        // stash requires a what argument
        let stash_whats = ["push", "pop"];
        for what in stash_whats {
            let result = Cli::try_parse_from(["rsmultigit", "stash", what]);
            assert!(result.is_ok(), "stash {what} should parse");
        }

        // build takes an optional what argument (falls back to the config
        // file's `default_build_method` when omitted)
        let build_whats = ["bootstrap", "make", "rsconstruct", "cargo"];
        for what in build_whats {
            let result = Cli::try_parse_from(["rsmultigit", "build", what]);
            assert!(result.is_ok(), "build {what} should parse");
        }
        // publishing and cleaning moved under `cargo`
        assert!(Cli::try_parse_from(["rsmultigit", "build", "cargo-publish"]).is_err());
        assert!(Cli::try_parse_from(["rsmultigit", "clean", "cargo"]).is_err());
        let result = Cli::try_parse_from(["rsmultigit", "build"]);
        assert!(result.is_ok(), "build without a method should parse");

        // rust requires a what argument
        let rust_whats = ["publish"];
        for what in rust_whats {
            let result = Cli::try_parse_from(["rsmultigit", "rust", what]);
            assert!(result.is_ok(), "rust {what} should parse");
        }

        // uv requires a what argument; --upgrade parses with lock
        let uv_whats = ["lock", "sync", "build", "publish"];
        for what in uv_whats {
            let result = Cli::try_parse_from(["rsmultigit", "uv", what]);
            assert!(result.is_ok(), "uv {what} should parse");
        }
        let result = Cli::try_parse_from(["rsmultigit", "uv", "lock", "--upgrade"]);
        assert!(result.is_ok(), "uv lock --upgrade should parse");
        let result = Cli::try_parse_from(["rsmultigit", "uv", "lock", "--check"]);
        assert!(result.is_ok(), "uv lock --check should parse");
        let result = Cli::try_parse_from(["rsmultigit", "uv"]);
        assert!(result.is_err(), "uv without a what should not parse");

        // gh requires a what argument
        let gh_whats = ["clean-all"];
        for what in gh_whats {
            let result = Cli::try_parse_from(["rsmultigit", "gh", what]);
            assert!(result.is_ok(), "gh {what} should parse");
        }
        let result = Cli::try_parse_from(["rsmultigit", "gh"]);
        assert!(result.is_err(), "gh without a what should not parse");

        // cargo requires a what argument
        let cargo_whats = [
            "build", "check", "clippy", "fmt", "test", "nextest", "doc", "deny", "fetch", "update",
            "clean", "publish",
        ];
        for what in cargo_whats {
            let result = Cli::try_parse_from(["rsmultigit", "cargo", what]);
            assert!(result.is_ok(), "cargo {what} should parse");
        }
        let result = Cli::try_parse_from(["rsmultigit", "cargo", "build", "--release"]);
        assert!(result.is_ok(), "cargo build --release should parse");
        let result = Cli::try_parse_from(["rsmultigit", "cargo", "build", "--profile", "dev"]);
        assert!(result.is_ok(), "cargo build --profile dev should parse");
        // --release, --profile and --check are mutually exclusive
        let result = Cli::try_parse_from([
            "rsmultigit",
            "cargo",
            "build",
            "--release",
            "--profile",
            "dev",
        ]);
        assert!(result.is_err(), "--release and --profile should conflict");
        let result =
            Cli::try_parse_from(["rsmultigit", "cargo", "fmt", "--check", "--profile", "dev"]);
        assert!(result.is_err(), "--check and --profile should conflict");
        let result = Cli::try_parse_from(["rsmultigit", "cargo", "fmt", "--check"]);
        assert!(result.is_ok(), "cargo fmt --check should parse");
        let result = Cli::try_parse_from(["rsmultigit", "cargo"]);
        assert!(result.is_err(), "cargo without a what should not parse");

        // npm requires a what argument; --fix parses with audit
        let npm_whats = [
            "install", "ci", "update", "audit", "outdated", "test", "publish",
        ];
        for what in npm_whats {
            let result = Cli::try_parse_from(["rsmultigit", "npm", what]);
            assert!(result.is_ok(), "npm {what} should parse");
        }
        let result = Cli::try_parse_from(["rsmultigit", "npm", "audit", "--fix"]);
        assert!(result.is_ok(), "npm audit --fix should parse");
        let result = Cli::try_parse_from(["rsmultigit", "npm"]);
        assert!(result.is_err(), "npm without a what should not parse");

        // complete requires an argument
        let complete_shells = ["bash", "zsh", "fish", "elvish", "powershell"];
        for shell in complete_shells {
            let result = Cli::try_parse_from(["rsmultigit", "complete", shell]);
            assert!(result.is_ok(), "complete {shell} should parse");
        }
    }

    #[test]
    fn parse_list_checks_kind() {
        let cli = parse(&["rsmultigit", "list-checks"]);
        assert!(matches!(
            cli.command,
            Commands::ListChecks {
                kind: RuleKind::Check
            }
        ));
        let cli = parse(&["rsmultigit", "list-checks", "exists"]);
        assert!(matches!(
            cli.command,
            Commands::ListChecks {
                kind: RuleKind::Exists
            }
        ));
        assert!(Cli::try_parse_from(["rsmultigit", "list-checks", "both"]).is_err());
    }

    #[test]
    fn completion_snippets_know_both_check_commands() {
        for snippet in [CHECKS_COMPLETION_BASH, CHECKS_COMPLETION_ZSH] {
            assert!(snippet.contains("check-same)"));
            assert!(snippet.contains("check-exists)"));
            assert!(snippet.contains(r#"list-checks "$kind""#));
        }
    }

    #[test]
    fn parse_complete_bash() {
        let cli = parse(&["rsmultigit", "complete", "bash"]);
        match &cli.command {
            Commands::Complete { shell } => {
                assert!(matches!(shell, Shell::Bash));
            }
            _ => panic!("expected Complete"),
        }
    }

    #[test]
    fn parse_grep_with_regexp() {
        let cli = parse(&["rsmultigit", "grep", "TODO"]);
        match &cli.command {
            Commands::Grep { regexp, files } => {
                assert_eq!(regexp, "TODO");
                assert!(!files);
            }
            _ => panic!("expected Grep"),
        }
    }

    #[test]
    fn parse_grep_with_files_flag() {
        let cli = parse(&["rsmultigit", "grep", "--files", "TODO"]);
        match &cli.command {
            Commands::Grep { regexp, files } => {
                assert_eq!(regexp, "TODO");
                assert!(files);
            }
            _ => panic!("expected Grep"),
        }
    }

    #[test]
    fn parse_grep_with_short_files_flag() {
        let cli = parse(&["rsmultigit", "grep", "-l", "TODO"]);
        match &cli.command {
            Commands::Grep { regexp, files } => {
                assert_eq!(regexp, "TODO");
                assert!(files);
            }
            _ => panic!("expected Grep"),
        }
    }

    #[test]
    fn parse_pull_quiet() {
        let cli = parse(&["rsmultigit", "pull", "--quiet"]);
        match &cli.command {
            Commands::Pull { quiet } => assert!(quiet),
            _ => panic!("expected Pull"),
        }
    }

    #[test]
    fn parse_global_flags() {
        let cli = parse(&[
            "rsmultigit",
            "--terse",
            "--no-output",
            "--verbose",
            "--print-not",
            "--no-stop",
            "--short-circuit",
            "count",
            "dirty",
        ]);
        assert!(cli.terse);
        assert!(cli.no_output);
        assert!(cli.verbose);
        assert!(cli.print_not);
        assert!(cli.no_stop);
        assert!(cli.short_circuit);
    }

    #[test]
    fn venv_defaults_to_on() {
        let cli = parse(&["rsmultigit", "build", "rsconstruct"]);
        assert!(cli.venv);
        assert!(!cli.no_venv);
    }

    #[test]
    fn parse_no_venv() {
        let cli = parse(&["rsmultigit", "--no-venv", "build", "rsconstruct"]);
        assert!(cli.no_venv);
        // Global flag also parses after the subcommand.
        let cli = parse(&["rsmultigit", "uv", "sync", "--no-venv"]);
        assert!(cli.no_venv);
    }

    #[test]
    fn parse_venv_overrides_no_venv() {
        // Last one wins: --no-venv --venv ends up with venv activation on.
        let cli = parse(&["rsmultigit", "--no-venv", "--venv", "run", "true"]);
        assert!(cli.venv);
        assert!(!cli.no_venv);
    }

    #[test]
    fn parse_build_without_method() {
        let cli = parse(&["rsmultigit", "build"]);
        assert!(matches!(cli.command, Commands::Build { what: None }));
        let cli = parse(&["rsmultigit", "build", "rsconstruct"]);
        assert!(matches!(
            cli.command,
            Commands::Build {
                what: Some(BuildWhat::Rsconstruct)
            }
        ));
    }

    #[test]
    fn build_what_deserializes_with_cli_spellings() {
        #[derive(Deserialize)]
        struct Probe {
            what: BuildWhat,
        }
        for (text, want) in [
            ("bootstrap", BuildWhat::Bootstrap),
            ("make", BuildWhat::Make),
            ("rsconstruct", BuildWhat::Rsconstruct),
            ("cargo", BuildWhat::Cargo),
        ] {
            let probe: Probe = toml::from_str(&format!("what = \"{text}\"")).unwrap();
            assert_eq!(probe.what, want, "{text}");
        }
        assert!(toml::from_str::<Probe>("what = \"Rsconstruct\"").is_err());
        assert!(toml::from_str::<Probe>("what = \"cargo-publish\"").is_err());
        assert!(toml::from_str::<Probe>("what = \"ninja\"").is_err());
    }

    #[test]
    fn build_what_names_are_the_cli_spellings() {
        assert_eq!(
            BuildWhat::names(),
            ["bootstrap", "make", "rsconstruct", "cargo"]
        );
    }

    #[test]
    fn parse_removed_venv_variants_fail() {
        // These were folded into the default-on --venv flag.
        assert!(Cli::try_parse_from(["rsmultigit", "build", "venv-rsconstruct"]).is_err());
        assert!(Cli::try_parse_from(["rsmultigit", "build", "venv-make"]).is_err());
        assert!(Cli::try_parse_from(["rsmultigit", "uv", "venv-sync"]).is_err());
    }

    #[test]
    fn short_circuit_defaults_to_off() {
        let cli = parse(&["rsmultigit", "check-same"]);
        assert!(!cli.short_circuit);
    }

    #[test]
    fn parse_jobs_flag() {
        let cli = parse(&["rsmultigit", "-j", "4", "list-repos"]);
        assert_eq!(cli.jobs, 4);
        let cli = parse(&["rsmultigit", "--jobs", "8", "list-repos"]);
        assert_eq!(cli.jobs, 8);
    }

    #[test]
    fn default_jobs_is_one() {
        let cli = parse(&["rsmultigit", "list-repos"]);
        assert_eq!(cli.jobs, 1);
    }

    #[test]
    fn parse_uv_lock_check() {
        let cli = parse(&["rsmultigit", "uv", "lock", "--check"]);
        match &cli.command {
            Commands::Uv {
                what,
                upgrade,
                check,
            } => {
                assert_eq!(*what, UvWhat::Lock);
                assert!(!*upgrade);
                assert!(*check);
            }
            _ => panic!("expected Uv"),
        }
    }

    #[test]
    fn parse_uv_lock_check_defaults_to_off() {
        let cli = parse(&["rsmultigit", "uv", "lock"]);
        match &cli.command {
            Commands::Uv { check, .. } => assert!(!*check),
            _ => panic!("expected Uv"),
        }
    }

    #[test]
    fn parse_uv_lock_check_conflicts_with_upgrade() {
        let result = Cli::try_parse_from(["rsmultigit", "uv", "lock", "--check", "--upgrade"]);
        assert!(result.is_err(), "--check and --upgrade should conflict");
    }

    #[test]
    fn parse_npm_audit_fix() {
        let cli = parse(&["rsmultigit", "npm", "audit", "--fix"]);
        match &cli.command {
            Commands::Npm { what, fix } => {
                assert_eq!(*what, NpmWhat::Audit);
                assert!(*fix);
            }
            _ => panic!("expected Npm"),
        }
        let cli = parse(&["rsmultigit", "npm", "install"]);
        match &cli.command {
            Commands::Npm { what, fix } => {
                assert_eq!(*what, NpmWhat::Install);
                assert!(!*fix);
            }
            _ => panic!("expected Npm"),
        }
    }

    #[test]
    fn npm_fix_applies_to_audit_only() {
        for what in [
            NpmWhat::Install,
            NpmWhat::Ci,
            NpmWhat::Update,
            NpmWhat::Outdated,
            NpmWhat::Test,
            NpmWhat::Publish,
        ] {
            assert!(!what.takes_fix(), "{what:?} should not take --fix");
        }
        assert!(NpmWhat::Audit.takes_fix());
    }

    #[test]
    fn parse_gh_clean_all_defaults_to_keep_4() {
        let cli = parse(&["rsmultigit", "gh", "clean-all"]);
        match &cli.command {
            Commands::Gh { what, keep } => {
                assert!(matches!(what, GhWhat::CleanAll));
                assert_eq!(*keep, 4);
            }
            _ => panic!("expected Gh"),
        }
    }

    #[test]
    fn parse_gh_clean_all_with_keep() {
        let cli = parse(&["rsmultigit", "gh", "clean-all", "--keep", "10"]);
        match &cli.command {
            Commands::Gh { keep, .. } => assert_eq!(*keep, 10),
            _ => panic!("expected Gh"),
        }
    }

    #[test]
    fn parse_gh_bad_keep_fails() {
        let result = Cli::try_parse_from(["rsmultigit", "gh", "clean-all", "--keep", "many"]);
        assert!(result.is_err());
    }

    #[test]
    fn parse_rust_publish_defaults_to_patch() {
        let cli = parse(&["rsmultigit", "rust", "publish"]);
        match &cli.command {
            Commands::Rust { what, release_type } => {
                assert!(matches!(what, RustWhat::Publish));
                assert_eq!(*release_type, ReleaseType::Patch);
            }
            _ => panic!("expected Rust"),
        }
    }

    #[test]
    fn parse_rust_publish_with_type() {
        for (arg, expected) in [
            ("patch", ReleaseType::Patch),
            ("minor", ReleaseType::Minor),
            ("major", ReleaseType::Major),
        ] {
            let cli = parse(&["rsmultigit", "rust", "publish", "--type", arg]);
            match &cli.command {
                Commands::Rust { release_type, .. } => {
                    assert_eq!(*release_type, expected);
                }
                _ => panic!("expected Rust"),
            }
        }
    }

    #[test]
    fn parse_rust_publish_bad_type_fails() {
        let result = Cli::try_parse_from(["rsmultigit", "rust", "publish", "--type", "huge"]);
        assert!(result.is_err());
    }

    #[test]
    fn parse_run_command() {
        let cli = parse(&["rsmultigit", "run", "git", "status"]);
        match &cli.command {
            Commands::Run { command } => {
                assert_eq!(command, &vec!["git", "status"]);
            }
            _ => panic!("expected Run"),
        }
    }

    #[test]
    fn parse_exec_command_alias() {
        let cli = parse(&["rsmultigit", "exec", "ls", "-la"]);
        match &cli.command {
            Commands::Run { command } => {
                assert_eq!(command, &vec!["ls", "-la"]);
            }
            _ => panic!("expected Run"),
        }
    }

    #[test]
    fn unknown_subcommand_fails() {
        let result = Cli::try_parse_from(["rsmultigit", "nonexistent"]);
        assert!(result.is_err());
    }
}
