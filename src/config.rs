use clap::Args;

/// The global flags, accepted before or after any subcommand. Parsed by clap
/// straight into this struct (flattened into `Cli`), so a new flag is
/// declared once and is available to every runner without a copy step.
#[derive(Args, Clone, Debug)]
pub struct AppConfig {
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
}

impl AppConfig {
    /// Whether to activate each repo's local .venv (`--venv`, the default,
    /// unless `--no-venv` was given last).
    pub fn use_venv(&self) -> bool {
        self.venv && !self.no_venv
    }
}

/// The values clap gives when no global flag is passed; used by tests that
/// build a config without parsing a command line.
impl Default for AppConfig {
    fn default() -> Self {
        Self {
            terse: false,
            no_header: false,
            no_output: false,
            verbose: false,
            print_not: false,
            no_stop: false,
            short_circuit: false,
            jobs: 1,
            venv: true,
            no_venv: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[derive(Parser)]
    struct Probe {
        #[command(flatten)]
        global: AppConfig,
    }

    #[test]
    fn default_matches_what_clap_parses_without_flags() {
        let parsed = Probe::parse_from(["probe"]).global;
        let default = AppConfig::default();
        assert_eq!(format!("{parsed:?}"), format!("{default:?}"));
    }

    #[test]
    fn no_venv_turns_the_venv_off() {
        assert!(Probe::parse_from(["probe"]).global.use_venv());
        assert!(!Probe::parse_from(["probe", "--no-venv"]).global.use_venv());
    }
}
