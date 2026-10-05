// CLI parser limitations we've deliberately left unaddressed are documented in
// docs/src/cli-parser-notes.md — notably, clap 4 has no per-subcommand
// `help_heading`, so the flat alphabetical subcommand listing in `--help` is
// intentional. Read those notes before attempting to "categorize" subcommands.

use std::io;

use clap::builder::StyledStr;
use clap::builder::styling::{AnsiColor, Effects, Styles};

use clap::{CommandFactory, Parser, Subcommand, ValueEnum};
use clap_complete::{Shell, generate};
use serde::Deserialize;

/// Colours for every help and error message clap renders (cargo's palette).
/// clap drops them by itself when the stream is not a terminal or `NO_COLOR`
/// is set; text we print ourselves goes through `anstream` for the same.
pub const STYLES: Styles = Styles::styled()
    .header(AnsiColor::Green.on_default().effects(Effects::BOLD))
    .usage(AnsiColor::Green.on_default().effects(Effects::BOLD))
    .literal(AnsiColor::Cyan.on_default().effects(Effects::BOLD))
    .placeholder(AnsiColor::Cyan.on_default())
    .error(AnsiColor::Red.on_default().effects(Effects::BOLD))
    .valid(AnsiColor::Cyan.on_default().effects(Effects::BOLD))
    .invalid(AnsiColor::Yellow.on_default().effects(Effects::BOLD));

/// The top-level help: the global flags stay out of it (they would bury the
/// command list), so the Options section is written out by hand, styled the
/// way clap styles the sections it renders itself.
fn top_help_template() -> StyledStr {
    let h = STYLES.get_header();
    let l = STYLES.get_literal();
    format!(
        "\
{{about}}

{{usage-heading}} {{usage}}

{h}Commands:{h:#}
{{subcommands}}

{h}Options:{h:#}
  {l}-h{l:#}, {l}--help{l:#}     Print help
  {l}-V{l:#}, {l}--version{l:#}  Print version

Use `rsmultigit <command> --help` for more options."
    )
    .into()
}

#[derive(Parser)]
#[command(name = "rsmultigit")]
#[command(version = concat!(env!("CARGO_PKG_VERSION"), " by ", env!("CARGO_PKG_AUTHORS")))]
#[command(about = "Manage multiple git repositories at once")]
#[command(styles = STYLES)]
#[command(help_template = top_help_template())]
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

    /// Do not stop on errors: report each failing repo and carry on, then
    /// exit non-zero if any repo failed
    #[arg(long, global = true, default_value_t = false)]
    pub no_stop: bool,

    /// Stop at the first negative result instead of processing everything.
    /// Off by default. Honoured by `check same` and `check exists`, which stop
    /// at the first broken rule instead of evaluating the remaining ones.
    #[arg(long, global = true, default_value_t = false)]
    pub short_circuit: bool,

    /// Number of parallel workers (default: 1; use 0 for num_cpus)
    #[arg(short = 'j', long, global = true, default_value_t = 1)]
    pub jobs: usize,

    /// Activate each repo's local .venv (prepend .venv/bin to PATH, set
    /// VIRTUAL_ENV) before running tool subprocesses. On by default; honoured
    /// by `run`, `build`, `cargo` and `npm`. Repos without a
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

// Every variant whose required positional picks the operation (`gh <WHAT>`,
// `cargo <WHAT>`, ...) carries `arg_required_else_help`, so invoking the
// command bare prints its help instead of clap's "required arguments were not
// provided" error. The error names only `<WHAT>`; the help lists the choices.
// The same goes for the groups whose operations are subcommands (`git`,
// `check`, `setup`). `main` upgrades that help to the long form (see
// `long_help_for`), so each choice comes with its one-line description.
#[derive(Subcommand)]
pub enum Commands {
    /// Build projects
    Build {
        /// What build system to use. Optional when the config file sets
        /// `default_build_method`; a value given here always wins over it.
        #[arg(value_enum)]
        what: Option<BuildWhat>,
    },
    /// Run cargo operations on projects that have a Cargo.toml file
    #[command(arg_required_else_help = true)]
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
        /// Which version component `release` bumps (default: patch). Only
        /// meaningful with `release`; combining it with any other operation
        /// is an error.
        #[arg(long = "type", value_enum, value_name = "TYPE")]
        release_type: Option<ReleaseType>,
    },
    /// Check the invariants declared in ~/.config/rsmultigit/config.toml
    #[command(arg_required_else_help = true, disable_help_subcommand = true)]
    Check {
        #[command(subcommand)]
        command: CheckCommand,
    },
    /// Generate shell completion scripts
    Complete {
        /// Shell to generate completions for
        #[arg(value_enum)]
        shell: Shell,
    },
    /// GitHub operations (via the `gh` CLI) on repos with a github.com remote
    #[command(arg_required_else_help = true)]
    Gh {
        /// What GitHub operation to perform
        #[arg(value_enum)]
        what: GhWhat,
        /// How many recent non-failed deployments/releases/workflow runs to keep
        /// (`clean-all` only; default 4)
        #[arg(long, value_name = "N")]
        keep: Option<usize>,
        /// Print what differs but change nothing on GitHub (`sync-metadata` only)
        #[arg(long, default_value_t = false)]
        dry_run: bool,
    },
    /// Git inspection and operations across all repositories
    #[command(arg_required_else_help = true, disable_help_subcommand = true)]
    Git {
        #[command(subcommand)]
        command: GitCommand,
    },
    /// Print the paths of the configured repos, all of them or those in a given state
    #[command(arg_required_else_help = true, disable_help_subcommand = true)]
    List {
        #[command(subcommand)]
        command: ListCommand,
    },
    /// Run npm operations on projects that have a package.json file
    #[command(arg_required_else_help = true)]
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
    /// Run an arbitrary command across all repositories
    #[command(alias = "exec")]
    Run {
        /// Command and arguments to execute
        #[arg(required = true, num_args = 1.., trailing_var_arg = true, allow_hyphen_values = true)]
        command: Vec<String>,
    },
    /// Create the ~/.config/rsmultigit/config.toml a new install needs
    #[command(arg_required_else_help = true, disable_help_subcommand = true)]
    Setup {
        #[command(subcommand)]
        command: SetupCommand,
    },
    /// Show status of repositories (shortcut for `git status`)
    Status,
    /// Run uv operations on projects that have a pyproject.toml file
    #[command(arg_required_else_help = true)]
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
    /// Print version information
    Version,
}

/// The `check` subcommands: each evaluates one kind of rule from the config
/// file. Organised by rule rather than by repo, so they bypass the runners.
#[derive(Subcommand)]
pub enum CheckCommand {
    /// Check that files declared in ~/.config/rsmultigit/config.toml are identical across repos
    Same {
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
    Exists {
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
    /// Run both `check same` and `check exists`. Exits non-zero if either fails.
    All {
        /// Print only failing rules from both checks.
        #[arg(long, default_value_t = false)]
        only_failed: bool,
        /// Treat rules that match nothing as passing, in both checks.
        #[arg(long, default_value_t = false)]
        allow_empty: bool,
    },
    /// Print the name of every rule of one kind defined in the config, one
    /// per line: the `[[check]]` rules by default, the `[[exists]]` rules with
    /// `exists`. Intended for use in shell-completion scripts. All rules are
    /// listed, including those with `enabled = false`.
    List {
        /// Which rule list to print
        #[arg(value_enum, default_value_t = RuleKind::Check)]
        kind: RuleKind,
    },
}

/// The `list` subcommands: each prints the absolute path of every repo it
/// selects, one per line and with no header, so the output feeds straight
/// into `xargs` or a shell loop. The path is the data, so a bracketed header
/// would only repeat it; `--verbose` adds it anyway on `list repos`, and
/// `--print-not` inverts the selection of the state filters.
#[derive(Subcommand, Clone, Copy)]
pub enum ListCommand {
    /// Print the path of every configured repo
    ///
    /// Pass --verbose to also emit the [project] header for each entry.
    Repos,
    /// Print repos with modified, deleted, staged or conflicted files
    Dirty,
    /// Print repos with untracked files
    Untracked,
    /// Print repos whose branch is ahead of or behind its upstream
    Unsynchronized,
    /// Print repos with local commits not yet pushed to the upstream
    Ahead,
    /// Print repos with upstream commits not yet pulled
    Behind,
}

/// The `setup` subcommands: both run before any config file exists, since
/// producing one is their whole point.
#[derive(Subcommand)]
pub enum SetupCommand {
    /// Write a first ~/.config/rsmultigit/config.toml for a new install.
    /// Asks which directory holds the git repositories (with tab completion)
    /// and which build tool a bare `rsmultigit build` should run, checks that
    /// the directory really contains git repos, and writes the config. Each
    /// question is skipped when its answer is given as an option; with every
    /// answer given, nothing is asked, so the command works in scripts.
    Interactive {
        /// Directory whose direct subdirectories are the git repositories
        /// (answers the first question)
        #[arg(long, value_name = "DIR")]
        repos_dir: Option<String>,
        /// Default build method for a bare `rsmultigit build` (answers the
        /// second question)
        #[arg(long, value_enum, value_name = "METHOD", conflicts_with = "no_build")]
        build: Option<BuildWhat>,
        /// Set no default build method (answers the second question)
        #[arg(long, default_value_t = false)]
        no_build: bool,
        /// Replace an existing config file without asking
        #[arg(long, default_value_t = false)]
        overwrite: bool,
    },
    /// Print a sample rsmultigit config.toml to stdout.
    /// Redirect to ~/.config/rsmultigit/config.toml to bootstrap a new install.
    ConfigSample,
}

/// The `git` subcommands, in two kinds. The reports (`status`, `dirty`,
/// `count`, `age`, `authors`, `size`, `last-tag`) inspect each repo through
/// libgit2 and print a summary. The rest run the git command of the same
/// name in every repo.
#[derive(Subcommand)]
pub enum GitCommand {
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
    #[command(arg_required_else_help = true)]
    Branch {
        /// What branch info to show
        #[arg(value_enum)]
        what: BranchWhat,
    },
    /// Checkout a branch across all repositories
    Checkout {
        /// Branch name to checkout
        branch: String,
    },
    /// Remove untracked files (git clean); repos with nothing to remove
    /// are skipped
    #[command(arg_required_else_help = true)]
    Clean {
        /// What kind of clean to perform
        #[arg(value_enum)]
        what: CleanWhat,
        /// List what would be removed in each repo, removing nothing
        #[arg(long, default_value_t = false)]
        dry_run: bool,
    },
    /// Commit all changes across all repositories
    Commit {
        /// Commit message
        #[arg(short, long)]
        message: String,
    },
    /// Show a git config value across all repos
    Config {
        /// Git config key to show
        key: String,
    },
    /// Count repositories matching a condition
    #[command(arg_required_else_help = true)]
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
    /// Show recent commits
    Log {
        /// Number of commits to show
        #[arg(long, default_value_t = 10)]
        count: u32,
    },
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
    // Not named `prune`: `git prune` is git's object pruning, a different thing.
    /// Prune stale remote-tracking branches (git remote prune origin)
    RemotePrune,
    /// Reset operations; repos where the reset would change nothing are
    /// skipped
    #[command(arg_required_else_help = true)]
    Reset {
        /// What kind of reset to perform
        #[arg(value_enum)]
        what: ResetWhat,
        /// List what would be discarded or unstaged in each repo, changing
        /// nothing
        #[arg(long, default_value_t = false)]
        dry_run: bool,
    },
    /// Discard unstaged changes to tracked files (git restore .); repos
    /// without any are skipped
    Restore {
        /// List what would be discarded in each repo, changing nothing
        #[arg(long, default_value_t = false)]
        dry_run: bool,
    },
    /// Show the size of the .git directory per repo
    Size,
    /// Stash operations
    #[command(arg_required_else_help = true)]
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
    #[command(arg_required_else_help = true)]
    Tag {
        /// What tags to show
        #[arg(value_enum)]
        what: TagWhat,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum RuleKind {
    /// The `[[check]]` rules, consumed by `check same`
    Check,
    /// The `[[exists]]` rules, consumed by `check exists`
    Exists,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum GhWhat {
    /// Delete old GitHub deployments, releases, and workflow runs, keeping
    /// only the --keep most recent non-failed of each (failed ones are
    /// always deleted)
    CleanAll,
    /// List the assets of the latest GitHub release (name, size, downloads)
    Artifacts,
    /// Print the conclusion of the most recent workflow run
    LastWorkflowState,
    /// Open the repo's GitHub Pages site in the browser (`xdg-open`)
    OpenSite,
    /// Sync GitHub description, topics and feature policy (wiki off,
    /// issues on, projects off) from config/project.lua, printing only
    /// what differs
    SyncMetadata,
}

impl GhWhat {
    /// Whether `--dry-run` makes sense for this operation (only
    /// `sync-metadata`). A dry run that silently went ahead and deleted
    /// things would be worse than a usage error, so the others reject it.
    pub fn takes_dry_run(self) -> bool {
        matches!(self, GhWhat::SyncMetadata)
    }

    /// Whether `--keep` makes sense for this operation (only `clean-all`).
    pub fn takes_keep(self) -> bool {
        matches!(self, GhWhat::CleanAll)
    }
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
    /// Release a new version via `cargo release` (bump, commit, tag, push,
    /// publish), with the crates.io token fetched from pass(1). Only crates
    /// cargo would publish are released; `--type` picks the version bump.
    Release,
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

    /// Whether `--type` makes sense for this operation (only `release`).
    pub fn takes_type(self) -> bool {
        matches!(self, CargoWhat::Release)
    }
}

#[derive(Clone, ValueEnum)]
pub enum CleanWhat {
    /// Hard-clean: remove all untracked and ignored files (git clean -ffxd)
    Hard,
    /// Soft-clean: remove untracked files only (git clean -fd)
    Soft,
}

#[derive(Clone, ValueEnum)]
pub enum CountWhat {
    /// Count dirty repositories
    Dirty,
    /// Count repositories with untracked files
    Untracked,
    /// Count repositories that are not synchronized: ahead of or behind
    /// their upstream
    Unsynchronized,
    /// Count repositories with local commits not yet pushed to the upstream
    Ahead,
    /// Count repositories with upstream commits not yet pulled
    Behind,
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
    /// Stash tracked changes under the message `rsmultigit stash`
    /// (git stash push -m); repos with nothing to stash are skipped
    Push,
    /// Pop the most recent `rsmultigit stash` stash; repos without one are
    /// skipped, and stashes made by hand are never touched
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
    /// Mixed reset: unstage changes (git reset --mixed HEAD)
    Mixed,
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
#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum, Deserialize)]
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

    // Append a dynamic extension that completes `check same --checks <names>`
    // and `check exists --checks <names>` against `rsmultigit check list`
    // (`check list exists` for the latter), which reads the user's config
    // file. clap_complete only knows about static ValueEnum choices, so
    // --checks (free-form names from the config) needs runtime help.
    match shell {
        Shell::Bash => print!("{}", CHECKS_COMPLETION_BASH),
        Shell::Zsh => print!("{}", CHECKS_COMPLETION_ZSH),
        _ => {}
    }
}

/// Help template for a command invoked without its operation word: about and
/// usage, then the operations list and the `--help` trailer, both passed in
/// as `after-help` (see `operations_list`). Options stay out - on a command
/// with the global flags that is a dozen entries which bury the choices the
/// user actually stopped to see - and the trailer points at `--help`, where
/// they remain.
const OPERATIONS_HELP_TEMPLATE: &str =
    "{before-help}{about-with-newline}\n{usage-heading} {usage}{after-help}";

/// The operations of `cmd`, each with its description: its subcommands for a
/// group (`git`), the possible values of its `what` positional for a command
/// that takes the operation as a word (`npm <WHAT>`). Hidden entries are left
/// out, as clap leaves them out of its own help.
fn operations_of(cmd: &clap::Command) -> Vec<(String, String)> {
    if cmd.has_subcommands() {
        return cmd
            .get_subcommands()
            .filter(|sub| !sub.is_hide_set())
            .map(|sub| {
                let about = sub.get_about().map(|a| a.to_string()).unwrap_or_default();
                (sub.get_name().to_string(), about)
            })
            .collect();
    }
    cmd.get_arguments()
        .find(|arg| arg.get_id() == "what")
        .map(|arg| {
            arg.get_possible_values()
                .into_iter()
                .filter(|value| !value.is_hide_set())
                .map(|value| {
                    let help = value.get_help().map(|h| h.to_string()).unwrap_or_default();
                    (value.get_name().to_string(), help)
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The `Commands:` section listing `operations`, laid out the way clap lays
/// out its subcommand list: names styled as literals in a column as wide as
/// the longest one, two spaces, then the description. Subcommand groups and
/// `<WHAT>` commands both go through here, so the two look the same.
fn operations_list(operations: &[(String, String)]) -> String {
    let h = STYLES.get_header();
    let l = STYLES.get_literal();
    let width = operations
        .iter()
        .map(|(name, _)| name.len())
        .max()
        .unwrap_or(0);
    let indent = " ".repeat(2 + width + 2);
    let mut out = format!("{h}Commands:{h:#}\n");
    for (name, help) in operations {
        let pad = " ".repeat(width - name.len());
        let help = help.trim().replace('\n', &format!("\n{indent}"));
        out.push_str(format!("  {l}{name}{l:#}{pad}  {help}").trim_end());
        out.push('\n');
    }
    out
}

/// The help of the (sub)command that `args` (argv, program name included)
/// names, for printing when such a command is invoked without the operation
/// (the `what` positional or the subcommand) it requires. clap's
/// `arg_required_else_help` renders the short help there, which lists the
/// choices as bare names and then every flag; this lists every operation with
/// its description and *no* flags, which is what someone who stopped at
/// `rsmultigit gh` is looking for. The top-level command (no subcommand named)
/// keeps its normal long help.
pub fn long_help_for<I, S>(args: I) -> StyledStr
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut cmd = Cli::command();
    cmd.build();
    let mut path: Vec<String> = Vec::new();
    {
        let mut cur = &cmd;
        for arg in args.into_iter().skip(1) {
            match cur.find_subcommand(arg.as_ref()) {
                Some(sub) => {
                    path.push(sub.get_name().to_string());
                    cur = sub;
                }
                None => break,
            }
        }
    }
    if path.is_empty() {
        return cmd.render_long_help();
    }
    let full_name = format!("{} {}", cmd.get_name(), path.join(" "));
    let mut cur = &mut cmd;
    for name in &path {
        cur = cur
            .find_subcommand_mut(name)
            .expect("subcommand path was taken from this very Command");
    }
    let operations = operations_list(&operations_of(cur));
    cur.clone()
        .help_template(OPERATIONS_HELP_TEMPLATE)
        .after_help(format!(
            "{operations}\nRun `{full_name} --help` for the options."
        ))
        .render_long_help()
}

/// Bash snippet appended to `rsmultigit complete bash`. Wraps clap's generated
/// `_rsmultigit` function so that tabbing after `check same --checks` or
/// `check exists --checks` completes the rule names returned by
/// `rsmultigit check list` / `rsmultigit check list exists`. `same` and
/// `exists` only count right after `check`, and only the first time, so a
/// rule that happens to be named `exists` cannot flip the kind.
const CHECKS_COMPLETION_BASH: &str = r#"
# rsmultigit: dynamic --checks completion (appended by `rsmultigit complete bash`)
if declare -F _rsmultigit >/dev/null; then
    eval "$(declare -f _rsmultigit | sed '1 s/^_rsmultigit /_rsmultigit_clap /')"

    _rsmultigit() {
        local i cur prev
        cur="${COMP_WORDS[COMP_CWORD]}"
        prev="${COMP_WORDS[COMP_CWORD-1]}"

        local in_checks=0
        local group=""
        local kind=""
        for ((i=1; i<COMP_CWORD; i++)); do
            local w="${COMP_WORDS[i]}"
            case "$w" in
                check)    [[ -z "$group" ]] && group=check ;;
                same)     [[ "$group" == check && -z "$kind" ]] && kind=check ;;
                exists)   [[ "$group" == check && -z "$kind" ]] && kind=exists ;;
                --checks) in_checks=1 ;;
                --*)      in_checks=0 ;;
            esac
        done
        if [[ "$prev" == "--checks" ]]; then
            in_checks=1
        fi

        if [[ -n "$kind" ]] && (( in_checks )); then
            local names
            names=$(rsmultigit check list "$kind" 2>/dev/null)
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
        local group=""
        local kind=""
        local i
        for ((i=1; i<CURRENT; i++)); do
            case "${words[i]}" in
                check)    [[ -z "$group" ]] && group=check ;;
                same)     [[ "$group" == check && -z "$kind" ]] && kind=check ;;
                exists)   [[ "$group" == check && -z "$kind" ]] && kind=exists ;;
                --checks) seen_checks=1 ;;
                --*)      seen_checks=0 ;;
            esac
        done
        if [[ "$prev" == "--checks" ]]; then
            seen_checks=1
        fi

        if [[ -n "$kind" ]] && (( seen_checks )); then
            local -a names
            names=(${(f)"$(rsmultigit check list "$kind" 2>/dev/null)"})
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
        let cli = parse(&["rsmultigit", "git", "count", "dirty"]);
        assert!(matches!(
            cli.command,
            Commands::Git {
                command: GitCommand::Count {
                    what: CountWhat::Dirty
                }
            }
        ));
    }

    #[test]
    fn bare_operation_commands_show_help_not_missing_argument() {
        // Every command whose positional selects the operation prints its
        // help when given bare, so the user sees the choices.
        let bare: [&[&str]; 10] = [
            &["git", "count"],
            &["gh"],
            &["uv"],
            &["cargo"],
            &["npm"],
            &["git", "branch"],
            &["git", "clean"],
            &["git", "reset"],
            &["git", "stash"],
            &["git", "tag"],
        ];
        for words in bare {
            let sub = words.join(" ");
            let err = match Cli::try_parse_from(
                std::iter::once("rsmultigit").chain(words.iter().copied()),
            ) {
                Err(err) => err,
                Ok(_) => panic!("bare `{sub}` should not parse"),
            };
            assert_eq!(
                err.kind(),
                clap::error::ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand,
                "bare `{sub}` should display help, got: {err}"
            );
            let rendered = err.to_string();
            assert!(
                rendered.contains("possible values:"),
                "help for bare `{sub}` should list the choices: {rendered}"
            );
        }
    }

    #[test]
    fn long_help_for_lists_each_choice_with_its_description() {
        let help = long_help_for(["rsmultigit", "gh"]).to_string();
        assert!(
            help.contains("Usage: rsmultigit gh"),
            "usage line should carry the full command path: {help}"
        );
        for name in [
            "clean-all",
            "artifacts",
            "last-workflow-state",
            "open-site",
            "sync-metadata",
        ] {
            assert!(help.contains(name), "gh help should list `{name}`: {help}");
        }
        assert!(
            help.contains("Print the conclusion of the most recent workflow run"),
            "long help should describe each choice: {help}"
        );
    }

    #[test]
    fn long_help_for_leaves_the_flags_to_help() {
        // The flags - the command's own and the global ones - would bury the
        // choices, so the bare invocation lists only the operations and says
        // where the flags are.
        for sub in ["gh", "cargo"] {
            let help = long_help_for(["rsmultigit", sub]).to_string();
            // Rendered option lines; a description may well mention a flag
            // (`clean-all` talks about --keep), and that is fine.
            for flag in [
                "Options:",
                "--keep <KEEP>",
                "--no-header\n",
                "-j, --jobs",
                "--venv\n",
                "-h, --help",
            ] {
                assert!(
                    !help.contains(flag),
                    "bare `{sub}` should not list `{flag}`: {help}"
                );
            }
            assert!(
                help.contains(&format!("Run `rsmultigit {sub} --help` for the options.")),
                "bare `{sub}` should point at --help: {help}"
            );
        }
        // `--help` itself is untouched.
        let mut cmd = Cli::command();
        let full = cmd
            .find_subcommand_mut("gh")
            .unwrap()
            .render_long_help()
            .to_string();
        assert!(
            full.contains("Options:") && full.contains("--keep"),
            "{full}"
        );
    }

    #[test]
    fn long_help_for_lists_groups_and_operation_words_alike() {
        // Every command that demands an operation - a subcommand group or a
        // `<WHAT>` word - is listed by the same code, as a `Commands:` table.
        fn walk(cmd: &clap::Command, path: &mut Vec<String>) {
            for sub in cmd.get_subcommands() {
                path.push(sub.get_name().to_string());
                if sub.is_arg_required_else_help_set() {
                    let operations = operations_of(sub);
                    assert!(
                        !operations.is_empty(),
                        "bare `{}` has no operations to list",
                        path.join(" ")
                    );
                    let mut argv = vec!["rsmultigit".to_string()];
                    argv.extend(path.iter().cloned());
                    let help = long_help_for(&argv).to_string();
                    assert!(help.contains("\n\nCommands:\n"), "{help}");
                    assert!(!help.contains("Possible values"), "{help}");
                    let (name, _) = &operations[0];
                    assert!(help.contains(&format!("\n  {name}  ")), "{help}");
                }
                walk(sub, path);
                path.pop();
            }
        }
        let mut cmd = Cli::command();
        cmd.build();
        walk(&cmd, &mut Vec::new());
    }

    #[test]
    fn long_help_for_stops_at_the_deepest_subcommand_named() {
        // Trailing non-subcommand words (flags, typos) do not derail the lookup.
        let help = long_help_for(["rsmultigit", "git", "count", "--verbose"]).to_string();
        assert!(help.contains("Usage: rsmultigit git count"), "{help}");
        // No subcommand at all falls back to the top-level help.
        let help = long_help_for(["rsmultigit"]).to_string();
        assert!(help.contains("Usage: rsmultigit"), "{help}");
        assert!(help.contains("Commands:"), "{help}");
    }

    #[test]
    fn parse_all_subcommands() {
        // `status` is the one git operation kept at the top level, as a
        // shortcut for `git status`.
        let subcommands = ["version", "status"];
        for sub in subcommands {
            let result = Cli::try_parse_from(["rsmultigit", sub]);
            assert!(result.is_ok(), "subcommand {sub} should parse");
        }
        for words in [
            &["setup", "interactive"][..],
            &["setup", "config-sample"],
            &["check", "same"],
            &["check", "exists"],
            &["check", "all"],
            &["check", "list"],
            &["list", "repos"],
            &["list", "dirty"],
            &["list", "untracked"],
            &["list", "unsynchronized"],
            &["list", "ahead"],
            &["list", "behind"],
        ] {
            let result =
                Cli::try_parse_from(std::iter::once("rsmultigit").chain(words.iter().copied()));
            assert!(result.is_ok(), "{words:?} should parse");
        }
        // The commands folded into a group left the top level.
        for sub in [
            "dirty",
            "count",
            "age",
            "authors",
            "size",
            "last-tag",
            "rust",
            "check same",
            "check exists",
            "check all",
            "check list",
            "setup config-sample",
            "list-repos",
        ] {
            assert!(
                Cli::try_parse_from(["rsmultigit", sub, "dirty"]).is_err(),
                "top-level {sub} should be gone"
            );
        }
        let git_subcommands = [
            "status",
            "dirty",
            "age",
            "authors",
            "size",
            "last-tag",
            "pull",
            "push",
            "fetch",
            "diff",
            "remote",
            "remote-prune",
            "gc",
            "submodule-update",
            "restore",
        ];
        for sub in git_subcommands {
            let result = Cli::try_parse_from(["rsmultigit", "git", sub]);
            assert!(result.is_ok(), "subcommand git {sub} should parse");
            if sub == "status" {
                continue;
            }
            // The git operations left the top level.
            assert!(
                Cli::try_parse_from(["rsmultigit", sub]).is_err(),
                "top-level {sub} should be gone"
            );
        }
        // `prune` would read as git's object pruning; it is `remote-prune`.
        assert!(Cli::try_parse_from(["rsmultigit", "git", "prune"]).is_err());

        // clean requires a what argument
        let clean_whats = ["hard", "soft"];
        for what in clean_whats {
            let result = Cli::try_parse_from(["rsmultigit", "git", "clean", what]);
            assert!(result.is_ok(), "clean {what} should parse");
        }

        // count requires a what argument
        let count_whats = ["dirty", "untracked", "unsynchronized", "ahead", "behind"];
        for what in count_whats {
            let result = Cli::try_parse_from(["rsmultigit", "git", "count", what]);
            assert!(result.is_ok(), "count {what} should parse");
        }

        // branch requires a what argument
        let branch_whats = ["local", "remote", "github"];
        for what in branch_whats {
            let result = Cli::try_parse_from(["rsmultigit", "git", "branch", what]);
            assert!(result.is_ok(), "branch {what} should parse");
        }

        // tag requires a what argument
        let tag_whats = ["local", "remote", "has-local", "has-remote"];
        for what in tag_whats {
            let result = Cli::try_parse_from(["rsmultigit", "git", "tag", what]);
            assert!(result.is_ok(), "tag {what} should parse");
        }

        // reset requires a what argument
        let reset_whats = ["hard", "mixed"];
        for what in reset_whats {
            let result = Cli::try_parse_from(["rsmultigit", "git", "reset", what]);
            assert!(result.is_ok(), "reset {what} should parse");
        }

        // log accepts optional --count
        let result = Cli::try_parse_from(["rsmultigit", "git", "log"]);
        assert!(result.is_ok(), "log should parse without args");
        let result = Cli::try_parse_from(["rsmultigit", "git", "log", "--count", "5"]);
        assert!(result.is_ok(), "log --count 5 should parse");

        // checkout requires a branch
        let result = Cli::try_parse_from(["rsmultigit", "git", "checkout", "main"]);
        assert!(result.is_ok(), "checkout main should parse");

        // commit requires -m
        let result = Cli::try_parse_from(["rsmultigit", "git", "commit", "-m", "test"]);
        assert!(result.is_ok(), "commit -m test should parse");

        // config requires a key
        let result = Cli::try_parse_from(["rsmultigit", "git", "config", "user.email"]);
        assert!(result.is_ok(), "config user.email should parse");

        // blame requires a file
        let result = Cli::try_parse_from(["rsmultigit", "git", "blame", "README.md"]);
        assert!(result.is_ok(), "blame README.md should parse");

        // stash requires a what argument
        let stash_whats = ["push", "pop"];
        for what in stash_whats {
            let result = Cli::try_parse_from(["rsmultigit", "git", "stash", what]);
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
        assert!(Cli::try_parse_from(["rsmultigit", "git", "clean", "cargo"]).is_err());
        let result = Cli::try_parse_from(["rsmultigit", "build"]);
        assert!(result.is_ok(), "build without a method should parse");

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
        let gh_whats = [
            "clean-all",
            "artifacts",
            "last-workflow-state",
            "open-site",
            "sync-metadata",
        ];
        for what in gh_whats {
            let result = Cli::try_parse_from(["rsmultigit", "gh", what]);
            assert!(result.is_ok(), "gh {what} should parse");
        }
        let result = Cli::try_parse_from(["rsmultigit", "gh"]);
        assert!(result.is_err(), "gh without a what should not parse");

        // cargo requires a what argument
        let cargo_whats = [
            "build", "check", "clippy", "fmt", "test", "nextest", "doc", "deny", "fetch", "update",
            "clean", "publish", "release",
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
    fn parse_check_list_kind() {
        let cli = parse(&["rsmultigit", "check", "list"]);
        assert!(matches!(
            cli.command,
            Commands::Check {
                command: CheckCommand::List {
                    kind: RuleKind::Check
                }
            }
        ));
        let cli = parse(&["rsmultigit", "check", "list", "exists"]);
        assert!(matches!(
            cli.command,
            Commands::Check {
                command: CheckCommand::List {
                    kind: RuleKind::Exists
                }
            }
        ));
        assert!(Cli::try_parse_from(["rsmultigit", "check", "list", "both"]).is_err());
    }

    #[test]
    fn completion_snippets_know_both_check_commands() {
        for snippet in [CHECKS_COMPLETION_BASH, CHECKS_COMPLETION_ZSH] {
            assert!(snippet.contains("same)"));
            assert!(snippet.contains("exists)"));
            assert!(snippet.contains(r#"check list "$kind""#));
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
        let cli = parse(&["rsmultigit", "git", "grep", "TODO"]);
        match &cli.command {
            Commands::Git {
                command: GitCommand::Grep { regexp, files },
            } => {
                assert_eq!(regexp, "TODO");
                assert!(!files);
            }
            _ => panic!("expected Grep"),
        }
    }

    #[test]
    fn parse_grep_with_files_flag() {
        let cli = parse(&["rsmultigit", "git", "grep", "--files", "TODO"]);
        match &cli.command {
            Commands::Git {
                command: GitCommand::Grep { regexp, files },
            } => {
                assert_eq!(regexp, "TODO");
                assert!(files);
            }
            _ => panic!("expected Grep"),
        }
    }

    #[test]
    fn parse_grep_with_short_files_flag() {
        let cli = parse(&["rsmultigit", "git", "grep", "-l", "TODO"]);
        match &cli.command {
            Commands::Git {
                command: GitCommand::Grep { regexp, files },
            } => {
                assert_eq!(regexp, "TODO");
                assert!(files);
            }
            _ => panic!("expected Grep"),
        }
    }

    #[test]
    fn parse_pull_quiet() {
        let cli = parse(&["rsmultigit", "git", "pull", "--quiet"]);
        match &cli.command {
            Commands::Git {
                command: GitCommand::Pull { quiet },
            } => assert!(quiet),
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
            "git",
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
        let cli = parse(&["rsmultigit", "check", "same"]);
        assert!(!cli.short_circuit);
    }

    #[test]
    fn parse_jobs_flag() {
        let cli = parse(&["rsmultigit", "-j", "4", "list", "repos"]);
        assert_eq!(cli.jobs, 4);
        let cli = parse(&["rsmultigit", "--jobs", "8", "list", "repos"]);
        assert_eq!(cli.jobs, 8);
    }

    #[test]
    fn default_jobs_is_one() {
        let cli = parse(&["rsmultigit", "list", "repos"]);
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
    fn parse_gh_clean_all_keep_defaults_to_unset() {
        let cli = parse(&["rsmultigit", "gh", "clean-all"]);
        match &cli.command {
            Commands::Gh {
                what,
                keep,
                dry_run,
            } => {
                assert!(matches!(what, GhWhat::CleanAll));
                assert_eq!(*keep, None);
                assert!(!*dry_run);
            }
            _ => panic!("expected Gh"),
        }
    }

    #[test]
    fn parse_gh_clean_all_with_keep() {
        let cli = parse(&["rsmultigit", "gh", "clean-all", "--keep", "10"]);
        match &cli.command {
            Commands::Gh { keep, .. } => assert_eq!(*keep, Some(10)),
            _ => panic!("expected Gh"),
        }
    }

    #[test]
    fn parse_gh_bad_keep_fails() {
        let result = Cli::try_parse_from(["rsmultigit", "gh", "clean-all", "--keep", "many"]);
        assert!(result.is_err());
    }

    #[test]
    fn parse_gh_sync_metadata_dry_run() {
        let cli = parse(&["rsmultigit", "gh", "sync-metadata", "--dry-run"]);
        match &cli.command {
            Commands::Gh { what, dry_run, .. } => {
                assert!(matches!(what, GhWhat::SyncMetadata));
                assert!(*dry_run);
            }
            _ => panic!("expected Gh"),
        }
    }

    #[test]
    fn gh_keep_applies_to_clean_all_only() {
        for what in GhWhat::value_variants() {
            assert_eq!(
                what.takes_keep(),
                matches!(what, GhWhat::CleanAll),
                "{what:?}"
            );
        }
    }

    #[test]
    fn gh_dry_run_applies_to_sync_metadata_only() {
        for what in [
            GhWhat::CleanAll,
            GhWhat::Artifacts,
            GhWhat::LastWorkflowState,
            GhWhat::OpenSite,
        ] {
            assert!(!what.takes_dry_run(), "{what:?} should not take --dry-run");
        }
        assert!(GhWhat::SyncMetadata.takes_dry_run());
    }

    #[test]
    fn parse_cargo_release_type() {
        let cli = parse(&["rsmultigit", "cargo", "release"]);
        match &cli.command {
            Commands::Cargo {
                what, release_type, ..
            } => {
                assert_eq!(*what, CargoWhat::Release);
                assert_eq!(*release_type, None);
            }
            _ => panic!("expected Cargo"),
        }
        for (arg, expected) in [
            ("patch", ReleaseType::Patch),
            ("minor", ReleaseType::Minor),
            ("major", ReleaseType::Major),
        ] {
            let cli = parse(&["rsmultigit", "cargo", "release", "--type", arg]);
            match &cli.command {
                Commands::Cargo { release_type, .. } => {
                    assert_eq!(*release_type, Some(expected));
                }
                _ => panic!("expected Cargo"),
            }
        }
        let result = Cli::try_parse_from(["rsmultigit", "cargo", "release", "--type", "huge"]);
        assert!(result.is_err());
    }

    #[test]
    fn cargo_type_applies_to_release_only() {
        for what in CargoWhat::value_variants() {
            assert_eq!(what.takes_type(), *what == CargoWhat::Release, "{what:?}");
        }
        assert!(!CargoWhat::Release.takes_release());
        assert!(!CargoWhat::Release.takes_check());
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
