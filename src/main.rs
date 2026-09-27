mod cli;
mod commands;
mod config;
mod runner;
mod subprocess_utils;

use camino::Utf8Path;

use anyhow::Result;
use clap::Parser;

use cli::{
    BranchWhat, BuildWhat, CargoWhat, CleanWhat, Cli, Commands, CountWhat, GhWhat, ResetWhat,
    RustWhat, StashWhat, TagWhat, UvWhat,
};
use commands::check_run::{self, CheckExistsOpts, CheckSameOpts};
use config::AppConfig;

fn main() -> Result<()> {
    let cli = Cli::parse();

    // Handle commands that don't need project discovery
    if let Commands::Complete { shell } = &cli.command {
        cli::print_completions(*shell);
        return Ok(());
    }
    if matches!(&cli.command, Commands::Version) {
        println!(
            "rsmultigit {} by {}",
            env!("CARGO_PKG_VERSION"),
            env!("CARGO_PKG_AUTHORS")
        );
        println!("GIT_DESCRIBE: {}", env!("GIT_DESCRIBE"));
        println!("GIT_SHA: {}", env!("GIT_SHA"));
        println!("GIT_BRANCH: {}", env!("GIT_BRANCH"));
        println!("GIT_DIRTY: {}", env!("GIT_DIRTY"));
        println!("RUSTC_SEMVER: {}", env!("RUSTC_SEMVER"));
        println!("RUST_EDITION: {}", env!("RUST_EDITION"));
        println!("BUILD_TIMESTAMP: {}", env!("BUILD_TIMESTAMP"));
        return Ok(());
    }
    if matches!(&cli.command, Commands::ConfigExample) {
        // Doesn't need (and must not require) a config file — this subcommand
        // is how a fresh user bootstraps their ~/.config/rsmultigit/config.toml.
        print!("{}", include_str!("../assets/config-example.toml"));
        return Ok(());
    }

    let config = AppConfig::from(&cli);

    // Repo list comes from ~/.config/rsmultigit/config.toml for every command.
    let config_path = commands::check::default_config_path()?;
    let file_config = commands::check::load_config(&config_path)?;
    let projects = commands::check::resolve_repos(&file_config)?;

    // The check commands are organised by rule rather than by repo and own
    // their exit codes, so they bypass the runners. Prompts (--diff, --copy,
    // --fix-missing) are served from stdin; everything is written to stdout.
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();

    if let Commands::CheckSame {
        checks,
        checks_re,
        only_failed,
        diff,
        copy,
        allow_empty,
        fix_missing,
    } = &cli.command
    {
        let exit_code = check_run::run_check_same(
            &config,
            &file_config,
            &projects,
            &CheckSameOpts {
                requested: checks,
                requested_re: checks_re,
                only_failed: *only_failed,
                show_diff: *diff,
                do_copy: *copy,
                allow_empty: *allow_empty,
                do_fix_missing: *fix_missing,
            },
            &mut stdin.lock(),
            &mut stdout.lock(),
        )?;
        std::process::exit(exit_code);
    }

    if let Commands::CheckExists {
        checks,
        checks_re,
        only_failed,
        allow_empty,
    } = &cli.command
    {
        let exit_code = check_run::run_check_exists(
            &config,
            &file_config,
            &projects,
            &CheckExistsOpts {
                requested: checks,
                requested_re: checks_re,
                only_failed: *only_failed,
                allow_empty: *allow_empty,
            },
            &mut stdout.lock(),
        )?;
        std::process::exit(exit_code);
    }

    if let Commands::CheckAll {
        only_failed,
        allow_empty,
    } = &cli.command
    {
        // Both halves always run: the point of check-all is one verdict over
        // every invariant, so a failure in the first must not hide the state of
        // the second. An empty-rule bail in either half still propagates as an
        // error, since that is a config bug rather than drift.
        let same = check_run::run_check_same(
            &config,
            &file_config,
            &projects,
            &CheckSameOpts {
                requested: &[],
                requested_re: &[],
                only_failed: *only_failed,
                show_diff: false,
                do_copy: false,
                allow_empty: *allow_empty,
                do_fix_missing: false,
            },
            &mut stdin.lock(),
            &mut stdout.lock(),
        )?;
        let exists = check_run::run_check_exists(
            &config,
            &file_config,
            &projects,
            &CheckExistsOpts {
                requested: &[],
                requested_re: &[],
                only_failed: *only_failed,
                allow_empty: *allow_empty,
            },
            &mut stdout.lock(),
        )?;
        std::process::exit(if same != 0 || exists != 0 { 1 } else { 0 });
    }

    match &cli.command {
        // ── do_count ──
        Commands::Count { what } => {
            let test_fn: fn(&Utf8Path) -> anyhow::Result<bool> = match what {
                CountWhat::Dirty => commands::count::is_dirty,
                CountWhat::Untracked => commands::count::has_untracked,
                CountWhat::Synchronized => commands::count::non_synchronized,
            };
            runner::do_count(&config, &projects, test_fn)?;
        }

        // ── print_if_data ──
        Commands::Status => {
            // Default: one-line summary of each repo's situation (counts of
            // modified/staged/untracked files, ahead/behind). --verbose switches
            // to the full `git status -s` per-file output.
            if config.verbose {
                runner::print_if_data(&config, &projects, commands::status::do_status)?;
            } else {
                runner::print_if_data(&config, &projects, commands::status::do_status_summary)?;
            }
        }
        Commands::Dirty => {
            runner::print_if_data(&config, &projects, commands::status::do_dirty)?;
        }
        Commands::ListChecks => {
            for rule in &file_config.check {
                println!("{}", rule.name);
            }
        }
        Commands::ListRepos => {
            // Prints one path per line with no header — the project path *is* the data,
            // so the bracketed header would be redundant. --verbose re-enables the
            // standard [project]\n<data> format for consistency with other commands.
            for project in &projects {
                if config.verbose && !config.terse && !config.no_header {
                    println!("[{}]", project);
                }
                println!("{}", project);
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
            runner::print_if_data(&config, &projects, move |project: &Utf8Path| {
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
            let branch_fn: fn(&Utf8Path) -> anyhow::Result<bool> = match what {
                BranchWhat::Local => commands::branch::branch_local,
                BranchWhat::Remote => commands::branch::branch_remote,
                BranchWhat::Github => commands::branch::branch_github,
            };
            runner::do_for_all_projects(&config, &projects, branch_fn)?;
        }
        Commands::Pull { quiet } => {
            let quiet = *quiet;
            runner::do_for_all_projects(
                &config,
                &projects,
                move |project: &Utf8Path| -> anyhow::Result<bool> {
                    commands::pull::do_pull(project, quiet)
                },
            )?;
        }
        Commands::Push => {
            runner::do_for_all_projects(&config, &projects, commands::push::do_push)?;
        }
        Commands::Fetch => {
            runner::do_for_all_projects(&config, &projects, commands::fetch::do_fetch)?;
        }
        Commands::Clean { what } => match what {
            CleanWhat::Make => {
                let venv = config.venv;
                runner::do_for_all_projects(
                    &config,
                    &projects,
                    move |project: &Utf8Path| -> anyhow::Result<bool> {
                        commands::clean::clean_make(project, venv)
                    },
                )?;
            }
            _ => {
                let clean_fn: fn(&Utf8Path) -> anyhow::Result<bool> = match what {
                    CleanWhat::Hard => commands::clean::clean_hard,
                    CleanWhat::Soft => commands::clean::clean_soft,
                    CleanWhat::Git => commands::clean::clean_git,
                    CleanWhat::Cargo => commands::clean::clean_cargo,
                    CleanWhat::Make => unreachable!("handled above"),
                };
                runner::do_for_all_projects(&config, &projects, clean_fn)?;
            }
        },
        Commands::Stash { what } => {
            let stash_fn: fn(&Utf8Path) -> anyhow::Result<bool> = match what {
                StashWhat::Push => commands::stash::stash_push,
                StashWhat::Pop => commands::stash::stash_pop,
            };
            runner::do_for_all_projects(&config, &projects, stash_fn)?;
        }
        Commands::Reset { what } => {
            let reset_fn: fn(&Utf8Path) -> anyhow::Result<bool> = match what {
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
            runner::do_for_all_projects(
                &config,
                &projects,
                move |project: &Utf8Path| -> anyhow::Result<bool> {
                    commands::log::do_log(project, count)
                },
            )?;
        }
        Commands::Tag { what } => match what {
            TagWhat::Local | TagWhat::Remote => {
                let tag_fn: fn(&Utf8Path) -> anyhow::Result<bool> = match what {
                    TagWhat::Local => commands::tag::tag_local,
                    TagWhat::Remote => commands::tag::tag_remote,
                    _ => unreachable!(),
                };
                runner::do_for_all_projects(&config, &projects, tag_fn)?;
            }
            TagWhat::HasLocal | TagWhat::HasRemote => {
                let test_fn: fn(&Utf8Path) -> anyhow::Result<bool> = match what {
                    TagWhat::HasLocal => commands::tag::tag_has_local,
                    TagWhat::HasRemote => commands::tag::tag_has_remote,
                    _ => unreachable!(),
                };
                runner::do_count(&config, &projects, test_fn)?;
            }
        },
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
            runner::do_for_all_projects(
                &config,
                &projects,
                move |project: &Utf8Path| -> anyhow::Result<bool> {
                    commands::checkout::do_checkout(project, &branch)
                },
            )?;
        }
        Commands::Commit { message } => {
            let message = message.clone();
            runner::do_for_all_projects(
                &config,
                &projects,
                move |project: &Utf8Path| -> anyhow::Result<bool> {
                    commands::commit::do_commit(project, &message)
                },
            )?;
        }
        Commands::SubmoduleUpdate => {
            runner::do_for_all_projects(&config, &projects, commands::submodule::submodule_update)?;
        }
        Commands::Blame { file } => {
            let file = file.clone();
            runner::do_for_all_projects(
                &config,
                &projects,
                move |project: &Utf8Path| -> anyhow::Result<bool> {
                    commands::blame::do_blame(project, &file)
                },
            )?;
        }
        Commands::Grep { regexp, files } => {
            let regexp = regexp.clone();
            let files = *files;
            runner::do_for_all_projects(
                &config,
                &projects,
                move |project: &Utf8Path| -> anyhow::Result<bool> {
                    commands::grep::do_grep(project, &regexp, files)
                },
            )?;
        }
        Commands::Run { command } => {
            let command = command.clone();
            let venv = config.venv;
            runner::do_for_all_projects(
                &config,
                &projects,
                move |project: &Utf8Path| -> anyhow::Result<bool> {
                    commands::run::do_run(project, &command, venv)
                },
            )?;
        }

        Commands::Gh { what, keep } => match what {
            GhWhat::CleanAll => {
                let keep = *keep;
                runner::do_for_all_projects_with_check(
                    &config,
                    &projects,
                    commands::gh::check_github,
                    move |project: &Utf8Path| -> anyhow::Result<bool> {
                        commands::gh::clean_all(project, keep)
                    },
                )?;
            }
        },
        Commands::Rust { what, release_type } => match what {
            RustWhat::Publish => {
                let level = release_type.as_str();
                runner::do_for_all_projects_with_check(
                    &config,
                    &projects,
                    commands::build::check_cargo,
                    move |project: &Utf8Path| -> anyhow::Result<bool> {
                        commands::rust::publish(project, level)
                    },
                )?;
            }
        },

        // ── build commands ──
        Commands::Build { what } => {
            // Command line wins; otherwise fall back to the config file's
            // `default_build_method`; otherwise it's a usage error.
            let what = match what.clone().or(file_config.default_build_method.clone()) {
                Some(what) => what,
                None => anyhow::bail!(
                    "build: no build method given and `default_build_method` is not set in {config_path}\n\
                     usage: rsmultigit build <{}>\n\
                     or add e.g. `default_build_method = \"rsconstruct\"` to the config file",
                    BuildWhat::names().join("|")
                ),
            };
            type CheckFn = fn(&Utf8Path) -> anyhow::Result<bool>;
            // All build actions take the effective --venv flag.
            type BuildFn = fn(&Utf8Path, bool) -> anyhow::Result<bool>;
            let (check_fn, build_fn): (CheckFn, BuildFn) = match what {
                BuildWhat::Bootstrap => (
                    commands::build::check_not_disabled,
                    commands::build::build_bootstrap,
                ),
                BuildWhat::Make => (
                    commands::build::check_not_disabled,
                    commands::build::build_make,
                ),
                BuildWhat::Rsconstruct => (
                    commands::build::check_rsconstruct,
                    commands::build::build_rsconstruct,
                ),
                BuildWhat::Cargo => (commands::build::check_cargo, commands::build::build_cargo),
                BuildWhat::CargoPublish => (
                    commands::build::check_cargo,
                    commands::build::build_cargo_publish,
                ),
            };
            let venv = config.venv;
            runner::do_for_all_projects_with_check(
                &config,
                &projects,
                check_fn,
                move |project: &Utf8Path| -> anyhow::Result<bool> { build_fn(project, venv) },
            )?;
        }

        Commands::Uv {
            what,
            upgrade,
            check,
        } => {
            let upgrade = *upgrade;
            let check = *check;
            if upgrade && *what != UvWhat::Lock {
                anyhow::bail!("--upgrade only applies to `uv lock`");
            }
            if check && *what != UvWhat::Lock {
                anyhow::bail!("--check only applies to `uv lock`");
            }
            // uv picks its target environment from the repo dir itself, so the
            // global --venv flag deliberately does not reach these calls.
            match what {
                UvWhat::Lock => {
                    runner::do_for_all_projects_with_check(
                        &config,
                        &projects,
                        commands::uv::check_pyproject,
                        move |project: &Utf8Path| -> anyhow::Result<bool> {
                            commands::uv::lock(project, upgrade, check)
                        },
                    )?;
                }
                UvWhat::Sync => {
                    runner::do_for_all_projects_with_check(
                        &config,
                        &projects,
                        commands::uv::check_pyproject,
                        commands::uv::sync,
                    )?;
                }
            }
        }

        Commands::Cargo { what } => {
            let venv = config.venv;
            match what {
                CargoWhat::Update => {
                    runner::do_for_all_projects_with_check(
                        &config,
                        &projects,
                        commands::build::check_cargo,
                        move |project: &Utf8Path| -> anyhow::Result<bool> {
                            commands::cargo::update(project, venv)
                        },
                    )?;
                }
            }
        }

        Commands::CheckSame { .. } => unreachable!("handled above"),
        Commands::CheckExists { .. } => unreachable!("handled above"),
        Commands::CheckAll { .. } => unreachable!("handled above"),
        Commands::Complete { .. } => unreachable!("handled above"),
        Commands::ConfigExample => unreachable!("handled above"),
        Commands::Version => unreachable!("handled above"),
    }

    Ok(())
}
