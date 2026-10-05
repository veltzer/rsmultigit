mod cli;
mod commands;
mod config;
mod runner;
mod subprocess_utils;

use camino::{Utf8Path, Utf8PathBuf};

use anyhow::Result;
use clap::Parser;
use clap::error::{ContextKind, ContextValue, ErrorKind};

use cli::{
    BranchWhat, BuildWhat, CleanWhat, Cli, Commands, CountWhat, GhWhat, GitCommand, ResetWhat,
    RuleKind, RustWhat, StashWhat, TagWhat, UvWhat,
};
use commands::check_run::{self, CheckExistsOpts, CheckSameOpts};
use config::AppConfig;

/// `Cli::parse()`, except that a command invoked without the positional that
/// selects its operation (`rsmultigit gh`, `rsmultigit gh --keep 3`) gets the
/// long help of that command, so every choice is listed with its description.
/// clap alone would print the short help for the bare form (choices as bare
/// names) and a "required arguments were not provided: <WHAT>" error for the
/// form with options, neither of which tells the user what to type. Same
/// stream and exit status as clap uses for those cases.
fn parse_cli() -> Cli {
    match Cli::try_parse() {
        Ok(cli) => cli,
        Err(err) if is_missing_operation(&err) => {
            let args: Vec<String> = std::env::args().collect();
            eprint!("{}", cli::long_help_for(&args));
            std::process::exit(2);
        }
        Err(err) => err.exit(),
    }
}

/// Whether `err` says the operation-selecting positional (`<WHAT>` on every
/// such command, see `Commands` in cli.rs) or the operation subcommand (on a
/// group like `git`) is missing: either nothing at all followed the command,
/// or only options did.
fn is_missing_operation(err: &clap::Error) -> bool {
    match err.kind() {
        ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand | ErrorKind::MissingSubcommand => true,
        ErrorKind::MissingRequiredArgument => matches!(
            err.get(ContextKind::InvalidArg),
            Some(ContextValue::Strings(missing)) if missing.iter().any(|arg| arg == "<WHAT>")
        ),
        _ => false,
    }
}

fn main() -> Result<()> {
    let cli = parse_cli();

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
    if let Commands::Setup {
        repos_dir,
        build,
        no_build,
        overwrite,
    } = &cli.command
    {
        // Same: setup is what writes the config file in the first place, so
        // it runs before anything tries to read one.
        let opts = commands::setup::SetupOpts {
            repos_dir: repos_dir.clone(),
            build: match (build, no_build) {
                (Some(m), _) => Some(Some(*m)),
                (None, true) => Some(None),
                (None, false) => None,
            },
            overwrite: *overwrite,
        };
        let config_path = commands::check::default_config_path()?;
        return commands::setup::run(&opts, &config_path, &mut std::io::stdout().lock());
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
        Commands::ListChecks { kind } => {
            let names: Vec<&str> = match kind {
                RuleKind::Check => file_config.check.iter().map(|r| r.name.as_str()).collect(),
                RuleKind::Exists => file_config.exists.iter().map(|r| r.name.as_str()).collect(),
            };
            for name in names {
                println!("{name}");
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
        Commands::Size => {
            runner::print_if_data(&config, &projects, commands::size::do_size)?;
        }
        Commands::LastTag => {
            runner::print_if_data(&config, &projects, commands::last_tag::do_last_tag)?;
        }

        // ── do_for_all_projects ──
        Commands::Git { command } => run_git_command(&config, &projects, command)?,
        Commands::Run { command } => {
            let command = command.clone();
            let venv = config.venv;
            runner::do_for_all_projects(
                &config,
                &projects,
                move |project: &Utf8Path| -> anyhow::Result<()> {
                    commands::run::do_run(project, &command, venv)
                },
            )?;
        }

        Commands::Gh {
            what,
            keep,
            dry_run,
        } => {
            let dry_run = *dry_run;
            if dry_run && !what.takes_dry_run() {
                anyhow::bail!("--dry-run only applies to `gh sync-metadata`");
            }
            match what {
                GhWhat::CleanAll => {
                    let keep = *keep;
                    runner::do_for_all_projects_with_check(
                        &config,
                        &projects,
                        commands::gh::check_github,
                        move |project: &Utf8Path| -> anyhow::Result<()> {
                            commands::gh::clean_all(project, keep)
                        },
                    )?;
                }
                GhWhat::Artifacts => {
                    runner::print_if_data(&config, &projects, commands::gh::artifacts)?;
                }
                GhWhat::LastWorkflowState => {
                    runner::print_if_data(&config, &projects, commands::gh::last_workflow_state)?;
                }
                GhWhat::OpenSite => {
                    runner::do_for_all_projects_with_check(
                        &config,
                        &projects,
                        commands::gh::check_github,
                        commands::gh::open_site,
                    )?;
                }
                GhWhat::SyncMetadata => {
                    // A data runner, although it writes to GitHub: the point of
                    // the command is to print only the repos where something
                    // differs, and a silent repo is one that is already in sync.
                    runner::print_if_data(&config, &projects, move |project: &Utf8Path| {
                        commands::gh::sync_metadata(project, dry_run)
                    })?;
                }
            }
        }
        Commands::Rust { what, release_type } => match what {
            RustWhat::Publish => {
                // Preflight once: cargo-release present, crates.io token in
                // hand. Only then start bumping versions.
                let release = commands::rust::Release::prepare(
                    release_type.as_str(),
                    &file_config.crates_io_pass_entry,
                )?;
                runner::do_for_all_projects_with_check(
                    &config,
                    &projects,
                    commands::rust::check_publishable,
                    |project: &Utf8Path| -> anyhow::Result<()> { release.run(project) },
                )?;
            }
        },

        // ── build commands ──
        Commands::Build { what } => {
            // Command line wins; otherwise fall back to the config file's
            // `default_build_method`; otherwise it's a usage error.
            let what = match what.or(file_config.default_build_method) {
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
            type BuildFn = fn(&Utf8Path, bool) -> anyhow::Result<()>;
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
            };
            let venv = config.venv;
            runner::do_for_all_projects_with_check(
                &config,
                &projects,
                check_fn,
                move |project: &Utf8Path| -> anyhow::Result<()> { build_fn(project, venv) },
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
                        move |project: &Utf8Path| -> anyhow::Result<()> {
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
                UvWhat::Build => {
                    runner::do_for_all_projects_with_check(
                        &config,
                        &projects,
                        commands::uv::check_pyproject,
                        commands::uv::build,
                    )?;
                }
                UvWhat::Publish => {
                    runner::do_for_all_projects_with_check(
                        &config,
                        &projects,
                        commands::uv::check_pyproject,
                        commands::uv::publish,
                    )?;
                }
            }
        }

        Commands::Cargo {
            what,
            release,
            profile,
            check,
        } => {
            let what = *what;
            let release = *release;
            let check = *check;
            if (release || profile.is_some()) && !what.takes_release() {
                anyhow::bail!(
                    "--release and --profile only apply to `cargo build`, `check`, `clippy`, `test`, `nextest` and `doc`"
                );
            }
            let profile = commands::cargo::Profile::from_flags(release, profile.as_deref());
            if check && !what.takes_check() {
                anyhow::bail!("--check only applies to `cargo fmt`");
            }
            let venv = config.venv;
            runner::do_for_all_projects_with_check(
                &config,
                &projects,
                commands::build::check_cargo,
                move |project: &Utf8Path| -> anyhow::Result<()> {
                    commands::cargo::run(project, venv, what, &profile, check)
                },
            )?;
        }

        Commands::Npm { what, fix } => {
            let what = *what;
            let fix = *fix;
            if fix && !what.takes_fix() {
                anyhow::bail!("--fix only applies to `npm audit`");
            }
            let venv = config.venv;
            runner::do_for_all_projects_with_check(
                &config,
                &projects,
                commands::npm::check_package_json,
                move |project: &Utf8Path| -> anyhow::Result<()> {
                    commands::npm::run(project, venv, what, fix)
                },
            )?;
        }

        Commands::CheckSame { .. } => unreachable!("handled above"),
        Commands::CheckExists { .. } => unreachable!("handled above"),
        Commands::CheckAll { .. } => unreachable!("handled above"),
        Commands::Complete { .. } => unreachable!("handled above"),
        Commands::ConfigExample => unreachable!("handled above"),
        Commands::Setup { .. } => unreachable!("handled above"),
        Commands::Version => unreachable!("handled above"),
    }

    Ok(())
}

/// The `git` group: each subcommand runs the git command of the same name
/// in every repo.
fn run_git_command(
    config: &AppConfig,
    projects: &[Utf8PathBuf],
    command: &GitCommand,
) -> Result<()> {
    match command {
        GitCommand::Config { key } => {
            let key = key.clone();
            runner::print_if_data(config, projects, move |project: &Utf8Path| {
                commands::config::do_config(project, &key)
            })?;
        }
        GitCommand::Branch { what } => {
            let branch_fn: fn(&Utf8Path) -> anyhow::Result<()> = match what {
                BranchWhat::Local => commands::branch::branch_local,
                BranchWhat::Remote => commands::branch::branch_remote,
                BranchWhat::Github => commands::branch::branch_github,
            };
            runner::do_for_all_projects(config, projects, branch_fn)?;
        }
        GitCommand::Pull { quiet } => {
            let quiet = *quiet;
            runner::do_for_all_projects(
                config,
                projects,
                move |project: &Utf8Path| -> anyhow::Result<()> {
                    commands::pull::do_pull(project, quiet)
                },
            )?;
        }
        GitCommand::Push => {
            runner::do_for_all_projects_with_check(
                config,
                projects,
                commands::count::is_ahead,
                commands::push::do_push,
            )?;
        }
        GitCommand::Fetch => {
            runner::do_for_all_projects(config, projects, commands::fetch::do_fetch)?;
        }
        GitCommand::Clean { what } => {
            let clean_fn: fn(&Utf8Path) -> anyhow::Result<()> = match what {
                CleanWhat::Hard => commands::clean::clean_hard,
                CleanWhat::Soft => commands::clean::clean_soft,
            };
            runner::do_for_all_projects(config, projects, clean_fn)?;
        }
        GitCommand::Stash { what } => {
            let stash_fn: fn(&Utf8Path) -> anyhow::Result<()> = match what {
                StashWhat::Push => commands::stash::stash_push,
                StashWhat::Pop => commands::stash::stash_pop,
            };
            runner::do_for_all_projects(config, projects, stash_fn)?;
        }
        GitCommand::Restore => {
            runner::do_for_all_projects(config, projects, commands::restore::restore)?;
        }
        GitCommand::Reset { what } => {
            let reset_fn: fn(&Utf8Path) -> anyhow::Result<()> = match what {
                ResetWhat::Hard => commands::reset::reset_hard,
                ResetWhat::Soft => commands::reset::reset_soft,
                ResetWhat::Mixed => commands::reset::reset_mixed,
            };
            runner::do_for_all_projects(config, projects, reset_fn)?;
        }
        GitCommand::Diff => {
            runner::do_for_all_projects(config, projects, commands::diff::do_diff)?;
        }
        GitCommand::Log { count } => {
            let count = *count;
            runner::do_for_all_projects(
                config,
                projects,
                move |project: &Utf8Path| -> anyhow::Result<()> {
                    commands::log::do_log(project, count)
                },
            )?;
        }
        GitCommand::Tag { what } => match what {
            TagWhat::Local | TagWhat::Remote => {
                let tag_fn: fn(&Utf8Path) -> anyhow::Result<()> = match what {
                    TagWhat::Local => commands::tag::tag_local,
                    TagWhat::Remote => commands::tag::tag_remote,
                    _ => unreachable!(),
                };
                runner::do_for_all_projects(config, projects, tag_fn)?;
            }
            TagWhat::HasLocal | TagWhat::HasRemote => {
                let test_fn: fn(&Utf8Path) -> anyhow::Result<bool> = match what {
                    TagWhat::HasLocal => commands::tag::tag_has_local,
                    TagWhat::HasRemote => commands::tag::tag_has_remote,
                    _ => unreachable!(),
                };
                runner::do_count(config, projects, test_fn)?;
            }
        },
        GitCommand::Remote => {
            runner::do_for_all_projects(config, projects, commands::remote::do_remote)?;
        }
        GitCommand::RemotePrune => {
            runner::do_for_all_projects(config, projects, commands::prune::do_prune)?;
        }
        GitCommand::Gc => {
            runner::do_for_all_projects(config, projects, commands::gc::do_gc)?;
        }
        GitCommand::Checkout { branch } => {
            let branch = branch.clone();
            runner::do_for_all_projects(
                config,
                projects,
                move |project: &Utf8Path| -> anyhow::Result<()> {
                    commands::checkout::do_checkout(project, &branch)
                },
            )?;
        }
        GitCommand::Commit { message } => {
            let message = message.clone();
            runner::do_for_all_projects_with_check(
                config,
                projects,
                commands::commit::has_anything_to_commit,
                move |project: &Utf8Path| -> anyhow::Result<()> {
                    commands::commit::do_commit(project, &message)
                },
            )?;
        }
        GitCommand::SubmoduleUpdate => {
            runner::do_for_all_projects(config, projects, commands::submodule::submodule_update)?;
        }
        GitCommand::Blame { file } => {
            let file = file.clone();
            let file_for_check = file.clone();
            runner::do_for_all_projects_with_check(
                config,
                projects,
                move |project: &Utf8Path| commands::blame::has_file(project, &file_for_check),
                move |project: &Utf8Path| -> anyhow::Result<()> {
                    commands::blame::do_blame(project, &file)
                },
            )?;
        }
        GitCommand::Grep { regexp, files } => {
            // A data command: a repo without a match prints nothing at all.
            let regexp = regexp.clone();
            let files = *files;
            runner::print_if_data(config, projects, move |project: &Utf8Path| {
                commands::grep::do_grep(project, &regexp, files)
            })?;
        }
    }
    Ok(())
}
