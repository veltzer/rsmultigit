use crate::cli::Cli;

pub struct AppConfig {
    // Output control
    pub terse: bool,
    pub no_header: bool,
    pub no_output: bool,
    pub verbose: bool,
    pub print_not: bool,

    // Execution
    pub no_stop: bool,
    pub jobs: usize,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            terse: false,
            no_header: false,
            no_output: false,
            verbose: false,
            print_not: false,
            no_stop: false,
            jobs: 1,
        }
    }
}

impl From<&Cli> for AppConfig {
    fn from(cli: &Cli) -> Self {
        Self {
            terse: cli.terse,
            no_header: cli.no_header,
            no_output: cli.no_output,
            verbose: cli.verbose,
            print_not: cli.print_not,
            no_stop: cli.no_stop,
            jobs: cli.jobs,
        }
    }
}
