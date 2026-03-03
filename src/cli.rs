use std::io;

use clap::{CommandFactory, Parser, Subcommand, ValueEnum};
use clap_complete::{generate, Shell};

#[derive(Parser)]
#[command(name = "rmg")]
#[command(version = concat!(env!("CARGO_PKG_VERSION"), " by ", env!("CARGO_PKG_AUTHORS")))]
#[command(about = "Manage multiple git repositories at once")]
pub struct Cli {
    // Output control
    /// Terse output
    #[arg(long, default_value_t = false)]
    pub terse: bool,

    /// Show statistics
    #[arg(long, default_value_t = false)]
    pub stats: bool,

    /// Suppress command output
    #[arg(long, default_value_t = false)]
    pub no_output: bool,

    /// Verbose output (print all projects, even when no action is taken)
    #[arg(short, long, default_value_t = false)]
    pub verbose: bool,

    /// Print repos that do NOT match (invert selection)
    #[arg(long, default_value_t = false)]
    pub print_not: bool,

    // Debug
    /// Pass --verbose to git commands
    #[arg(long, default_value_t = false)]
    pub git_verbose: bool,

    /// Pass --quiet to git commands
    #[arg(long, default_value_t = false)]
    pub git_quiet: bool,

    // Main
    /// Do not sort project list
    #[arg(long, default_value_t = false)]
    pub no_sort: bool,

    /// Glob pattern for discovering projects (default: */*)
    #[arg(long, default_value = "*/*")]
    pub glob: String,

    /// Disable glob-based discovery
    #[arg(long, default_value_t = false)]
    pub no_glob: bool,

    /// Explicit list of folders to operate on
    #[arg(long, value_delimiter = ',')]
    pub folders: Vec<String>,

    /// Do not stop on errors
    #[arg(long, default_value_t = false)]
    pub no_stop: bool,

    /// Do not print 'no projects found' message
    #[arg(long, default_value_t = false)]
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
    /// Clean repositories
    Clean {
        /// What kind of clean to perform
        #[arg(value_enum)]
        what: CleanWhat,
    },
    /// Show diff for all repositories
    Diff,
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
    /// Run rsb build on projects that have an rsb.toml file
    Rsb,
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
            "pull",
            "diff",
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

        // build requires a what argument
        let build_whats = [
            "bootstrap",
            "pydmt",
            "make",
            "venv-make",
            "venv-pydmt",
            "pydmt-build-venv",
            "rsb",
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
