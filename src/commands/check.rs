use camino::{Utf8Path, Utf8PathBuf};
use std::collections::BTreeMap;
use std::fs::File;
use std::io::{BufReader, Read};

use anyhow::{Context, Result};
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::cli::BuildWhat;

// Every config struct denies unknown fields: a misspelled key (`must_hav`,
// `exlude`) would otherwise be dropped silently, and the rule would quietly
// check something other than what was written.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CheckConfig {
    /// Glob patterns (with shell expansion) identifying repo roots.
    /// Non-git directories matching the pattern are filtered out.
    #[serde(default)]
    pub repos: Vec<String>,
    /// Build method `rsmultigit build` uses when none is given on the command
    /// line, so a fleet that is all-rsconstruct can type `rsmultigit build`.
    /// Spelled as on the command line (`rsconstruct`, `cargo`, ...).
    #[serde(default)]
    pub default_build_method: Option<BuildWhat>,
    /// The pass(1) entry `cargo release` reads the crates.io token from when
    /// `CARGO_REGISTRY_TOKEN` is not already set. Defaults to `keys/crates.io`.
    #[serde(default = "crate::commands::release::default_pass_entry")]
    pub crates_io_pass_entry: String,
    #[serde(default)]
    pub check: Vec<Rule>,
    /// Presence-only rules, consumed by `check exists`. Unlike `[[check]]`,
    /// these never compare content: they assert that every in-scope repo has
    /// the file, whatever it contains. This is the right rule type for files
    /// that must exist but legitimately differ per repo (README.md, LICENSE
    /// where the year varies, and so on).
    #[serde(default)]
    pub exists: Vec<ExistsRule>,
}

/// A presence-only rule. Shares the selection vocabulary of [`Rule`] —
/// `select`/`exclude`/`marker`/`marker_absent`/`enabled` mean exactly the same
/// thing — but has no content dimension, so no `must_have` field either:
/// requiring the file *is* the whole rule.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExistsRule {
    pub name: String,
    pub select: String,
    #[serde(default)]
    pub exclude: Option<String>,
    #[serde(default)]
    pub marker: Option<String>,
    #[serde(default)]
    pub marker_absent: Option<String>,
    pub path: String,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

/// Outcome of evaluating a single presence rule.
pub struct ExistsResult {
    pub name: String,
    /// The file each in-scope repo was required to contain (the rule's `path`).
    pub path: String,
    /// Repos that were in scope and had the file.
    pub present: Vec<Utf8PathBuf>,
    /// Repos that were in scope and did not. These are the violations.
    pub missing: Vec<Utf8PathBuf>,
}

impl ExistsResult {
    pub fn is_satisfied(&self) -> bool {
        self.missing.is_empty()
    }

    /// Number of repos the rule actually examined.
    pub fn total_repos(&self) -> usize {
        self.present.len() + self.missing.len()
    }

    /// True when the rule selected no repos at all — almost always a stale
    /// `select`, so `check exists` treats it as a failure unless
    /// `--allow-empty` is passed. Mirrors [`RuleResult::matched_nothing`].
    pub fn matched_nothing(&self) -> bool {
        self.total_repos() == 0
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rule {
    pub name: String,
    pub select: String,
    #[serde(default)]
    pub exclude: Option<String>,
    #[serde(default)]
    pub marker: Option<String>,
    /// Inverse of `marker`: a repo containing this file is dropped from the
    /// rule's scope. This is how a repo opts out of an invariant from inside
    /// itself (e.g. `.noci` for a repo that deliberately has no CI), rather
    /// than by name in an `exclude` glob that lives far from the repo.
    #[serde(default)]
    pub marker_absent: Option<String>,
    pub path: String,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    /// When true, every in-scope repo must contain `path`. Missing files become
    /// rule violations (reported as "missing in: ..." and counted as mismatches).
    /// When false (default), missing files are silently skipped.
    #[serde(default)]
    pub must_have: bool,
}

fn default_enabled() -> bool {
    true
}

/// Outcome of evaluating a single rule.
pub struct RuleResult {
    pub name: String,
    /// The file inside each repo that was hashed (the rule's `path`).
    pub path: String,
    /// Files grouped by SHA-256 digest, ordered by group size descending
    /// (largest group first — the presumed "canonical" version).
    pub groups: Vec<Vec<Utf8PathBuf>>,
    /// Total number of files considered.
    pub total_files: usize,
    /// Repos that matched selection but lacked `path`. When the rule has
    /// `must_have = false`, they're treated as "skipped" (not a violation).
    pub skipped: Vec<Utf8PathBuf>,
    /// Repos that matched selection but lacked `path` *and* the rule has
    /// `must_have = true`. These are rule violations.
    pub must_have_violations: Vec<Utf8PathBuf>,
    /// Files that exist but could not be read, with the reason. Each is a
    /// failure of the rule (its content is unknown, so it cannot be said to
    /// match), reported per file rather than aborting the whole run.
    pub unreadable: Vec<(Utf8PathBuf, String)>,
}

impl RuleResult {
    pub fn is_consistent(&self) -> bool {
        self.groups.len() <= 1 && self.must_have_violations.is_empty() && self.unreadable.is_empty()
    }

    /// True when the rule hashed no files and has no must_have violations to
    /// report — i.e. it checked nothing at all. Usually a stale `select`/`path`,
    /// so check same treats this as a failure unless --allow-empty is passed.
    pub fn matched_nothing(&self) -> bool {
        self.total_files == 0 && self.must_have_violations.is_empty() && self.unreadable.is_empty()
    }
}

/// Environment variable used to override the config path (tests set this).
pub const CONFIG_PATH_ENV: &str = "RSMULTIGIT_CONFIG";

/// Resolve the path of the config file. Tests can override via `RSMULTIGIT_CONFIG`.
pub fn default_config_path() -> Result<Utf8PathBuf> {
    if let Ok(p) = std::env::var(CONFIG_PATH_ENV) {
        return Ok(Utf8PathBuf::from(p));
    }
    let expanded = shellexpand::full("~/.config/rsmultigit/config.toml")
        .context("failed to expand default config path")?;
    Ok(Utf8PathBuf::from(expanded.into_owned()))
}

/// Parse a config file from disk.
pub fn load_config(path: &Utf8Path) -> Result<CheckConfig> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read config file {}", path))?;
    let config: CheckConfig =
        toml::from_str(&text).with_context(|| format!("failed to parse config file {}", path))?;
    let paths = config
        .check
        .iter()
        .map(|r| ("check", &r.name, &r.path))
        .chain(config.exists.iter().map(|r| ("exists", &r.name, &r.path)));
    for (kind, name, rule_path) in paths {
        validate_rule_path(rule_path)
            .with_context(|| format!("[[{kind}]] rule `{name}` in {path}"))?;
    }
    Ok(config)
}

/// A rule's `path` names a file inside each repo, so it must be relative and
/// stay inside: an absolute path would make `repo.join` drop the repo and
/// compare one file with itself (a rule that always passes), and `..` would
/// reach outside the repo.
fn validate_rule_path(path: &str) -> Result<()> {
    let p = Utf8Path::new(path);
    if path.is_empty() {
        anyhow::bail!("`path` is empty");
    }
    if p.is_absolute() {
        anyhow::bail!("`path = \"{path}\"` is absolute; it must be relative to the repo");
    }
    if p.components()
        .any(|c| c == camino::Utf8Component::ParentDir)
    {
        anyhow::bail!("`path = \"{path}\"` contains `..`; it must stay inside the repo");
    }
    Ok(())
}

/// Whether `path` is the top of a git checkout: `.git` is either the git
/// directory itself or, in a linked worktree or a submodule, a file holding
/// a `gitdir: <path>` pointer to it.
pub fn is_git_checkout(path: &std::path::Path) -> bool {
    let dot_git = path.join(".git");
    if dot_git.is_dir() {
        return true;
    }
    dot_git.is_file()
        && std::fs::read_to_string(&dot_git).is_ok_and(|text| text.starts_with("gitdir:"))
}

/// Expand globs in `config.repos`, keep the git checkouts among them (see
/// [`is_git_checkout`]), dedupe, and sort. Returns an error if `repos` is empty or no matches exist.
pub fn resolve_repos(config: &CheckConfig) -> Result<Vec<Utf8PathBuf>> {
    if config.repos.is_empty() {
        anyhow::bail!("config must set `repos = [...]` with at least one entry");
    }

    let mut out: Vec<Utf8PathBuf> = Vec::new();
    for entry in &config.repos {
        let expanded = shellexpand::full(entry)
            .with_context(|| format!("failed to expand `{entry}` in repos"))?;
        let matches =
            glob::glob(&expanded).with_context(|| format!("invalid glob pattern `{entry}`"))?;
        for m in matches {
            let path = m.with_context(|| format!("error iterating glob `{entry}`"))?;
            if path.is_dir() && is_git_checkout(&path) {
                let path = Utf8PathBuf::from_path_buf(path).map_err(|p| {
                    anyhow::anyhow!("repo path is not valid UTF-8: {}", p.display())
                })?;
                out.push(path);
            }
        }
    }

    out.sort();
    out.dedup();

    if out.is_empty() {
        anyhow::bail!("no git repositories matched `repos` patterns");
    }
    Ok(out)
}

fn match_glob(pattern: &str, name: &str) -> Result<bool> {
    glob::Pattern::new(pattern)
        .map(|p| p.matches(name))
        .with_context(|| format!("invalid glob pattern: {pattern}"))
}

/// The selection fields shared by `[[check]]` and `[[exists]]` rules, borrowed
/// so one filter implementation serves both.
struct Selection<'a> {
    select: &'a str,
    exclude: Option<&'a str>,
    marker: Option<&'a str>,
    marker_absent: Option<&'a str>,
}

impl Rule {
    fn selection(&self) -> Selection<'_> {
        Selection {
            select: &self.select,
            exclude: self.exclude.as_deref(),
            marker: self.marker.as_deref(),
            marker_absent: self.marker_absent.as_deref(),
        }
    }
}

impl ExistsRule {
    fn selection(&self) -> Selection<'_> {
        Selection {
            select: &self.select,
            exclude: self.exclude.as_deref(),
            marker: self.marker.as_deref(),
            marker_absent: self.marker_absent.as_deref(),
        }
    }
}

/// Apply `select`, `exclude`, `marker` and `marker_absent` filters to the
/// discovered repo list.
/// `repos` are absolute or relative paths to repo roots.
fn select_repos(rule: &Selection<'_>, repos: &[Utf8PathBuf]) -> Result<Vec<Utf8PathBuf>> {
    let mut out = Vec::new();
    for repo in repos {
        let name = match repo.file_name() {
            Some(n) => n.to_string(),
            None => continue,
        };
        if !match_glob(rule.select, &name)? {
            continue;
        }
        if let Some(ex) = rule.exclude
            && match_glob(ex, &name)?
        {
            continue;
        }
        if let Some(marker) = rule.marker
            && !repo.join(marker).exists()
        {
            continue;
        }
        if let Some(marker) = rule.marker_absent
            && repo.join(marker).exists()
        {
            continue;
        }
        out.push(repo.clone());
    }
    Ok(out)
}

fn hash_file(path: &Utf8Path) -> Result<[u8; 32]> {
    let file = File::open(path).with_context(|| format!("failed to open {}", path))?;
    let mut reader = BufReader::with_capacity(64 * 1024, file);
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = reader
            .read(&mut buf)
            .with_context(|| format!("failed to read {}", path))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher.finalize().into())
}

/// Evaluate a single rule against the set of discovered repos.
pub fn evaluate_rule(rule: &Rule, repos: &[Utf8PathBuf]) -> Result<RuleResult> {
    let candidates = select_repos(&rule.selection(), repos)?;

    let mut files: Vec<Utf8PathBuf> = Vec::new();
    let mut missing: Vec<Utf8PathBuf> = Vec::new();
    for repo in candidates {
        let target = repo.join(&rule.path);
        if target.is_file() {
            files.push(target);
        } else {
            missing.push(repo);
        }
    }

    let mut buckets: BTreeMap<[u8; 32], Vec<Utf8PathBuf>> = BTreeMap::new();
    let mut unreadable: Vec<(Utf8PathBuf, String)> = Vec::new();
    let mut hashed = 0usize;
    for file in &files {
        match hash_file(file) {
            Ok(digest) => {
                buckets.entry(digest).or_default().push(file.clone());
                hashed += 1;
            }
            Err(e) => unreadable.push((file.clone(), format!("{e:#}"))),
        }
    }

    let mut groups: Vec<Vec<Utf8PathBuf>> = buckets.into_values().collect();
    groups.sort_by_key(|g| std::cmp::Reverse(g.len()));

    let (skipped, must_have_violations) = if rule.must_have {
        (Vec::new(), missing)
    } else {
        (missing, Vec::new())
    };

    Ok(RuleResult {
        name: rule.name.clone(),
        path: rule.path.clone(),
        groups,
        total_files: hashed,
        skipped,
        must_have_violations,
        unreadable,
    })
}

/// Evaluate a single presence rule against the set of discovered repos.
///
/// No content is read: the rule passes when every selected repo contains
/// `path`. A directory at `path` does not count — the rule asserts a file,
/// matching `evaluate_rule`'s `is_file()` test.
pub fn evaluate_exists_rule(rule: &ExistsRule, repos: &[Utf8PathBuf]) -> Result<ExistsResult> {
    let candidates = select_repos(&rule.selection(), repos)?;

    let mut present: Vec<Utf8PathBuf> = Vec::new();
    let mut missing: Vec<Utf8PathBuf> = Vec::new();
    for repo in candidates {
        if repo.join(&rule.path).is_file() {
            present.push(repo);
        } else {
            missing.push(repo);
        }
    }

    Ok(ExistsResult {
        name: rule.name.clone(),
        path: rule.path.clone(),
        present,
        missing,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn rule_paths_must_be_relative_and_inside_the_repo() {
        let tmp = TempDir::new().unwrap();
        let cfg = Utf8Path::from_path(tmp.path()).unwrap().join("c.toml");
        for (kind, bad, needle) in [
            ("check", "/etc/hostname", "absolute"),
            ("exists", "../outside", "`..`"),
            ("check", "a/../../b", "`..`"),
            ("check", "", "empty"),
        ] {
            fs::write(
                &cfg,
                format!(
                    "repos = [\"x\"]\n[[{kind}]]\nname = \"r\"\nselect = \"*\"\npath = \"{bad}\"\n"
                ),
            )
            .unwrap();
            let err = format!("{:#}", load_config(&cfg).unwrap_err());
            assert!(
                err.contains(needle) && err.contains("rule `r`"),
                "{bad}: {err}"
            );
        }
        fs::write(
            &cfg,
            "repos = [\"x\"]\n[[check]]\nname = \"r\"\nselect = \"*\"\npath = \".github/workflows/ci.yml\"\n",
        )
        .unwrap();
        assert!(load_config(&cfg).is_ok());
    }

    #[test]
    fn misspelled_config_keys_are_rejected() {
        let cases = [
            ("top level", "repo = []\n", "repo"),
            (
                "[[check]]",
                "[[check]]\nname = \"a\"\nselect = \"*\"\npath = \"f\"\nmust_hav = true\n",
                "must_hav",
            ),
            (
                "[[exists]]",
                "[[exists]]\nname = \"a\"\nselect = \"*\"\npath = \"f\"\nexlude = \"x\"\n",
                "exlude",
            ),
        ];
        for (where_, text, key) in cases {
            let err = toml::from_str::<CheckConfig>(text).unwrap_err().to_string();
            assert!(
                err.contains(&format!("unknown field `{key}`")),
                "{where_}: {err}"
            );
        }
    }

    #[test]
    fn config_example_uses_only_known_keys() {
        let text = include_str!("../../assets/config-example.toml");
        toml::from_str::<CheckConfig>(text).unwrap();
    }

    fn write(path: &Utf8Path, content: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, content).unwrap();
    }

    #[test]
    fn config_parses() {
        let toml = r#"
            [[check]]
            name = "gi"
            select = "*"
            path = ".gitignore"

            [[check]]
            name = "py-make"
            select = "py*"
            exclude = "pydraft*"
            marker = ".veltzer.tag"
            path = "Makefile"
        "#;
        let config: CheckConfig = toml::from_str(toml).unwrap();
        assert_eq!(config.check.len(), 2);
        assert_eq!(config.check[0].name, "gi");
        assert_eq!(config.check[1].exclude.as_deref(), Some("pydraft*"));
        assert_eq!(config.check[1].marker.as_deref(), Some(".veltzer.tag"));
    }

    #[test]
    fn empty_config_has_no_checks() {
        let config: CheckConfig = toml::from_str("").unwrap();
        assert!(config.check.is_empty());
    }

    #[test]
    fn enabled_defaults_to_true() {
        let toml = r#"
            [[check]]
            name = "a"
            select = "*"
            path = "x"
        "#;
        let config: CheckConfig = toml::from_str(toml).unwrap();
        assert!(config.check[0].enabled);
    }

    #[test]
    fn default_build_method_parses_and_defaults_to_none() {
        let cfg: CheckConfig = toml::from_str("repos = [\"x\"]\n").unwrap();
        assert_eq!(cfg.default_build_method, None);

        let cfg: CheckConfig =
            toml::from_str("repos = [\"x\"]\ndefault_build_method = \"rsconstruct\"\n").unwrap();
        assert_eq!(cfg.default_build_method, Some(BuildWhat::Rsconstruct));

        // Unknown methods are a config error, not a silent None.
        assert!(
            toml::from_str::<CheckConfig>("repos = [\"x\"]\ndefault_build_method = \"ninja\"\n")
                .is_err()
        );
    }

    #[test]
    fn crates_io_pass_entry_defaults_to_keys_crates_io() {
        let cfg: CheckConfig = toml::from_str("repos = [\"x\"]\n").unwrap();
        assert_eq!(cfg.crates_io_pass_entry, "keys/crates.io");

        let cfg: CheckConfig =
            toml::from_str("repos = [\"x\"]\ncrates_io_pass_entry = \"work/crates\"\n").unwrap();
        assert_eq!(cfg.crates_io_pass_entry, "work/crates");
    }

    #[test]
    fn resolve_repos_requires_non_empty() {
        let cfg = CheckConfig {
            repos: vec![],
            default_build_method: None,
            crates_io_pass_entry: crate::commands::release::default_pass_entry(),
            check: vec![],
            exists: vec![],
        };
        assert!(resolve_repos(&cfg).is_err());
    }

    #[test]
    fn resolve_repos_filters_to_git_dirs() {
        let tmp = TempDir::new().unwrap();
        // Two git repos, one plain dir.
        fs::create_dir_all(
            camino::Utf8Path::from_path(tmp.path())
                .unwrap()
                .join("a/.git"),
        )
        .unwrap();
        fs::create_dir_all(
            camino::Utf8Path::from_path(tmp.path())
                .unwrap()
                .join("b/.git"),
        )
        .unwrap();
        fs::create_dir_all(camino::Utf8Path::from_path(tmp.path()).unwrap().join("c")).unwrap();

        let cfg = CheckConfig {
            repos: vec![format!(
                "{}/*",
                camino::Utf8Path::from_path(tmp.path()).unwrap()
            )],
            default_build_method: None,
            crates_io_pass_entry: crate::commands::release::default_pass_entry(),
            check: vec![],
            exists: vec![],
        };
        let repos = resolve_repos(&cfg).unwrap();
        assert_eq!(repos.len(), 2);
        assert!(repos.iter().all(|p| p.join(".git").is_dir()));
    }

    #[test]
    fn resolve_repos_dedupes() {
        let tmp = TempDir::new().unwrap();
        fs::create_dir_all(
            camino::Utf8Path::from_path(tmp.path())
                .unwrap()
                .join("a/.git"),
        )
        .unwrap();

        let cfg = CheckConfig {
            repos: vec![
                format!("{}/a", camino::Utf8Path::from_path(tmp.path()).unwrap()),
                format!("{}/*", camino::Utf8Path::from_path(tmp.path()).unwrap()),
            ],
            default_build_method: None,
            crates_io_pass_entry: crate::commands::release::default_pass_entry(),
            check: vec![],
            exists: vec![],
        };
        let repos = resolve_repos(&cfg).unwrap();
        assert_eq!(repos.len(), 1);
    }

    #[test]
    fn resolve_repos_no_matches_errors() {
        let tmp = TempDir::new().unwrap();
        let cfg = CheckConfig {
            repos: vec![format!(
                "{}/nonexistent*",
                camino::Utf8Path::from_path(tmp.path()).unwrap()
            )],
            default_build_method: None,
            crates_io_pass_entry: crate::commands::release::default_pass_entry(),
            check: vec![],
            exists: vec![],
        };
        assert!(resolve_repos(&cfg).is_err());
    }

    #[test]
    fn enabled_can_be_disabled() {
        let toml = r#"
            [[check]]
            name = "a"
            select = "*"
            path = "x"
            enabled = false
        "#;
        let config: CheckConfig = toml::from_str(toml).unwrap();
        assert!(!config.check[0].enabled);
    }

    #[test]
    fn identical_files_form_one_group() {
        let tmp = TempDir::new().unwrap();
        for r in ["a", "b", "c"] {
            write(
                &camino::Utf8Path::from_path(tmp.path())
                    .unwrap()
                    .join(r)
                    .join(".gitignore"),
                "target\n",
            );
        }
        let repos: Vec<Utf8PathBuf> = ["a", "b", "c"]
            .iter()
            .map(|r| camino::Utf8Path::from_path(tmp.path()).unwrap().join(r))
            .collect();
        let rule = Rule {
            name: "gi".into(),
            select: "*".into(),
            exclude: None,
            marker: None,
            marker_absent: None,
            path: ".gitignore".into(),
            enabled: true,
            must_have: false,
        };
        let result = evaluate_rule(&rule, &repos).unwrap();
        assert!(result.is_consistent());
        assert_eq!(result.groups.len(), 1);
        assert_eq!(result.total_files, 3);
    }

    #[test]
    fn divergent_files_form_multiple_groups() {
        let tmp = TempDir::new().unwrap();
        write(
            &camino::Utf8Path::from_path(tmp.path())
                .unwrap()
                .join("a/.gitignore"),
            "x\n",
        );
        write(
            &camino::Utf8Path::from_path(tmp.path())
                .unwrap()
                .join("b/.gitignore"),
            "x\n",
        );
        write(
            &camino::Utf8Path::from_path(tmp.path())
                .unwrap()
                .join("c/.gitignore"),
            "y\n",
        );
        let repos: Vec<Utf8PathBuf> = ["a", "b", "c"]
            .iter()
            .map(|r| camino::Utf8Path::from_path(tmp.path()).unwrap().join(r))
            .collect();
        let rule = Rule {
            name: "gi".into(),
            select: "*".into(),
            exclude: None,
            marker: None,
            marker_absent: None,
            path: ".gitignore".into(),
            enabled: true,
            must_have: false,
        };
        let result = evaluate_rule(&rule, &repos).unwrap();
        assert!(!result.is_consistent());
        assert_eq!(result.groups.len(), 2);
        // Largest group (2 files) first.
        assert_eq!(result.groups[0].len(), 2);
        assert_eq!(result.groups[1].len(), 1);
    }

    #[test]
    fn missing_files_are_skipped_not_flagged() {
        let tmp = TempDir::new().unwrap();
        write(
            &camino::Utf8Path::from_path(tmp.path())
                .unwrap()
                .join("a/.gitignore"),
            "x\n",
        );
        write(
            &camino::Utf8Path::from_path(tmp.path())
                .unwrap()
                .join("b/.gitignore"),
            "x\n",
        );
        fs::create_dir_all(camino::Utf8Path::from_path(tmp.path()).unwrap().join("c")).unwrap();
        let repos: Vec<Utf8PathBuf> = ["a", "b", "c"]
            .iter()
            .map(|r| camino::Utf8Path::from_path(tmp.path()).unwrap().join(r))
            .collect();
        let rule = Rule {
            name: "gi".into(),
            select: "*".into(),
            exclude: None,
            marker: None,
            marker_absent: None,
            path: ".gitignore".into(),
            enabled: true,
            must_have: false,
        };
        let result = evaluate_rule(&rule, &repos).unwrap();
        assert!(result.is_consistent());
        assert_eq!(result.total_files, 2);
        assert_eq!(result.skipped.len(), 1);
    }

    #[test]
    fn select_filters_by_repo_name_glob() {
        let tmp = TempDir::new().unwrap();
        write(
            &camino::Utf8Path::from_path(tmp.path())
                .unwrap()
                .join("pyalpha/Makefile"),
            "PY\n",
        );
        write(
            &camino::Utf8Path::from_path(tmp.path())
                .unwrap()
                .join("pybeta/Makefile"),
            "PY\n",
        );
        write(
            &camino::Utf8Path::from_path(tmp.path())
                .unwrap()
                .join("go-proj/Makefile"),
            "GO\n",
        );
        let repos: Vec<Utf8PathBuf> = ["pyalpha", "pybeta", "go-proj"]
            .iter()
            .map(|r| camino::Utf8Path::from_path(tmp.path()).unwrap().join(r))
            .collect();
        let rule = Rule {
            name: "py-make".into(),
            select: "py*".into(),
            exclude: None,
            marker: None,
            marker_absent: None,
            path: "Makefile".into(),
            enabled: true,
            must_have: false,
        };
        let result = evaluate_rule(&rule, &repos).unwrap();
        assert!(result.is_consistent());
        assert_eq!(result.total_files, 2);
    }

    #[test]
    fn exclude_drops_matching_repos() {
        let tmp = TempDir::new().unwrap();
        write(
            &camino::Utf8Path::from_path(tmp.path())
                .unwrap()
                .join("pyalpha/Makefile"),
            "A\n",
        );
        write(
            &camino::Utf8Path::from_path(tmp.path())
                .unwrap()
                .join("pybeta/Makefile"),
            "A\n",
        );
        write(
            &camino::Utf8Path::from_path(tmp.path())
                .unwrap()
                .join("pydraft/Makefile"),
            "B\n",
        );
        let repos: Vec<Utf8PathBuf> = ["pyalpha", "pybeta", "pydraft"]
            .iter()
            .map(|r| camino::Utf8Path::from_path(tmp.path()).unwrap().join(r))
            .collect();
        let rule = Rule {
            name: "py-make".into(),
            select: "py*".into(),
            exclude: Some("pydraft*".into()),
            marker: None,
            marker_absent: None,
            path: "Makefile".into(),
            enabled: true,
            must_have: false,
        };
        let result = evaluate_rule(&rule, &repos).unwrap();
        assert!(result.is_consistent());
        assert_eq!(result.total_files, 2);
    }

    #[test]
    fn must_have_defaults_to_false() {
        let toml = r#"
            [[check]]
            name = "a"
            select = "*"
            path = "x"
        "#;
        let config: CheckConfig = toml::from_str(toml).unwrap();
        assert!(!config.check[0].must_have);
    }

    #[test]
    fn must_have_false_keeps_missing_in_skipped() {
        let tmp = TempDir::new().unwrap();
        write(
            &camino::Utf8Path::from_path(tmp.path())
                .unwrap()
                .join("a/.gitignore"),
            "x\n",
        );
        write(
            &camino::Utf8Path::from_path(tmp.path())
                .unwrap()
                .join("b/.gitignore"),
            "x\n",
        );
        fs::create_dir_all(camino::Utf8Path::from_path(tmp.path()).unwrap().join("c")).unwrap();
        let repos: Vec<Utf8PathBuf> = ["a", "b", "c"]
            .iter()
            .map(|r| camino::Utf8Path::from_path(tmp.path()).unwrap().join(r))
            .collect();
        let rule = Rule {
            name: "gi".into(),
            select: "*".into(),
            exclude: None,
            marker: None,
            marker_absent: None,
            path: ".gitignore".into(),
            enabled: true,
            must_have: false,
        };
        let result = evaluate_rule(&rule, &repos).unwrap();
        assert!(result.is_consistent());
        assert_eq!(result.skipped.len(), 1);
        assert!(result.must_have_violations.is_empty());
    }

    #[test]
    fn must_have_true_moves_missing_to_violations() {
        let tmp = TempDir::new().unwrap();
        write(
            &camino::Utf8Path::from_path(tmp.path())
                .unwrap()
                .join("a/.gitignore"),
            "x\n",
        );
        write(
            &camino::Utf8Path::from_path(tmp.path())
                .unwrap()
                .join("b/.gitignore"),
            "x\n",
        );
        fs::create_dir_all(camino::Utf8Path::from_path(tmp.path()).unwrap().join("c")).unwrap();
        let repos: Vec<Utf8PathBuf> = ["a", "b", "c"]
            .iter()
            .map(|r| camino::Utf8Path::from_path(tmp.path()).unwrap().join(r))
            .collect();
        let rule = Rule {
            name: "gi".into(),
            select: "*".into(),
            exclude: None,
            marker: None,
            marker_absent: None,
            path: ".gitignore".into(),
            enabled: true,
            must_have: true,
        };
        let result = evaluate_rule(&rule, &repos).unwrap();
        assert!(!result.is_consistent());
        assert!(result.skipped.is_empty());
        assert_eq!(result.must_have_violations.len(), 1);
        assert!(result.must_have_violations[0].ends_with("c"));
    }

    #[test]
    fn must_have_true_all_present_is_consistent() {
        let tmp = TempDir::new().unwrap();
        for r in ["a", "b", "c"] {
            write(
                &camino::Utf8Path::from_path(tmp.path())
                    .unwrap()
                    .join(r)
                    .join(".gitignore"),
                "same\n",
            );
        }
        let repos: Vec<Utf8PathBuf> = ["a", "b", "c"]
            .iter()
            .map(|r| camino::Utf8Path::from_path(tmp.path()).unwrap().join(r))
            .collect();
        let rule = Rule {
            name: "gi".into(),
            select: "*".into(),
            exclude: None,
            marker: None,
            marker_absent: None,
            path: ".gitignore".into(),
            enabled: true,
            must_have: true,
        };
        let result = evaluate_rule(&rule, &repos).unwrap();
        assert!(result.is_consistent());
        assert!(result.must_have_violations.is_empty());
    }

    #[test]
    fn rule_matching_no_files_reports_matched_nothing() {
        let tmp = TempDir::new().unwrap();
        fs::create_dir_all(camino::Utf8Path::from_path(tmp.path()).unwrap().join("a")).unwrap();
        fs::create_dir_all(camino::Utf8Path::from_path(tmp.path()).unwrap().join("b")).unwrap();
        let repos: Vec<Utf8PathBuf> = ["a", "b"]
            .iter()
            .map(|r| camino::Utf8Path::from_path(tmp.path()).unwrap().join(r))
            .collect();
        let rule = Rule {
            name: "gi".into(),
            select: "*".into(),
            exclude: None,
            marker: None,
            marker_absent: None,
            path: ".gitignore".into(),
            enabled: true,
            must_have: false,
        };
        let result = evaluate_rule(&rule, &repos).unwrap();
        assert!(result.matched_nothing());
        assert_eq!(result.total_files, 0);
        // A must_have rule with the same emptiness has violations to report,
        // so it is not "matched nothing" — it's a plain failure.
        let must_have_rule = Rule {
            must_have: true,
            name: "gi".into(),
            select: "*".into(),
            exclude: None,
            marker: None,
            marker_absent: None,
            path: ".gitignore".into(),
            enabled: true,
        };
        let result = evaluate_rule(&must_have_rule, &repos).unwrap();
        assert!(!result.matched_nothing());
    }

    #[test]
    fn rule_with_files_is_not_matched_nothing() {
        let tmp = TempDir::new().unwrap();
        write(
            &camino::Utf8Path::from_path(tmp.path())
                .unwrap()
                .join("a/.gitignore"),
            "x\n",
        );
        let repos = vec![camino::Utf8Path::from_path(tmp.path()).unwrap().join("a")];
        let rule = Rule {
            name: "gi".into(),
            select: "*".into(),
            exclude: None,
            marker: None,
            marker_absent: None,
            path: ".gitignore".into(),
            enabled: true,
            must_have: false,
        };
        let result = evaluate_rule(&rule, &repos).unwrap();
        assert!(!result.matched_nothing());
    }

    #[test]
    fn marker_absent_filters_out_opted_out_repos() {
        let tmp = TempDir::new().unwrap();
        write(
            &camino::Utf8Path::from_path(tmp.path())
                .unwrap()
                .join("a/.gitignore"),
            "x\n",
        );
        write(
            &camino::Utf8Path::from_path(tmp.path())
                .unwrap()
                .join("b/.gitignore"),
            "DIFFERENT\n",
        );
        write(
            &camino::Utf8Path::from_path(tmp.path())
                .unwrap()
                .join("b/.noci"),
            "",
        );
        let repos: Vec<Utf8PathBuf> = ["a", "b"]
            .iter()
            .map(|r| camino::Utf8Path::from_path(tmp.path()).unwrap().join(r))
            .collect();
        let rule = Rule {
            name: "gi".into(),
            select: "*".into(),
            exclude: None,
            marker: None,
            marker_absent: Some(".noci".into()),
            path: ".gitignore".into(),
            enabled: true,
            must_have: false,
        };
        let result = evaluate_rule(&rule, &repos).unwrap();
        // b opted out, so its divergent .gitignore never enters the comparison.
        assert!(result.is_consistent());
        assert_eq!(result.total_files, 1);
    }

    #[test]
    fn marker_absent_exempts_repo_from_must_have() {
        let tmp = TempDir::new().unwrap();
        write(
            &camino::Utf8Path::from_path(tmp.path())
                .unwrap()
                .join("a/build.yml"),
            "x\n",
        );
        // b has no build.yml at all, but opts out of the rule.
        write(
            &camino::Utf8Path::from_path(tmp.path())
                .unwrap()
                .join("b/.noci"),
            "",
        );
        let repos: Vec<Utf8PathBuf> = ["a", "b"]
            .iter()
            .map(|r| camino::Utf8Path::from_path(tmp.path()).unwrap().join(r))
            .collect();
        let rule = Rule {
            name: "ci".into(),
            select: "*".into(),
            exclude: None,
            marker: None,
            marker_absent: Some(".noci".into()),
            path: "build.yml".into(),
            enabled: true,
            must_have: true,
        };
        let result = evaluate_rule(&rule, &repos).unwrap();
        assert!(result.must_have_violations.is_empty());
        assert!(result.is_consistent());
    }

    #[test]
    fn marker_filters_by_presence() {
        let tmp = TempDir::new().unwrap();
        write(
            &camino::Utf8Path::from_path(tmp.path())
                .unwrap()
                .join("a/.tag"),
            "",
        );
        write(
            &camino::Utf8Path::from_path(tmp.path())
                .unwrap()
                .join("a/.gitignore"),
            "x\n",
        );
        write(
            &camino::Utf8Path::from_path(tmp.path())
                .unwrap()
                .join("b/.gitignore"),
            "y\n",
        );
        let repos: Vec<Utf8PathBuf> = ["a", "b"]
            .iter()
            .map(|r| camino::Utf8Path::from_path(tmp.path()).unwrap().join(r))
            .collect();
        let rule = Rule {
            name: "gi".into(),
            select: "*".into(),
            exclude: None,
            marker: Some(".tag".into()),
            marker_absent: None,
            path: ".gitignore".into(),
            enabled: true,
            must_have: false,
        };
        let result = evaluate_rule(&rule, &repos).unwrap();
        assert!(result.is_consistent());
        assert_eq!(result.total_files, 1);
    }

    fn exists_rule(path: &str, select: &str) -> ExistsRule {
        ExistsRule {
            name: "r".into(),
            select: select.into(),
            exclude: None,
            marker: None,
            marker_absent: None,
            path: path.into(),
            enabled: true,
        }
    }

    #[test]
    fn exists_config_parses() {
        let toml = r#"
            [[exists]]
            name = "readme"
            select = "*"
            path = "README.md"
        "#;
        let config: CheckConfig = toml::from_str(toml).unwrap();
        assert_eq!(config.exists.len(), 1);
        assert_eq!(config.exists[0].name, "readme");
        assert!(config.exists[0].enabled);
        // [[check]] and [[exists]] are independent lists.
        assert!(config.check.is_empty());
    }

    #[test]
    fn exists_and_check_rules_coexist() {
        let toml = r#"
            [[check]]
            name = "gi"
            select = "*"
            path = ".gitignore"

            [[exists]]
            name = "readme"
            select = "*"
            path = "README.md"
        "#;
        let config: CheckConfig = toml::from_str(toml).unwrap();
        assert_eq!(config.check.len(), 1);
        assert_eq!(config.exists.len(), 1);
    }

    #[test]
    fn exists_passes_when_all_present_despite_differing_content() {
        let tmp = TempDir::new().unwrap();
        // Deliberately different content in every repo: presence is the point.
        write(
            &camino::Utf8Path::from_path(tmp.path())
                .unwrap()
                .join("a/README.md"),
            "# a\n",
        );
        write(
            &camino::Utf8Path::from_path(tmp.path())
                .unwrap()
                .join("b/README.md"),
            "# b, entirely different\n",
        );
        write(
            &camino::Utf8Path::from_path(tmp.path())
                .unwrap()
                .join("c/README.md"),
            "",
        );
        let repos: Vec<Utf8PathBuf> = ["a", "b", "c"]
            .iter()
            .map(|r| camino::Utf8Path::from_path(tmp.path()).unwrap().join(r))
            .collect();
        let result = evaluate_exists_rule(&exists_rule("README.md", "*"), &repos).unwrap();
        assert!(result.is_satisfied());
        assert_eq!(result.present.len(), 3);
        assert!(result.missing.is_empty());
        assert_eq!(result.total_repos(), 3);
    }

    #[test]
    fn exists_flags_the_repo_that_lacks_the_file() {
        let tmp = TempDir::new().unwrap();
        write(
            &camino::Utf8Path::from_path(tmp.path())
                .unwrap()
                .join("a/README.md"),
            "# a\n",
        );
        fs::create_dir_all(camino::Utf8Path::from_path(tmp.path()).unwrap().join("b")).unwrap();
        let repos: Vec<Utf8PathBuf> = ["a", "b"]
            .iter()
            .map(|r| camino::Utf8Path::from_path(tmp.path()).unwrap().join(r))
            .collect();
        let result = evaluate_exists_rule(&exists_rule("README.md", "*"), &repos).unwrap();
        assert!(!result.is_satisfied());
        assert_eq!(result.missing.len(), 1);
        assert!(result.missing[0].ends_with("b"));
        assert_eq!(result.present.len(), 1);
    }

    #[test]
    fn exists_does_not_accept_a_directory() {
        let tmp = TempDir::new().unwrap();
        write(
            &camino::Utf8Path::from_path(tmp.path())
                .unwrap()
                .join("a/README.md"),
            "x\n",
        );
        // b has a *directory* named README.md, which must not satisfy the rule.
        fs::create_dir_all(
            camino::Utf8Path::from_path(tmp.path())
                .unwrap()
                .join("b/README.md"),
        )
        .unwrap();
        let repos: Vec<Utf8PathBuf> = ["a", "b"]
            .iter()
            .map(|r| camino::Utf8Path::from_path(tmp.path()).unwrap().join(r))
            .collect();
        let result = evaluate_exists_rule(&exists_rule("README.md", "*"), &repos).unwrap();
        assert!(!result.is_satisfied());
        assert_eq!(result.missing.len(), 1);
    }

    #[test]
    fn exists_honours_select_and_exclude() {
        let tmp = TempDir::new().unwrap();
        write(
            &camino::Utf8Path::from_path(tmp.path())
                .unwrap()
                .join("pyalpha/README.md"),
            "x\n",
        );
        fs::create_dir_all(
            camino::Utf8Path::from_path(tmp.path())
                .unwrap()
                .join("pydraft"),
        )
        .unwrap();
        fs::create_dir_all(
            camino::Utf8Path::from_path(tmp.path())
                .unwrap()
                .join("go-proj"),
        )
        .unwrap();
        let repos: Vec<Utf8PathBuf> = ["pyalpha", "pydraft", "go-proj"]
            .iter()
            .map(|r| camino::Utf8Path::from_path(tmp.path()).unwrap().join(r))
            .collect();
        let mut rule = exists_rule("README.md", "py*");
        rule.exclude = Some("pydraft*".into());
        let result = evaluate_exists_rule(&rule, &repos).unwrap();
        // go-proj is out of select, pydraft is excluded: only pyalpha is judged.
        assert!(result.is_satisfied());
        assert_eq!(result.total_repos(), 1);
    }

    #[test]
    fn exists_marker_absent_exempts_opted_out_repo() {
        let tmp = TempDir::new().unwrap();
        write(
            &camino::Utf8Path::from_path(tmp.path())
                .unwrap()
                .join("a/README.md"),
            "x\n",
        );
        // b has no README but opts out from inside itself.
        write(
            &camino::Utf8Path::from_path(tmp.path())
                .unwrap()
                .join("b/.noreadme"),
            "",
        );
        let repos: Vec<Utf8PathBuf> = ["a", "b"]
            .iter()
            .map(|r| camino::Utf8Path::from_path(tmp.path()).unwrap().join(r))
            .collect();
        let mut rule = exists_rule("README.md", "*");
        rule.marker_absent = Some(".noreadme".into());
        let result = evaluate_exists_rule(&rule, &repos).unwrap();
        assert!(result.is_satisfied());
        assert_eq!(result.total_repos(), 1);
    }

    #[test]
    fn exists_marker_limits_scope_to_tagged_repos() {
        let tmp = TempDir::new().unwrap();
        write(
            &camino::Utf8Path::from_path(tmp.path())
                .unwrap()
                .join("a/.tag"),
            "",
        );
        fs::create_dir_all(camino::Utf8Path::from_path(tmp.path()).unwrap().join("b")).unwrap();
        let repos: Vec<Utf8PathBuf> = ["a", "b"]
            .iter()
            .map(|r| camino::Utf8Path::from_path(tmp.path()).unwrap().join(r))
            .collect();
        let mut rule = exists_rule("README.md", "*");
        rule.marker = Some(".tag".into());
        let result = evaluate_exists_rule(&rule, &repos).unwrap();
        // Only a is in scope, and it lacks the README, so the rule fails on it.
        assert_eq!(result.total_repos(), 1);
        assert_eq!(result.missing.len(), 1);
    }

    #[test]
    fn exists_selecting_no_repos_reports_matched_nothing() {
        let tmp = TempDir::new().unwrap();
        fs::create_dir_all(camino::Utf8Path::from_path(tmp.path()).unwrap().join("a")).unwrap();
        let repos = vec![camino::Utf8Path::from_path(tmp.path()).unwrap().join("a")];
        let result = evaluate_exists_rule(&exists_rule("README.md", "zz*"), &repos).unwrap();
        assert!(result.matched_nothing());
        assert!(result.is_satisfied());
    }

    #[test]
    fn exists_with_a_missing_file_is_not_matched_nothing() {
        let tmp = TempDir::new().unwrap();
        fs::create_dir_all(camino::Utf8Path::from_path(tmp.path()).unwrap().join("a")).unwrap();
        let repos = vec![camino::Utf8Path::from_path(tmp.path()).unwrap().join("a")];
        let result = evaluate_exists_rule(&exists_rule("README.md", "*"), &repos).unwrap();
        assert!(!result.matched_nothing());
        assert!(!result.is_satisfied());
    }
}
