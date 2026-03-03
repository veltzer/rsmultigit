use crate::cli::Cli;

#[allow(dead_code)]
pub struct AppConfig {
    // Output control
    pub terse: bool,
    pub stats: bool,
    pub no_output: bool,
    pub verbose: bool,
    pub print_not: bool,

    // Debug
    pub git_verbose: bool,
    pub git_quiet: bool,

    // Main
    pub no_sort: bool,
    pub glob: String,
    pub no_glob: bool,
    pub folders: Vec<String>,
    pub no_stop: bool,
    pub no_print_no_projects: bool,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            terse: false,
            stats: false,
            no_output: false,
            verbose: false,
            print_not: false,
            git_verbose: false,
            git_quiet: false,
            no_sort: false,
            glob: "*/*".to_string(),
            no_glob: false,
            folders: Vec::new(),
            no_stop: false,
            no_print_no_projects: false,
        }
    }
}

impl AppConfig {
    pub fn from_cli(cli: &Cli) -> Self {
        Self {
            terse: cli.terse,
            stats: cli.stats,
            no_output: cli.no_output,
            verbose: cli.verbose,
            print_not: cli.print_not,
            git_verbose: cli.git_verbose,
            git_quiet: cli.git_quiet,
            no_sort: cli.no_sort,
            glob: cli.glob.clone(),
            no_glob: cli.no_glob,
            folders: cli.folders.clone(),
            no_stop: cli.no_stop,
            no_print_no_projects: cli.no_print_no_projects,
        }
    }
}
