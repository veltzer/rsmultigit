mod cli;
mod commands;
mod config;
mod discovery;
mod runner;
mod subprocess_utils;

use std::path::Path;

use anyhow::Result;
use clap::Parser;

use cli::{BranchWhat, BuildWhat, Cli, CleanWhat, Commands, CountWhat};
use config::AppConfig;

fn main() -> Result<()> {
    let cli = Cli::parse();

    // Handle commands that don't need project discovery
    if let Commands::Complete { shell } = &cli.command {
        cli::print_completions(*shell);
        return Ok(());
    }
    if matches!(&cli.command, Commands::Version) {
        println!("rmg {} by {}", env!("CARGO_PKG_VERSION"), env!("CARGO_PKG_AUTHORS"));
        println!("RMG_GIT_DESCRIBE: {}", env!("RMG_GIT_DESCRIBE"));
        println!("RMG_GIT_SHA: {}", env!("RMG_GIT_SHA"));
        println!("RMG_GIT_BRANCH: {}", env!("RMG_GIT_BRANCH"));
        println!("RMG_GIT_DIRTY: {}", env!("RMG_GIT_DIRTY"));
        println!("RMG_RUSTC_SEMVER: {}", env!("RMG_RUSTC_SEMVER"));
        return Ok(());
    }

    let config = AppConfig::from_cli(&cli);
    let projects = discovery::discover_projects(&config)?;

    if projects.is_empty() && !config.no_print_no_projects {
        eprintln!("no projects found");
        return Ok(());
    }

    match &cli.command {
        // ── do_count ──
        Commands::Count { what } => {
            let test_fn: fn(&Path) -> anyhow::Result<bool> = match what {
                CountWhat::Dirty => commands::count::is_dirty,
                CountWhat::Untracked => commands::count::has_untracked,
                CountWhat::Synchronized => commands::count::non_synchronized,
            };
            runner::do_count(&config, &projects, test_fn)?;
        }

        // ── print_if_data ──
        Commands::Status => {
            runner::print_if_data(&config, &projects, commands::status::do_status)?;
        }
        Commands::Dirty => {
            runner::print_if_data(&config, &projects, commands::status::do_dirty)?;
        }
        Commands::ListProjects => {
            runner::print_if_data(&config, &projects, |project: &Path| {
                Ok(Some(project.display().to_string()))
            })?;
        }

        // ── do_for_all_projects ──
        Commands::Branch { what } => {
            let branch_fn: fn(&Path) -> anyhow::Result<bool> = match what {
                BranchWhat::Local => commands::branch::branch_local,
                BranchWhat::Remote => commands::branch::branch_remote,
                BranchWhat::Github => commands::branch::branch_github,
            };
            runner::do_for_all_projects(&config, &projects, branch_fn)?;
        }
        Commands::Pull { quiet } => {
            let quiet = *quiet;
            runner::do_for_all_projects(&config, &projects, move |project: &Path| -> anyhow::Result<bool> {
                commands::pull::do_pull(project, quiet)
            })?;
        }
        Commands::Clean { what } => {
            let clean_fn: fn(&Path) -> anyhow::Result<bool> = match what {
                CleanWhat::Hard => commands::clean::clean_hard,
                CleanWhat::Soft => commands::clean::clean_soft,
                CleanWhat::Make => commands::clean::clean_make,
                CleanWhat::Git => commands::clean::clean_git,
                CleanWhat::Cargo => commands::clean::clean_cargo,
            };
            runner::do_for_all_projects(&config, &projects, clean_fn)?;
        }
        Commands::Diff => {
            runner::do_for_all_projects(&config, &projects, commands::diff::do_diff)?;
        }
        Commands::Grep { regexp, files } => {
            let regexp = regexp.clone();
            let files = *files;
            runner::do_for_all_projects(&config, &projects, move |project: &Path| -> anyhow::Result<bool> {
                commands::grep::do_grep(project, &regexp, files)
            })?;
        }

        // ── build commands ──
        Commands::Build { what } => {
            let build_fn: fn(&Path) -> anyhow::Result<bool> = match what {
                BuildWhat::Bootstrap => commands::build::build_bootstrap,
                BuildWhat::Pydmt => commands::build::build_pydmt,
                BuildWhat::Make => commands::build::build_make,
                BuildWhat::VenvMake => commands::build::build_venv_make,
                BuildWhat::VenvPydmt => commands::build::build_venv_pydmt,
                BuildWhat::PydmtBuildVenv => commands::build::build_pydmt_build_venv,
                BuildWhat::Rsb => commands::build::build_rsb,
            };
            runner::do_for_all_projects(&config, &projects, build_fn)?;
        }

        Commands::Complete { .. } => unreachable!("handled above"),
        Commands::Version => unreachable!("handled above"),
    }

    Ok(())
}
