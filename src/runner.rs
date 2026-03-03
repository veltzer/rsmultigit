use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::config::AppConfig;

fn print_project_header(project: &Path) {
    println!("[{}]", project.display());
}

/// Runner for "count" commands.
/// Calls `test_fn` on each project; if it returns true, the project is printed.
/// Optionally prints statistics at the end.
pub fn do_count<F>(config: &AppConfig, projects: &[PathBuf], test_fn: F) -> Result<()>
where
    F: Fn(&Path) -> Result<bool>,
{
    let mut count = 0u32;
    let total = projects.len() as u32;

    for project in projects {
        let matches = test_fn(project).with_context(|| {
            format!("error testing project {}", project.display())
        })?;

        let should_print = if config.print_not { !matches } else { matches };
        if should_print {
            if !config.terse {
                println!("{}", project.display());
            }
            count += 1;
        }
    }

    println!("{count}/{total}");

    Ok(())
}

/// Runner for "do for all projects" commands.
/// Changes into each project directory, runs an action, and handles errors.
/// The action returns `Result<bool>` — `true` if it did something, `false` if it skipped.
/// The project header is only printed when the action did something, or when `--verbose` is set.
pub fn do_for_all_projects<F>(
    config: &AppConfig,
    projects: &[PathBuf],
    action: F,
) -> Result<()>
where
    F: Fn(&Path) -> Result<bool>,
{
    let original_dir = std::env::current_dir().context("failed to get current directory")?;

    for project in projects {
        let abs_path = if project.is_absolute() {
            project.clone()
        } else {
            original_dir.join(project)
        };

        if config.verbose && !config.terse {
            print_project_header(project);
        }

        std::env::set_current_dir(&abs_path).with_context(|| {
            format!("failed to cd into {}", abs_path.display())
        })?;

        match action(&abs_path) {
            Ok(did_work) => {
                if did_work && !config.verbose && !config.terse {
                    print_project_header(project);
                }
            }
            Err(e) => {
                if config.no_stop {
                    eprintln!("error in {}: {e:#}", project.display());
                } else {
                    std::env::set_current_dir(&original_dir).ok();
                    return Err(e).with_context(|| {
                        format!("error in project {}", project.display())
                    });
                }
            }
        }
    }

    std::env::set_current_dir(&original_dir).ok();
    Ok(())
}

/// Runner for "print projects that return data" commands.
/// Calls `data_fn` on each project; if it returns Some(text), prints the project and data.
pub fn print_if_data<F>(
    config: &AppConfig,
    projects: &[PathBuf],
    data_fn: F,
) -> Result<()>
where
    F: Fn(&Path) -> Result<Option<String>>,
{
    let original_dir = std::env::current_dir().context("failed to get current directory")?;

    for project in projects {
        let abs_path = if project.is_absolute() {
            project.clone()
        } else {
            original_dir.join(project)
        };

        std::env::set_current_dir(&abs_path).with_context(|| {
            format!("failed to cd into {}", abs_path.display())
        })?;

        let result = data_fn(&abs_path).with_context(|| {
            format!("error in project {}", project.display())
        });

        match result {
            Ok(Some(data)) => {
                let should_print = !config.print_not;
                if should_print {
                    print_project_header(project);
                    if !config.no_output {
                        println!("{data}");
                    }
                }
            }
            Ok(None) => {
                if config.print_not {
                    print_project_header(project);
                }
            }
            Err(e) => {
                if config.no_stop {
                    eprintln!("error in {}: {e:#}", project.display());
                } else {
                    std::env::set_current_dir(&original_dir).ok();
                    return Err(e);
                }
            }
        }
    }

    std::env::set_current_dir(&original_dir).ok();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use std::fs;
    use std::sync::atomic::{AtomicU32, Ordering};
    use tempfile::TempDir;

    fn default_config() -> AppConfig {
        AppConfig::default()
    }

    fn make_dirs(tmp: &std::path::Path, names: &[&str]) -> Vec<PathBuf> {
        names
            .iter()
            .map(|n| {
                let p = tmp.join(n);
                fs::create_dir_all(&p).unwrap();
                p
            })
            .collect()
    }

    /// Ensure cwd is valid before calling functions that use current_dir().
    /// A previous serial test may have left cwd in a deleted tempdir.
    fn ensure_valid_cwd(dir: &std::path::Path) {
        std::env::set_current_dir(dir).unwrap();
    }

    #[test]
    fn do_count_counts_matching() {
        let tmp = TempDir::new().unwrap();
        let projects = make_dirs(tmp.path(), &["a", "b", "c"]);
        let config = default_config();

        let result = do_count(&config, &projects, |p| {
            let name = p.file_name().unwrap().to_str().unwrap();
            Ok(name == "a" || name == "c")
        });
        assert!(result.is_ok());
    }

    #[test]
    fn do_count_print_not_inverts() {
        let tmp = TempDir::new().unwrap();
        let projects = make_dirs(tmp.path(), &["a", "b"]);
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
        let projects = make_dirs(tmp.path(), &["a"]);
        let config = default_config();

        let result = do_count(&config, &projects, |_| {
            anyhow::bail!("test error")
        });
        assert!(result.is_err());
    }

    #[test]
    #[serial]
    fn do_for_all_visits_every_project() {
        let tmp = TempDir::new().unwrap();
        ensure_valid_cwd(tmp.path());
        let projects = make_dirs(tmp.path(), &["x", "y", "z"]);
        let config = default_config();

        let counter = AtomicU32::new(0);
        let result = do_for_all_projects(&config, &projects, |_| {
            counter.fetch_add(1, Ordering::SeqCst);
            Ok(true)
        });
        assert!(result.is_ok());
        assert_eq!(counter.load(Ordering::SeqCst), 3);
    }

    #[test]
    #[serial]
    fn do_for_all_stops_on_error_by_default() {
        let tmp = TempDir::new().unwrap();
        ensure_valid_cwd(tmp.path());
        let projects = make_dirs(tmp.path(), &["a", "b"]);
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
    #[serial]
    fn do_for_all_continues_with_no_stop() {
        let tmp = TempDir::new().unwrap();
        ensure_valid_cwd(tmp.path());
        let projects = make_dirs(tmp.path(), &["a", "b", "c"]);
        let mut config = default_config();
        config.no_stop = true;

        let counter = AtomicU32::new(0);
        let result = do_for_all_projects(&config, &projects, |_| {
            counter.fetch_add(1, Ordering::SeqCst);
            anyhow::bail!("fail")
        });
        assert!(result.is_ok());
        assert_eq!(counter.load(Ordering::SeqCst), 3);
    }

    #[test]
    #[serial]
    fn do_for_all_restores_cwd() {
        let tmp = TempDir::new().unwrap();
        ensure_valid_cwd(tmp.path());
        let projects = make_dirs(tmp.path(), &["a"]);
        let config = default_config();

        do_for_all_projects(&config, &projects, |_| Ok(true)).unwrap();
        assert_eq!(std::env::current_dir().unwrap(), tmp.path());
    }

    #[test]
    #[serial]
    fn print_if_data_runs_ok_with_some() {
        let tmp = TempDir::new().unwrap();
        ensure_valid_cwd(tmp.path());
        let projects = make_dirs(tmp.path(), &["a", "b"]);
        let config = default_config();

        let result = print_if_data(&config, &projects, |_| {
            Ok(Some("data".to_string()))
        });
        assert!(result.is_ok());
    }

    #[test]
    #[serial]
    fn print_if_data_runs_ok_with_none() {
        let tmp = TempDir::new().unwrap();
        ensure_valid_cwd(tmp.path());
        let projects = make_dirs(tmp.path(), &["a"]);
        let config = default_config();

        let result = print_if_data(&config, &projects, |_| Ok(None));
        assert!(result.is_ok());
    }

    #[test]
    #[serial]
    fn print_if_data_restores_cwd() {
        let tmp = TempDir::new().unwrap();
        ensure_valid_cwd(tmp.path());
        let projects = make_dirs(tmp.path(), &["a"]);
        let config = default_config();

        print_if_data(&config, &projects, |_| Ok(None)).unwrap();
        assert_eq!(std::env::current_dir().unwrap(), tmp.path());
    }

    #[test]
    #[serial]
    fn print_if_data_no_stop_continues() {
        let tmp = TempDir::new().unwrap();
        ensure_valid_cwd(tmp.path());
        let projects = make_dirs(tmp.path(), &["a", "b"]);
        let mut config = default_config();
        config.no_stop = true;

        let counter = AtomicU32::new(0);
        let result = print_if_data(&config, &projects, |_| {
            counter.fetch_add(1, Ordering::SeqCst);
            anyhow::bail!("fail")
        });
        assert!(result.is_ok());
        assert_eq!(counter.load(Ordering::SeqCst), 2);
    }
}
