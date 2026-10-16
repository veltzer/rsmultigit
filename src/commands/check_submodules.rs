//! `check submodules`: every submodule that several fleet repos share must be
//! checked out at the same commit in all of them.
//!
//! A submodule is the fleet's way of sharing a repo (design tokens, tag
//! vocabularies, term lists) and the pin is the part that drifts: one
//! consumer bumps it, the others do not, and the sites stop agreeing on
//! something that was meant to be identical. Nothing in the submodule
//! machinery itself notices. This check does, without any configuration:
//! the fleet's repos are scanned, their submodules grouped by remote URL,
//! and a group whose members pin different commits is a failure.
//!
//! The commit compared is the one checked out in the submodule's working
//! directory, the same view `check same` takes of ordinary files: what is on
//! disk and what a build would use. A submodule that is not checked out falls
//! back to the commit recorded in the superproject's index, then in HEAD.

use std::collections::BTreeMap;
use std::io::Write;

use anyhow::{Context, Result};
use camino::Utf8PathBuf;

use crate::config::AppConfig;

/// One submodule as found in one repo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubmoduleUse {
    /// The superproject.
    pub repo: Utf8PathBuf,
    /// The submodule's path inside the superproject.
    pub path: String,
    /// The pinned commit, abbreviated to what `git` shows (7 characters), or
    /// `None` when neither the checkout, the index nor HEAD records one.
    pub commit: Option<String>,
}

/// Every use of one submodule across the fleet.
#[derive(Debug)]
pub struct SubmoduleResult {
    /// The last path segment of the normalised URL: `shared-themes`.
    pub name: String,
    /// The normalised URL every member shares: `github.com/veltzer/shared-themes`.
    pub url: String,
    /// Uses grouped by commit, largest group first, each group sorted by repo.
    /// The `None` key collects uses that record no commit at all.
    pub groups: Vec<(Option<String>, Vec<SubmoduleUse>)>,
}

impl SubmoduleResult {
    /// Every member pins the same, known commit.
    pub fn is_consistent(&self) -> bool {
        self.groups.len() <= 1 && self.groups.iter().all(|(commit, _)| commit.is_some())
    }

    pub fn total_repos(&self) -> usize {
        self.groups.iter().map(|(_, uses)| uses.len()).sum()
    }
}

/// Reduce a submodule URL to the identity of the repo it names, so the same
/// submodule added over ssh in one repo and https in another is one group:
/// scheme and user dropped, the scp-style colon turned into a slash, trailing
/// slash and `.git` removed, case folded. A relative URL (`../shared-themes`)
/// is kept as written; it resolves against each superproject's own remote,
/// which this check does not consult.
pub fn normalize_url(url: &str) -> String {
    let mut s = url.trim().to_string();
    for scheme in ["git+ssh://", "ssh://", "https://", "http://", "git://"] {
        if let Some(rest) = s.strip_prefix(scheme) {
            s = rest.to_string();
            break;
        }
    }
    // user@host: drop the user. Only before the first path separator, so a
    // path component containing `@` is left alone.
    let authority_end = s.find(['/', ':']).unwrap_or(s.len());
    if let Some(at) = s[..authority_end].rfind('@') {
        s = s[at + 1..].to_string();
    }
    // scp-style host:path becomes host/path.
    if let Some(colon) = s.find(':')
        && !s[..colon].contains('/')
    {
        s.replace_range(colon..=colon, "/");
    }
    let s = s.trim_end_matches('/');
    let s = s.strip_suffix(".git").unwrap_or(s);
    s.trim_end_matches('/').to_ascii_lowercase()
}

/// The display name of a submodule: the last segment of its normalised URL.
fn name_of(url: &str) -> String {
    url.rsplit('/')
        .find(|segment| !segment.is_empty() && *segment != "..")
        .unwrap_or(url)
        .to_string()
}

fn short(oid: git2::Oid) -> String {
    oid.to_string()[..7].to_string()
}

/// Scan `repos` for submodules and group them by normalised URL. A repo
/// without submodules contributes nothing; a submodule declared without a
/// URL is keyed by its path instead, which keeps it visible rather than
/// silently dropped.
pub fn evaluate_submodules(repos: &[Utf8PathBuf]) -> Result<Vec<SubmoduleResult>> {
    let mut by_url: BTreeMap<String, Vec<SubmoduleUse>> = BTreeMap::new();
    for repo in repos {
        let git = git2::Repository::open(repo).with_context(|| format!("failed to open {repo}"))?;
        let submodules = git
            .submodules()
            .with_context(|| format!("failed to read submodules of {repo}"))?;
        for sm in submodules {
            let path = sm.path().to_string_lossy().to_string();
            // A URL git2 cannot hand back as text counts as absent, and the
            // submodule is keyed by its path.
            let key = sm
                .url()
                .ok()
                .flatten()
                .map(normalize_url)
                .unwrap_or_else(|| path.clone());
            let commit = sm
                .workdir_id()
                .or_else(|| sm.index_id())
                .or_else(|| sm.head_id())
                .map(short);
            by_url.entry(key).or_default().push(SubmoduleUse {
                repo: repo.clone(),
                path,
                commit,
            });
        }
    }

    let mut results = Vec::new();
    for (url, uses) in by_url {
        let mut by_commit: BTreeMap<Option<String>, Vec<SubmoduleUse>> = BTreeMap::new();
        for u in uses {
            by_commit.entry(u.commit.clone()).or_default().push(u);
        }
        let mut groups: Vec<(Option<String>, Vec<SubmoduleUse>)> = by_commit.into_iter().collect();
        for (_, members) in &mut groups {
            members.sort_by(|a, b| a.repo.cmp(&b.repo));
        }
        groups.sort_by_key(|(_, members)| std::cmp::Reverse(members.len()));
        results.push(SubmoduleResult {
            name: name_of(&url),
            url,
            groups,
        });
    }
    Ok(results)
}

/// Run the check submodules command. Returns the process exit code.
///
/// Passing submodules print `ok (N repos @ <commit>)` unless `only_failed`
/// or `--terse` is set; `--terse` prints only the names of failing
/// submodules, one per line. A fleet without any submodule passes: there is
/// nothing to drift. With `--short-circuit`, evaluation stops at the first
/// failing submodule.
pub fn run_check_submodules<W: Write>(
    app: &AppConfig,
    projects: &[Utf8PathBuf],
    only_failed: bool,
    out: &mut W,
) -> Result<i32> {
    let results = evaluate_submodules(projects)?;
    if results.is_empty() {
        if app.verbose {
            writeln!(out, "no submodules found")?;
        }
        return Ok(0);
    }

    let mut any_failed = false;
    for result in &results {
        if result.is_consistent() {
            if !only_failed && !app.terse {
                header(app, out, result)?;
                let commit = result
                    .groups
                    .first()
                    .and_then(|(commit, _)| commit.as_deref())
                    .unwrap_or("-");
                writeln!(out, "ok ({} repos @ {commit})", result.total_repos())?;
            }
            continue;
        }

        any_failed = true;
        if app.terse {
            writeln!(out, "{}", result.name)?;
        } else {
            header(app, out, result)?;
            let noun = if result.groups.len() == 1 {
                "commit"
            } else {
                "commits"
            };
            writeln!(
                out,
                "{} repos, {} {noun}",
                result.total_repos(),
                result.groups.len()
            )?;
            if !app.no_output {
                for (commit, members) in &result.groups {
                    let label = commit.as_deref().unwrap_or("no commit recorded");
                    let noun = if members.len() == 1 { "repo" } else { "repos" };
                    writeln!(out, "  {label} ({} {noun}):", members.len())?;
                    for u in members {
                        writeln!(out, "    {} ({})", u.repo, u.path)?;
                    }
                }
            }
        }
        if app.short_circuit {
            break;
        }
    }

    Ok(if any_failed { 1 } else { 0 })
}

/// `[name] url=<normalised url>`, unless `--no-header` suppressed it.
fn header<W: Write>(app: &AppConfig, out: &mut W, result: &SubmoduleResult) -> Result<()> {
    if !app.no_header {
        writeln!(out, "[{}] url={}", result.name, result.url)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls_normalise_to_one_identity() {
        let expected = "github.com/veltzer/shared-themes";
        for url in [
            "git@github.com:veltzer/shared-themes.git",
            "https://github.com/veltzer/shared-themes",
            "https://github.com/veltzer/shared-themes.git/",
            "ssh://git@github.com/veltzer/shared-themes.git",
            "git+ssh://git@github.com/Veltzer/Shared-Themes",
            "GitHub.com/veltzer/shared-themes",
        ] {
            assert_eq!(normalize_url(url), expected, "{url}");
        }
    }

    #[test]
    fn relative_urls_are_kept_and_named_by_their_last_segment() {
        assert_eq!(normalize_url("../shared-themes.git"), "../shared-themes");
        assert_eq!(name_of("../shared-themes"), "shared-themes");
        assert_eq!(name_of("github.com/veltzer/shared-tags"), "shared-tags");
    }

    #[test]
    fn a_colon_inside_the_path_is_not_scp_syntax() {
        assert_eq!(
            normalize_url("https://host/owner/repo:with:colons"),
            "host/owner/repo:with:colons"
        );
    }

    #[test]
    fn consistency_needs_one_group_with_a_known_commit() {
        let use_at = |repo: &str, commit: Option<&str>| SubmoduleUse {
            repo: Utf8PathBuf::from(repo),
            path: "shared/x".into(),
            commit: commit.map(str::to_string),
        };
        let one = SubmoduleResult {
            name: "x".into(),
            url: "h/x".into(),
            groups: vec![(Some("abc1234".into()), vec![use_at("a", Some("abc1234"))])],
        };
        assert!(one.is_consistent());
        let two = SubmoduleResult {
            name: "x".into(),
            url: "h/x".into(),
            groups: vec![
                (Some("abc1234".into()), vec![use_at("a", Some("abc1234"))]),
                (Some("def5678".into()), vec![use_at("b", Some("def5678"))]),
            ],
        };
        assert!(!two.is_consistent());
        assert_eq!(two.total_repos(), 2);
        let unpinned = SubmoduleResult {
            name: "x".into(),
            url: "h/x".into(),
            groups: vec![(None, vec![use_at("a", None)])],
        };
        assert!(!unpinned.is_consistent());
    }
}
