mod cli;
mod commands;
mod config;
mod runner;
mod subprocess_utils;

use std::path::Path;

use anyhow::{Context, Result};
use clap::Parser;

use cli::{
    BranchWhat, BuildWhat, CleanWhat, Cli, Commands, CountWhat, ResetWhat, StashWhat, TagWhat,
};
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

    if let Commands::CheckSame {
        checks,
        diff,
        copy,
        fix_missing,
    } = &cli.command
    {
        let exit_code = run_check_same(
            &config,
            &file_config,
            &projects,
            checks,
            *diff,
            *copy,
            *fix_missing,
        )?;
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
            runner::do_for_all_projects(
                &config,
                &projects,
                move |project: &Path| -> anyhow::Result<bool> {
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
            runner::do_for_all_projects(
                &config,
                &projects,
                move |project: &Path| -> anyhow::Result<bool> {
                    commands::log::do_log(project, count)
                },
            )?;
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
                move |project: &Path| -> anyhow::Result<bool> {
                    commands::checkout::do_checkout(project, &branch)
                },
            )?;
        }
        Commands::Commit { message } => {
            let message = message.clone();
            runner::do_for_all_projects(
                &config,
                &projects,
                move |project: &Path| -> anyhow::Result<bool> {
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
                move |project: &Path| -> anyhow::Result<bool> {
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
                move |project: &Path| -> anyhow::Result<bool> {
                    commands::grep::do_grep(project, &regexp, files)
                },
            )?;
        }

        // ── build commands ──
        Commands::Build { what } => {
            type ProjectFn = fn(&Path) -> anyhow::Result<bool>;
            let (check_fn, build_fn): (ProjectFn, ProjectFn) = match what {
                BuildWhat::Bootstrap => (
                    commands::build::check_not_disabled,
                    commands::build::build_bootstrap,
                ),
                BuildWhat::Pydmt => (commands::build::check_pydmt, commands::build::build_pydmt),
                BuildWhat::Make => (
                    commands::build::check_not_disabled,
                    commands::build::build_make,
                ),
                BuildWhat::VenvMake => (
                    commands::build::check_not_disabled,
                    commands::build::build_venv_make,
                ),
                BuildWhat::VenvPydmt => (
                    commands::build::check_pydmt,
                    commands::build::build_venv_pydmt,
                ),
                BuildWhat::PydmtBuildVenv => (
                    commands::build::check_pydmt,
                    commands::build::build_pydmt_build_venv,
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
            runner::do_for_all_projects_with_check(&config, &projects, check_fn, build_fn)?;
        }

        Commands::CheckSame { .. } => unreachable!("handled above"),
        Commands::Complete { .. } => unreachable!("handled above"),
        Commands::ConfigExample => unreachable!("handled above"),
        Commands::Version => unreachable!("handled above"),
    }

    Ok(())
}

/// Run the check-same command. Returns the process exit code.
///
/// `requested` is the (possibly empty) list of rule names from `--checks`.
/// Empty → run all enabled rules. Non-empty → run exactly those rules, even if
/// they are `enabled = false`; any unknown name is a hard error.
///
/// When `copy` is set, interactive prompts are served. In that mode the overall
/// exit code is always 0 regardless of mismatches — `--copy` is a tool to fix
/// drift, not a pass/fail check.
fn run_check_same(
    app: &AppConfig,
    file_config: &commands::check::CheckConfig,
    projects: &[std::path::PathBuf],
    requested: &[String],
    show_diff: bool,
    do_copy: bool,
    do_fix_missing: bool,
) -> Result<i32> {
    use commands::check;
    use commands::interactive;

    let rules: Vec<&check::Rule> = if requested.is_empty() {
        file_config.check.iter().filter(|r| r.enabled).collect()
    } else {
        let known: std::collections::HashSet<&str> =
            file_config.check.iter().map(|r| r.name.as_str()).collect();
        let unknown: Vec<&String> = requested
            .iter()
            .filter(|name| !known.contains(name.as_str()))
            .collect();
        if !unknown.is_empty() {
            let joined = unknown
                .iter()
                .map(|s| format!("{s:?}"))
                .collect::<Vec<_>>()
                .join(", ");
            anyhow::bail!("unknown check name(s): {joined}");
        }
        requested
            .iter()
            .filter_map(|name| file_config.check.iter().find(|r| &r.name == name))
            .collect()
    };

    if rules.is_empty() {
        if app.verbose {
            println!("no rules to check");
        }
        return Ok(0);
    }

    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut any_mismatch = false;
    let mut quit_requested = false;

    for rule in rules {
        if quit_requested {
            break;
        }
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
        let mut suffix_parts: Vec<String> = Vec::new();
        if !result.must_have_violations.is_empty() {
            suffix_parts.push(format!("{} missing", result.must_have_violations.len()));
        }
        if !result.skipped.is_empty() {
            suffix_parts.push(format!("{} skipped", result.skipped.len()));
        }
        let suffix = if suffix_parts.is_empty() {
            String::new()
        } else {
            format!(" ({})", suffix_parts.join(", "))
        };
        println!(
            "{} files, {} groups{suffix}",
            result.total_files,
            result.groups.len(),
        );
        if !app.no_output {
            for (i, group) in result.groups.iter().enumerate() {
                let label = interactive::group_label(i);
                println!("  group {label} ({} files):", group.len());
                for file in group {
                    println!("    {}", file.display());
                }
            }

            if !result.must_have_violations.is_empty() {
                println!("  missing in:");
                for repo in &result.must_have_violations {
                    println!("    {}", repo.display());
                }
            }

            if show_diff {
                match run_diff(&result, &mut stdin.lock(), &mut stdout.lock())? {
                    FlowControl::Quit => {
                        quit_requested = true;
                        continue;
                    }
                    FlowControl::Continue => {}
                }
            }

            if do_copy {
                match run_copy(&result, &mut stdin.lock(), &mut stdout.lock())? {
                    FlowControl::Quit => {
                        quit_requested = true;
                        continue;
                    }
                    FlowControl::Continue => {}
                }
            }

            if do_fix_missing && !result.must_have_violations.is_empty() {
                match run_fix_missing(&result, &mut stdin.lock(), &mut stdout.lock())? {
                    FlowControl::Quit => {
                        quit_requested = true;
                        continue;
                    }
                    FlowControl::Continue => {}
                }
            }
        }
    }

    if do_copy || do_fix_missing {
        Ok(0)
    } else {
        Ok(if any_mismatch { 1 } else { 0 })
    }
}

#[derive(Debug, PartialEq, Eq)]
enum FlowControl {
    Continue,
    Quit,
}

/// Run the diff flow for a rule with at least two content groups.
/// - 2 groups: auto-pair and diff, no prompting.
/// - 3+ groups: prompt for from/to, diff, then offer to diff another pair.
fn run_diff<R: std::io::BufRead, W: std::io::Write>(
    result: &commands::check::RuleResult,
    reader: &mut R,
    writer: &mut W,
) -> Result<FlowControl> {
    use commands::interactive::{Choice, confirm, pick_group};

    let n = result.groups.len();
    if n == 2 {
        emit_pair_diff(result, 0, 1, writer);
        return Ok(FlowControl::Continue);
    }

    loop {
        let from = match pick_group(&mut *reader, &mut *writer, "diff from group?", n, None)? {
            Choice::Value(i) => i,
            Choice::Skip => return Ok(FlowControl::Continue),
            Choice::Quit => return Ok(FlowControl::Quit),
        };
        let to = match pick_group(&mut *reader, &mut *writer, "diff to group?", n, Some(from))? {
            Choice::Value(i) => i,
            Choice::Skip => return Ok(FlowControl::Continue),
            Choice::Quit => return Ok(FlowControl::Quit),
        };
        emit_pair_diff(result, from, to, writer);

        if !confirm(&mut *reader, &mut *writer, "diff another pair?")? {
            return Ok(FlowControl::Continue);
        }
    }
}

/// Write a unified diff between representatives of `groups[a]` and `groups[b]`
/// to `writer`. Handles I/O errors and non-UTF-8 content gracefully.
fn emit_pair_diff<W: std::io::Write>(
    result: &commands::check::RuleResult,
    a_idx: usize,
    b_idx: usize,
    writer: &mut W,
) {
    let a = &result.groups[a_idx][0];
    let b = &result.groups[b_idx][0];

    let a_bytes = match std::fs::read(a) {
        Ok(b) => b,
        Err(e) => {
            let _ = writeln!(writer, "  (could not read {}: {e})", a.display());
            return;
        }
    };
    let b_bytes = match std::fs::read(b) {
        Ok(b) => b,
        Err(e) => {
            let _ = writeln!(writer, "  (could not read {}: {e})", b.display());
            return;
        }
    };
    let (a_text, b_text) = match (std::str::from_utf8(&a_bytes), std::str::from_utf8(&b_bytes)) {
        (Ok(a), Ok(b)) => (a, b),
        _ => {
            let _ = writeln!(writer, "  (binary files differ, not shown)");
            return;
        }
    };

    let diff = similar::TextDiff::from_lines(a_text, b_text);
    let _ = write!(
        writer,
        "{}",
        diff.unified_diff()
            .context_radius(3)
            .header(&a.display().to_string(), &b.display().to_string())
    );
}

/// Run the interactive copy flow for a rule: prompt for "from" and "to" groups,
/// confirm, then overwrite every file in the "to" group with the content of a
/// representative from the "from" group (preserving the destination's mode).
fn run_copy<R: std::io::BufRead, W: std::io::Write>(
    result: &commands::check::RuleResult,
    reader: &mut R,
    writer: &mut W,
) -> Result<FlowControl> {
    use commands::interactive::{Choice, confirm, group_label, pick_group};

    let n = result.groups.len();
    let from = match pick_group(&mut *reader, &mut *writer, "copy from group?", n, None)? {
        Choice::Value(i) => i,
        Choice::Skip => return Ok(FlowControl::Continue),
        Choice::Quit => return Ok(FlowControl::Quit),
    };
    let to = match pick_group(&mut *reader, &mut *writer, "copy to group?", n, Some(from))? {
        Choice::Value(i) => i,
        Choice::Skip => return Ok(FlowControl::Continue),
        Choice::Quit => return Ok(FlowControl::Quit),
    };

    let src = &result.groups[from][0];
    let dst_group = &result.groups[to];
    let prompt = format!(
        "overwrite {} file(s) in group {} with content from {}?",
        dst_group.len(),
        group_label(to),
        src.display()
    );
    if !confirm(&mut *reader, &mut *writer, &prompt)? {
        let _ = writeln!(writer, "  (skipped)");
        return Ok(FlowControl::Continue);
    }

    for dst in dst_group {
        if let Err(e) = copy_preserving_mode(src, dst) {
            let _ = writeln!(
                writer,
                "  error: {} -> {}: {e}",
                src.display(),
                dst.display()
            );
        } else {
            let _ = writeln!(writer, "  copied -> {}", dst.display());
        }
    }
    Ok(FlowControl::Continue)
}

/// `fs::copy` replaces the destination's permissions with the source's. We want
/// the opposite — overwrite the *content* but keep the destination's mode.
fn copy_preserving_mode(src: &std::path::Path, dst: &std::path::Path) -> Result<()> {
    let original_mode = std::fs::metadata(dst)
        .with_context(|| format!("failed to stat {}", dst.display()))?
        .permissions();
    std::fs::copy(src, dst)
        .with_context(|| format!("failed to copy {} -> {}", src.display(), dst.display()))?;
    std::fs::set_permissions(dst, original_mode)
        .with_context(|| format!("failed to restore permissions on {}", dst.display()))?;
    Ok(())
}

/// Run the interactive fix-missing flow for a rule that has `must_have`
/// violations. Prompts for a "seed" group to copy from, then creates the file
/// in each violating repo, using plain `fs::copy` (so the new file inherits the
/// source's mode). Parent directories are created as needed.
///
/// When the rule has zero content groups (no repo has the file at all) there's
/// nothing to seed from — print a note and skip.
fn run_fix_missing<R: std::io::BufRead, W: std::io::Write>(
    result: &commands::check::RuleResult,
    reader: &mut R,
    writer: &mut W,
) -> Result<FlowControl> {
    use commands::interactive::{Choice, confirm, pick_group};

    if result.groups.is_empty() {
        let _ = writeln!(
            writer,
            "  (cannot --fix-missing: no repo has {} — nothing to seed from)",
            result.path
        );
        return Ok(FlowControl::Continue);
    }

    let n = result.groups.len();
    let from = match pick_group(
        &mut *reader,
        &mut *writer,
        "seed missing files from which group?",
        n,
        None,
    )? {
        Choice::Value(i) => i,
        Choice::Skip => return Ok(FlowControl::Continue),
        Choice::Quit => return Ok(FlowControl::Quit),
    };

    let src = &result.groups[from][0];
    let violators = &result.must_have_violations;
    let prompt = format!(
        "create {} file(s) using content from {}?",
        violators.len(),
        src.display()
    );
    if !confirm(&mut *reader, &mut *writer, &prompt)? {
        let _ = writeln!(writer, "  (skipped)");
        return Ok(FlowControl::Continue);
    }

    for repo in violators {
        let dst = repo.join(&result.path);
        if let Some(parent) = dst.parent()
            && let Err(e) = std::fs::create_dir_all(parent)
        {
            let _ = writeln!(
                writer,
                "  error: failed to create directory {}: {e}",
                parent.display()
            );
            continue;
        }
        match std::fs::copy(src, &dst) {
            Ok(_) => {
                let _ = writeln!(writer, "  created -> {}", dst.display());
            }
            Err(e) => {
                let _ = writeln!(
                    writer,
                    "  error: {} -> {}: {e}",
                    src.display(),
                    dst.display()
                );
            }
        }
    }
    Ok(FlowControl::Continue)
}
