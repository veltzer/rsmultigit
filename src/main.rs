mod cli;
mod commands;
mod config;
mod discovery;
mod runner;
mod subprocess_utils;

use std::path::Path;

use anyhow::Result;
use clap::Parser;

use cli::{BranchWhat, BuildWhat, Cli, CleanWhat, Commands, CountWhat, ResetWhat, StashWhat, TagWhat};
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
        println!("RMG_RUST_EDITION: {}", env!("RMG_RUST_EDITION"));
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
        Commands::Age => {
            runner::print_if_data(&config, &projects, commands::age::do_age)?;
        }
        Commands::Authors => {
            runner::print_if_data(&config, &projects, commands::authors::do_authors)?;
        }
        Commands::Config { key } => {
            let key = key.clone();
            runner::print_if_data(&config, &projects, move |project: &Path| {
                commands::config::do_config(project, &key)
            })?;
        }
        Commands::Size => {
            runner::print_if_data(&config, &projects, commands::size::do_size)?;
        }
        Commands::LastTag => {
            runner::print_if_data(&config, &projects, commands::last_tag::do_last_tag)?;
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
        Commands::Push => {
            runner::do_for_all_projects(&config, &projects, commands::push::do_push)?;
        }
        Commands::Fetch => {
            runner::do_for_all_projects(&config, &projects, commands::fetch::do_fetch)?;
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
        Commands::Stash { what } => {
            let stash_fn: fn(&Path) -> anyhow::Result<bool> = match what {
                StashWhat::Push => commands::stash::stash_push,
                StashWhat::Pop => commands::stash::stash_pop,
            };
            runner::do_for_all_projects(&config, &projects, stash_fn)?;
        }
        Commands::Reset { what } => {
            let reset_fn: fn(&Path) -> anyhow::Result<bool> = match what {
                ResetWhat::Hard => commands::reset::reset_hard,
                ResetWhat::Soft => commands::reset::reset_soft,
                ResetWhat::Mixed => commands::reset::reset_mixed,
            };
            runner::do_for_all_projects(&config, &projects, reset_fn)?;
        }
        Commands::Diff => {
            runner::do_for_all_projects(&config, &projects, commands::diff::do_diff)?;
        }
        Commands::Log { count } => {
            let count = *count;
            runner::do_for_all_projects(&config, &projects, move |project: &Path| -> anyhow::Result<bool> {
                commands::log::do_log(project, count)
            })?;
        }
        Commands::Tag { what } => match what {
            TagWhat::Local | TagWhat::Remote => {
                let tag_fn: fn(&Path) -> anyhow::Result<bool> = match what {
                    TagWhat::Local => commands::tag::tag_local,
                    TagWhat::Remote => commands::tag::tag_remote,
                    _ => unreachable!(),
                };
                runner::do_for_all_projects(&config, &projects, tag_fn)?;
            }
            TagWhat::HasLocal | TagWhat::HasRemote => {
                let test_fn: fn(&Path) -> anyhow::Result<bool> = match what {
                    TagWhat::HasLocal => commands::tag::tag_has_local,
                    TagWhat::HasRemote => commands::tag::tag_has_remote,
                    _ => unreachable!(),
                };
                runner::do_count(&config, &projects, test_fn)?;
            }
        }
        Commands::Remote => {
            runner::do_for_all_projects(&config, &projects, commands::remote::do_remote)?;
        }
        Commands::Prune => {
            runner::do_for_all_projects(&config, &projects, commands::prune::do_prune)?;
        }
        Commands::Gc => {
            runner::do_for_all_projects(&config, &projects, commands::gc::do_gc)?;
        }
        Commands::Checkout { branch } => {
            let branch = branch.clone();
            runner::do_for_all_projects(&config, &projects, move |project: &Path| -> anyhow::Result<bool> {
                commands::checkout::do_checkout(project, &branch)
            })?;
        }
        Commands::Commit { message } => {
            let message = message.clone();
            runner::do_for_all_projects(&config, &projects, move |project: &Path| -> anyhow::Result<bool> {
                commands::commit::do_commit(project, &message)
            })?;
        }
        Commands::SubmoduleUpdate => {
            runner::do_for_all_projects(&config, &projects, commands::submodule::submodule_update)?;
        }
        Commands::Blame { file } => {
            let file = file.clone();
            runner::do_for_all_projects(&config, &projects, move |project: &Path| -> anyhow::Result<bool> {
                commands::blame::do_blame(project, &file)
            })?;
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
                BuildWhat::Rsbuild => commands::build::build_rsbuild,
            };
            runner::do_for_all_projects(&config, &projects, build_fn)?;
        }

        Commands::Complete { .. } => unreachable!("handled above"),
        Commands::Version => unreachable!("handled above"),
    }

    Ok(())
}
