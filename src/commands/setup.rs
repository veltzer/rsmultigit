//! `rsmultigit setup interactive`: write a first `~/.config/rsmultigit/config.toml` for a
//! new user.
//!
//! The command asks two questions - where the git repositories live, and which
//! build tool `rsmultigit build` should run by default - and writes a minimal
//! config from the answers. Each answer can also be given on the command line
//! (`--repos-dir`, `--build`/`--no-build`), in which case its question is not
//! asked; with every answer given the command is fully non-interactive, which
//! is what scripts and the integration tests use.
//!
//! The interactive layer (the `inquire` prompts) is kept thin and at the
//! bottom of the file. Everything above it - scanning a directory for repos,
//! choosing the default build method, rendering the TOML - is plain functions
//! over plain data, so it is unit-tested without a terminal.

use std::fmt;
use std::io::{IsTerminal, Write};

use anyhow::{Context, Result};
use camino::{Utf8Path, Utf8PathBuf};
use clap::ValueEnum;
use inquire::autocompletion::{Autocomplete, Replacement};
use inquire::error::CustomUserError;
use inquire::validator::Validation;
use inquire::{Confirm, InquireError, Select, Text};

use crate::cli::BuildWhat;

/// The answers `setup` needs. `None` means "not given on the command line, ask".
#[derive(Debug, Default, Clone)]
pub struct SetupOpts {
    /// Directory whose immediate subdirectories are the git repositories.
    pub repos_dir: Option<String>,
    /// `Some(Some(m))`: set `default_build_method = m`. `Some(None)`: write no
    /// default build method (`--no-build`). `None`: ask.
    pub build: Option<Option<BuildWhat>>,
    /// Replace an existing config file without asking.
    pub overwrite: bool,
}

/// What a scan of the repositories directory found.
#[derive(Debug, PartialEq, Eq)]
pub struct Scan {
    /// The directory scanned, absolute, as it will appear in the config.
    pub dir: Utf8PathBuf,
    /// Immediate subdirectories that are git repositories, sorted.
    pub repos: Vec<Utf8PathBuf>,
    /// For every build method, how many of `repos` carry its marker file.
    /// Same order as `BuildWhat::value_variants()`.
    pub counts: Vec<(BuildWhat, usize)>,
}

impl Scan {
    /// The build method most repos are set up for, if any repo is set up for
    /// anything at all. Ties go to the method declared first in `BuildWhat`.
    pub fn suggested_build(&self) -> Option<BuildWhat> {
        // max_by_key keeps the *last* maximum, so walk backwards to make a tie
        // fall on the first declared method.
        self.counts
            .iter()
            .rev()
            .filter(|(_, n)| *n > 0)
            .max_by_key(|(_, n)| *n)
            .map(|(m, _)| *m)
    }

    fn count_for(&self, method: BuildWhat) -> usize {
        self.counts
            .iter()
            .find(|(m, _)| *m == method)
            .map(|(_, n)| *n)
            .unwrap_or(0)
    }
}

/// The file whose presence in a repo says the repo is built with `method`.
/// `bootstrap` runs `python bootstrap.py`; the others run their tool, which
/// reads the named manifest.
pub fn marker_file(method: BuildWhat) -> &'static str {
    match method {
        BuildWhat::Bootstrap => "bootstrap.py",
        BuildWhat::Make => "Makefile",
        BuildWhat::Rsconstruct => "rsconstruct.toml",
        BuildWhat::Cargo => "Cargo.toml",
    }
}

/// Turn what the user typed into the absolute directory path the config will
/// hold: `~` and `$VAR` expanded, made absolute against the current directory,
/// trailing slashes dropped. Symlinks are left alone - the user named this
/// path on purpose.
pub fn normalize_dir(input: &str) -> Result<Utf8PathBuf> {
    let input = input.trim();
    if input.is_empty() {
        anyhow::bail!("no directory given");
    }
    let expanded = shellexpand::full(input).with_context(|| format!("cannot expand `{input}`"))?;
    let mut path = Utf8PathBuf::from(expanded.into_owned());
    if path.is_relative() {
        let cwd = std::env::current_dir().context("cannot determine the current directory")?;
        let cwd = Utf8PathBuf::from_path_buf(cwd)
            .map_err(|p| anyhow::anyhow!("current directory is not UTF-8: {}", p.display()))?;
        path = cwd.join(path);
    }
    // Utf8PathBuf keeps a trailing slash in its string form; the config glob
    // is built by appending "/*", so strip it.
    let trimmed = path.as_str().trim_end_matches('/');
    if trimmed.is_empty() {
        return Ok(Utf8PathBuf::from("/"));
    }
    Ok(Utf8PathBuf::from(trimmed))
}

/// Find the git repositories directly under `dir`. The criterion is the one
/// `resolve_repos` applies to the `repos` globs, so what setup counts is what
/// every later command will see.
pub fn scan(dir: &Utf8Path) -> Result<Scan> {
    if !dir.is_dir() {
        anyhow::bail!("{dir} is not a directory");
    }
    let entries = dir
        .read_dir_utf8()
        .with_context(|| format!("cannot read {dir}"))?;
    let mut repos = Vec::new();
    for entry in entries {
        let entry = entry.with_context(|| format!("cannot read an entry of {dir}"))?;
        let path = entry.into_path();
        if path.is_dir() && path.join(".git").is_dir() {
            repos.push(path);
        }
    }
    repos.sort();
    let counts = BuildWhat::value_variants()
        .iter()
        .map(|m| {
            let n = repos
                .iter()
                .filter(|r| r.join(marker_file(*m)).exists())
                .count();
            (*m, n)
        })
        .collect();
    Ok(Scan {
        dir: dir.to_path_buf(),
        repos,
        counts,
    })
}

/// `dir` with the home directory contracted back to `~`, so the written
/// config reads like the one `setup config-sample` prints and survives a rename of
/// the home directory. Paths outside the home directory are returned as-is.
pub fn contract_home(dir: &Utf8Path) -> String {
    let home = std::env::var("HOME").unwrap_or_default();
    let home = home.trim_end_matches('/');
    if home.is_empty() {
        return dir.to_string();
    }
    if dir.as_str() == home {
        return "~".to_string();
    }
    match dir.as_str().strip_prefix(home) {
        Some(rest) if rest.starts_with('/') => format!("~{rest}"),
        _ => dir.to_string(),
    }
}

/// The command-line spelling of a build method (`rsconstruct`, `cargo`, ...),
/// which is also how the config file spells it.
pub fn method_name(method: BuildWhat) -> String {
    method
        .to_possible_value()
        .expect("every BuildWhat variant has a command-line name")
        .get_name()
        .to_string()
}

/// The config file text for the given answers.
pub fn render_config(dir: &Utf8Path, build: Option<BuildWhat>) -> String {
    let mut out = String::new();
    out.push_str(
        "# rsmultigit config, written by `rsmultigit setup interactive`.\n\
         #\n\
         # `repos` lists shell-expanded globs; every match that is a git repository\n\
         # is operated on. Add more patterns to cover more directories.\n\
         #\n\
         # `rsmultigit setup config-sample` prints a fully commented example of every\n\
         # key, including the [[check]] and [[exists]] rules that keep shared files\n\
         # identical across repos.\n\n",
    );
    out.push_str(&format!(
        "repos = [\n    \"{}/*\",\n]\n",
        contract_home(dir)
    ));
    match build {
        Some(m) => {
            let name = method_name(m);
            out.push_str(&format!(
                "\n# What a bare `rsmultigit build` runs; a method given on the command line wins.\n\
                 default_build_method = \"{name}\"\n"
            ));
        }
        None => out.push_str(
            "\n# No default build method: `rsmultigit build` needs an explicit method\n\
             # (bootstrap, make, rsconstruct, cargo). Set `default_build_method` to\n\
             # make one of them the default.\n",
        ),
    }
    out
}

/// Write `text` to `path`, creating the parent directories.
pub fn write_config(path: &Utf8Path, text: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).with_context(|| format!("cannot create {parent}"))?;
    }
    std::fs::write(path, text).with_context(|| format!("cannot write {path}"))?;
    Ok(())
}

/// Run `setup`: collect the answers (from `opts`, asking for whatever is
/// missing), write the config to `config_path`, and report on `out`.
pub fn run(opts: &SetupOpts, config_path: &Utf8Path, out: &mut dyn Write) -> Result<()> {
    if config_path.exists() && !opts.overwrite {
        let replace = ask(
            || {
                Confirm::new(&format!("{config_path} exists. Replace it?"))
                    .with_default(false)
                    .with_help_message("The current file is overwritten, not merged.")
                    .prompt()
            },
            "pass --overwrite to replace the existing file",
        )?;
        if !replace {
            writeln!(out, "Keeping {config_path}; nothing written.")?;
            return Ok(());
        }
    }

    let scan = match &opts.repos_dir {
        Some(given) => {
            let dir = normalize_dir(given)?;
            let scan = scan(&dir)?;
            if scan.repos.is_empty() {
                anyhow::bail!("no git repositories directly under {dir}");
            }
            scan
        }
        None => ask(ask_repos_dir, "pass --repos-dir <DIR>")?,
    };

    let build = match opts.build {
        Some(choice) => choice,
        None => ask(|| ask_build(&scan), "pass --build <METHOD> or --no-build")?,
    };

    let text = render_config(&scan.dir, build);
    write_config(config_path, &text)?;

    writeln!(out, "Wrote {config_path}")?;
    writeln!(
        out,
        "  repos: {}/* ({} git {})",
        contract_home(&scan.dir),
        scan.repos.len(),
        if scan.repos.len() == 1 {
            "repository"
        } else {
            "repositories"
        }
    )?;
    match build {
        Some(m) => writeln!(
            out,
            "  default build method: {} ({} of {} repos have {})",
            method_name(m),
            scan.count_for(m),
            scan.repos.len(),
            marker_file(m)
        )?,
        None => writeln!(out, "  default build method: none")?,
    }
    writeln!(
        out,
        "Next: `rsmultigit list repos`, then `rsmultigit git status`."
    )?;
    Ok(())
}

// ── interactive layer ────────────────────────────────────────────────────────

/// Run one prompt. Refuses up front when there is no terminal to ask on,
/// naming the flag that answers the question instead, and turns Esc/Ctrl-C
/// into a plain "cancelled" error rather than a stack of inquire internals.
fn ask<T>(prompt: impl FnOnce() -> Result<T, InquireError>, flag_hint: &str) -> Result<T> {
    if !std::io::stdin().is_terminal() || !std::io::stdout().is_terminal() {
        anyhow::bail!(
            "setup needs a terminal to ask questions on; {flag_hint} to answer non-interactively"
        );
    }
    match prompt() {
        Ok(v) => Ok(v),
        Err(InquireError::OperationCanceled | InquireError::OperationInterrupted) => {
            anyhow::bail!("setup cancelled; nothing written")
        }
        Err(InquireError::NotTTY) => anyhow::bail!(
            "setup needs a terminal to ask questions on; {flag_hint} to answer non-interactively"
        ),
        Err(e) => Err(e).context("prompt failed"),
    }
}

/// The directory to offer before the user types anything: `~/git` when it
/// exists, otherwise the home directory.
fn default_repos_dir() -> String {
    match shellexpand::tilde("~/git") {
        p if std::path::Path::new(p.as_ref()).is_dir() => "~/git".to_string(),
        _ => "~".to_string(),
    }
}

fn ask_repos_dir() -> Result<Scan, InquireError> {
    let default = default_repos_dir();
    let answer = Text::new("Directory containing your git repositories:")
        .with_default(&default)
        .with_help_message(
            "Tab completes directories. Its direct subdirectories that are git repos are used.",
        )
        .with_autocomplete(DirCompleter)
        .with_validator(|input: &str| -> Result<Validation, CustomUserError> {
            let dir = match normalize_dir(input) {
                Ok(d) => d,
                Err(e) => return Ok(Validation::Invalid(e.to_string().into())),
            };
            match scan(&dir) {
                Err(e) => Ok(Validation::Invalid(e.to_string().into())),
                Ok(s) if s.repos.is_empty() => Ok(Validation::Invalid(
                    format!("no git repositories directly under {dir}").into(),
                )),
                Ok(_) => Ok(Validation::Valid),
            }
        })
        .prompt()?;
    // The validator already accepted this input, so both steps succeed.
    let dir = normalize_dir(&answer).map_err(|e| InquireError::Custom(e.into()))?;
    scan(&dir).map_err(|e| InquireError::Custom(e.into()))
}

/// One row of the build-method menu.
struct BuildChoice {
    method: Option<BuildWhat>,
    count: usize,
    total: usize,
}

impl fmt::Display for BuildChoice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.method {
            Some(m) => write!(
                f,
                "{:<12} {} of {} repos have {}",
                method_name(m),
                self.count,
                self.total,
                marker_file(m)
            ),
            None => write!(
                f,
                "{:<12} always name the method on the command line",
                "none"
            ),
        }
    }
}

fn ask_build(scan: &Scan) -> Result<Option<BuildWhat>, InquireError> {
    let total = scan.repos.len();
    let mut choices: Vec<BuildChoice> = scan
        .counts
        .iter()
        .map(|(m, n)| BuildChoice {
            method: Some(*m),
            count: *n,
            total,
        })
        .collect();
    choices.push(BuildChoice {
        method: None,
        count: 0,
        total,
    });
    let suggested = scan.suggested_build();
    let cursor = choices
        .iter()
        .position(|c| c.method == suggested)
        .unwrap_or(choices.len() - 1);
    let picked = Select::new("Default build tool for `rsmultigit build`:", choices)
        .with_starting_cursor(cursor)
        .with_help_message("↑↓ to move, enter to select. The counts say what your repos contain.")
        .prompt()?;
    Ok(picked.method)
}

/// Tab completion over directories for the repos-dir prompt. Suggestions are
/// the subdirectories of the directory being typed whose name starts with the
/// typed prefix; `~` is expanded for lookup but kept in what is shown.
#[derive(Clone)]
struct DirCompleter;

impl DirCompleter {
    fn suggestions(input: &str) -> Vec<String> {
        let (parent, prefix) = match input.rsplit_once('/') {
            Some((p, pre)) => (if p.is_empty() { "/" } else { p }, pre),
            None if input.is_empty() => return vec!["~/".to_string()],
            None => return vec![],
        };
        let expanded = shellexpand::tilde(parent);
        let Ok(entries) = std::fs::read_dir(expanded.as_ref()) else {
            return vec![];
        };
        let mut out: Vec<String> = entries
            .filter_map(|e| e.ok())
            .filter(|e| e.path().is_dir())
            .filter_map(|e| e.file_name().into_string().ok())
            .filter(|name| name.starts_with(prefix))
            .filter(|name| prefix.starts_with('.') || !name.starts_with('.'))
            .map(|name| {
                if parent == "/" {
                    format!("/{name}/")
                } else {
                    format!("{parent}/{name}/")
                }
            })
            .collect();
        out.sort();
        out
    }
}

impl Autocomplete for DirCompleter {
    fn get_suggestions(&mut self, input: &str) -> Result<Vec<String>, CustomUserError> {
        Ok(Self::suggestions(input))
    }

    fn get_completion(
        &mut self,
        input: &str,
        highlighted_suggestion: Option<String>,
    ) -> Result<Replacement, CustomUserError> {
        if let Some(s) = highlighted_suggestion {
            return Ok(Some(s));
        }
        // No highlighted row: complete to the single match, or to the common
        // prefix of all matches, the way a shell does.
        let suggestions = Self::suggestions(input);
        match suggestions.as_slice() {
            [] => Ok(None),
            [only] => Ok(Some(only.clone())),
            many => {
                let common = common_prefix(many);
                if common.len() > input.len() {
                    Ok(Some(common))
                } else {
                    Ok(None)
                }
            }
        }
    }
}

fn common_prefix(items: &[String]) -> String {
    let Some(first) = items.first() else {
        return String::new();
    };
    let mut end = first.len();
    for item in &items[1..] {
        end = first
            .char_indices()
            .zip(item.chars())
            .take_while(|((_, a), b)| a == b)
            .map(|((i, a), _)| i + a.len_utf8())
            .last()
            .unwrap_or(0)
            .min(end);
    }
    first[..end].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn utf8(tmp: &TempDir) -> &Utf8Path {
        Utf8Path::from_path(tmp.path()).unwrap()
    }

    fn git_repo(root: &Utf8Path, name: &str, markers: &[&str]) {
        let repo = root.join(name);
        fs::create_dir_all(repo.join(".git")).unwrap();
        for m in markers {
            fs::write(repo.join(m), "").unwrap();
        }
    }

    #[test]
    fn scan_finds_git_dirs_and_counts_markers() {
        let tmp = TempDir::new().unwrap();
        let root = utf8(&tmp);
        git_repo(root, "a", &["Cargo.toml"]);
        git_repo(root, "b", &["Cargo.toml", "Makefile"]);
        git_repo(root, "c", &["rsconstruct.toml"]);
        fs::create_dir_all(root.join("plain")).unwrap();
        fs::write(root.join("file"), "").unwrap();
        // A `.git` *file* (worktree) is not a repo for resolve_repos, so not here.
        fs::create_dir_all(root.join("wt")).unwrap();
        fs::write(root.join("wt/.git"), "gitdir: elsewhere").unwrap();

        let s = scan(root).unwrap();
        assert_eq!(
            s.repos,
            vec![root.join("a"), root.join("b"), root.join("c")]
        );
        assert_eq!(s.count_for(BuildWhat::Cargo), 2);
        assert_eq!(s.count_for(BuildWhat::Make), 1);
        assert_eq!(s.count_for(BuildWhat::Rsconstruct), 1);
        assert_eq!(s.count_for(BuildWhat::Bootstrap), 0);
        assert_eq!(s.suggested_build(), Some(BuildWhat::Cargo));
    }

    #[test]
    fn scan_suggests_nothing_without_markers() {
        let tmp = TempDir::new().unwrap();
        let root = utf8(&tmp);
        git_repo(root, "a", &[]);
        let s = scan(root).unwrap();
        assert_eq!(s.repos.len(), 1);
        assert_eq!(s.suggested_build(), None);
    }

    #[test]
    fn scan_tie_goes_to_first_declared_method() {
        let tmp = TempDir::new().unwrap();
        let root = utf8(&tmp);
        git_repo(root, "a", &["Makefile", "Cargo.toml"]);
        let s = scan(root).unwrap();
        assert_eq!(s.suggested_build(), Some(BuildWhat::Make));
    }

    #[test]
    fn scan_rejects_non_directories() {
        let tmp = TempDir::new().unwrap();
        let root = utf8(&tmp);
        assert!(scan(&root.join("missing")).is_err());
        fs::write(root.join("f"), "").unwrap();
        assert!(scan(&root.join("f")).is_err());
    }

    #[test]
    fn normalize_dir_expands_and_strips() {
        let tmp = TempDir::new().unwrap();
        let root = utf8(&tmp);
        assert_eq!(
            normalize_dir(&format!("{root}/")).unwrap(),
            root.to_path_buf()
        );
        assert_eq!(
            normalize_dir(&format!("  {root}//  ")).unwrap(),
            root.to_path_buf()
        );
        assert!(normalize_dir("   ").is_err());
        let home = std::env::var("HOME").unwrap();
        assert_eq!(normalize_dir("~/x").unwrap().as_str(), format!("{home}/x"));
        assert_eq!(normalize_dir("/").unwrap().as_str(), "/");
    }

    #[test]
    fn contract_home_uses_tilde_only_for_the_home_tree() {
        let home = std::env::var("HOME").unwrap();
        let home = home.trim_end_matches('/');
        assert_eq!(
            contract_home(Utf8Path::new(&format!("{home}/git"))),
            "~/git"
        );
        assert_eq!(contract_home(Utf8Path::new(home)), "~");
        // A sibling that merely shares the prefix is not inside home.
        assert_eq!(
            contract_home(Utf8Path::new(&format!("{home}2/git"))),
            format!("{home}2/git")
        );
        assert_eq!(contract_home(Utf8Path::new("/srv/git")), "/srv/git");
    }

    #[test]
    fn render_config_parses_back_with_the_same_answers() {
        let text = render_config(Utf8Path::new("/srv/git"), Some(BuildWhat::Cargo));
        let cfg: crate::commands::check::CheckConfig = toml::from_str(&text).unwrap();
        assert_eq!(cfg.repos, vec!["/srv/git/*".to_string()]);
        assert_eq!(cfg.default_build_method, Some(BuildWhat::Cargo));

        let text = render_config(Utf8Path::new("/srv/git"), None);
        let cfg: crate::commands::check::CheckConfig = toml::from_str(&text).unwrap();
        assert_eq!(cfg.default_build_method, None);
        assert!(
            text.contains("default_build_method"),
            "comment explains the key"
        );
        assert!(
            !text.contains("\ndefault_build_method"),
            "but does not set it"
        );
    }

    #[test]
    fn render_config_contracts_home() {
        let home = std::env::var("HOME").unwrap();
        let text = render_config(Utf8Path::new(&format!("{home}/git")), None);
        assert!(text.contains("\"~/git/*\""), "{text}");
    }

    #[test]
    fn method_names_match_the_command_line() {
        assert_eq!(method_name(BuildWhat::Rsconstruct), "rsconstruct");
        assert_eq!(method_name(BuildWhat::Bootstrap), "bootstrap");
        assert_eq!(BuildWhat::names().len(), BuildWhat::value_variants().len());
    }

    #[test]
    fn run_writes_config_non_interactively() {
        let tmp = TempDir::new().unwrap();
        let root = utf8(&tmp);
        git_repo(root, "a", &["rsconstruct.toml"]);
        let cfg = root.join("deep/er/config.toml");
        let opts = SetupOpts {
            repos_dir: Some(root.to_string()),
            build: Some(Some(BuildWhat::Rsconstruct)),
            overwrite: false,
        };
        let mut out = Vec::new();
        run(&opts, &cfg, &mut out).unwrap();
        let out = String::from_utf8(out).unwrap();
        assert!(out.contains(&format!("Wrote {cfg}")), "{out}");
        assert!(out.contains("1 git repository"), "{out}");
        assert!(out.contains("rsconstruct (1 of 1 repos"), "{out}");
        let text = fs::read_to_string(&cfg).unwrap();
        assert!(text.contains(&format!("\"{root}/*\"")), "{text}");
        assert!(
            text.contains("default_build_method = \"rsconstruct\""),
            "{text}"
        );
    }

    #[test]
    fn run_refuses_to_overwrite_without_a_terminal_or_flag() {
        let tmp = TempDir::new().unwrap();
        let root = utf8(&tmp);
        git_repo(root, "a", &[]);
        let cfg = root.join("config.toml");
        fs::write(&cfg, "repos = []\n").unwrap();
        let opts = SetupOpts {
            repos_dir: Some(root.to_string()),
            build: Some(None),
            overwrite: false,
        };
        // The unit tests run without a terminal on stdin, so the confirm
        // prompt is refused with a pointer at --overwrite.
        let err = run(&opts, &cfg, &mut Vec::new()).unwrap_err().to_string();
        assert!(err.contains("--overwrite"), "{err}");
        assert_eq!(fs::read_to_string(&cfg).unwrap(), "repos = []\n");

        let opts = SetupOpts {
            overwrite: true,
            ..opts
        };
        run(&opts, &cfg, &mut Vec::new()).unwrap();
        let text = fs::read_to_string(&cfg).unwrap();
        assert!(text.contains(&format!("\"{root}/*\"")), "{text}");
        assert!(!text.contains("\ndefault_build_method ="), "{text}");
    }

    #[test]
    fn run_rejects_a_directory_without_repos() {
        let tmp = TempDir::new().unwrap();
        let root = utf8(&tmp);
        fs::create_dir_all(root.join("plain")).unwrap();
        let cfg = root.join("config.toml");
        let opts = SetupOpts {
            repos_dir: Some(root.to_string()),
            build: Some(None),
            overwrite: false,
        };
        let err = run(&opts, &cfg, &mut Vec::new()).unwrap_err().to_string();
        assert!(err.contains("no git repositories directly under"), "{err}");
        assert!(!cfg.exists());
    }

    #[test]
    fn dir_completer_lists_matching_subdirectories() {
        let tmp = TempDir::new().unwrap();
        let root = utf8(&tmp);
        for d in ["alpha", "alps", "beta", ".hidden"] {
            fs::create_dir_all(root.join(d)).unwrap();
        }
        fs::write(root.join("alfile"), "").unwrap();

        let got = DirCompleter::suggestions(&format!("{root}/al"));
        assert_eq!(got, vec![format!("{root}/alpha/"), format!("{root}/alps/")]);
        // Hidden directories only when the prefix asks for them.
        let got = DirCompleter::suggestions(&format!("{root}/"));
        assert_eq!(got.len(), 3, "{got:?}");
        let got = DirCompleter::suggestions(&format!("{root}/.h"));
        assert_eq!(got, vec![format!("{root}/.hidden/")]);
        // Empty input offers the home tree; a bare word has no parent to list.
        assert_eq!(DirCompleter::suggestions(""), vec!["~/".to_string()]);
        assert!(DirCompleter::suggestions("nonsense").is_empty());
    }

    #[test]
    fn dir_completer_tab_completes_to_common_prefix() {
        let tmp = TempDir::new().unwrap();
        let root = utf8(&tmp);
        for d in ["alpha", "alps", "beta"] {
            fs::create_dir_all(root.join(d)).unwrap();
        }
        let mut c = DirCompleter;
        // Two matches sharing "al" + "p": completes to the common prefix.
        assert_eq!(
            c.get_completion(&format!("{root}/a"), None).unwrap(),
            Some(format!("{root}/alp"))
        );
        // Nothing to add once the input is already the common prefix.
        assert_eq!(
            c.get_completion(&format!("{root}/alp"), None).unwrap(),
            None
        );
        // One match: completes fully, with a trailing slash to keep typing.
        assert_eq!(
            c.get_completion(&format!("{root}/b"), None).unwrap(),
            Some(format!("{root}/beta/"))
        );
        // A highlighted row wins over everything.
        assert_eq!(
            c.get_completion("x", Some("picked".to_string())).unwrap(),
            Some("picked".to_string())
        );
    }

    #[test]
    fn common_prefix_handles_edge_cases() {
        assert_eq!(common_prefix(&[]), "");
        assert_eq!(common_prefix(&["abc".to_string()]), "abc");
        assert_eq!(
            common_prefix(&["abc".to_string(), "abd".to_string(), "ab".to_string()]),
            "ab"
        );
        assert_eq!(common_prefix(&["x".to_string(), "y".to_string()]), "");
    }
}
