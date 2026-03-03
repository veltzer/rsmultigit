mod cli;
mod commands;
mod config;
mod discovery;
mod runner;
mod subprocess_utils;

use std::path::Path;

use anyhow::Result;
use clap::Parser;

use cli::{Cli, Commands};
use config::AppConfig;

fn main() -> Result<()> {
    let cli = Cli::parse();

    // Handle version before project discovery
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
        Commands::CountDirty => {
            runner::do_count(&config, &projects, commands::count::is_dirty)?;
        }
        Commands::Untracked => {
            runner::do_count(&config, &projects, commands::count::has_untracked)?;
        }
        Commands::Synchronized => {
            runner::do_count(&config, &projects, commands::count::non_synchronized)?;
        }

        // ── print_if_data ──
        Commands::Status => {
            runner::print_if_data(&config, &projects, commands::status::do_status)?;
        }
        Commands::Dirty => {
            runner::print_if_data(&config, &projects, commands::status::do_dirty)?;
        }
        Commands::CheckWorkflowExistsForMakefile => {
            runner::print_if_data(
                &config,
                &projects,
                commands::workflow::check_workflow_exists_for_makefile,
            )?;
        }
        Commands::ListProjects => {
            runner::print_if_data(&config, &projects, |project: &Path| {
                Ok(Some(project.display().to_string()))
            })?;
        }

        // ── do_for_all_projects ──
        Commands::BranchLocal => {
            runner::do_for_all_projects(&config, &projects, commands::branch::branch_local)?;
        }
        Commands::BranchRemote => {
            runner::do_for_all_projects(&config, &projects, commands::branch::branch_remote)?;
        }
        Commands::BranchGithub => {
            runner::do_for_all_projects(&config, &projects, commands::branch::branch_github)?;
        }
        Commands::Pull { quiet } => {
            let quiet = *quiet;
            runner::do_for_all_projects(&config, &projects, move |project: &Path| {
                commands::pull::do_pull(project, quiet)
            })?;
        }
        Commands::CleanHard => {
            runner::do_for_all_projects(&config, &projects, commands::clean::do_clean)?;
        }
        Commands::Diff => {
            runner::do_for_all_projects(&config, &projects, commands::diff::do_diff)?;
        }
        Commands::Grep { regexp, files } => {
            let regexp = regexp.clone();
            let files = *files;
            runner::do_for_all_projects(&config, &projects, move |project: &Path| {
                commands::grep::do_grep(project, &regexp, files)
            })?;
        }

        // ── build commands ──
        Commands::BuildBootstrap => {
            runner::do_for_all_projects(&config, &projects, commands::build::build_bootstrap)?;
        }
        Commands::BuildPydmt => {
            runner::do_for_all_projects(&config, &projects, commands::build::build_pydmt)?;
        }
        Commands::BuildMake => {
            runner::do_for_all_projects(&config, &projects, commands::build::build_make)?;
        }
        Commands::BuildVenvMake => {
            runner::do_for_all_projects(&config, &projects, commands::build::build_venv_make)?;
        }
        Commands::BuildVenvPydmt => {
            runner::do_for_all_projects(&config, &projects, commands::build::build_venv_pydmt)?;
        }
        Commands::BuildPydmtBuildVenv => {
            runner::do_for_all_projects(
                &config,
                &projects,
                commands::build::build_pydmt_build_venv,
            )?;
        }
        Commands::BuildRsb => {
            runner::do_for_all_projects(&config, &projects, commands::build::build_rsb)?;
        }

        Commands::Version => unreachable!("handled above"),
    }

    Ok(())
}
