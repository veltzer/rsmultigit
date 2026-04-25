use std::collections::BTreeMap;
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Deserialize;
use sha2::{Digest, Sha256};

#[derive(Debug, Deserialize)]
pub struct CheckConfig {
    /// Glob patterns (with shell expansion) identifying repo roots.
    /// Non-git directories matching the pattern are filtered out.
    #[serde(default)]
    pub repos: Vec<String>,
    #[serde(default)]
    pub check: Vec<Rule>,
}

#[derive(Debug, Deserialize)]
pub struct Rule {
    pub name: String,
    pub select: String,
    #[serde(default)]
    pub exclude: Option<String>,
    #[serde(default)]
    pub marker: Option<String>,
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
    pub groups: Vec<Vec<PathBuf>>,
    /// Total number of files considered.
    pub total_files: usize,
    /// Repos that matched selection but lacked `path`. When the rule has
    /// `must_have = false`, they're treated as "skipped" (not a violation).
    pub skipped: Vec<PathBuf>,
    /// Repos that matched selection but lacked `path` *and* the rule has
    /// `must_have = true`. These are rule violations.
    pub must_have_violations: Vec<PathBuf>,
}

impl RuleResult {
    pub fn is_consistent(&self) -> bool {
        self.groups.len() <= 1 && self.must_have_violations.is_empty()
    }
}

/// Environment variable used to override the config path (tests set this).
pub const CONFIG_PATH_ENV: &str = "RSMULTIGIT_CONFIG";

/// Resolve the path of the config file. Tests can override via `RSMULTIGIT_CONFIG`.
pub fn default_config_path() -> Result<PathBuf> {
    if let Ok(p) = std::env::var(CONFIG_PATH_ENV) {
        return Ok(PathBuf::from(p));
    }
    let expanded = shellexpand::full("~/.config/rsmultigit/config.toml")
        .context("failed to expand default config path")?;
    Ok(PathBuf::from(expanded.into_owned()))
}

/// Parse a config file from disk.
pub fn load_config(path: &Path) -> Result<CheckConfig> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read config file {}", path.display()))?;
    let config: CheckConfig = toml::from_str(&text)
        .with_context(|| format!("failed to parse config file {}", path.display()))?;
    Ok(config)
}

/// Expand globs in `config.repos`, filter to directories containing `.git/`,
/// dedupe, and sort. Returns an error if `repos` is empty or no matches exist.
pub fn resolve_repos(config: &CheckConfig) -> Result<Vec<PathBuf>> {
    if config.repos.is_empty() {
        anyhow::bail!("config must set `repos = [...]` with at least one entry");
    }

    let mut out: Vec<PathBuf> = Vec::new();
    for entry in &config.repos {
        let expanded = shellexpand::full(entry)
            .with_context(|| format!("failed to expand `{entry}` in repos"))?;
        let matches =
            glob::glob(&expanded).with_context(|| format!("invalid glob pattern `{entry}`"))?;
        for m in matches {
            let path = m.with_context(|| format!("error iterating glob `{entry}`"))?;
            if path.is_dir() && path.join(".git").is_dir() {
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

/// Apply `select`, `exclude`, and `marker` filters to the discovered repo list.
/// `repos` are absolute or relative paths to repo roots.
fn select_repos(rule: &Rule, repos: &[PathBuf]) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    for repo in repos {
        let name = match repo.file_name() {
            Some(n) => n.to_string_lossy().into_owned(),
            None => continue,
        };
        if !match_glob(&rule.select, &name)? {
            continue;
        }
        if let Some(ex) = &rule.exclude
            && match_glob(ex, &name)?
        {
            continue;
        }
        if let Some(marker) = &rule.marker
            && !repo.join(marker).exists()
        {
            continue;
        }
        out.push(repo.clone());
    }
    Ok(out)
}

fn hash_file(path: &Path) -> Result<[u8; 32]> {
    let file = File::open(path).with_context(|| format!("failed to open {}", path.display()))?;
    let mut reader = BufReader::with_capacity(64 * 1024, file);
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = reader
            .read(&mut buf)
            .with_context(|| format!("failed to read {}", path.display()))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher.finalize().into())
}

/// Evaluate a single rule against the set of discovered repos.
pub fn evaluate_rule(rule: &Rule, repos: &[PathBuf]) -> Result<RuleResult> {
    let candidates = select_repos(rule, repos)?;

    let mut files: Vec<PathBuf> = Vec::new();
    let mut missing: Vec<PathBuf> = Vec::new();
    for repo in candidates {
        let target = repo.join(&rule.path);
        if target.is_file() {
            files.push(target);
        } else {
            missing.push(repo);
        }
    }

    let mut buckets: BTreeMap<[u8; 32], Vec<PathBuf>> = BTreeMap::new();
    for file in &files {
        let digest = hash_file(file)?;
        buckets.entry(digest).or_default().push(file.clone());
    }

    let mut groups: Vec<Vec<PathBuf>> = buckets.into_values().collect();
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
        total_files: files.len(),
        skipped,
        must_have_violations,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn write(path: &Path, content: &str) {
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
    fn resolve_repos_requires_non_empty() {
        let cfg = CheckConfig {
            repos: vec![],
            check: vec![],
        };
        assert!(resolve_repos(&cfg).is_err());
    }

    #[test]
    fn resolve_repos_filters_to_git_dirs() {
        let tmp = TempDir::new().unwrap();
        // Two git repos, one plain dir.
        fs::create_dir_all(tmp.path().join("a/.git")).unwrap();
        fs::create_dir_all(tmp.path().join("b/.git")).unwrap();
        fs::create_dir_all(tmp.path().join("c")).unwrap();

        let cfg = CheckConfig {
            repos: vec![format!("{}/*", tmp.path().display())],
            check: vec![],
        };
        let repos = resolve_repos(&cfg).unwrap();
        assert_eq!(repos.len(), 2);
        assert!(repos.iter().all(|p| p.join(".git").is_dir()));
    }

    #[test]
    fn resolve_repos_dedupes() {
        let tmp = TempDir::new().unwrap();
        fs::create_dir_all(tmp.path().join("a/.git")).unwrap();

        let cfg = CheckConfig {
            repos: vec![
                format!("{}/a", tmp.path().display()),
                format!("{}/*", tmp.path().display()),
            ],
            check: vec![],
        };
        let repos = resolve_repos(&cfg).unwrap();
        assert_eq!(repos.len(), 1);
    }

    #[test]
    fn resolve_repos_no_matches_errors() {
        let tmp = TempDir::new().unwrap();
        let cfg = CheckConfig {
            repos: vec![format!("{}/nonexistent*", tmp.path().display())],
            check: vec![],
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
            write(&tmp.path().join(r).join(".gitignore"), "target\n");
        }
        let repos: Vec<PathBuf> = ["a", "b", "c"].iter().map(|r| tmp.path().join(r)).collect();
        let rule = Rule {
            name: "gi".into(),
            select: "*".into(),
            exclude: None,
            marker: None,
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
        write(&tmp.path().join("a/.gitignore"), "x\n");
        write(&tmp.path().join("b/.gitignore"), "x\n");
        write(&tmp.path().join("c/.gitignore"), "y\n");
        let repos: Vec<PathBuf> = ["a", "b", "c"].iter().map(|r| tmp.path().join(r)).collect();
        let rule = Rule {
            name: "gi".into(),
            select: "*".into(),
            exclude: None,
            marker: None,
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
        write(&tmp.path().join("a/.gitignore"), "x\n");
        write(&tmp.path().join("b/.gitignore"), "x\n");
        fs::create_dir_all(tmp.path().join("c")).unwrap();
        let repos: Vec<PathBuf> = ["a", "b", "c"].iter().map(|r| tmp.path().join(r)).collect();
        let rule = Rule {
            name: "gi".into(),
            select: "*".into(),
            exclude: None,
            marker: None,
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
        write(&tmp.path().join("pyalpha/Makefile"), "PY\n");
        write(&tmp.path().join("pybeta/Makefile"), "PY\n");
        write(&tmp.path().join("go-proj/Makefile"), "GO\n");
        let repos: Vec<PathBuf> = ["pyalpha", "pybeta", "go-proj"]
            .iter()
            .map(|r| tmp.path().join(r))
            .collect();
        let rule = Rule {
            name: "py-make".into(),
            select: "py*".into(),
            exclude: None,
            marker: None,
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
        write(&tmp.path().join("pyalpha/Makefile"), "A\n");
        write(&tmp.path().join("pybeta/Makefile"), "A\n");
        write(&tmp.path().join("pydraft/Makefile"), "B\n");
        let repos: Vec<PathBuf> = ["pyalpha", "pybeta", "pydraft"]
            .iter()
            .map(|r| tmp.path().join(r))
            .collect();
        let rule = Rule {
            name: "py-make".into(),
            select: "py*".into(),
            exclude: Some("pydraft*".into()),
            marker: None,
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
        write(&tmp.path().join("a/.gitignore"), "x\n");
        write(&tmp.path().join("b/.gitignore"), "x\n");
        fs::create_dir_all(tmp.path().join("c")).unwrap();
        let repos: Vec<PathBuf> = ["a", "b", "c"].iter().map(|r| tmp.path().join(r)).collect();
        let rule = Rule {
            name: "gi".into(),
            select: "*".into(),
            exclude: None,
            marker: None,
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
        write(&tmp.path().join("a/.gitignore"), "x\n");
        write(&tmp.path().join("b/.gitignore"), "x\n");
        fs::create_dir_all(tmp.path().join("c")).unwrap();
        let repos: Vec<PathBuf> = ["a", "b", "c"].iter().map(|r| tmp.path().join(r)).collect();
        let rule = Rule {
            name: "gi".into(),
            select: "*".into(),
            exclude: None,
            marker: None,
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
            write(&tmp.path().join(r).join(".gitignore"), "same\n");
        }
        let repos: Vec<PathBuf> = ["a", "b", "c"].iter().map(|r| tmp.path().join(r)).collect();
        let rule = Rule {
            name: "gi".into(),
            select: "*".into(),
            exclude: None,
            marker: None,
            path: ".gitignore".into(),
            enabled: true,
            must_have: true,
        };
        let result = evaluate_rule(&rule, &repos).unwrap();
        assert!(result.is_consistent());
        assert!(result.must_have_violations.is_empty());
    }

    #[test]
    fn marker_filters_by_presence() {
        let tmp = TempDir::new().unwrap();
        write(&tmp.path().join("a/.tag"), "");
        write(&tmp.path().join("a/.gitignore"), "x\n");
        write(&tmp.path().join("b/.gitignore"), "y\n");
        let repos: Vec<PathBuf> = ["a", "b"].iter().map(|r| tmp.path().join(r)).collect();
        let rule = Rule {
            name: "gi".into(),
            select: "*".into(),
            exclude: None,
            marker: Some(".tag".into()),
            path: ".gitignore".into(),
            enabled: true,
            must_have: false,
        };
        let result = evaluate_rule(&rule, &repos).unwrap();
        assert!(result.is_consistent());
        assert_eq!(result.total_files, 1);
    }
}
