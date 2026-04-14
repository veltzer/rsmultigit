mod cli;
mod commands;
mod config;
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
        println!("rsmultigit {} by {}", env!("CARGO_PKG_VERSION"), env!("CARGO_PKG_AUTHORS"));
        println!("GIT_DESCRIBE: {}", env!("GIT_DESCRIBE"));
        println!("GIT_SHA: {}", env!("GIT_SHA"));
        println!("GIT_BRANCH: {}", env!("GIT_BRANCH"));
        println!("GIT_DIRTY: {}", env!("GIT_DIRTY"));
        println!("RUSTC_SEMVER: {}", env!("RUSTC_SEMVER"));
        println!("RUST_EDITION: {}", env!("RUST_EDITION"));
        println!("BUILD_TIMESTAMP: {}", env!("BUILD_TIMESTAMP"));
        return Ok(());
    }

    let config = AppConfig::from(&cli);

    // Repo list comes from ~/.config/rsmultigit/config.toml for every command.
    let config_path = commands::check::default_config_path()?;
    let file_config = commands::check::load_config(&config_path)?;
    let projects = commands::check::resolve_repos(&file_config)?;

    if let Commands::CheckSame { rule } = &cli.command {
        let exit_code = run_check_same(&config, &file_config, &projects, rule.as_deref())?;
        std::process::exit(exit_code);
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
            // Prints one path per line with no header — the project path *is* the data,
            // so the bracketed header would be redundant. --verbose re-enables the
            // standard [project]\n<data> format for consistency with other commands.
            for project in &projects {
                if config.verbose && !config.terse && !config.no_header {
                    println!("[{}]", project.display());
                }
                println!("{}", project.display());
            }
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
            type ProjectFn = fn(&Path) -> anyhow::Result<bool>;
            let (check_fn, build_fn): (ProjectFn, ProjectFn) = match what {
                BuildWhat::Bootstrap => (commands::build::check_not_disabled, commands::build::build_bootstrap),
                BuildWhat::Pydmt => (commands::build::check_pydmt, commands::build::build_pydmt),
                BuildWhat::Make => (commands::build::check_not_disabled, commands::build::build_make),
                BuildWhat::VenvMake => (commands::build::check_not_disabled, commands::build::build_venv_make),
                BuildWhat::VenvPydmt => (commands::build::check_pydmt, commands::build::build_venv_pydmt),
                BuildWhat::PydmtBuildVenv => (commands::build::check_pydmt, commands::build::build_pydmt_build_venv),
                BuildWhat::Rsconstruct => (commands::build::check_rsconstruct, commands::build::build_rsconstruct),
                BuildWhat::Cargo => (commands::build::check_cargo, commands::build::build_cargo),
                BuildWhat::CargoPublish => (commands::build::check_cargo, commands::build::build_cargo_publish),
            };
            runner::do_for_all_projects_with_check(&config, &projects, check_fn, build_fn)?;
        }

        Commands::CheckSame { .. } => unreachable!("handled above"),
        Commands::Complete { .. } => unreachable!("handled above"),
        Commands::Version => unreachable!("handled above"),
    }

    Ok(())
}

/// Run the check-same command. Returns the process exit code.
fn run_check_same(
    app: &AppConfig,
    file_config: &commands::check::CheckConfig,
    projects: &[std::path::PathBuf],
    only_rule: Option<&str>,
) -> Result<i32> {
    use commands::check;

    // `--rule` overrides the `enabled` flag (user explicitly asked for that rule);
    // otherwise disabled rules are silently skipped.
    let rules: Vec<&check::Rule> = file_config
        .check
        .iter()
        .filter(|r| match only_rule {
            Some(name) => r.name == name,
            None => r.enabled,
        })
        .collect();

    if rules.is_empty() {
        if let Some(name) = only_rule {
            eprintln!("no rule named {name:?} in config");
            return Ok(1);
        }
        if app.verbose {
            println!("no rules to check");
        }
        return Ok(0);
    }

    let mut any_mismatch = false;
    for rule in rules {
        let result = check::evaluate_rule(rule, projects)?;
        if result.is_consistent() {
            if app.verbose {
                if !app.terse && !app.no_header {
                    println!("[{}]", result.name);
                }
                println!("ok ({} files)", result.total_files);
            }
            continue;
        }

        any_mismatch = true;

        if app.terse {
            println!("{}", result.name);
            continue;
        }

        if !app.no_header {
            println!("[{}]", result.name);
        }
        println!(
            "{} files, {} groups{}",
            result.total_files,
            result.groups.len(),
            if result.skipped.is_empty() {
                String::new()
            } else {
                format!(" ({} skipped)", result.skipped.len())
            },
        );
        if !app.no_output {
            for (i, group) in result.groups.iter().enumerate() {
                let label = group_label(i);
                println!("  group {label} ({} files):", group.len());
                for file in group {
                    println!("    {}", file.display());
                }
            }
        }
    }

    Ok(if any_mismatch { 1 } else { 0 })
}

fn group_label(i: usize) -> String {
    let mut n = i;
    let mut s = String::new();
    loop {
        s.insert(0, (b'A' + (n % 26) as u8) as char);
        n /= 26;
        if n == 0 {
            break;
        }
        n -= 1;
    }
    s
}
