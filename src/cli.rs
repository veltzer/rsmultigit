use std::io;

use clap::{CommandFactory, Parser, Subcommand, ValueEnum};
use clap_complete::{generate, Shell};

#[derive(Parser)]
#[command(name = "rmg")]
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

Use `rmg <command> --help` for more options.")]
pub struct Cli {
    // Output control
    /// Terse output
    #[arg(long, global = true, default_value_t = false)]
    pub terse: bool,

    /// Show statistics
    #[arg(long, global = true, default_value_t = false)]
    pub stats: bool,

    /// Suppress command output
    #[arg(long, global = true, default_value_t = false)]
    pub no_output: bool,

    /// Verbose output (print all projects, even when no action is taken)
    #[arg(short, long, global = true, default_value_t = false)]
    pub verbose: bool,

    /// Print repos that do NOT match (invert selection)
    #[arg(long, global = true, default_value_t = false)]
    pub print_not: bool,

    // Debug
    /// Pass --verbose to git commands
    #[arg(long, global = true, default_value_t = false)]
    pub git_verbose: bool,

    /// Pass --quiet to git commands
    #[arg(long, global = true, default_value_t = false)]
    pub git_quiet: bool,

    // Main
    /// Do not sort project list
    #[arg(long, global = true, default_value_t = false)]
    pub no_sort: bool,

    /// Glob pattern for discovering projects (default: */*)
    #[arg(long, global = true, default_value = "*/*")]
    pub glob: String,

    /// Disable glob-based discovery
    #[arg(long, global = true, default_value_t = false)]
    pub no_glob: bool,

    /// Explicit list of folders to operate on
    #[arg(long, global = true, value_delimiter = ',')]
    pub folders: Vec<String>,

    /// Do not stop on errors
    #[arg(long, global = true, default_value_t = false)]
    pub no_stop: bool,

    /// Do not print 'no projects found' message
    #[arg(long, global = true, default_value_t = false)]
    pub no_print_no_projects: bool,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    // ── do_count ──
    /// Count repositories matching a condition
    Count {
        /// What to count
        #[arg(value_enum)]
        what: CountWhat,
    },

    // ── print_projects_that_return_data ──
    /// Show status of repositories
    Status,
    /// Show dirty repositories
    Dirty,
    /// List discovered projects
    ListProjects,
    /// Show the age of the last commit per repo
    Age,
    /// Show unique commit authors per repo
    Authors,
    /// Show a git config value across all repos
    Config {
        /// Git config key to show
        key: String,
    },
    /// Show the size of the .git directory per repo
    Size,
    /// Show the most recent tag per repo
    LastTag,

    // ── do_for_all_projects ──
    /// Branch operations
    Branch {
        /// What branch info to show
        #[arg(value_enum)]
        what: BranchWhat,
    },
    /// Pull all repositories
    Pull {
        /// Pass --quiet to git pull
        #[arg(long, default_value_t = false)]
        quiet: bool,
    },
    /// Push all repositories
    Push,
    /// Fetch from origin for all repositories
    Fetch,
    /// Clean repositories
    Clean {
        /// What kind of clean to perform
        #[arg(value_enum)]
        what: CleanWhat,
    },
    /// Stash operations
    Stash {
        /// What stash operation to perform
        #[arg(value_enum)]
        what: StashWhat,
    },
    /// Reset operations
    Reset {
        /// What kind of reset to perform
        #[arg(value_enum)]
        what: ResetWhat,
    },
    /// Show diff for all repositories
    Diff,
    /// Show recent commits
    Log {
        /// Number of commits to show
        #[arg(long, default_value_t = 10)]
        count: u32,
    },
    /// List tags
    Tag {
        /// What tags to show
        #[arg(value_enum)]
        what: TagWhat,
    },
    /// Show remote URLs
    Remote,
    /// Prune stale remote-tracking branches
    Prune,
    /// Run git garbage collection
    Gc,
    /// Checkout a branch across all repositories
    Checkout {
        /// Branch name to checkout
        branch: String,
    },
    /// Commit all changes across all repositories
    Commit {
        /// Commit message
        #[arg(short, long)]
        message: String,
    },
    /// Update submodules recursively
    SubmoduleUpdate,
    /// Run git blame on a file across all repositories
    Blame {
        /// File path to blame
        file: String,
    },
    /// Grep across all repositories
    Grep {
        /// Regular expression to search for
        regexp: String,
        /// Only show filenames
        #[arg(long, default_value_t = false)]
        files: bool,
    },

    // ── build commands ──
    /// Build projects
    Build {
        /// What build system to use
        #[arg(value_enum)]
        what: BuildWhat,
    },

    /// Generate shell completion scripts
    Complete {
        /// Shell to generate completions for
        #[arg(value_enum)]
        shell: Shell,
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
    /// Run rsbuild build on projects that have an rsbuild.toml file
    Rsbuild,
}

/// Generate shell completions and print to stdout.
pub fn print_completions(shell: Shell) {
    let mut cmd = Cli::command();
    generate(shell, &mut cmd, "rmg", &mut io::stdout());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Cli {
        Cli::parse_from(args)
    }

    #[test]
    fn parse_count_dirty() {
        let cli = parse(&["rmg", "count", "dirty"]);
        assert!(matches!(cli.command, Commands::Count { what: CountWhat::Dirty }));
    }

    #[test]
    fn parse_all_subcommands() {
        let subcommands = [
            "status",
            "dirty",
            "list-projects",
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
            let result = Cli::try_parse_from(["rmg", sub]);
            assert!(result.is_ok(), "subcommand {sub} should parse");
        }

        // clean requires a what argument
        let clean_whats = ["hard", "soft", "make", "git", "cargo"];
        for what in clean_whats {
            let result = Cli::try_parse_from(["rmg", "clean", what]);
            assert!(result.is_ok(), "clean {what} should parse");
        }

        // count requires a what argument
        let count_whats = ["dirty", "untracked", "synchronized"];
        for what in count_whats {
            let result = Cli::try_parse_from(["rmg", "count", what]);
            assert!(result.is_ok(), "count {what} should parse");
        }

        // branch requires a what argument
        let branch_whats = ["local", "remote", "github"];
        for what in branch_whats {
            let result = Cli::try_parse_from(["rmg", "branch", what]);
            assert!(result.is_ok(), "branch {what} should parse");
        }

        // tag requires a what argument
        let tag_whats = ["local", "remote", "has-local", "has-remote"];
        for what in tag_whats {
            let result = Cli::try_parse_from(["rmg", "tag", what]);
            assert!(result.is_ok(), "tag {what} should parse");
        }

        // reset requires a what argument
        let reset_whats = ["hard", "soft", "mixed"];
        for what in reset_whats {
            let result = Cli::try_parse_from(["rmg", "reset", what]);
            assert!(result.is_ok(), "reset {what} should parse");
        }

        // log accepts optional --count
        let result = Cli::try_parse_from(["rmg", "log"]);
        assert!(result.is_ok(), "log should parse without args");
        let result = Cli::try_parse_from(["rmg", "log", "--count", "5"]);
        assert!(result.is_ok(), "log --count 5 should parse");

        // checkout requires a branch
        let result = Cli::try_parse_from(["rmg", "checkout", "main"]);
        assert!(result.is_ok(), "checkout main should parse");

        // commit requires -m
        let result = Cli::try_parse_from(["rmg", "commit", "-m", "test"]);
        assert!(result.is_ok(), "commit -m test should parse");

        // config requires a key
        let result = Cli::try_parse_from(["rmg", "config", "user.email"]);
        assert!(result.is_ok(), "config user.email should parse");

        // blame requires a file
        let result = Cli::try_parse_from(["rmg", "blame", "README.md"]);
        assert!(result.is_ok(), "blame README.md should parse");

        // stash requires a what argument
        let stash_whats = ["push", "pop"];
        for what in stash_whats {
            let result = Cli::try_parse_from(["rmg", "stash", what]);
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
            "rsbuild",
        ];
        for what in build_whats {
            let result = Cli::try_parse_from(["rmg", "build", what]);
            assert!(result.is_ok(), "build {what} should parse");
        }

        // complete requires an argument
        let complete_shells = ["bash", "zsh", "fish", "elvish", "powershell"];
        for shell in complete_shells {
            let result = Cli::try_parse_from(["rmg", "complete", shell]);
            assert!(result.is_ok(), "complete {shell} should parse");
        }
    }

    #[test]
    fn parse_complete_bash() {
        let cli = parse(&["rmg", "complete", "bash"]);
        match &cli.command {
            Commands::Complete { shell } => {
                assert!(matches!(shell, Shell::Bash));
            }
            _ => panic!("expected Complete"),
        }
    }

    #[test]
    fn parse_grep_with_regexp() {
        let cli = parse(&["rmg", "grep", "TODO"]);
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
        let cli = parse(&["rmg", "grep", "--files", "TODO"]);
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
        let cli = parse(&["rmg", "pull", "--quiet"]);
        match &cli.command {
            Commands::Pull { quiet } => assert!(quiet),
            _ => panic!("expected Pull"),
        }
    }

    #[test]
    fn parse_global_flags() {
        let cli = parse(&[
            "rmg", "--terse", "--stats", "--no-output", "--verbose", "--print-not",
            "--no-sort", "--no-stop", "--no-print-no-projects",
            "count", "dirty",
        ]);
        assert!(cli.terse);
        assert!(cli.stats);
        assert!(cli.no_output);
        assert!(cli.verbose);
        assert!(cli.print_not);
        assert!(cli.no_sort);
        assert!(cli.no_stop);
        assert!(cli.no_print_no_projects);
    }

    #[test]
    fn parse_glob_pattern() {
        let cli = parse(&["rmg", "--glob", "myorg/*", "list-projects"]);
        assert_eq!(cli.glob, "myorg/*");
    }

    #[test]
    fn parse_folders() {
        let cli = parse(&["rmg", "--folders", "a,b,c", "list-projects"]);
        assert_eq!(cli.folders, vec!["a", "b", "c"]);
    }

    #[test]
    fn default_glob_is_star_star() {
        let cli = parse(&["rmg", "list-projects"]);
        assert_eq!(cli.glob, "*/*");
    }

    #[test]
    fn unknown_subcommand_fails() {
        let result = Cli::try_parse_from(["rmg", "nonexistent"]);
        assert!(result.is_err());
    }
}
