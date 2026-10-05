use camino::{Utf8Path, Utf8PathBuf};
use std::io::{self, Write};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc;

use anyhow::{Context, Result};

use crate::config::AppConfig;

fn resolve_jobs(config: &AppConfig) -> usize {
    let n = if config.jobs == 0 {
        std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1)
    } else {
        config.jobs
    };
    n.max(1)
}

fn absolute(project: &Utf8Path, base: &Utf8Path) -> Utf8PathBuf {
    if project.is_absolute() {
        project.to_path_buf()
    } else {
        base.join(project)
    }
}

fn print_project_header(project: &Utf8Path) {
    println!("[{}]", project);
}

/// The `[project]` header line is suppressed when either `--no-header` or
/// `--terse` is set. `--terse` has a richer meaning in some runners (e.g.
/// switching `print_if_data` to "project-name only" mode); this helper only
/// captures the shared "should I emit the bracketed header?" rule.
fn headers_suppressed(config: &AppConfig) -> bool {
    config.no_header || config.terse
}

/// Report a per-repo error under `--no-stop` and count it, so the run can
/// carry on past it and still fail at the end (see [`no_stop_outcome`]).
fn report_no_stop(project: &Utf8Path, e: &anyhow::Error, failed: &mut usize) {
    eprintln!("error in {}: {e:#}", project);
    *failed += 1;
}

/// The final result of a `--no-stop` run: errors were reported as they
/// happened, but a run in which any repo failed must not exit 0, or scripts
/// cannot tell it from a clean one.
fn no_stop_outcome(failed: usize, total: usize) -> Result<()> {
    if failed == 0 {
        Ok(())
    } else {
        anyhow::bail!("{failed} of {total} repos failed")
    }
}

/// The current directory as a UTF-8 path, for resolving relative repo paths.
fn current_dir() -> Result<Utf8PathBuf> {
    let cwd = std::env::current_dir().context("failed to get current directory")?;
    Utf8PathBuf::from_path_buf(cwd)
        .map_err(|p| anyhow::anyhow!("current directory is not valid UTF-8: {}", p.display()))
}

/// Execute `work` across `projects`, optionally in parallel. Results are delivered
/// to `on_result` in input order on the calling thread so stdout stays ordered.
///
/// An error returned by `on_result` ends the run: no further results are
/// delivered, and workers stop claiming projects (those already in flight
/// finish). This matches the serial path, which returns at the first error.
/// Under `--no-stop` the callbacks report the error themselves and return
/// `Ok`, so everything runs to completion.
fn for_each_project_ordered<T, W, R>(
    jobs: usize,
    projects: &[Utf8PathBuf],
    work: W,
    mut on_result: R,
) -> Result<()>
where
    T: Send,
    W: Fn(&Utf8Path) -> Result<T> + Sync,
    R: FnMut(&Utf8PathBuf, Result<T>) -> Result<()>,
{
    if jobs <= 1 || projects.len() <= 1 {
        for project in projects {
            let result = work(project);
            on_result(project, result)?;
        }
        return Ok(());
    }

    let next = AtomicUsize::new(0);
    let cancel = AtomicBool::new(false);
    let (tx, rx) = mpsc::channel::<(usize, Result<T>)>();

    let pb = {
        use std::io::IsTerminal;
        if projects.len() > 1 && io::stderr().is_terminal() {
            let style = indicatif::ProgressStyle::default_bar()
                .template(
                    "{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} {msg}",
                )
                .expect("static progress-bar template is valid")
                .progress_chars("#>-");
            let pb = indicatif::ProgressBar::new(projects.len() as u64);
            pb.set_style(style);
            Some(pb)
        } else {
            None
        }
    };

    std::thread::scope(|scope| -> Result<()> {
        for _ in 0..jobs.min(projects.len()) {
            let tx = tx.clone();
            let next = &next;
            let cancel = &cancel;
            let work = &work;
            scope.spawn(move || {
                loop {
                    if cancel.load(Ordering::SeqCst) {
                        break;
                    }
                    let idx = next.fetch_add(1, Ordering::SeqCst);
                    if idx >= projects.len() {
                        break;
                    }
                    let result = work(&projects[idx]);
                    // A closed receiver means the main thread gave up on the
                    // run; nothing more to do here either.
                    if tx.send((idx, result)).is_err() {
                        break;
                    }
                }
            });
        }
        drop(tx);

        let mut buffer: Vec<Option<Result<T>>> = (0..projects.len()).map(|_| None).collect();
        let mut next_emit = 0usize;
        let mut outcome: Result<()> = Ok(());

        'receive: for (idx, result) in rx {
            buffer[idx] = Some(result);
            if let Some(pb) = &pb {
                pb.inc(1);
            }
            while next_emit < projects.len() && buffer[next_emit].is_some() {
                let result = buffer[next_emit].take().unwrap();
                let project = &projects[next_emit];
                let emitted = match &pb {
                    Some(pb) => pb.suspend(|| on_result(project, result)),
                    None => on_result(project, result),
                };
                next_emit += 1;
                if let Err(e) = emitted {
                    // Stop the run the way the serial path does: nothing
                    // further is delivered. Breaking drops the receiver, so
                    // workers exit on their next send; the flag stops those
                    // between jobs from claiming another.
                    cancel.store(true, Ordering::SeqCst);
                    outcome = Err(e);
                    break 'receive;
                }
            }
        }

        if let Some(pb) = &pb {
            pb.finish_and_clear();
        }

        outcome
    })
}

/// Runner for "count" commands. test_fn just returns bool — no subprocess output,
/// so parallelism is trivially safe.
pub fn do_count<F>(config: &AppConfig, projects: &[Utf8PathBuf], test_fn: F) -> Result<()>
where
    F: Fn(&Utf8Path) -> Result<bool> + Sync,
{
    let total = projects.len() as u32;
    // on_result runs only on the calling thread, so a plain counter suffices.
    let mut count = 0u32;
    let mut failed = 0usize;
    let jobs = resolve_jobs(config);

    for_each_project_ordered(
        jobs,
        projects,
        |project| test_fn(project).with_context(|| format!("error testing project {}", project)),
        |project, result| {
            let matches = match result {
                Ok(m) => m,
                Err(e) => {
                    if config.no_stop {
                        report_no_stop(project, &e, &mut failed);
                        return Ok(());
                    }
                    return Err(e);
                }
            };
            let should_print = if config.print_not { !matches } else { matches };
            if should_print {
                if !config.terse {
                    println!("{}", project);
                }
                count += 1;
            }
            Ok(())
        },
    )?;

    println!("{count}/{total}");
    no_stop_outcome(failed, projects.len())
}

/// Runner for "do for all projects" commands.
/// Parallel execution preserves per-project output ordering by capturing subprocess
/// stdout/stderr into a buffer and replaying on the main thread in input order.
pub fn do_for_all_projects<F>(config: &AppConfig, projects: &[Utf8PathBuf], action: F) -> Result<()>
where
    F: Fn(&Utf8Path) -> Result<()> + Sync,
{
    do_for_all_projects_with_check(config, projects, |_| Ok(true), action)
}

/// Like [`do_for_all_projects`], but `check` decides per repo whether the
/// action applies at all. A repo where it returns `Ok(false)` is skipped:
/// no header (unless `--verbose`), no output. This is the only place a
/// skip can be decided, because the serial path prints the header before
/// the action runs so that the action's live output lands under it; an
/// action cannot skip after the fact, which is why actions return `()`.
pub fn do_for_all_projects_with_check<C, F>(
    config: &AppConfig,
    projects: &[Utf8PathBuf],
    check: C,
    action: F,
) -> Result<()>
where
    C: Fn(&Utf8Path) -> Result<bool> + Sync,
    F: Fn(&Utf8Path) -> Result<()> + Sync,
{
    let base = current_dir()?;
    let jobs = resolve_jobs(config);
    let mut failed = 0usize;

    // Serial fast path: action writes live to inherited stdout/stderr. With
    // --no-output the action runs under capture and the buffer is dropped,
    // which is the same silencing the parallel path applies.
    if jobs <= 1 || projects.len() <= 1 {
        let show_header = !headers_suppressed(config);
        for project in projects {
            let abs = absolute(project, &base);

            if show_header && config.verbose {
                print_project_header(project);
            }

            let passed =
                match check(&abs).with_context(|| format!("error checking project {}", project)) {
                    Ok(p) => p,
                    Err(e) => {
                        if config.no_stop {
                            report_no_stop(project, &e, &mut failed);
                            continue;
                        }
                        return Err(e);
                    }
                };
            if !passed {
                continue;
            }

            if show_header && !config.verbose {
                print_project_header(project);
            }

            let outcome = if config.no_output {
                crate::subprocess_utils::enter_capture();
                let r = action(&abs);
                let captured = crate::subprocess_utils::leave_capture();
                r.map_err(|e| attach_captured(e, &captured))
            } else {
                action(&abs)
            };
            if let Err(e) = outcome.with_context(|| format!("error in project {}", project)) {
                if config.no_stop {
                    report_no_stop(project, &e, &mut failed);
                } else {
                    return Err(e);
                }
            }
        }
        return no_stop_outcome(failed, projects.len());
    }

    // Parallel path: subprocess output is captured per-thread by subprocess_utils
    // and replayed in project order here on the main thread.
    let projects_vec: Vec<Utf8PathBuf> = projects.to_vec();

    for_each_project_ordered(
        jobs,
        &projects_vec,
        |project| -> Result<(bool, Vec<u8>)> {
            let abs = absolute(project, &base);
            // Capture output on this worker thread for the duration of check+action.
            crate::subprocess_utils::enter_capture();
            let r: Result<bool> = (|| -> Result<bool> {
                let passed =
                    check(&abs).with_context(|| format!("error checking project {}", project))?;
                if !passed {
                    return Ok(false);
                }
                action(&abs).with_context(|| format!("error in project {}", project))?;
                Ok(true)
            })();
            let captured = crate::subprocess_utils::leave_capture();
            match r {
                Ok(passed) => Ok((passed, captured)),
                Err(e) => Err(attach_captured(e, &captured)),
            }
        },
        |project, result| -> Result<()> {
            match result {
                Ok((passed, captured)) => {
                    let show_header = !headers_suppressed(config);
                    if passed || (config.verbose && show_header) {
                        if show_header {
                            print_project_header(project);
                        }
                        if passed && !config.no_output {
                            let stdout = io::stdout();
                            let mut lock = stdout.lock();
                            lock.write_all(&captured).ok();
                        }
                    }
                    Ok(())
                }
                Err(e) => {
                    if config.no_stop {
                        report_no_stop(project, &e, &mut failed);
                        Ok(())
                    } else {
                        Err(e)
                    }
                }
            }
        },
    )?;
    no_stop_outcome(failed, projects.len())
}

/// Attach a failed action's captured subprocess output to its error, so a
/// failure that happened under capture is not reported without its cause.
fn attach_captured(e: anyhow::Error, captured: &[u8]) -> anyhow::Error {
    if captured.is_empty() {
        e
    } else {
        e.context(format!("captured: {}", String::from_utf8_lossy(captured)))
    }
}

/// Runner for "print projects that return data" commands.
pub fn print_if_data<F>(config: &AppConfig, projects: &[Utf8PathBuf], data_fn: F) -> Result<()>
where
    F: Fn(&Utf8Path) -> Result<Option<String>> + Sync,
{
    let base = current_dir()?;
    let jobs = resolve_jobs(config);
    let mut failed = 0usize;
    let projects_vec: Vec<Utf8PathBuf> = projects.to_vec();

    for_each_project_ordered(
        jobs,
        &projects_vec,
        |project| -> Result<Option<String>> {
            let abs = absolute(project, &base);
            data_fn(&abs).with_context(|| format!("error in project {}", project))
        },
        |project, result| -> Result<()> {
            let out = io::stdout();
            let mut out = out.lock();
            match result {
                Ok(Some(data)) => {
                    if !config.print_not {
                        if config.terse {
                            writeln!(out, "{}", project).ok();
                        } else {
                            if !config.no_header {
                                writeln!(out, "[{}]", project).ok();
                            }
                            if !config.no_output {
                                writeln!(out, "{data}").ok();
                            }
                        }
                    }
                    Ok(())
                }
                Ok(None) => {
                    if config.print_not || config.verbose {
                        if config.terse {
                            writeln!(out, "{}", project).ok();
                        } else if !config.no_header {
                            writeln!(out, "[{}]", project).ok();
                        }
                    }
                    Ok(())
                }
                Err(e) => {
                    if config.no_stop {
                        report_no_stop(project, &e, &mut failed);
                        Ok(())
                    } else {
                        Err(e)
                    }
                }
            }
        },
    )?;
    no_stop_outcome(failed, projects.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::atomic::{AtomicU32, Ordering};
    use tempfile::TempDir;

    fn default_config() -> AppConfig {
        AppConfig::default()
    }

    fn make_dirs(tmp: &camino::Utf8Path, names: &[&str]) -> Vec<Utf8PathBuf> {
        names
            .iter()
            .map(|n| {
                let p = tmp.join(n);
                fs::create_dir_all(&p).unwrap();
                p
            })
            .collect()
    }

    #[test]
    fn do_count_counts_matching() {
        let tmp = TempDir::new().unwrap();
        let projects = make_dirs(
            camino::Utf8Path::from_path(tmp.path()).unwrap(),
            &["a", "b", "c"],
        );
        let config = default_config();

        let result = do_count(&config, &projects, |p| {
            let name = p.file_name().unwrap();
            Ok(name == "a" || name == "c")
        });
        assert!(result.is_ok());
    }

    #[test]
    fn do_count_print_not_inverts() {
        let tmp = TempDir::new().unwrap();
        let projects = make_dirs(
            camino::Utf8Path::from_path(tmp.path()).unwrap(),
            &["a", "b"],
        );
        let mut config = default_config();
        config.print_not = true;
        config.terse = true;

        let result = do_count(&config, &projects, |_| Ok(true));
        assert!(result.is_ok());
    }

    #[test]
    fn do_count_empty_projects() {
        let config = default_config();
        let result = do_count(&config, &[], |_| Ok(true));
        assert!(result.is_ok());
    }

    #[test]
    fn do_count_propagates_errors() {
        let tmp = TempDir::new().unwrap();
        let projects = make_dirs(camino::Utf8Path::from_path(tmp.path()).unwrap(), &["a"]);
        let config = default_config();

        let result = do_count(&config, &projects, |_| anyhow::bail!("test error"));
        assert!(result.is_err());
    }

    #[test]
    fn do_count_parallel() {
        let tmp = TempDir::new().unwrap();
        let projects = make_dirs(
            camino::Utf8Path::from_path(tmp.path()).unwrap(),
            &["a", "b", "c", "d"],
        );
        let mut config = default_config();
        config.jobs = 4;

        let result = do_count(&config, &projects, |_| Ok(true));
        assert!(result.is_ok());
    }

    #[test]
    fn do_for_all_visits_every_project() {
        let tmp = TempDir::new().unwrap();
        let projects = make_dirs(
            camino::Utf8Path::from_path(tmp.path()).unwrap(),
            &["x", "y", "z"],
        );
        let config = default_config();

        let counter = AtomicU32::new(0);
        let result = do_for_all_projects(&config, &projects, |_| {
            counter.fetch_add(1, Ordering::SeqCst);
            Ok(())
        });
        assert!(result.is_ok());
        assert_eq!(counter.load(Ordering::SeqCst), 3);
    }

    #[test]
    fn do_for_all_stops_on_error_by_default() {
        let tmp = TempDir::new().unwrap();
        let projects = make_dirs(
            camino::Utf8Path::from_path(tmp.path()).unwrap(),
            &["a", "b"],
        );
        let config = default_config();

        let counter = AtomicU32::new(0);
        let result = do_for_all_projects(&config, &projects, |_| {
            counter.fetch_add(1, Ordering::SeqCst);
            anyhow::bail!("fail")
        });
        assert!(result.is_err());
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn do_for_all_continues_with_no_stop() {
        let tmp = TempDir::new().unwrap();
        let projects = make_dirs(
            camino::Utf8Path::from_path(tmp.path()).unwrap(),
            &["a", "b", "c"],
        );
        let mut config = default_config();
        config.no_stop = true;

        let counter = AtomicU32::new(0);
        let result = do_for_all_projects(&config, &projects, |_| {
            counter.fetch_add(1, Ordering::SeqCst);
            anyhow::bail!("fail")
        });
        // Every repo ran, but the run as a whole still fails.
        assert_eq!(result.unwrap_err().to_string(), "3 of 3 repos failed");
        assert_eq!(counter.load(Ordering::SeqCst), 3);
    }

    #[test]
    fn do_for_all_no_stop_counts_only_the_failures() {
        let tmp = TempDir::new().unwrap();
        let projects = make_dirs(
            camino::Utf8Path::from_path(tmp.path()).unwrap(),
            &["a", "b", "c"],
        );
        let mut config = default_config();
        config.no_stop = true;

        let result = do_for_all_projects(&config, &projects, |p| {
            if p.file_name() == Some("b") {
                anyhow::bail!("fail")
            }
            Ok(())
        });
        assert_eq!(result.unwrap_err().to_string(), "1 of 3 repos failed");
    }

    #[test]
    fn do_count_no_stop_fails_after_counting() {
        let tmp = TempDir::new().unwrap();
        let projects = make_dirs(
            camino::Utf8Path::from_path(tmp.path()).unwrap(),
            &["a", "b"],
        );
        let mut config = default_config();
        config.no_stop = true;

        let counter = AtomicU32::new(0);
        let result = do_count(&config, &projects, |p| {
            counter.fetch_add(1, Ordering::SeqCst);
            if p.file_name() == Some("a") {
                anyhow::bail!("fail")
            }
            Ok(true)
        });
        assert_eq!(result.unwrap_err().to_string(), "1 of 2 repos failed");
        assert_eq!(counter.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn do_for_all_parallel_visits_every_project() {
        let tmp = TempDir::new().unwrap();
        let projects = make_dirs(
            camino::Utf8Path::from_path(tmp.path()).unwrap(),
            &["a", "b", "c", "d"],
        );
        let mut config = default_config();
        config.jobs = 2;

        let counter = AtomicU32::new(0);
        let result = do_for_all_projects(&config, &projects, |_| {
            counter.fetch_add(1, Ordering::SeqCst);
            Ok(())
        });
        assert!(result.is_ok());
        assert_eq!(counter.load(Ordering::SeqCst), 4);
    }

    #[test]
    fn do_for_all_parallel_stops_on_error_by_default() {
        let tmp = TempDir::new().unwrap();
        let projects = make_dirs(
            camino::Utf8Path::from_path(tmp.path()).unwrap(),
            &["a", "b", "c", "d", "e", "f", "g", "h"],
        );
        let mut config = default_config();
        config.jobs = 2;

        let counter = AtomicU32::new(0);
        let result = do_for_all_projects(&config, &projects, |p| {
            counter.fetch_add(1, Ordering::SeqCst);
            if p.file_name() == Some("a") {
                anyhow::bail!("fail")
            }
            // Give the main thread time to see the failure before this
            // worker looks for more work.
            std::thread::sleep(std::time::Duration::from_millis(50));
            Ok(())
        });
        assert!(result.is_err());
        // "a" fails at once; the other worker is at most one project in, and
        // neither claims anything further once the failure is delivered.
        let visited = counter.load(Ordering::SeqCst);
        assert!(visited < projects.len() as u32, "visited {visited}");
    }

    #[test]
    fn do_for_all_parallel_no_stop_visits_everything() {
        let tmp = TempDir::new().unwrap();
        let projects = make_dirs(
            camino::Utf8Path::from_path(tmp.path()).unwrap(),
            &["a", "b", "c", "d"],
        );
        let mut config = default_config();
        config.jobs = 2;
        config.no_stop = true;

        let counter = AtomicU32::new(0);
        let result = do_for_all_projects(&config, &projects, |_| {
            counter.fetch_add(1, Ordering::SeqCst);
            anyhow::bail!("fail")
        });
        assert_eq!(result.unwrap_err().to_string(), "4 of 4 repos failed");
        assert_eq!(counter.load(Ordering::SeqCst), 4);
    }

    #[test]
    fn print_if_data_runs_ok_with_some() {
        let tmp = TempDir::new().unwrap();
        let projects = make_dirs(
            camino::Utf8Path::from_path(tmp.path()).unwrap(),
            &["a", "b"],
        );
        let config = default_config();

        let result = print_if_data(&config, &projects, |_| Ok(Some("data".to_string())));
        assert!(result.is_ok());
    }

    #[test]
    fn print_if_data_runs_ok_with_none() {
        let tmp = TempDir::new().unwrap();
        let projects = make_dirs(camino::Utf8Path::from_path(tmp.path()).unwrap(), &["a"]);
        let config = default_config();

        let result = print_if_data(&config, &projects, |_| Ok(None));
        assert!(result.is_ok());
    }

    #[test]
    fn print_if_data_no_stop_continues() {
        let tmp = TempDir::new().unwrap();
        let projects = make_dirs(
            camino::Utf8Path::from_path(tmp.path()).unwrap(),
            &["a", "b"],
        );
        let mut config = default_config();
        config.no_stop = true;

        let counter = AtomicU32::new(0);
        let result = print_if_data(&config, &projects, |_| {
            counter.fetch_add(1, Ordering::SeqCst);
            anyhow::bail!("fail")
        });
        assert_eq!(result.unwrap_err().to_string(), "2 of 2 repos failed");
        assert_eq!(counter.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn print_if_data_parallel() {
        let tmp = TempDir::new().unwrap();
        let projects = make_dirs(
            camino::Utf8Path::from_path(tmp.path()).unwrap(),
            &["a", "b", "c"],
        );
        let mut config = default_config();
        config.jobs = 3;

        let counter = AtomicU32::new(0);
        let result = print_if_data(&config, &projects, |_| {
            counter.fetch_add(1, Ordering::SeqCst);
            Ok(Some("x".to_string()))
        });
        assert!(result.is_ok());
        assert_eq!(counter.load(Ordering::SeqCst), 3);
    }
}
