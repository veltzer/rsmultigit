use std::path::Path;

use anyhow::{Context, Result};

use crate::subprocess_utils::capture_output;

/// Repos the gh commands can operate on: those with a remote on github.com.
pub fn check_github(project: &Path) -> Result<bool> {
    let repo = crate::commands::count::open_repo(project)?;
    let remotes = repo
        .remotes()
        .with_context(|| format!("failed to list remotes for {}", project.display()))?;
    for name in remotes.iter().flatten() {
        if let Ok(remote) = repo.find_remote(name)
            && let Some(url) = remote.url()
            && url.contains("github.com")
        {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Clean up GitHub deployments, releases, and workflow runs for a repository,
/// keeping only the `keep` most recent non-failed of each and deleting the rest.
pub fn clean_all(project: &Path, keep: usize) -> Result<bool> {
    let repo = repo_name_with_owner(project)?;
    clean_deployments(project, &repo, keep)?;
    clean_releases(project, &repo, keep)?;
    clean_workflows(project, &repo, keep)?;
    Ok(true)
}

/// The repo's `owner/name` as GitHub knows it (resolved by gh from the remote).
fn repo_name_with_owner(project: &Path) -> Result<String> {
    capture_output(
        project,
        "gh",
        &[
            "repo",
            "view",
            "--json",
            "nameWithOwner",
            "--jq",
            ".nameWithOwner",
        ],
    )
}

/// Run `gh api <endpoint> --paginate --jq <jq>` and return the non-empty
/// output lines.
fn api_lines(project: &Path, endpoint: &str, jq: &str) -> Result<Vec<String>> {
    let out = capture_output(project, "gh", &["api", endpoint, "--paginate", "--jq", jq])?;
    Ok(out
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(String::from)
        .collect())
}

/// Given (id, failed) pairs ordered newest first, return the ids to delete:
/// every failed entry, plus every non-failed entry beyond the first `keep`.
fn select_deletions(items: &[(u64, bool)], keep: usize) -> Vec<u64> {
    let mut kept = 0;
    let mut to_delete = Vec::new();
    for &(id, failed) in items {
        if !failed && kept < keep {
            kept += 1;
        } else {
            to_delete.push(id);
        }
    }
    to_delete
}

fn clean_deployments(project: &Path, repo: &str, keep: usize) -> Result<()> {
    let ids: Vec<u64> = api_lines(project, &format!("repos/{repo}/deployments"), ".[].id")?
        .iter()
        .map(|l| l.parse().with_context(|| format!("bad deployment id {l:?}")))
        .collect::<Result<_>>()?;

    // A deployment counts as failed when its most recent status is
    // failure/error; those are always deleted, regardless of recency.
    let mut items = Vec::with_capacity(ids.len());
    for id in ids {
        let state = capture_output(
            project,
            "gh",
            &[
                "api",
                &format!("repos/{repo}/deployments/{id}/statuses"),
                "--jq",
                ".[0].state",
            ],
        )?;
        let failed = state == "failure" || state == "error";
        items.push((id, failed));
    }

    let to_delete = select_deletions(&items, keep);
    println!(
        "deployments: {} found, deleting {}",
        items.len(),
        to_delete.len()
    );
    for id in to_delete {
        // GitHub refuses to delete an active deployment, so mark it
        // inactive first.
        capture_output(
            project,
            "gh",
            &[
                "api",
                &format!("repos/{repo}/deployments/{id}/statuses"),
                "-X",
                "POST",
                "-f",
                "state=inactive",
            ],
        )?;
        capture_output(
            project,
            "gh",
            &["api", &format!("repos/{repo}/deployments/{id}"), "-X", "DELETE"],
        )?;
        println!("  deleted deployment {id}");
    }
    Ok(())
}

fn clean_releases(project: &Path, repo: &str, keep: usize) -> Result<()> {
    let ids: Vec<u64> = api_lines(project, &format!("repos/{repo}/releases"), ".[].id")?
        .iter()
        .map(|l| l.parse().with_context(|| format!("bad release id {l:?}")))
        .collect::<Result<_>>()?;

    let to_delete: Vec<u64> = ids.iter().skip(keep).copied().collect();
    println!(
        "releases: {} found, deleting {}",
        ids.len(),
        to_delete.len()
    );
    for id in to_delete {
        capture_output(
            project,
            "gh",
            &["api", &format!("repos/{repo}/releases/{id}"), "-X", "DELETE"],
        )?;
        println!("  deleted release {id}");
    }
    Ok(())
}

fn clean_workflows(project: &Path, repo: &str, keep: usize) -> Result<()> {
    let lines = api_lines(
        project,
        &format!("repos/{repo}/actions/runs"),
        r#".workflow_runs[] | "\(.id) \(.conclusion // "")""#,
    )?;
    let mut items = Vec::with_capacity(lines.len());
    for line in &lines {
        let (id, conclusion) = line.split_once(' ').unwrap_or((line.as_str(), ""));
        let id: u64 = id
            .parse()
            .with_context(|| format!("bad workflow run line {line:?}"))?;
        items.push((id, is_failed_conclusion(conclusion)));
    }

    let to_delete = select_deletions(&items, keep);
    println!(
        "workflow runs: {} found, deleting {}",
        items.len(),
        to_delete.len()
    );
    for id in to_delete {
        capture_output(
            project,
            "gh",
            &["api", &format!("repos/{repo}/actions/runs/{id}"), "-X", "DELETE"],
        )?;
        println!("  deleted workflow run {id}");
    }
    Ok(())
}

fn is_failed_conclusion(conclusion: &str) -> bool {
    matches!(
        conclusion,
        "failure" | "cancelled" | "timed_out" | "startup_failure" | "action_required"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn select_deletions_keeps_first_n_non_failed() {
        let items = [(1, false), (2, false), (3, false), (4, false)];
        assert_eq!(select_deletions(&items, 2), vec![3, 4]);
    }

    #[test]
    fn select_deletions_always_deletes_failed() {
        // 2 is failed: deleted even though it is among the most recent;
        // its keep slot goes to the next non-failed entry (3).
        let items = [(1, false), (2, true), (3, false), (4, false)];
        assert_eq!(select_deletions(&items, 2), vec![2, 4]);
    }

    #[test]
    fn select_deletions_nothing_to_delete() {
        let items = [(1, false), (2, false)];
        assert!(select_deletions(&items, 4).is_empty());
    }

    #[test]
    fn select_deletions_keep_zero_deletes_everything() {
        let items = [(1, false), (2, true)];
        assert_eq!(select_deletions(&items, 0), vec![1, 2]);
    }

    #[test]
    fn failed_conclusions() {
        for c in [
            "failure",
            "cancelled",
            "timed_out",
            "startup_failure",
            "action_required",
        ] {
            assert!(is_failed_conclusion(c), "{c} should count as failed");
        }
        for c in ["success", "skipped", "neutral", ""] {
            assert!(!is_failed_conclusion(c), "{c} should not count as failed");
        }
    }
}
