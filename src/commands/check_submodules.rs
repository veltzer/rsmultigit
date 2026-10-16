//! `check submodules`: each `[[check-submodules]]` rule names a submodule by its
//! remote URL and requires every selected repo that carries it to have it
//! checked out at the same commit.
//!
//! A submodule is the fleet's way of sharing a repo (design tokens, tag
//! vocabularies, term lists) and the pin is the part that drifts: one
//! consumer bumps it, the others do not, and the sites stop agreeing on
//! something that was meant to be identical. Nothing in the submodule
//! machinery itself notices. A rule here does, for the submodules it names
//! and nothing else: a submodule without a rule is deliberately free to
//! differ per repo.
//!
//! The commit compared is the one checked out in the submodule's working
//! directory, the same view `check same` takes of ordinary files: what is on
//! disk and what a build would use. A submodule that is not checked out falls
//! back to the commit recorded in the superproject's index, then in HEAD.

use std::collections::BTreeMap;
use std::io::Write;

use anyhow::{Context, Result};
use camino::Utf8PathBuf;

use crate::commands::check::{self, CheckConfig, SubmoduleRule};
use crate::commands::check_run::{fail_on_empty_rules, rule_header, select_rules};
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

/// Outcome of evaluating one `[[check-submodules]]` rule.
#[derive(Debug)]
pub struct SubmoduleResult {
    pub name: String,
    /// Uses grouped by commit, largest group first, each group sorted by repo.
    /// The `None` key collects uses that record no commit at all.
    pub groups: Vec<(Option<String>, Vec<SubmoduleUse>)>,
    /// Repos the rule selected that do not carry the submodule. Not a
    /// violation: the rule constrains the pin where the submodule is used,
    /// not which repos use it.
    pub skipped: Vec<Utf8PathBuf>,
}

impl SubmoduleResult {
    /// Every member pins the same, known commit.
    pub fn is_consistent(&self) -> bool {
        self.groups.len() <= 1 && self.groups.iter().all(|(commit, _)| commit.is_some())
    }

    /// Number of repos that carry the submodule.
    pub fn total_repos(&self) -> usize {
        self.groups.iter().map(|(_, uses)| uses.len()).sum()
    }

    /// True when no selected repo carries the submodule, which almost always
    /// means a stale `select` or a misspelled `url`. Mirrors the other rule
    /// types: a failure unless `--allow-empty` is passed.
    pub fn matched_nothing(&self) -> bool {
        self.total_repos() == 0
    }
}

/// Reduce a submodule URL to the identity of the repo it names, so a rule's
/// `url` matches the submodule however it was added: scheme and user
/// dropped, the scp-style colon turned into a slash, trailing slash and
/// `.git` removed, case folded. A relative URL (`../shared-themes`) is kept
/// as written; it resolves against each superproject's own remote, which
/// this check does not consult.
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
    // Case first, so `.GIT` is stripped like `.git`.
    let s = s.to_ascii_lowercase();
    let s = s.trim_end_matches('/');
    let s = s.strip_suffix(".git").unwrap_or(s);
    s.trim_end_matches('/').to_string()
}

fn short(oid: git2::Oid) -> String {
    oid.to_string()[..7].to_string()
}

/// Evaluate one rule: in every repo it selects, find the submodules whose
/// normalised URL is the rule's, and group them by the commit checked out.
pub fn evaluate_submodule_rule(
    rule: &SubmoduleRule,
    repos: &[Utf8PathBuf],
) -> Result<SubmoduleResult> {
    let candidates = check::select_repos(&rule.selection(), repos)?;
    let want = normalize_url(&rule.url);

    let mut by_commit: BTreeMap<Option<String>, Vec<SubmoduleUse>> = BTreeMap::new();
    let mut skipped: Vec<Utf8PathBuf> = Vec::new();
    for repo in candidates {
        let git =
            git2::Repository::open(&repo).with_context(|| format!("failed to open {repo}"))?;
        let submodules = git
            .submodules()
            .with_context(|| format!("failed to read submodules of {repo}"))?;
        let mut found = false;
        for sm in submodules {
            // A submodule with no URL, or one git2 cannot hand back as text,
            // cannot be the one the rule names.
            let Some(url) = sm.url().ok().flatten() else {
                continue;
            };
            if normalize_url(url) != want {
                continue;
            }
            found = true;
            let commit = sm
                .workdir_id()
                .or_else(|| sm.index_id())
                .or_else(|| sm.head_id())
                .map(short);
            by_commit
                .entry(commit.clone())
                .or_default()
                .push(SubmoduleUse {
                    repo: repo.clone(),
                    path: sm.path().to_string_lossy().to_string(),
                    commit,
                });
        }
        if !found {
            skipped.push(repo);
        }
    }

    let mut groups: Vec<(Option<String>, Vec<SubmoduleUse>)> = by_commit.into_iter().collect();
    for (_, members) in &mut groups {
        members.sort_by(|a, b| a.repo.cmp(&b.repo));
    }
    groups.sort_by_key(|(_, members)| std::cmp::Reverse(members.len()));

    Ok(SubmoduleResult {
        name: rule.name.clone(),
        groups,
        skipped,
    })
}

/// The check submodules flag set, bundled to keep the runner's signature small.
pub struct CheckSubmodulesOpts<'a> {
    pub requested: &'a [String],
    pub requested_re: &'a [String],
    pub only_failed: bool,
    pub allow_empty: bool,
}

/// Run the check submodules command. Returns the process exit code.
///
/// Rule selection is [`select_rules`]. Passing rules print
/// `ok (N repos @ <commit>)` unless `only_failed` or `--terse` is set;
/// `--terse` prints only the names of failing rules, one per line. A rule
/// that finds the submodule in no selected repo fails unless `allow_empty`
/// is set; see [`fail_on_empty_rules`] for the end-of-run error. With the
/// global `--short-circuit` flag, evaluation stops at the first failing rule.
pub fn run_check_submodules<W: Write>(
    app: &AppConfig,
    file_config: &CheckConfig,
    projects: &[Utf8PathBuf],
    opts: &CheckSubmodulesOpts<'_>,
    out: &mut W,
) -> Result<i32> {
    let CheckSubmodulesOpts {
        requested,
        requested_re,
        only_failed,
        allow_empty,
    } = *opts;

    let rules = select_rules(
        &file_config.submodule,
        requested,
        requested_re,
        "check-submodules rule",
    )?;
    if rules.is_empty() {
        if app.verbose {
            writeln!(out, "no submodule rules to check")?;
        }
        return Ok(0);
    }

    let mut any_failed = false;
    let mut empty_rules: Vec<String> = Vec::new();

    for rule in rules {
        let result = evaluate_submodule_rule(rule, projects)?;

        if result.matched_nothing() && !allow_empty {
            any_failed = true;
            empty_rules.push(result.name.clone());
            if app.terse {
                writeln!(out, "{}", result.name)?;
            } else {
                rule_header(app, out, rule)?;
                writeln!(out, "no repos have this submodule")?;
            }
            if app.short_circuit {
                break;
            }
            continue;
        }

        if result.is_consistent() {
            if !only_failed && !app.terse {
                rule_header(app, out, rule)?;
                let commit = result
                    .groups
                    .first()
                    .and_then(|(commit, _)| commit.as_deref())
                    .unwrap_or("-");
                write!(out, "ok ({} repos @ {commit}", result.total_repos())?;
                if !result.skipped.is_empty() {
                    write!(out, ", {} skipped", result.skipped.len())?;
                }
                writeln!(out, ")")?;
            }
            continue;
        }

        any_failed = true;
        if app.terse {
            writeln!(out, "{}", result.name)?;
        } else {
            rule_header(app, out, rule)?;
            let noun = if result.groups.len() == 1 {
                "commit"
            } else {
                "commits"
            };
            write!(
                out,
                "{} repos, {} {noun}",
                result.total_repos(),
                result.groups.len()
            )?;
            if !result.skipped.is_empty() {
                write!(out, " ({} skipped)", result.skipped.len())?;
            }
            writeln!(out)?;
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

    fail_on_empty_rules(
        "check submodules",
        "found the submodule in no repo",
        "stale select or url?",
        &empty_rules,
    )?;

    Ok(if any_failed { 1 } else { 0 })
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
            "github.com/veltzer/shared-themes.GIT",
        ] {
            assert_eq!(normalize_url(url), expected, "{url}");
        }
    }

    #[test]
    fn relative_urls_are_kept() {
        assert_eq!(normalize_url("../shared-themes.git"), "../shared-themes");
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
        let result = |groups| SubmoduleResult {
            name: "x".into(),
            groups,
            skipped: Vec::new(),
        };
        let one = result(vec![(
            Some("abc1234".into()),
            vec![use_at("a", Some("abc1234"))],
        )]);
        assert!(one.is_consistent());
        assert!(!one.matched_nothing());
        let two = result(vec![
            (Some("abc1234".into()), vec![use_at("a", Some("abc1234"))]),
            (Some("def5678".into()), vec![use_at("b", Some("def5678"))]),
        ]);
        assert!(!two.is_consistent());
        assert_eq!(two.total_repos(), 2);
        let unpinned = result(vec![(None, vec![use_at("a", None)])]);
        assert!(!unpinned.is_consistent());
        let none = result(Vec::new());
        assert!(none.matched_nothing());
    }
}
