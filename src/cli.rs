// CLI parser limitations we've deliberately left unaddressed are documented in
// docs/src/cli-parser-notes.md — notably, clap 4 has no per-subcommand
// `help_heading`, so the flat alphabetical subcommand listing in `--help` is
// intentional. Read those notes before attempting to "categorize" subcommands.

use std::io;

use clap::{CommandFactory, Parser, Subcommand, ValueEnum};
use clap_complete::{generate, Shell};

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

    /// Number of parallel workers (default: 1; use 0 for num_cpus)
    #[arg(short = 'j', long, global = true, default_value_t = 1)]
    pub jobs: usize,

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
        /// What build system to use
        #[arg(value_enum)]
        what: BuildWhat,
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
        /// Interactively create missing files in repos that violate a rule's
        /// `must_have = true` requirement. Prompts for which content group to
        /// seed from, then writes the file (creating parent directories as needed).
        /// Always exits 0 on success.
        #[arg(long, default_value_t = false)]
        fix_missing: bool,
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
    /// Print the name of every check rule defined in the config, one per line.
    /// Intended for use in shell-completion scripts. All rules are listed,
    /// including those with `enabled = false`.
    ListChecks,
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
    /// Print version information
    Version,
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
    /// Run cargo clean (skip if no Cargo.toml)
    Cargo,
}

#[derive(Clone, ValueEnum)]
pub enum CountWhat {
    /// Count dirty repositories
    Dirty,
    /// Count repositories with untracked files
    Untracked,
    /// Count non-synchronized repositories (ahead/behind remote)
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
pub enum BuildWhat {
    /// Run bootstrap across all projects
    Bootstrap,
    /// Run pydmt build across all projects
    Pydmt,
    /// Run make across all projects
    Make,
    /// Run make inside a virtualenv across all projects
    VenvMake,
    /// Run pydmt inside a virtualenv across all projects
    VenvPydmt,
    /// Run pydmt build_venv across all projects
    PydmtBuildVenv,
    /// Run rsconstruct build on projects that have an rsconstruct.toml file
    Rsconstruct,
    /// Run cargo build on projects that have a Cargo.toml file
    Cargo,
    /// Run cargo publish on projects that have a Cargo.toml file
    CargoPublish,
}

/// Generate shell completions and print to stdout.
pub fn print_completions(shell: Shell) {
    let mut cmd = Cli::command();
    generate(shell, &mut cmd, "rsmultigit", &mut io::stdout());

    // Append a dynamic extension that completes `check-same --checks <names>`
    // against `rsmultigit list-checks`, which reads the user's config file.
    // clap_complete only knows about static ValueEnum choices, so --checks
    // (free-form names from the config) needs runtime help.
    match shell {
        Shell::Bash => print!("{}", CHECKS_COMPLETION_BASH),
        Shell::Zsh => print!("{}", CHECKS_COMPLETION_ZSH),
        _ => {}
    }
}

/// Bash snippet appended to `rsmultigit complete bash`. Wraps clap's generated
/// `_rsmultigit` function so that tabbing after `check-same --checks` completes
/// check names returned by `rsmultigit list-checks`.
const CHECKS_COMPLETION_BASH: &str = r#"
# rsmultigit: dynamic --checks completion (appended by `rsmultigit complete bash`)
if declare -F _rsmultigit >/dev/null; then
    eval "$(declare -f _rsmultigit | sed '1 s/^_rsmultigit /_rsmultigit_clap /')"

    _rsmultigit() {
        local i cur prev
        cur="${COMP_WORDS[COMP_CWORD]}"
        prev="${COMP_WORDS[COMP_CWORD-1]}"

        local in_checks=0
        local saw_check_same=0
        for ((i=1; i<COMP_CWORD; i++)); do
            local w="${COMP_WORDS[i]}"
            case "$w" in
                check-same) saw_check_same=1 ;;
                --checks)   in_checks=1 ;;
                --*)        in_checks=0 ;;
            esac
        done
        if [[ "$prev" == "--checks" ]]; then
            in_checks=1
        fi

        if (( saw_check_same && in_checks )); then
            local names
            names=$(rsmultigit list-checks 2>/dev/null)
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
        local seen_check_same=0
        local i
        for ((i=1; i<CURRENT; i++)); do
            case "${words[i]}" in
                check-same) seen_check_same=1 ;;
                --checks)   seen_checks=1 ;;
                --*)        seen_checks=0 ;;
            esac
        done
        if [[ "$prev" == "--checks" ]]; then
            seen_checks=1
        fi

        if (( seen_check_same && seen_checks )); then
            local -a names
            names=(${(f)"$(rsmultigit list-checks 2>/dev/null)"})
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
        assert!(matches!(cli.command, Commands::Count { what: CountWhat::Dirty }));
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
        let clean_whats = ["hard", "soft", "make", "git", "cargo"];
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

        // build requires a what argument
        let build_whats = [
            "bootstrap",
            "pydmt",
            "make",
            "venv-make",
            "venv-pydmt",
            "pydmt-build-venv",
            "rsconstruct",
            "cargo",
            "cargo-publish",
        ];
        for what in build_whats {
            let result = Cli::try_parse_from(["rsmultigit", "build", what]);
            assert!(result.is_ok(), "build {what} should parse");
        }

        // complete requires an argument
        let complete_shells = ["bash", "zsh", "fish", "elvish", "powershell"];
        for shell in complete_shells {
            let result = Cli::try_parse_from(["rsmultigit", "complete", shell]);
            assert!(result.is_ok(), "complete {shell} should parse");
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
            "rsmultigit", "--terse", "--no-output", "--verbose", "--print-not",
            "--no-stop",
            "count", "dirty",
        ]);
        assert!(cli.terse);
        assert!(cli.no_output);
        assert!(cli.verbose);
        assert!(cli.print_not);
        assert!(cli.no_stop);
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
    fn unknown_subcommand_fails() {
        let result = Cli::try_parse_from(["rsmultigit", "nonexistent"]);
        assert!(result.is_err());
    }
}
