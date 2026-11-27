//! rsmultigit: Manage multiple git repositories at once.

mod cli;
mod commands;
mod config;
mod git_url;
mod github;
mod runner;
mod subprocess_utils;

use camino::{Utf8Path, Utf8PathBuf};

use anyhow::Result;
use clap::Parser;
use clap::error::{ContextKind, ContextValue, ErrorKind};

use cli::{
    BranchWhat, BuildWhat, CargoWhat, CheckCommand, CleanWhat, Cli, Commands, CountWhat,
    GhSingleWhat, GhWhat, GitCommand, ListCommand, NpmWhat, ReleaseType, ResetWhat,
    RsconstructWhat, RuleKind, SetupCommand, StashWhat, TagWhat, UvWhat,
};
use commands::check_run::{self, CheckExistsOpts, CheckSameOpts, Interact};
use commands::check_submodules::{self, CheckSubmodulesOpts};
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
            anstream::eprint!("{}", cli::long_help_for(&args).ansi());
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

/// `gh-single`: GitHub operations on the one repo containing the current
/// directory, for actions that make no sense fleet-wide (one browser tab, not
/// hundreds). Needs no config file and ignores the configured repo list.
fn run_gh_single_command(what: GhSingleWhat) -> Result<()> {
    let project = commands::gh::current_repo()?;
    if !commands::gh::check_github(&project)? {
        anyhow::bail!("{project} has no github.com remote");
    }
    match what {
        GhSingleWhat::OpenSite => commands::gh::open_site(&project),
    }
}

/// An infallible per-repo precondition (`true` = act on this repo).
type InfallibleCheckFn = fn(&Utf8Path) -> bool;
/// A build action; all take the effective --venv flag.
type BuildFn = fn(&Utf8Path, bool) -> anyhow::Result<()>;

/// Adapt an infallible per-repo precondition to the fallible shape the
/// runners take.
fn infallible(check: InfallibleCheckFn) -> impl Fn(&Utf8Path) -> Result<bool> + Sync {
    move |project| Ok(check(project))
}

/// `gh`: GitHub operations over every repo.
fn run_gh_command(
    config: &AppConfig,
    projects: &[Utf8PathBuf],
    what: GhWhat,
    keep: Option<usize>,
    dry_run: bool,
    no_summary: bool,
) -> Result<()> {
    if dry_run && !what.takes_dry_run() {
        anyhow::bail!("--dry-run only applies to `gh sync-metadata`");
    }
    if keep.is_some() && !what.takes_keep() {
        anyhow::bail!("--keep only applies to the `gh prune-ci-*` variants");
    }
    if no_summary && !what.prints_summary() {
        anyhow::bail!("--no-summary only applies to the `gh prune-ci-*` variants");
    }
    // The prune-ci-* variants add every repo's counts here; the
    // summary is printed once the run is over, whether or not a
    // repo failed, and before the error (if any) ends the program.
    let summary = commands::gh::PruneSummary::new();
    let keep = keep.unwrap_or(4);
    let result = match what {
        GhWhat::PruneCiRestObo => runner::do_for_all_projects_with_check(
            config,
            projects,
            commands::gh::check_github,
            |project: &Utf8Path| -> anyhow::Result<()> {
                commands::gh::prune_ci_rest_obo(project, keep, &summary)
            },
        ),
        GhWhat::PruneCiGqlObo => runner::do_for_all_projects_with_check(
            config,
            projects,
            commands::gh::check_github,
            |project: &Utf8Path| -> anyhow::Result<()> {
                commands::gh::prune_ci_gql_obo(project, keep, &summary)
            },
        ),
        GhWhat::PruneCiGqlBatch => {
            // Fleet-level: it batches across repos, so it cannot be
            // a per-repo action; it prints through the runner itself.
            commands::gh_batch::prune_ci_gql_batch(config, projects, keep, &summary)
        }
        GhWhat::Artifacts => runner::print_if_data(config, projects, commands::gh::artifacts),
        GhWhat::LastWorkflowState => {
            runner::print_if_data(config, projects, commands::gh::last_workflow_state)
        }
        GhWhat::SyncMetadata => {
            // A data runner, although it writes to GitHub: the point of
            // the command is to print only the repos where something
            // differs, and a silent repo is one that is already in sync.
            runner::print_if_data(config, projects, move |project: &Utf8Path| {
                commands::gh::sync_metadata(project, dry_run)
            })
        }
    };
    if what.prints_summary() && !no_summary {
        summary.print();
    }
    result?;
    Ok(())
}

/// `build`: the command line's method, else the config file's default.
fn run_build_command(
    config: &AppConfig,
    file_config: &commands::check::CheckConfig,
    config_path: &Utf8Path,
    projects: &[Utf8PathBuf],
    what: Option<BuildWhat>,
) -> Result<()> {
    // Command line wins; otherwise fall back to the config file's
    // `default_build_method`; otherwise it's a usage error.
    let Some(what) = what.or(file_config.default_build_method) else {
        anyhow::bail!(
            "build: no build method given and `default_build_method` is not set in {config_path}\n\
             usage: rsmultigit build <{}>\n\
             or add e.g. `default_build_method = \"rsconstruct\"` to the config file",
            BuildWhat::names().join("|")
        );
    };
    let (check_fn, build_fn): (InfallibleCheckFn, BuildFn) = match what {
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
    let venv = config.use_venv();
    runner::do_for_all_projects_with_check(
        config,
        projects,
        infallible(check_fn),
        move |project: &Utf8Path| -> anyhow::Result<()> { build_fn(project, venv) },
    )?;
    Ok(())
}

/// `uv`: lock/sync/build/publish where there is a pyproject.toml.
fn run_uv_command(
    config: &AppConfig,
    projects: &[Utf8PathBuf],
    what: UvWhat,
    upgrade: bool,
    check: bool,
) -> Result<()> {
    if upgrade && what != UvWhat::Lock {
        anyhow::bail!("--upgrade only applies to `uv lock`");
    }
    if check && what != UvWhat::Lock {
        anyhow::bail!("--check only applies to `uv lock`");
    }
    // uv picks its target environment from the repo dir itself, so the
    // global --venv flag deliberately does not reach these calls.
    match what {
        UvWhat::Lock => {
            runner::do_for_all_projects_with_check(
                config,
                projects,
                infallible(commands::uv::check_pyproject),
                move |project: &Utf8Path| -> anyhow::Result<()> {
                    commands::uv::lock(project, upgrade, check)
                },
            )?;
        }
        UvWhat::Sync => {
            runner::do_for_all_projects_with_check(
                config,
                projects,
                infallible(commands::uv::check_pyproject),
                commands::uv::sync,
            )?;
        }
        UvWhat::Build => {
            runner::do_for_all_projects_with_check(
                config,
                projects,
                infallible(commands::uv::check_pyproject),
                commands::uv::build,
            )?;
        }
        UvWhat::Publish => {
            runner::do_for_all_projects_with_check(
                config,
                projects,
                infallible(commands::uv::check_pyproject),
                commands::uv::publish,
            )?;
        }
    }
    Ok(())
}

/// The flags of `cargo <what>`, bundled to keep the runner's signature small.
struct CargoOpts<'a> {
    what: CargoWhat,
    release: bool,
    profile: Option<&'a str>,
    check: bool,
    release_type: Option<ReleaseType>,
}

/// `cargo`: the cargo operations over every repo with a Cargo.toml.
fn run_cargo_command(
    config: &AppConfig,
    file_config: &commands::check::CheckConfig,
    projects: &[Utf8PathBuf],
    opts: &CargoOpts<'_>,
) -> Result<()> {
    let CargoOpts {
        what,
        release,
        profile,
        check,
        release_type,
    } = *opts;
    if release_type.is_some() && !what.takes_type() {
        anyhow::bail!("--type only applies to `cargo release`");
    }
    if (release || profile.is_some()) && !what.takes_release() {
        anyhow::bail!(
            "--release and --profile only apply to `cargo build`, `check`, `clippy`, `test`, `nextest` and `doc`"
        );
    }
    let profile = commands::cargo::Profile::from_flags(release, profile);
    if check && !what.takes_check() {
        anyhow::bail!("--check only applies to `cargo fmt`");
    }
    if what == CargoWhat::Release {
        // Preflight once: cargo-release present, crates.io token in
        // hand. Only then start bumping versions.
        let release = commands::release::Release::prepare(
            release_type.unwrap_or(ReleaseType::Patch).as_str(),
            &file_config.crates_io_pass_entry,
        )?;
        runner::do_for_all_projects_with_check(
            config,
            projects,
            commands::release::check_publishable,
            |project: &Utf8Path| -> anyhow::Result<()> { release.run(project) },
        )?;
        return Ok(());
    }
    let venv = config.use_venv();
    runner::do_for_all_projects_with_check(
        config,
        projects,
        infallible(commands::build::check_cargo),
        move |project: &Utf8Path| -> anyhow::Result<()> {
            commands::cargo::run(project, venv, what, &profile, check)
        },
    )?;
    Ok(())
}

/// `npm`: the npm operations over every repo with a package.json.
fn run_npm_command(
    config: &AppConfig,
    projects: &[Utf8PathBuf],
    what: NpmWhat,
    fix: bool,
) -> Result<()> {
    if fix && !what.takes_fix() {
        anyhow::bail!("--fix only applies to `npm audit`");
    }
    let venv = config.use_venv();
    runner::do_for_all_projects_with_check(
        config,
        projects,
        infallible(commands::npm::check_package_json),
        move |project: &Utf8Path| -> anyhow::Result<()> {
            commands::npm::run(project, venv, what, fix)
        },
    )?;
    Ok(())
}

/// `version`: the crate version and the build metadata `build.rs` captured.
fn print_version() {
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
}

/// `setup`: neither form needs (nor may require) a config file: producing one
/// is how a fresh user bootstraps their ~/.config/rsmultigit/config.toml.
fn run_setup_command(command: &SetupCommand) -> Result<()> {
    match command {
        SetupCommand::ConfigSample => {
            print!("{}", include_str!("../assets/config-example.toml"));
            Ok(())
        }
        SetupCommand::Interactive {
            repos_dir,
            build,
            no_build,
            overwrite,
        } => {
            let opts = commands::setup::SetupOpts {
                repos_dir: repos_dir.clone(),
                build: match (build, no_build) {
                    (Some(m), _) => commands::setup::BuildSetting::Method(*m),
                    (None, true) => commands::setup::BuildSetting::NoDefault,
                    (None, false) => commands::setup::BuildSetting::Ask,
                },
                overwrite: *overwrite,
            };
            let config_path = commands::check::default_config_path()?;
            commands::setup::run(&opts, &config_path, &mut std::io::stdout().lock())
        }
    }
}

fn main() -> Result<()> {
    let cli = parse_cli();

    // Handle commands that don't need project discovery
    if let Commands::GhSingle { what } = &cli.command {
        return run_gh_single_command(*what);
    }
    if let Commands::Complete { shell } = &cli.command {
        cli::print_completions(*shell);
        return Ok(());
    }
    if matches!(&cli.command, Commands::Version) {
        print_version();
        return Ok(());
    }
    if let Commands::Setup { command } = &cli.command {
        return run_setup_command(command);
    }

    let config = cli.global.clone();

    // Repo list comes from ~/.config/rsmultigit/config.toml for every command.
    let config_path = commands::check::default_config_path()?;
    let file_config = commands::check::load_config(&config_path)?;
    let projects = commands::check::resolve_repos(&file_config)?;

    if let Commands::Check { command } = &cli.command {
        return run_check_command(&config, &file_config, &projects, command);
    }

    match &cli.command {
        Commands::List { command } => run_list_command(&config, &projects, *command)?,

        // ── do_for_all_projects ──
        Commands::Git { command } => run_git_command(&config, &projects, command)?,
        Commands::Status => run_git_command(&config, &projects, &GitCommand::Status)?,
        Commands::Run { command } => {
            let command = command.clone();
            let venv = config.use_venv();
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
            no_summary,
        } => run_gh_command(&config, &projects, *what, *keep, *dry_run, *no_summary)?,

        // ── build commands ──
        Commands::Build { what } => {
            run_build_command(&config, &file_config, &config_path, &projects, *what)?;
        }
        Commands::Rsconstruct { what } => match what {
            RsconstructWhat::ToolLockUpgrade => {
                let venv = config.use_venv();
                runner::do_for_all_projects_with_check(
                    &config,
                    &projects,
                    infallible(commands::rsconstruct::check_rsconstruct),
                    move |project: &Utf8Path| -> anyhow::Result<()> {
                        commands::rsconstruct::tool_lock_upgrade(project, venv)
                    },
                )?;
            }
        },

        Commands::Uv {
            what,
            upgrade,
            check,
        } => run_uv_command(&config, &projects, *what, *upgrade, *check)?,

        Commands::Cargo {
            what,
            release,
            profile,
            check,
            release_type,
        } => run_cargo_command(
            &config,
            &file_config,
            &projects,
            &CargoOpts {
                what: *what,
                release: *release,
                profile: profile.as_deref(),
                check: *check,
                release_type: *release_type,
            },
        )?,

        Commands::Npm { what, fix } => run_npm_command(&config, &projects, *what, *fix)?,

        Commands::GhSingle { .. }
        | Commands::Check { .. }
        | Commands::Complete { .. }
        | Commands::Setup { .. }
        | Commands::Version => unreachable!("handled above"),
    }

    Ok(())
}

/// `check all`: the three checks (same, exists, submodules) in one verdict.
fn run_check_all(
    config: &AppConfig,
    file_config: &commands::check::CheckConfig,
    projects: &[Utf8PathBuf],
    only_failed: bool,
    allow_empty: bool,
) -> Result<i32> {
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    // All three parts always run: the point of `check all` is one
    // verdict over every invariant, so a failure in one must not hide
    // the state of the others. An empty-rule bail in `same` or
    // `exists` still propagates as an error, since that is a config
    // bug rather than drift.
    let same = check_run::run_check_same(
        config,
        file_config,
        projects,
        &CheckSameOpts {
            requested: &[],
            requested_re: &[],
            only_failed,
            allow_empty,
            interact: Interact::default(),
        },
        &mut stdin.lock(),
        &mut stdout.lock(),
    )?;
    let exists = check_run::run_check_exists(
        config,
        file_config,
        projects,
        &CheckExistsOpts {
            requested: &[],
            requested_re: &[],
            only_failed,
            allow_empty,
        },
        &mut stdout.lock(),
    )?;
    let submodules = check_submodules::run_check_submodules(
        config,
        file_config,
        projects,
        &CheckSubmodulesOpts {
            requested: &[],
            requested_re: &[],
            only_failed,
            allow_empty,
        },
        &mut stdout.lock(),
    )?;
    Ok(i32::from(same != 0 || exists != 0 || submodules != 0))
}

/// The `check` group. The check commands are organised by rule rather than
/// by repo and own their exit codes, so they bypass the runners. Prompts
/// (--diff, --copy, --fix-missing) are served from stdin; everything is
/// written to stdout.
fn run_check_command(
    config: &AppConfig,
    file_config: &commands::check::CheckConfig,
    projects: &[Utf8PathBuf],
    command: &CheckCommand,
) -> Result<()> {
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let exit_code = match command {
        CheckCommand::Same {
            checks,
            checks_re,
            only_failed,
            diff,
            copy,
            allow_empty,
            fix_missing,
        } => check_run::run_check_same(
            config,
            file_config,
            projects,
            &CheckSameOpts {
                requested: checks,
                requested_re: checks_re,
                only_failed: *only_failed,
                allow_empty: *allow_empty,
                interact: Interact {
                    show_diff: *diff,
                    do_copy: *copy,
                    do_fix_missing: *fix_missing,
                },
            },
            &mut stdin.lock(),
            &mut stdout.lock(),
        )?,
        CheckCommand::Exists {
            checks,
            checks_re,
            only_failed,
            allow_empty,
        } => check_run::run_check_exists(
            config,
            file_config,
            projects,
            &CheckExistsOpts {
                requested: checks,
                requested_re: checks_re,
                only_failed: *only_failed,
                allow_empty: *allow_empty,
            },
            &mut stdout.lock(),
        )?,
        CheckCommand::Submodules {
            checks,
            checks_re,
            only_failed,
            allow_empty,
        } => check_submodules::run_check_submodules(
            config,
            file_config,
            projects,
            &CheckSubmodulesOpts {
                requested: checks,
                requested_re: checks_re,
                only_failed: *only_failed,
                allow_empty: *allow_empty,
            },
            &mut stdout.lock(),
        )?,
        CheckCommand::All {
            only_failed,
            allow_empty,
        } => run_check_all(config, file_config, projects, *only_failed, *allow_empty)?,
        CheckCommand::List { kind } => {
            let names: Vec<&str> = match kind {
                RuleKind::Same => file_config.check.iter().map(|r| r.name.as_str()).collect(),
                RuleKind::Exists => file_config.exists.iter().map(|r| r.name.as_str()).collect(),
                RuleKind::Submodules => file_config
                    .submodule
                    .iter()
                    .map(|r| r.name.as_str())
                    .collect(),
            };
            for name in names {
                println!("{name}");
            }
            return Ok(());
        }
    };
    std::process::exit(exit_code);
}

/// The `list` group: `repos` prints every configured repo, the rest print
/// the repos in a given state, one path per line.
fn run_list_command(
    config: &AppConfig,
    projects: &[Utf8PathBuf],
    command: ListCommand,
) -> Result<()> {
    let test_fn: fn(&Utf8Path) -> anyhow::Result<bool> = match command {
        ListCommand::Repos => {
            // Every repo, no git inspection. --verbose re-enables the standard
            // [project]\n<data> format for consistency with other commands.
            for project in projects {
                if config.verbose && !config.terse && !config.no_header {
                    println!("[{project}]");
                }
                println!("{project}");
            }
            return Ok(());
        }
        ListCommand::Dirty => commands::count::is_dirty,
        ListCommand::Untracked => commands::count::has_untracked,
        ListCommand::Unsynchronized => commands::count::non_synchronized,
        ListCommand::Ahead => commands::count::is_ahead,
        ListCommand::Behind => commands::count::is_behind,
    };
    runner::print_matching(config, projects, test_fn)
}

/// A per-repo precondition (`true` = act on this repo) and a per-repo action,
/// as `runner::do_for_all_projects_with_check` takes them.
type CheckFn = fn(&Utf8Path) -> anyhow::Result<bool>;
type ActionFn = fn(&Utf8Path) -> anyhow::Result<()>;

/// `git clean`: hard or soft, or a preview of it.
fn git_clean(
    config: &AppConfig,
    projects: &[Utf8PathBuf],
    what: &CleanWhat,
    dry_run: bool,
) -> Result<()> {
    use commands::clean::{
        clean_hard, clean_soft, preview_clean_hard, preview_clean_soft, would_clean_hard,
        would_clean_soft,
    };
    let (check_fn, run_fn, preview_fn): (CheckFn, ActionFn, ActionFn) = match what {
        CleanWhat::Hard => (would_clean_hard, clean_hard, preview_clean_hard),
        CleanWhat::Soft => (would_clean_soft, clean_soft, preview_clean_soft),
    };
    let action = if dry_run { preview_fn } else { run_fn };
    runner::do_for_all_projects_with_check(config, projects, check_fn, action)?;

    Ok(())
}

/// `git stash push|pop` where there is something to stash or pop.
fn git_stash(config: &AppConfig, projects: &[Utf8PathBuf], what: &StashWhat) -> Result<()> {
    let (check_fn, stash_fn): (CheckFn, ActionFn) = match what {
        StashWhat::Push => (
            commands::stash::has_changes_to_stash,
            commands::stash::stash_push,
        ),
        StashWhat::Pop => (commands::stash::has_own_stash, commands::stash::stash_pop),
    };
    runner::do_for_all_projects_with_check(config, projects, check_fn, stash_fn)?;

    Ok(())
}

/// `git reset`: hard or mixed, or a preview of it.
fn git_reset(
    config: &AppConfig,
    projects: &[Utf8PathBuf],
    what: &ResetWhat,
    dry_run: bool,
) -> Result<()> {
    use commands::reset::{
        preview_reset_hard, preview_reset_mixed, reset_hard, reset_mixed, would_reset_hard,
        would_reset_mixed,
    };
    let (check_fn, run_fn, preview_fn): (CheckFn, ActionFn, ActionFn) = match what {
        ResetWhat::Hard => (would_reset_hard, reset_hard, preview_reset_hard),
        ResetWhat::Mixed => (would_reset_mixed, reset_mixed, preview_reset_mixed),
    };
    let action = if dry_run { preview_fn } else { run_fn };
    runner::do_for_all_projects_with_check(config, projects, check_fn, action)?;

    Ok(())
}

/// `git tag`: create tags, or count the repos that have one.
fn git_tag(config: &AppConfig, projects: &[Utf8PathBuf], what: &TagWhat) -> Result<()> {
    match what {
        TagWhat::Local => runner::do_for_all_projects(config, projects, commands::tag::tag_local),
        TagWhat::Remote => runner::do_for_all_projects(config, projects, commands::tag::tag_remote),
        TagWhat::HasLocal => runner::do_count(config, projects, commands::tag::tag_has_local),
        TagWhat::HasRemote => runner::do_count(config, projects, commands::tag::tag_has_remote),
    }
}

/// `git blame <file>` in the repos that have the file.
fn git_blame(config: &AppConfig, projects: &[Utf8PathBuf], file: &str) -> Result<()> {
    let file = file.to_string();
    let file_for_check = file.clone();
    runner::do_for_all_projects_with_check(
        config,
        projects,
        move |project: &Utf8Path| Ok(commands::blame::has_file(project, &file_for_check)),
        move |project: &Utf8Path| -> anyhow::Result<()> {
            commands::blame::do_blame(project, &file)
        },
    )?;

    Ok(())
}

/// `git commit` in the repos with something to commit.
fn git_commit(config: &AppConfig, projects: &[Utf8PathBuf], message: &str) -> Result<()> {
    let message = message.to_string();
    runner::do_for_all_projects_with_check(
        config,
        projects,
        commands::commit::has_anything_to_commit,
        move |project: &Utf8Path| -> anyhow::Result<()> {
            commands::commit::do_commit(project, &message)
        },
    )?;

    Ok(())
}

/// `git count`: how many repos are in a given state.
fn git_count(config: &AppConfig, projects: &[Utf8PathBuf], what: &CountWhat) -> Result<()> {
    let test_fn: fn(&Utf8Path) -> anyhow::Result<bool> = match what {
        CountWhat::Dirty => commands::count::is_dirty,
        CountWhat::Untracked => commands::count::has_untracked,
        CountWhat::Unsynchronized => commands::count::non_synchronized,
        CountWhat::Ahead => commands::count::is_ahead,
        CountWhat::Behind => commands::count::is_behind,
    };
    runner::do_count(config, projects, test_fn)?;

    Ok(())
}

/// `git branch`: local, remote or GitHub branches.
fn git_branch(config: &AppConfig, projects: &[Utf8PathBuf], what: &BranchWhat) -> Result<()> {
    let branch_fn: fn(&Utf8Path) -> anyhow::Result<()> = match what {
        BranchWhat::Local => commands::branch::branch_local,
        BranchWhat::Remote => commands::branch::branch_remote,
        BranchWhat::Github => commands::branch::branch_github,
    };
    runner::do_for_all_projects(config, projects, branch_fn)?;

    Ok(())
}

/// `git status`: one line per repo, or the full listing with --verbose.
fn git_status(config: &AppConfig, projects: &[Utf8PathBuf]) -> Result<()> {
    // Default: one-line summary of each repo's situation (counts of
    // modified/staged/untracked files, ahead/behind). --verbose switches
    // to the full `git status -s` per-file output.
    if config.verbose {
        runner::print_if_data(config, projects, commands::status::do_status)?;
    } else {
        runner::print_if_data(config, projects, commands::status::do_status_summary)?;
    }

    Ok(())
}

/// `git checkout <branch>` in every repo.
fn git_checkout(config: &AppConfig, projects: &[Utf8PathBuf], branch: &str) -> Result<()> {
    let branch = branch.to_string();
    runner::do_for_all_projects(
        config,
        projects,
        move |project: &Utf8Path| -> anyhow::Result<()> {
            commands::checkout::do_checkout(project, &branch)
        },
    )?;

    Ok(())
}

/// `git log -<count>` in every repo.
fn git_log(config: &AppConfig, projects: &[Utf8PathBuf], count: u32) -> Result<()> {
    runner::do_for_all_projects(
        config,
        projects,
        move |project: &Utf8Path| -> anyhow::Result<()> { commands::log::do_log(project, count) },
    )?;

    Ok(())
}

/// The `git` group: the libgit2 reports, then the subcommands that run the
/// git command of the same name in every repo.
fn run_git_command(
    config: &AppConfig,
    projects: &[Utf8PathBuf],
    command: &GitCommand,
) -> Result<()> {
    match command {
        // ── do_count ──
        GitCommand::Count { what } => git_count(config, projects, what)?,

        // ── print_if_data ──
        GitCommand::Status => git_status(config, projects)?,
        GitCommand::Dirty => {
            runner::print_if_data(config, projects, commands::status::do_dirty)?;
        }
        GitCommand::Age => {
            runner::print_if_data(config, projects, commands::age::do_age)?;
        }
        GitCommand::Authors => {
            runner::print_if_data(config, projects, commands::authors::do_authors)?;
        }
        GitCommand::Size => {
            runner::print_if_data(config, projects, commands::size::do_size)?;
        }
        GitCommand::LastTag => {
            runner::print_if_data(config, projects, commands::last_tag::do_last_tag)?;
        }

        // ── git commands ──
        GitCommand::Config { key } => {
            let key = key.clone();
            runner::print_if_data(config, projects, move |project: &Utf8Path| {
                commands::config::do_config(project, &key)
            })?;
        }
        GitCommand::Branch { what } => git_branch(config, projects, what)?,
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
        GitCommand::Clean { what, dry_run } => git_clean(config, projects, what, *dry_run)?,
        GitCommand::Stash { what } => git_stash(config, projects, what)?,
        GitCommand::Restore { dry_run } => {
            use commands::restore::{preview_restore, restore, would_restore};
            let action: ActionFn = if *dry_run { preview_restore } else { restore };
            runner::do_for_all_projects_with_check(config, projects, would_restore, action)?;
        }
        GitCommand::Reset { what, dry_run } => git_reset(config, projects, what, *dry_run)?,
        GitCommand::Diff => {
            runner::do_for_all_projects(config, projects, commands::diff::do_diff)?;
        }
        GitCommand::Log { count } => git_log(config, projects, *count)?,
        GitCommand::Tag { what } => git_tag(config, projects, what)?,
        GitCommand::Remote => {
            runner::do_for_all_projects(config, projects, commands::remote::do_remote)?;
        }
        GitCommand::RemotePrune => {
            runner::do_for_all_projects(config, projects, commands::prune::do_prune)?;
        }
        GitCommand::Gc => {
            runner::do_for_all_projects(config, projects, commands::gc::do_gc)?;
        }
        GitCommand::Checkout { branch } => git_checkout(config, projects, branch)?,
        GitCommand::Commit { message } => git_commit(config, projects, message)?,
        GitCommand::SubmoduleUpdate => {
            runner::do_for_all_projects(config, projects, commands::submodule::submodule_update)?;
        }
        GitCommand::Blame { file } => git_blame(config, projects, file)?,
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
