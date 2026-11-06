use camino::Utf8Path;

use anyhow::{Context, Result};

use crate::subprocess_utils::{capture_output, capture_output_allow_failure, check_call, out_line};

/// Repos the gh commands can operate on: those with a remote on github.com.
pub fn check_github(project: &Utf8Path) -> Result<bool> {
    let repo = crate::commands::count::open_repo(project)?;
    let remotes = repo
        .remotes()
        .with_context(|| format!("failed to list remotes for {}", project))?;
    for name in remotes.iter() {
        // Err is a non-UTF-8 remote name, Ok(None) a null entry: neither can
        // be looked up, so skip both.
        let Ok(Some(name)) = name else {
            continue;
        };
        if let Ok(remote) = repo.find_remote(name)
            && let Ok(url) = remote.url()
            && url.contains("github.com")
        {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Clean up GitHub deployments, releases, and workflow runs for a repository,
/// keeping only the `keep` most recent non-failed of each and deleting the rest.
pub fn prune_ci_rest_obo(project: &Utf8Path, keep: usize) -> Result<()> {
    let repo = repo_name_with_owner(project)?;
    clean_deployments(project, &repo, keep)?;
    clean_releases(project, &repo, keep)?;
    clean_workflows(project, &repo, keep)?;
    Ok(())
}

/// The repo's `owner/name` as GitHub knows it (resolved by gh from the remote).
fn repo_name_with_owner(project: &Utf8Path) -> Result<String> {
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
pub(crate) fn api_lines(project: &Utf8Path, endpoint: &str, jq: &str) -> Result<Vec<String>> {
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
pub(crate) fn select_deletions(items: &[(u64, bool)], keep: usize) -> Vec<u64> {
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

fn clean_deployments(project: &Utf8Path, repo: &str, keep: usize) -> Result<()> {
    let ids: Vec<u64> = api_lines(project, &format!("repos/{repo}/deployments"), ".[].id")?
        .iter()
        .map(|l| {
            l.parse()
                .with_context(|| format!("bad deployment id {l:?}"))
        })
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
    out_line(&format!(
        "deployments: {} found, deleting {}",
        items.len(),
        to_delete.len()
    ));
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
            &[
                "api",
                &format!("repos/{repo}/deployments/{id}"),
                "-X",
                "DELETE",
            ],
        )?;
        out_line(&format!("  deleted deployment {id}"));
    }
    Ok(())
}

fn clean_releases(project: &Utf8Path, repo: &str, keep: usize) -> Result<()> {
    let ids: Vec<u64> = api_lines(project, &format!("repos/{repo}/releases"), ".[].id")?
        .iter()
        .map(|l| l.parse().with_context(|| format!("bad release id {l:?}")))
        .collect::<Result<_>>()?;

    let to_delete: Vec<u64> = ids.iter().skip(keep).copied().collect();
    out_line(&format!(
        "releases: {} found, deleting {}",
        ids.len(),
        to_delete.len()
    ));
    for id in to_delete {
        capture_output(
            project,
            "gh",
            &[
                "api",
                &format!("repos/{repo}/releases/{id}"),
                "-X",
                "DELETE",
            ],
        )?;
        out_line(&format!("  deleted release {id}"));
    }
    Ok(())
}

fn clean_workflows(project: &Utf8Path, repo: &str, keep: usize) -> Result<()> {
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
    out_line(&format!(
        "workflow runs: {} found, deleting {}",
        items.len(),
        to_delete.len()
    ));
    for id in to_delete {
        capture_output(
            project,
            "gh",
            &[
                "api",
                &format!("repos/{repo}/actions/runs/{id}"),
                "-X",
                "DELETE",
            ],
        )?;
        out_line(&format!("  deleted workflow run {id}"));
    }
    Ok(())
}

fn is_failed_conclusion(conclusion: &str) -> bool {
    matches!(
        conclusion,
        "failure" | "cancelled" | "timed_out" | "startup_failure" | "action_required"
    )
}

// ── prune-ci-gql-obo ──────────────────────────────────────────────────
//
// Same policy as `prune_ci_rest_obo`, but with fewer and overlapping round trips:
//
// - One GraphQL query (100 per page) lists the deployments together with
//   their latest status, and the releases. The REST version makes one call
//   per deployment just to read its status.
// - Deployments are deactivated and deleted by aliased mutations, many per
//   request. The mutations in one request run in order, so each deployment
//   is deactivated before it is deleted.
// - GraphQL has no mutation that deletes a release or a workflow run, and
//   no listing of a repo's workflow runs. Those stay on REST: the runs are
//   listed 100 per page instead of the default 30, and the deletes run on
//   a few threads at once.
//
// The repository is named through gh's `{owner}`/`{repo}` placeholders, so
// there is no separate `gh repo view` call either.

/// Items per page of a GraphQL connection (GitHub's maximum).
pub(crate) const GRAPHQL_PAGE_SIZE: usize = 100;
/// Deployments per mutation request; each one takes two mutations.
pub(crate) const DEPLOYMENTS_PER_MUTATION: usize = 50;
/// How many REST deletes run at once. Small, so the batch stays clear of
/// GitHub's secondary rate limit, which also counts concurrent requests.
pub(crate) const REST_DELETE_THREADS: usize = 8;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GqlDeployment {
    /// The GraphQL node id that the mutations take.
    pub(crate) node_id: String,
    /// The REST id, used in the output so it matches `prune-ci-rest-obo`.
    pub(crate) database_id: u64,
    /// Deployment state `INACTIVE`: deletable without deactivating first.
    pub(crate) inactive: bool,
    /// Latest status `FAILURE` or `ERROR`: always deleted.
    pub(crate) failed: bool,
}

/// Where a GraphQL connection's pagination stands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Page {
    First,
    After(String),
    Done,
}

impl Page {
    pub(crate) fn wanted(&self) -> bool {
        *self != Page::Done
    }

    pub(crate) fn cursor(&self) -> Option<&str> {
        match self {
            Page::After(c) => Some(c),
            _ => None,
        }
    }
}

/// One page of the inventory query, as printed by `INVENTORY_JQ`.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct InventoryPage {
    pub(crate) deployments: Vec<GqlDeployment>,
    pub(crate) releases: Vec<u64>,
    /// Next page for each connection that was queried; None when it was not.
    pub(crate) deployments_next: Option<Page>,
    pub(crate) releases_next: Option<Page>,
}

/// Flattens the query result into one record per line, so the output is
/// parsed without a JSON dependency. A connection that was not queried is
/// absent and prints nothing.
const INVENTORY_JQ: &str = r#".data.repository
  | (.deployments // empty
      | (.nodes[] | "D \(.id) \(.databaseId) \(.state) \(.latestStatus.state // "-")"),
        "DP \(.pageInfo.hasNextPage) \(.pageInfo.endCursor // "-")"),
    (.releases // empty
      | (.nodes[] | "R \(.databaseId)"),
        "RP \(.pageInfo.hasNextPage) \(.pageInfo.endCursor // "-")")"#;

/// The inventory query for the connections that still have pages to fetch.
/// Only the cursor variables actually used are declared, since GraphQL
/// rejects an unused variable.
fn inventory_query(deployments: &Page, releases: &Page) -> String {
    let mut vars = vec!["$owner:String!", "$name:String!"];
    let mut body = String::new();
    if deployments.wanted() {
        vars.push("$dafter:String");
        body.push_str(&format!(
            "deployments(first:{GRAPHQL_PAGE_SIZE},after:$dafter,orderBy:{{field:CREATED_AT,direction:DESC}})\
             {{pageInfo{{hasNextPage endCursor}} nodes{{id databaseId state latestStatus{{state}}}}}} "
        ));
    }
    if releases.wanted() {
        vars.push("$rafter:String");
        body.push_str(&format!(
            "releases(first:{GRAPHQL_PAGE_SIZE},after:$rafter,orderBy:{{field:CREATED_AT,direction:DESC}})\
             {{pageInfo{{hasNextPage endCursor}} nodes{{databaseId}}}}"
        ));
    }
    format!(
        "query({}){{repository(owner:$owner,name:$name){{{}}}}}",
        vars.join(","),
        body.trim_end()
    )
}

fn parse_page_info(line: &str, rest: &str) -> Result<Page> {
    let (has_next, cursor) = rest
        .split_once(' ')
        .with_context(|| format!("bad page info {line:?}"))?;
    match (has_next, cursor) {
        ("true", c) if c != "-" => Ok(Page::After(c.to_string())),
        ("true", _) => anyhow::bail!("next page without a cursor: {line:?}"),
        ("false", _) => Ok(Page::Done),
        _ => anyhow::bail!("bad page info {line:?}"),
    }
}

pub(crate) fn parse_inventory_page(out: &str) -> Result<InventoryPage> {
    let mut page = InventoryPage::default();
    for line in out.lines().map(str::trim).filter(|l| !l.is_empty()) {
        let (kind, rest) = line.split_once(' ').unwrap_or((line, ""));
        match kind {
            "D" => {
                let fields: Vec<&str> = rest.split(' ').collect();
                let [node_id, database_id, state, latest] = fields[..] else {
                    anyhow::bail!("bad deployment line {line:?}");
                };
                page.deployments.push(GqlDeployment {
                    node_id: node_id.to_string(),
                    database_id: database_id
                        .parse()
                        .with_context(|| format!("bad deployment id in {line:?}"))?,
                    inactive: state == "INACTIVE",
                    failed: latest == "FAILURE" || latest == "ERROR",
                });
            }
            "R" => page.releases.push(
                rest.parse()
                    .with_context(|| format!("bad release id in {line:?}"))?,
            ),
            "DP" => page.deployments_next = Some(parse_page_info(line, rest)?),
            "RP" => page.releases_next = Some(parse_page_info(line, rest)?),
            _ => anyhow::bail!("unexpected inventory line {line:?}"),
        }
    }
    Ok(page)
}

/// All deployments and releases of the repo, newest first, paging each
/// connection until it is exhausted.
fn fetch_inventory(project: &Utf8Path) -> Result<(Vec<GqlDeployment>, Vec<u64>)> {
    let mut deployments = Vec::new();
    let mut releases = Vec::new();
    let mut dpage = Page::First;
    let mut rpage = Page::First;
    while dpage.wanted() || rpage.wanted() {
        let query = inventory_query(&dpage, &rpage);
        let query_arg = format!("query={query}");
        let mut args = vec![
            "api".to_string(),
            "graphql".to_string(),
            "-F".to_string(),
            "owner={owner}".to_string(),
            "-F".to_string(),
            "name={repo}".to_string(),
            "-f".to_string(),
            query_arg,
        ];
        if let Some(c) = dpage.cursor() {
            args.push("-f".to_string());
            args.push(format!("dafter={c}"));
        }
        if let Some(c) = rpage.cursor() {
            args.push("-f".to_string());
            args.push(format!("rafter={c}"));
        }
        args.push("--jq".to_string());
        args.push(INVENTORY_JQ.to_string());
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        let page = parse_inventory_page(&capture_output(project, "gh", &args)?)?;
        // A connection that was asked for must report where it stands;
        // silence means the repository did not resolve.
        if dpage.wanted() {
            dpage = page
                .deployments_next
                .context("GraphQL query returned no deployments connection")?;
            deployments.extend(page.deployments);
        }
        if rpage.wanted() {
            rpage = page
                .releases_next
                .context("GraphQL query returned no releases connection")?;
            releases.extend(page.releases);
        }
    }
    Ok((deployments, releases))
}

/// The workflow runs, newest first, as (id, failed) pairs: REST, since
/// GraphQL cannot list a repo's runs, but 100 per page.
fn fetch_workflow_runs(project: &Utf8Path) -> Result<Vec<(u64, bool)>> {
    fetch_workflow_runs_of(project, "{owner}/{repo}")
}

/// [`fetch_workflow_runs`] for an explicit `owner/name` (or gh's
/// `{owner}/{repo}` placeholders, resolved from `project`).
pub(crate) fn fetch_workflow_runs_of(project: &Utf8Path, repo: &str) -> Result<Vec<(u64, bool)>> {
    let lines = api_lines(
        project,
        &format!("repos/{repo}/actions/runs?per_page=100"),
        r#".workflow_runs[] | "\(.id) \(.conclusion // "")""#,
    )?;
    lines
        .iter()
        .map(|line| {
            let (id, conclusion) = line.split_once(' ').unwrap_or((line.as_str(), ""));
            let id: u64 = id
                .parse()
                .with_context(|| format!("bad workflow run line {line:?}"))?;
            Ok((id, is_failed_conclusion(conclusion)))
        })
        .collect()
}

/// A GraphQL node id is interpolated into the mutation text, so make sure
/// it cannot break out of its string literal.
pub(crate) fn is_safe_node_id(id: &str) -> bool {
    !id.is_empty()
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '=' | '+' | '/'))
}

/// One mutation request that deactivates (where needed) and deletes each of
/// `batch`, in order.
pub(crate) fn deployment_mutation(batch: &[&GqlDeployment]) -> Result<String> {
    let mut m = String::from("mutation{");
    for (i, d) in batch.iter().enumerate() {
        if !is_safe_node_id(&d.node_id) {
            anyhow::bail!("unexpected deployment node id {:?}", d.node_id);
        }
        // GitHub refuses to delete an active deployment.
        if !d.inactive {
            m.push_str(&format!(
                "s{i}:createDeploymentStatus(input:{{deploymentId:\"{}\",state:INACTIVE}}){{clientMutationId}} ",
                d.node_id
            ));
        }
        m.push_str(&format!(
            "d{i}:deleteDeployment(input:{{id:\"{}\"}}){{clientMutationId}} ",
            d.node_id
        ));
    }
    m.push('}');
    Ok(m)
}

/// The `errors` of a GraphQL response, as (alias, message) pairs; the
/// alias is the first element of the error's `path`, when it has one. None
/// when `json` carries no `errors` array, which means the request failed
/// before GraphQL ran (network, authentication) and nothing in it took
/// effect.
///
/// `gh api graphql` exits non-zero on any error and then prints the raw
/// response instead of applying `--jq`, so this is how the aliases that
/// did succeed are told apart from those that did not. A small scanner
/// rather than a JSON dependency: it only splits the array into its
/// top-level objects, minding strings, and reads two fields from each.
pub(crate) fn graphql_errors(json: &str) -> Option<Vec<(Option<String>, String)>> {
    let start = json.find("\"errors\":[")? + "\"errors\":[".len();
    let path_re = regex_lite::Regex::new(r#""path":\["([^"\\]*)""#).expect("valid regex");
    let msg_re = regex_lite::Regex::new(r#""message":"((?:[^"\\]|\\.)*)""#).expect("valid regex");
    let mut errors = Vec::new();
    let (mut depth, mut in_str, mut escaped) = (0usize, false, false);
    let mut obj_start = None;
    for (i, c) in json[start..].char_indices() {
        if in_str {
            match c {
                _ if escaped => escaped = false,
                '\\' => escaped = true,
                '"' => in_str = false,
                _ => {}
            }
            continue;
        }
        match c {
            '"' => in_str = true,
            '{' | '[' => {
                if depth == 0 {
                    obj_start = Some(start + i);
                }
                depth += 1;
            }
            '}' | ']' if depth == 0 => break,
            '}' | ']' => {
                depth -= 1;
                if depth == 0
                    && let Some(s) = obj_start.take()
                {
                    let obj = &json[s..=start + i];
                    let alias = path_re.captures(obj).map(|c| c[1].to_string());
                    let message = msg_re
                        .captures(obj)
                        .map(|c| c[1].replace("\\\"", "\"").replace("\\\\", "\\"))
                        .unwrap_or_else(|| obj.to_string());
                    errors.push((alias, message));
                }
            }
            _ => {}
        }
    }
    Some(errors)
}

/// Run one deployment mutation request and say, per deployment, whether
/// it is gone. A failing request can still have deleted some of them: only
/// those whose `d<i>` alias errored, or every one when the request never
/// reached GraphQL, failed.
pub(crate) fn run_deployment_batch(cwd: &Utf8Path, batch: &[&GqlDeployment]) -> Vec<Result<()>> {
    let all_failed = |msg: String| {
        batch
            .iter()
            .map(|_| Err(anyhow::anyhow!("{msg}")))
            .collect()
    };
    let mutation = match deployment_mutation(batch) {
        Ok(m) => m,
        Err(e) => return all_failed(format!("{e:#}")),
    };
    let (code, stdout, stderr) = match capture_output_allow_failure(
        cwd,
        "gh",
        &["api", "graphql", "-f", &format!("query={mutation}")],
    ) {
        Ok(r) => r,
        Err(e) => return all_failed(format!("{e:#}")),
    };
    if code == 0 {
        return batch.iter().map(|_| Ok(())).collect();
    }
    let Some(errors) = graphql_errors(&stdout) else {
        return all_failed(format!("gh api graphql failed: {}", stderr.trim()));
    };
    batch
        .iter()
        .enumerate()
        .map(|(i, _)| {
            let alias = format!("d{i}");
            match errors
                .iter()
                .find(|(a, _)| a.as_deref() == Some(alias.as_str()))
            {
                Some((_, msg)) => Err(anyhow::anyhow!("{msg}")),
                None => Ok(()),
            }
        })
        .collect()
}

/// Delete the deployments, `DEPLOYMENTS_PER_MUTATION` per request; one
/// result per deployment, in order.
fn delete_deployments(project: &Utf8Path, to_delete: &[&GqlDeployment]) -> Vec<Result<()>> {
    to_delete
        .chunks(DEPLOYMENTS_PER_MUTATION)
        .flat_map(|batch| run_deployment_batch(project, batch))
        .collect()
}

/// Apply `f` to every item on up to `threads` threads; results come back in
/// input order. `f` must not print: output capture is per thread, so the
/// caller prints.
pub(crate) fn parallel_map<T, R, F>(items: &[T], threads: usize, f: F) -> Vec<R>
where
    T: Sync,
    R: Send,
    F: Fn(&T) -> R + Sync,
{
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};

    let next = AtomicUsize::new(0);
    let results: Mutex<Vec<Option<R>>> = Mutex::new((0..items.len()).map(|_| None).collect());
    std::thread::scope(|scope| {
        for _ in 0..threads.min(items.len()) {
            scope.spawn(|| {
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    let Some(item) = items.get(i) else {
                        break;
                    };
                    let r = f(item);
                    results.lock().expect("results lock poisoned")[i] = Some(r);
                }
            });
        }
    });
    results
        .into_inner()
        .expect("results lock poisoned")
        .into_iter()
        .map(|r| r.expect("every item is processed"))
        .collect()
}

/// Run `gh api -X DELETE <path>` for every path, `REST_DELETE_THREADS` at a
/// time, from `cwd`. Results come back in input order.
pub(crate) fn rest_delete_parallel(cwd: &Utf8Path, paths: &[String]) -> Vec<Result<()>> {
    parallel_map(paths, REST_DELETE_THREADS, |path| {
        capture_output(cwd, "gh", &["api", path, "-X", "DELETE"]).map(|_| ())
    })
}

/// `prune_ci_rest_obo` through GitHub's GraphQL API where it can do the job, with
/// the remaining REST calls run in parallel. Same keep policy, same output.
/// Unlike `prune_ci_rest_obo`, a failed delete does not stop the others; every
/// failure is reported at the end.
pub fn prune_ci_gql_obo(project: &Utf8Path, keep: usize) -> Result<()> {
    // The two listings are independent: run them at the same time.
    let (inventory, runs) = std::thread::scope(|scope| {
        let runs = scope.spawn(|| fetch_workflow_runs(project));
        let inventory = fetch_inventory(project);
        (
            inventory,
            runs.join().expect("workflow run listing panicked"),
        )
    });
    let (deployments, releases) = inventory?;
    let runs = runs?;

    let deployment_items: Vec<(u64, bool)> = deployments
        .iter()
        .map(|d| (d.database_id, d.failed))
        .collect();
    let deployment_ids = select_deletions(&deployment_items, keep);
    let deployments_to_delete: Vec<&GqlDeployment> = deployments
        .iter()
        .filter(|d| deployment_ids.contains(&d.database_id))
        .collect();
    let releases_to_delete: Vec<u64> = releases.iter().skip(keep).copied().collect();
    let runs_to_delete = select_deletions(&runs, keep);

    // Releases and runs share one REST pool; the deployment mutations run
    // alongside it.
    let rest_paths: Vec<String> = releases_to_delete
        .iter()
        .map(|id| format!("repos/{{owner}}/{{repo}}/releases/{id}"))
        .chain(
            runs_to_delete
                .iter()
                .map(|id| format!("repos/{{owner}}/{{repo}}/actions/runs/{id}")),
        )
        .collect();
    let (deployment_results, rest_results) = std::thread::scope(|scope| {
        let d = scope.spawn(|| delete_deployments(project, &deployments_to_delete));
        let rest = rest_delete_parallel(project, &rest_paths);
        (d.join().expect("deployment deletion panicked"), rest)
    });

    let deployment_ids: Vec<u64> = deployments_to_delete
        .iter()
        .map(|d| d.database_id)
        .collect();
    let (release_results, run_results) = rest_results.split_at(releases_to_delete.len());
    let mut lines = Vec::new();
    let mut errors = Vec::new();
    for (label, found, ids, results) in [
        (
            "deployments",
            deployments.len(),
            &deployment_ids,
            &deployment_results[..],
        ),
        (
            "releases",
            releases.len(),
            &releases_to_delete,
            release_results,
        ),
        ("workflow runs", runs.len(), &runs_to_delete, run_results),
    ] {
        report_section(label, found, ids, results, &mut lines, &mut errors);
    }
    for line in &lines {
        out_line(line);
    }
    combine_errors(errors)
}

/// The output for one kind of item, as `prune-ci-rest-obo` prints it: a summary
/// line, then one line per deleted item. Failures go to `errors`.
pub(crate) fn report_section(
    label: &str,
    found: usize,
    ids: &[u64],
    results: &[Result<()>],
    lines: &mut Vec<String>,
    errors: &mut Vec<String>,
) {
    lines.push(format!("{label}: {found} found, deleting {}", ids.len()));
    let singular = label.trim_end_matches('s');
    for (id, result) in ids.iter().zip(results) {
        match result {
            Ok(()) => lines.push(format!("  deleted {singular} {id}")),
            Err(e) => errors.push(format!("deleting {singular} {id}: {e:#}")),
        }
    }
}

/// One error for all the failures of a repo, or Ok when there were none.
pub(crate) fn combine_errors(errors: Vec<String>) -> Result<()> {
    match errors.len() {
        0 => Ok(()),
        1 => Err(anyhow::anyhow!("{}", errors[0])),
        n => anyhow::bail!("{n} deletions failed:\n{}", errors.join("\n")),
    }
}

// ── artifacts ──────────────────────────────────────────────────────────

/// The assets of the latest release as an aligned NAME/SIZE/DOWNLOADS table,
/// or None when the repo has no release (or no github.com remote).
pub fn artifacts(project: &Utf8Path) -> Result<Option<String>> {
    if !check_github(project)? {
        return Ok(None);
    }
    let (code, stdout, stderr) = capture_output_allow_failure(
        project,
        "gh",
        &[
            "release",
            "view",
            "--json",
            "assets",
            "--jq",
            r#".assets[] | "\(.name)\t\(.size)\t\(.downloadCount)""#,
        ],
    )?;
    if code != 0 {
        // A repo that has never had a release is the common case, not an
        // error; anything else (not authenticated, network) is.
        if stderr.contains("release not found") {
            return Ok(None);
        }
        anyhow::bail!("gh release view failed: {}", stderr.trim());
    }
    let rows: Vec<[&str; 3]> = stdout
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| {
            let mut it = l.split('\t');
            let name = it.next().unwrap_or("");
            let size = it.next().unwrap_or("");
            let downloads = it.next().unwrap_or("");
            [name, size, downloads]
        })
        .collect();
    if rows.is_empty() {
        // A release with no assets: nothing to list.
        return Ok(None);
    }
    Ok(Some(format_table(&["NAME", "SIZE", "DOWNLOADS"], &rows)))
}

/// Render rows under a header with every column padded to its widest cell,
/// what `column -t -N ...` does for the shell script.
fn format_table(header: &[&str; 3], rows: &[[&str; 3]]) -> String {
    let mut widths = header.map(str::len);
    for row in rows {
        for (w, cell) in widths.iter_mut().zip(row) {
            *w = (*w).max(cell.len());
        }
    }
    let render = |row: &[&str; 3]| -> String {
        let cells: Vec<String> = row
            .iter()
            .zip(widths)
            .map(|(cell, w)| format!("{cell:<w$}"))
            .collect();
        cells.join("  ").trim_end().to_string()
    };
    let mut lines = vec![render(header)];
    lines.extend(rows.iter().map(render));
    lines.join("\n")
}

// ── last-workflow-state ────────────────────────────────────────────────

/// The conclusion of the most recent workflow run (`success`, `failure`,
/// ...), or its status (`in_progress`, `queued`) while it has none yet.
/// None when the repo has no runs or no github.com remote.
pub fn last_workflow_state(project: &Utf8Path) -> Result<Option<String>> {
    if !check_github(project)? {
        return Ok(None);
    }
    let out = capture_output(
        project,
        "gh",
        &[
            "run",
            "list",
            "--limit",
            "1",
            "--json",
            "status,conclusion",
            "--jq",
            r#".[0] | select(. != null) | "\(.status)\t\(.conclusion)""#,
        ],
    )?;
    Ok(workflow_state_from_line(&out))
}

/// `status<TAB>conclusion` as printed by the jq above; the conclusion wins
/// when the run has one, the status stands in until then.
fn workflow_state_from_line(line: &str) -> Option<String> {
    let line = line.trim();
    if line.is_empty() {
        return None;
    }
    let (status, conclusion) = line.split_once('\t').unwrap_or((line, ""));
    let conclusion = conclusion.trim();
    let state = if conclusion.is_empty() || conclusion == "null" {
        status.trim()
    } else {
        conclusion
    };
    (!state.is_empty()).then(|| state.to_string())
}

// ── open-site ──────────────────────────────────────────────────────────

/// Open the repo's GitHub Pages site in the browser via `xdg-open`. A repo
/// without a Pages site says so and is otherwise left alone.
pub fn open_site(project: &Utf8Path) -> Result<()> {
    let (code, stdout, stderr) = capture_output_allow_failure(
        project,
        "gh",
        &["api", "repos/{owner}/{repo}/pages", "--jq", ".html_url"],
    )?;
    if code != 0 {
        if stderr.contains("HTTP 404") {
            out_line("no GitHub Pages site");
            return Ok(());
        }
        anyhow::bail!("gh api pages failed: {}", stderr.trim());
    }
    let url = stdout.trim();
    if url.is_empty() {
        out_line("no GitHub Pages site");
        return Ok(());
    }
    out_line(url);
    check_call(project, "xdg-open", &[url])
}

// ── sync-metadata ──────────────────────────────────────────────────────

/// Fixed fleet feature policy, not read from the repo.
const POLICY_WIKI: bool = false;
const POLICY_ISSUES: bool = true;
const POLICY_PROJECTS: bool = false;

/// What config/project.lua says about the repo. Description and keywords
/// are optional: "unset locally" means "leave GitHub alone", not "clear it".
#[derive(Debug, PartialEq, Eq)]
pub struct LocalMeta {
    pub name: String,
    pub description: Option<String>,
    pub keywords: Option<Vec<String>>,
}

/// What GitHub currently has.
#[derive(Debug, PartialEq, Eq)]
pub struct GithubMeta {
    pub name_with_owner: String,
    pub description: String,
    /// Sorted.
    pub topics: Vec<String>,
    pub wiki: bool,
    pub issues: bool,
    pub projects: bool,
}

/// The differences between local and GitHub, as a report and as the
/// `gh repo edit` arguments that would resolve them.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct SyncPlan {
    pub report: Vec<String>,
    pub edit_args: Vec<String>,
}

/// Sync the repo's GitHub description, topics and feature policy from
/// config/project.lua. Returns the report of what differed (and, unless
/// `dry_run`, was changed), or None when GitHub already matches. A repo
/// without a project.lua, or without a github.com remote, is skipped.
pub fn sync_metadata(project: &Utf8Path, dry_run: bool) -> Result<Option<String>> {
    if !check_github(project)? {
        return Ok(None);
    }
    let lua_path = project.join("config/project.lua");
    if !lua_path.is_file() {
        return Ok(None);
    }
    let text =
        std::fs::read_to_string(&lua_path).with_context(|| format!("failed to read {lua_path}"))?;
    let local = parse_project_lua(&text).with_context(|| format!("failed to parse {lua_path}"))?;
    let remote = github_meta(project)?;
    let plan = plan_sync(&local, &remote);
    if plan.report.is_empty() {
        return Ok(None);
    }
    if !dry_run && !plan.edit_args.is_empty() {
        let mut args = vec!["repo", "edit", remote.name_with_owner.as_str()];
        args.extend(plan.edit_args.iter().map(String::as_str));
        capture_output(project, "gh", &args)?;
    }
    Ok(Some(plan.report.join("\n")))
}

/// One `gh repo view` for everything the sync compares, laid out line by
/// line so no JSON parsing is needed: name, features, topics, description.
fn github_meta(project: &Utf8Path) -> Result<GithubMeta> {
    let out = capture_output(
        project,
        "gh",
        &[
            "repo",
            "view",
            "--json",
            "nameWithOwner,description,repositoryTopics,hasWikiEnabled,hasIssuesEnabled,hasProjectsEnabled",
            "--jq",
            r#""\(.nameWithOwner)\n\(.hasWikiEnabled) \(.hasIssuesEnabled) \(.hasProjectsEnabled)\n\([.repositoryTopics[].name] | sort | join(" "))\n\(.description // "")""#,
        ],
    )?;
    parse_github_meta(&out)
}

fn parse_github_meta(out: &str) -> Result<GithubMeta> {
    let mut lines = out.lines();
    let name_with_owner = lines
        .next()
        .filter(|l| !l.is_empty())
        .context("gh repo view printed no nameWithOwner")?
        .to_string();
    let features = lines
        .next()
        .context("gh repo view printed no feature flags")?;
    let mut flags = features.split_whitespace().map(|f| match f {
        "true" => Ok(true),
        "false" => Ok(false),
        other => Err(anyhow::anyhow!(
            "bad feature flag {other:?} in {features:?}"
        )),
    });
    let mut next_flag = || flags.next().context("missing feature flag")?;
    let wiki = next_flag()?;
    let issues = next_flag()?;
    let projects = next_flag()?;
    let topics: Vec<String> = lines
        .next()
        .unwrap_or("")
        .split_whitespace()
        .map(String::from)
        .collect();
    // The description is whatever is left; capture_output already trimmed
    // the trailing newline.
    let description = lines.collect::<Vec<_>>().join("\n").trim().to_string();
    Ok(GithubMeta {
        name_with_owner,
        description,
        topics,
        wiki,
        issues,
        projects,
    })
}

/// Compare local against GitHub. Only fields the local file declares are
/// compared; the feature policy is fleet-wide and always enforced.
pub fn plan_sync(local: &LocalMeta, remote: &GithubMeta) -> SyncPlan {
    let mut plan = SyncPlan::default();

    // The repo is addressed by its remote, so a NAME that disagrees with it
    // is reported, never acted on.
    let github_name = remote
        .name_with_owner
        .rsplit('/')
        .next()
        .unwrap_or(&remote.name_with_owner);
    if local.name != github_name {
        plan.report.push("name".to_string());
        plan.report.push(format!("  local:  {}", local.name));
        plan.report.push(format!("  github: {github_name}"));
    }

    if let Some(want) = &local.description
        && !want.is_empty()
        && *want != remote.description
    {
        plan.report.push("description".to_string());
        plan.report.push(format!("  local:  {want}"));
        plan.report
            .push(format!("  github: {}", remote.description));
        plan.edit_args.push("--description".to_string());
        plan.edit_args.push(want.clone());
    }

    if let Some(keywords) = &local.keywords
        && !keywords.is_empty()
    {
        let mut want: Vec<String> = keywords.clone();
        want.sort();
        want.dedup();
        if want != remote.topics {
            plan.report.push("topics".to_string());
            plan.report.push(format!("  local:  {}", want.join(" ")));
            plan.report
                .push(format!("  github: {}", remote.topics.join(" ")));
            // Only the topics on one side move. A topic on both sides must
            // not be named at all: gh applies --remove-topic and --add-topic
            // in one call and the remove wins, which would drop it.
            for t in &remote.topics {
                if !want.contains(t) {
                    plan.edit_args.push("--remove-topic".to_string());
                    plan.edit_args.push(t.clone());
                }
            }
            for t in &want {
                if !remote.topics.contains(t) {
                    plan.edit_args.push("--add-topic".to_string());
                    plan.edit_args.push(t.clone());
                }
            }
        }
    }

    let features = [
        ("wiki", remote.wiki, POLICY_WIKI),
        ("issues", remote.issues, POLICY_ISSUES),
        ("projects", remote.projects, POLICY_PROJECTS),
    ];
    if features.iter().any(|(_, have, want)| have != want) {
        plan.report.push("features".to_string());
        plan.report.push(format!(
            "  want:   wiki={POLICY_WIKI} issues={POLICY_ISSUES} projects={POLICY_PROJECTS}"
        ));
        plan.report.push(format!(
            "  github: wiki={} issues={} projects={}",
            remote.wiki, remote.issues, remote.projects
        ));
        for (name, have, want) in features {
            if have != want {
                plan.edit_args.push(format!("--enable-{name}={want}"));
            }
        }
    }

    plan
}

// ── project.lua ────────────────────────────────────────────────────────
//
// The fleet's config/project.lua files are plain data: `NAME = "..."`
// assignments (quoted or `[[long]]` strings), booleans (`PYPI = true`), a
// `KEYWORDS = { "...", ... }` list, `--` comments and the odd `X = Y`
// alias. That subset is parsed here rather than by shelling out to a lua
// interpreter, so the binary stays self-contained. Anything outside it is
// an error naming the line, never a guess.

#[derive(Debug, Clone, PartialEq, Eq)]
enum Token {
    Ident(String),
    Str(String),
    Eq,
    LBrace,
    RBrace,
    Comma,
}

fn tokenize_lua(text: &str) -> Result<Vec<(usize, Token)>> {
    let mut tokens = Vec::new();
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    let mut line = 1;
    while i < chars.len() {
        let c = chars[i];
        match c {
            '\n' => {
                line += 1;
                i += 1;
            }
            c if c.is_whitespace() => i += 1,
            '-' if chars.get(i + 1) == Some(&'-') => {
                // Comment to end of line; the newline itself is counted
                // by the next iteration.
                while i < chars.len() && chars[i] != '\n' {
                    i += 1;
                }
            }
            '=' => {
                tokens.push((line, Token::Eq));
                i += 1;
            }
            '{' => {
                tokens.push((line, Token::LBrace));
                i += 1;
            }
            '}' => {
                tokens.push((line, Token::RBrace));
                i += 1;
            }
            ',' => {
                tokens.push((line, Token::Comma));
                i += 1;
            }
            '[' if chars.get(i + 1) == Some(&'[') => {
                // Long string: `[[` ... `]]`, verbatim, no escapes. Lua
                // drops a newline that immediately follows the opening.
                let start_line = line;
                i += 2;
                if chars.get(i) == Some(&'\n') {
                    i += 1;
                    line += 1;
                }
                let mut s = String::new();
                loop {
                    let Some(&ch) = chars.get(i) else {
                        anyhow::bail!("line {start_line}: unterminated long string");
                    };
                    if ch == ']' && chars.get(i + 1) == Some(&']') {
                        i += 2;
                        break;
                    }
                    if ch == '\n' {
                        line += 1;
                    }
                    s.push(ch);
                    i += 1;
                }
                tokens.push((start_line, Token::Str(s)));
            }
            '"' | '\'' => {
                let quote = c;
                let start_line = line;
                let mut s = String::new();
                i += 1;
                loop {
                    let Some(&ch) = chars.get(i) else {
                        anyhow::bail!("line {start_line}: unterminated string");
                    };
                    i += 1;
                    match ch {
                        ch if ch == quote => break,
                        '\\' => {
                            let Some(&esc) = chars.get(i) else {
                                anyhow::bail!("line {start_line}: unterminated string");
                            };
                            i += 1;
                            s.push(match esc {
                                'n' => '\n',
                                't' => '\t',
                                '\\' => '\\',
                                '"' => '"',
                                '\'' => '\'',
                                other => anyhow::bail!(
                                    "line {line}: unsupported escape \\{other} in string"
                                ),
                            });
                        }
                        '\n' => anyhow::bail!("line {start_line}: newline in string"),
                        ch => s.push(ch),
                    }
                }
                tokens.push((start_line, Token::Str(s)));
            }
            c if c.is_ascii_alphabetic() || c == '_' => {
                let start = i;
                while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') {
                    i += 1;
                }
                let ident: String = chars[start..i].iter().collect();
                tokens.push((line, Token::Ident(ident)));
            }
            other => anyhow::bail!("line {line}: unsupported character {other:?}"),
        }
    }
    Ok(tokens)
}

/// A value on the right-hand side of a project.lua assignment.
#[derive(Debug, Clone, PartialEq, Eq)]
enum LuaValue {
    Str(String),
    List(Vec<String>),
    /// `true`, `false` or `nil`: carried so an alias to it resolves, never
    /// used by the sync itself.
    Keyword(String),
}

/// Parse the assignment subset of lua used by config/project.lua into the
/// fields sync-metadata cares about.
pub fn parse_project_lua(text: &str) -> Result<LocalMeta> {
    let tokens = tokenize_lua(text)?;
    let mut vars: Vec<(String, LuaValue)> = Vec::new();
    let lookup = |vars: &[(String, LuaValue)], name: &str| -> Option<LuaValue> {
        vars.iter()
            .rev()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.clone())
    };
    let mut i = 0;
    while i < tokens.len() {
        let (line, ref tok) = tokens[i];
        let Token::Ident(name) = tok else {
            anyhow::bail!("line {line}: expected an assignment, found {tok:?}");
        };
        let Some((_, Token::Eq)) = tokens.get(i + 1) else {
            anyhow::bail!("line {line}: expected `=` after {name}");
        };
        i += 2;
        let value = match tokens.get(i) {
            Some((_, Token::Str(s))) => {
                i += 1;
                LuaValue::Str(s.clone())
            }
            Some((_, Token::Ident(kw))) if matches!(kw.as_str(), "true" | "false" | "nil") => {
                i += 1;
                LuaValue::Keyword(kw.clone())
            }
            Some((l, Token::Ident(other))) => {
                i += 1;
                lookup(&vars, other)
                    .with_context(|| format!("line {l}: {name} refers to undefined {other}"))?
            }
            Some((_, Token::LBrace)) => {
                i += 1;
                let mut items = Vec::new();
                loop {
                    match tokens.get(i) {
                        Some((_, Token::RBrace)) => {
                            i += 1;
                            break;
                        }
                        Some((_, Token::Str(s))) => {
                            items.push(s.clone());
                            i += 1;
                            match tokens.get(i) {
                                Some((_, Token::Comma)) => i += 1,
                                Some((_, Token::RBrace)) => {}
                                Some((l, t)) => {
                                    anyhow::bail!(
                                        "line {l}: expected `,` or `}}` in list, found {t:?}"
                                    )
                                }
                                None => anyhow::bail!("line {line}: unterminated list for {name}"),
                            }
                        }
                        Some((l, t)) => {
                            anyhow::bail!("line {l}: expected a string in list, found {t:?}")
                        }
                        None => anyhow::bail!("line {line}: unterminated list for {name}"),
                    }
                }
                LuaValue::List(items)
            }
            Some((l, t)) => anyhow::bail!("line {l}: unsupported value for {name}: {t:?}"),
            None => anyhow::bail!("line {line}: missing value for {name}"),
        };
        vars.push((name.clone(), value));
    }

    let name = match lookup(&vars, "NAME") {
        Some(LuaValue::Str(s)) if !s.is_empty() => s,
        Some(LuaValue::Str(_)) | None => anyhow::bail!("no NAME"),
        Some(other) => anyhow::bail!("NAME is {other:?}, expected a string"),
    };
    let description = match lookup(&vars, "DESCRIPTION_SHORT") {
        Some(LuaValue::Str(s)) => Some(s),
        Some(LuaValue::Keyword(k)) if k == "nil" => None,
        Some(other) => anyhow::bail!("DESCRIPTION_SHORT is {other:?}, expected a string"),
        None => None,
    };
    let keywords = match lookup(&vars, "KEYWORDS") {
        Some(LuaValue::List(l)) => Some(l),
        Some(LuaValue::Keyword(k)) if k == "nil" => None,
        Some(other) => anyhow::bail!("KEYWORDS is {other:?}, expected a list"),
        None => None,
    };
    Ok(LocalMeta {
        name,
        description,
        keywords,
    })
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

    // ── prune-ci-gql-obo ──

    #[test]
    fn inventory_query_first_page_asks_for_both() {
        let q = inventory_query(&Page::First, &Page::First);
        assert!(q.starts_with("query($owner:String!,$name:String!,$dafter:String,$rafter:String)"));
        assert!(q.contains("deployments(first:100,after:$dafter"));
        assert!(q.contains("releases(first:100,after:$rafter"));
        assert!(q.contains("latestStatus{state}"));
    }

    #[test]
    fn inventory_query_leaves_out_finished_connections() {
        // An unused variable is a GraphQL error, so a finished connection
        // takes its variable with it.
        let q = inventory_query(&Page::Done, &Page::After("c".into()));
        assert!(!q.contains("deployments"), "{q}");
        assert!(!q.contains("$dafter"), "{q}");
        assert!(q.contains("releases("), "{q}");
        let q = inventory_query(&Page::After("c".into()), &Page::Done);
        assert!(!q.contains("releases"), "{q}");
        assert!(!q.contains("$rafter"), "{q}");
    }

    #[test]
    fn parse_inventory_page_full() {
        let out = "D DE_a 11 ACTIVE SUCCESS\n\
                   D DE_b 12 INACTIVE FAILURE\n\
                   D DE_c 13 INACTIVE -\n\
                   DP true Y3Vy==\n\
                   R 21\n\
                   R 22\n\
                   RP false -";
        let page = parse_inventory_page(out).unwrap();
        assert_eq!(
            page.deployments,
            vec![
                GqlDeployment {
                    node_id: "DE_a".into(),
                    database_id: 11,
                    inactive: false,
                    failed: false,
                },
                GqlDeployment {
                    node_id: "DE_b".into(),
                    database_id: 12,
                    inactive: true,
                    failed: true,
                },
                GqlDeployment {
                    node_id: "DE_c".into(),
                    database_id: 13,
                    inactive: true,
                    failed: false,
                },
            ]
        );
        assert_eq!(page.releases, vec![21, 22]);
        assert_eq!(page.deployments_next, Some(Page::After("Y3Vy==".into())));
        assert_eq!(page.releases_next, Some(Page::Done));
    }

    #[test]
    fn parse_inventory_page_error_state_counts_as_failed() {
        let page = parse_inventory_page("D DE_a 1 ERROR ERROR\nDP false -").unwrap();
        assert!(page.deployments[0].failed);
        assert_eq!(page.releases_next, None);
    }

    #[test]
    fn parse_inventory_page_rejects_garbage() {
        assert!(parse_inventory_page("D DE_a notanumber ACTIVE SUCCESS").is_err());
        assert!(parse_inventory_page("D DE_a 1").is_err());
        assert!(parse_inventory_page("DP maybe x").is_err());
        assert!(parse_inventory_page("DP true -").is_err());
        assert!(parse_inventory_page("X 1").is_err());
    }

    #[test]
    fn deployment_mutation_deactivates_only_active_ones_first() {
        let active = GqlDeployment {
            node_id: "DE_a".into(),
            database_id: 1,
            inactive: false,
            failed: false,
        };
        let inactive = GqlDeployment {
            node_id: "DE_b".into(),
            database_id: 2,
            inactive: true,
            failed: false,
        };
        let m = deployment_mutation(&[&active, &inactive]).unwrap();
        assert_eq!(
            m,
            "mutation{\
             s0:createDeploymentStatus(input:{deploymentId:\"DE_a\",state:INACTIVE}){clientMutationId} \
             d0:deleteDeployment(input:{id:\"DE_a\"}){clientMutationId} \
             d1:deleteDeployment(input:{id:\"DE_b\"}){clientMutationId} \
             }"
        );
    }

    #[test]
    fn graphql_errors_reads_alias_and_message() {
        // As gh prints a failing response: the data, then the errors.
        let json = r#"{"data":{"r0":{"x":1},"r1":null},"errors":[{"type":"NOT_FOUND","path":["r1"],"locations":[{"line":1,"column":89}],"message":"Could not resolve to a Repository with the name 'o/\"q\"'."},{"message":"no path here"}]}"#;
        let errors = graphql_errors(json).unwrap();
        assert_eq!(
            errors,
            vec![
                (
                    Some("r1".to_string()),
                    "Could not resolve to a Repository with the name 'o/\"q\"'.".to_string()
                ),
                (None, "no path here".to_string()),
            ]
        );
    }

    #[test]
    fn graphql_errors_none_without_an_errors_array() {
        assert_eq!(graphql_errors(""), None);
        assert_eq!(graphql_errors(r#"{"data":{}}"#), None);
    }

    #[test]
    fn deployment_mutation_rejects_unsafe_node_ids() {
        let bad = GqlDeployment {
            node_id: "x\"){evil}".into(),
            database_id: 1,
            inactive: true,
            failed: false,
        };
        assert!(deployment_mutation(&[&bad]).is_err());
    }

    // ── artifacts / last-workflow-state ──

    #[test]
    fn format_table_pads_columns() {
        let rows = [["a-long-name", "12", "3"], ["b", "123456", "0"]];
        let out = format_table(&["NAME", "SIZE", "DOWNLOADS"], &rows);
        assert_eq!(
            out,
            "NAME         SIZE    DOWNLOADS\n\
             a-long-name  12      3\n\
             b            123456  0"
        );
    }

    #[test]
    fn workflow_state_prefers_conclusion() {
        assert_eq!(
            workflow_state_from_line("completed\tsuccess"),
            Some("success".to_string())
        );
    }

    #[test]
    fn workflow_state_falls_back_to_status_while_running() {
        assert_eq!(
            workflow_state_from_line("in_progress\t"),
            Some("in_progress".to_string())
        );
        assert_eq!(
            workflow_state_from_line("queued\tnull"),
            Some("queued".to_string())
        );
    }

    #[test]
    fn workflow_state_none_without_runs() {
        assert_eq!(workflow_state_from_line(""), None);
        assert_eq!(workflow_state_from_line("  \n"), None);
    }

    // ── project.lua ──

    #[test]
    fn parse_project_lua_multiline_keywords() {
        let text = r#"-- project definitions
NAME = "demos-os-linux"
DESCRIPTION_SHORT = "demos-os-linux is a project to demo the Linux API"
DESCRIPTION_LONG = DESCRIPTION_SHORT
KEYWORDS = {
    "linux",
    "api", -- trailing comment
    "c-plus-plus",
}
"#;
        let meta = parse_project_lua(text).unwrap();
        assert_eq!(
            meta,
            LocalMeta {
                name: "demos-os-linux".to_string(),
                description: Some("demos-os-linux is a project to demo the Linux API".to_string()),
                keywords: Some(vec![
                    "linux".to_string(),
                    "api".to_string(),
                    "c-plus-plus".to_string()
                ]),
            }
        );
    }

    #[test]
    fn parse_project_lua_single_line_keywords_and_extra_fields() {
        let text = r#"NAME = "x"
NAME_FANCY = "X"
LICENSE_TYPE = "MIT"
PYPI = "x-py"
KEYWORDS = {"a", "b", "c"}
"#;
        let meta = parse_project_lua(text).unwrap();
        assert_eq!(meta.name, "x");
        assert_eq!(meta.description, None);
        assert_eq!(
            meta.keywords,
            Some(vec!["a".to_string(), "b".to_string(), "c".to_string()])
        );
    }

    #[test]
    fn parse_project_lua_string_escapes_and_single_quotes() {
        let text = "NAME = 'n'\nDESCRIPTION_SHORT = \"say \\\"hi\\\"\"\n";
        let meta = parse_project_lua(text).unwrap();
        assert_eq!(meta.name, "n");
        assert_eq!(meta.description.as_deref(), Some("say \"hi\""));
    }

    #[test]
    fn parse_project_lua_booleans_and_long_strings() {
        let text = "NAME = \"n\"\nPYPI = true\nDESCRIPTION_LONG = [[\nfirst line\nsecond line]]\nDESCRIPTION_SHORT = [[one-liner]]\nKEYWORDS = nil\n";
        let meta = parse_project_lua(text).unwrap();
        assert_eq!(meta.name, "n");
        assert_eq!(meta.description.as_deref(), Some("one-liner"));
        assert_eq!(meta.keywords, None);
        // A boolean where a string is expected is a type error, not a guess.
        let err = parse_project_lua("NAME = \"n\"\nDESCRIPTION_SHORT = true\n").unwrap_err();
        assert!(err.to_string().contains("DESCRIPTION_SHORT"), "{err}");
    }

    #[test]
    fn parse_project_lua_long_string_keeps_inner_newlines() {
        let text = "NAME = \"n\"\nDESCRIPTION_SHORT = [[\na\nb]]\n";
        let meta = parse_project_lua(text).unwrap();
        assert_eq!(meta.description.as_deref(), Some("a\nb"));
    }

    #[test]
    fn parse_project_lua_requires_name() {
        let err = parse_project_lua("KEYWORDS = {\"a\"}\n").unwrap_err();
        assert!(err.to_string().contains("NAME"), "{err}");
    }

    #[test]
    fn parse_project_lua_rejects_unsupported_syntax_with_line() {
        let err = parse_project_lua("NAME = \"n\"\nKEYWORDS = { \"a\" .. \"b\" }\n").unwrap_err();
        assert!(err.to_string().contains("line 2"), "{err}");
        let err = parse_project_lua("NAME = \"n\"\nlocal x = 1\n").unwrap_err();
        assert!(err.to_string().contains("line 2"), "{err}");
    }

    #[test]
    fn parse_project_lua_alias_to_undefined_is_an_error() {
        let err = parse_project_lua("NAME = \"n\"\nDESCRIPTION_LONG = NOPE\n").unwrap_err();
        assert!(err.to_string().contains("NOPE"), "{err}");
    }

    // ── gh repo view output ──

    #[test]
    fn parse_github_meta_full() {
        let out = "veltzer/demo\nfalse true false\napi linux\nA demo repo";
        let meta = parse_github_meta(out).unwrap();
        assert_eq!(
            meta,
            GithubMeta {
                name_with_owner: "veltzer/demo".to_string(),
                description: "A demo repo".to_string(),
                topics: vec!["api".to_string(), "linux".to_string()],
                wiki: false,
                issues: true,
                projects: false,
            }
        );
    }

    #[test]
    fn parse_github_meta_empty_topics_and_description() {
        // capture_output trims, so the trailing newlines are gone.
        let out = "veltzer/demo\ntrue true true";
        let meta = parse_github_meta(out).unwrap();
        assert!(meta.topics.is_empty());
        assert_eq!(meta.description, "");
        assert!(meta.wiki && meta.issues && meta.projects);
    }

    #[test]
    fn parse_github_meta_rejects_garbage() {
        assert!(parse_github_meta("").is_err());
        assert!(parse_github_meta("veltzer/demo\nyes no maybe").is_err());
    }

    // ── plan_sync ──

    fn local(desc: Option<&str>, keywords: Option<&[&str]>) -> LocalMeta {
        LocalMeta {
            name: "demo".to_string(),
            description: desc.map(String::from),
            keywords: keywords.map(|k| k.iter().map(|s| s.to_string()).collect()),
        }
    }

    fn remote(desc: &str, topics: &[&str]) -> GithubMeta {
        GithubMeta {
            name_with_owner: "veltzer/demo".to_string(),
            description: desc.to_string(),
            topics: topics.iter().map(|s| s.to_string()).collect(),
            wiki: POLICY_WIKI,
            issues: POLICY_ISSUES,
            projects: POLICY_PROJECTS,
        }
    }

    #[test]
    fn plan_sync_in_sync_is_empty() {
        let plan = plan_sync(
            &local(Some("d"), Some(&["a", "b"])),
            &remote("d", &["a", "b"]),
        );
        assert_eq!(plan, SyncPlan::default());
    }

    #[test]
    fn plan_sync_topic_order_does_not_matter() {
        let plan = plan_sync(&local(None, Some(&["b", "a"])), &remote("", &["a", "b"]));
        assert_eq!(plan, SyncPlan::default());
    }

    #[test]
    fn plan_sync_unset_local_fields_leave_github_alone() {
        let plan = plan_sync(&local(None, None), &remote("keep me", &["keep"]));
        assert_eq!(plan, SyncPlan::default());
        let plan = plan_sync(&local(Some(""), Some(&[])), &remote("keep me", &["keep"]));
        assert_eq!(plan, SyncPlan::default());
    }

    #[test]
    fn plan_sync_description() {
        let plan = plan_sync(&local(Some("new"), None), &remote("old", &[]));
        assert_eq!(
            plan.report,
            vec!["description", "  local:  new", "  github: old"]
        );
        assert_eq!(plan.edit_args, vec!["--description", "new"]);
    }

    #[test]
    fn plan_sync_topics_touch_only_the_difference() {
        let plan = plan_sync(
            &local(None, Some(&["shared", "added"])),
            &remote("", &["removed", "shared"]),
        );
        assert_eq!(
            plan.report,
            vec![
                "topics",
                "  local:  added shared",
                "  github: removed shared"
            ]
        );
        // "shared" is named in neither list: gh would let the remove win.
        assert_eq!(
            plan.edit_args,
            vec!["--remove-topic", "removed", "--add-topic", "added"]
        );
    }

    #[test]
    fn plan_sync_features_only_the_divergent_flags() {
        let mut r = remote("", &[]);
        r.wiki = true;
        r.projects = true;
        let plan = plan_sync(&local(None, None), &r);
        assert_eq!(
            plan.report,
            vec![
                "features",
                "  want:   wiki=false issues=true projects=false",
                "  github: wiki=true issues=true projects=true",
            ]
        );
        assert_eq!(
            plan.edit_args,
            vec!["--enable-wiki=false", "--enable-projects=false"]
        );
    }

    #[test]
    fn plan_sync_name_mismatch_is_reported_not_edited() {
        let mut l = local(None, None);
        l.name = "other".to_string();
        let plan = plan_sync(&l, &remote("", &[]));
        assert_eq!(
            plan.report,
            vec!["name", "  local:  other", "  github: demo"]
        );
        assert!(plan.edit_args.is_empty());
    }
}
