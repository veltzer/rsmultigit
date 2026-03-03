use std::io;

use clap::{CommandFactory, Parser, Subcommand};
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
    /// Count dirty repositories
    CountDirty,
    /// Count repositories with untracked files
    Untracked,
    /// Count non-synchronized repositories (ahead/behind remote)
    Synchronized,

    // ── print_projects_that_return_data ──
    /// Show status of repositories
    Status,
    /// Show dirty repositories
    Dirty,
    /// Check if workflow exists for repos with Makefile
    CheckWorkflowExistsForMakefile,
    /// List discovered projects
    ListProjects,

    // ── do_for_all_projects ──
    /// Show local branches
    BranchLocal,
    /// Show remote branches
    BranchRemote,
    /// Show GitHub default branch
    BranchGithub,
    /// Pull all repositories
    Pull {
        /// Pass --quiet to git pull
        #[arg(long, default_value_t = false)]
        quiet: bool,
    },
    /// Hard-clean all repositories (git clean -ffxd)
    CleanHard,
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
    /// Run bootstrap across all projects
    BuildBootstrap,
    /// Run pydmt build across all projects
    BuildPydmt,
    /// Run make across all projects
    BuildMake,
    /// Run make inside a virtualenv across all projects
    BuildVenvMake,
    /// Run pydmt inside a virtualenv across all projects
    BuildVenvPydmt,
    /// Run pydmt build_venv across all projects
    BuildPydmtBuildVenv,
    /// Run rsb build on projects that have an rsb.toml file
    BuildRsb,

    /// Generate shell completion scripts
    Complete {
        /// Shell to generate completions for
        #[arg(value_enum)]
        shell: Shell,
    },

    /// Print version information
    Version,
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
        let cli = parse(&["rmg", "count-dirty"]);
        assert!(matches!(cli.command, Commands::CountDirty));
    }

    #[test]
    fn parse_all_subcommands() {
        let subcommands = [
            "count-dirty",
            "untracked",
            "synchronized",
            "status",
            "dirty",
            "check-workflow-exists-for-makefile",
            "list-projects",
            "branch-local",
            "branch-remote",
            "branch-github",
            "pull",
            "clean-hard",
            "diff",
            "build-bootstrap",
            "build-pydmt",
            "build-make",
            "build-venv-make",
            "build-venv-pydmt",
            "build-pydmt-build-venv",
            "build-rsb",
            "version",
        ];
        for sub in subcommands {
            let result = Cli::try_parse_from(["rmg", sub]);
            assert!(result.is_ok(), "subcommand {sub} should parse");
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
            "rmg", "--terse", "--stats", "--no-output", "--print-not",
            "--no-sort", "--no-stop", "--no-print-no-projects",
            "count-dirty",
        ]);
        assert!(cli.terse);
        assert!(cli.stats);
        assert!(cli.no_output);
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
