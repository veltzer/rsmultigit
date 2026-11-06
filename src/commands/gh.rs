//! The `gh` and `gh-single` commands, talking to GitHub through the API
//! client in [`crate::github`]. Which GitHub repository a checkout is comes
//! from its git remotes (see [`github_repo_id`]), the way the gh CLI
//! resolves it, so no request is spent on that.

use std::collections::HashSet;

use anyhow::{Context, Result};
use camino::{Utf8Path, Utf8PathBuf};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::commands::project_lua::{LocalMeta, parse_project_lua};
use crate::git_url::{GitUrl, RepoId};
use crate::github::{Github, PAGE_SIZE, github};
use crate::subprocess_utils::{check_call, out_line};

// ── which repository ───────────────────────────────────────────────────

/// The github.com repositories the checkout's remotes point at, each with
/// the name of its remote.
fn github_remotes(project: &Utf8Path) -> Result<(git2::Repository, Vec<(String, RepoId)>)> {
    let repo = crate::commands::count::open_repo(project)?;
    let names = repo
        .remotes()
        .with_context(|| format!("failed to list remotes for {project}"))?;
    let mut found = Vec::new();
    for name in names.iter() {
        // Err is a non-UTF-8 remote name, Ok(None) a null entry: neither can
        // be looked up, so skip both.
        let Ok(Some(name)) = name else {
            continue;
        };
        if let Ok(remote) = repo.find_remote(name)
            && let Ok(url) = remote.url()
            && let Some(id) = GitUrl::parse(url).github_repo()
        {
            found.push((name.to_string(), id));
        }
    }
    Ok((repo, found))
}

/// Repos the gh commands can operate on: those with a remote on github.com.
pub fn check_github(project: &Utf8Path) -> Result<bool> {
    Ok(!github_remotes(project)?.1.is_empty())
}

/// How strongly gh prefers a remote as the repo to act on, when no
/// `gh repo set-default` says otherwise.
fn remote_rank(name: &str) -> u8 {
    match name {
        "upstream" => 3,
        "github" => 2,
        "origin" => 1,
        _ => 0,
    }
}

/// The GitHub repository gh would act on in this checkout: the one set by
/// `gh repo set-default` (`remote.<name>.gh-resolved`), else the github.com
/// remote gh prefers by name (upstream, github, origin, then the rest).
pub fn github_repo_id(project: &Utf8Path) -> Result<Option<RepoId>> {
    let (repo, remotes) = github_remotes(project)?;
    let config = repo
        .config()
        .with_context(|| format!("failed to read the git config of {project}"))?;
    let mut best: Option<(u8, RepoId)> = None;
    for (name, id) in remotes {
        let rank = match config.get_string(&format!("remote.{name}.gh-resolved")) {
            // `base` marks this remote; a slug names the repo outright.
            Ok(v) if v == "base" => 4,
            Ok(v) => match RepoId::from_slug(&v) {
                Some(id) => return Ok(Some(id)),
                None => remote_rank(&name),
            },
            Err(_) => remote_rank(&name),
        };
        if best.as_ref().is_none_or(|(r, _)| rank > *r) {
            best = Some((rank, id));
        }
    }
    Ok(best.map(|(_, id)| id))
}

/// [`github_repo_id`], for a repo already known to be on GitHub.
pub fn require_repo_id(project: &Utf8Path) -> Result<RepoId> {
    github_repo_id(project)?
        .with_context(|| format!("cannot tell the GitHub repository of {project} from its remotes"))
}

/// The working tree of the git repo containing the current directory, for
/// the `gh-single` operations.
pub fn current_repo() -> Result<Utf8PathBuf> {
    let cwd = crate::runner::current_dir()?;
    let repo = git2::Repository::discover(&cwd)
        .with_context(|| format!("{cwd} is not inside a git repository"))?;
    let workdir = repo
        .workdir()
        .with_context(|| format!("the repository at {cwd} has no working tree"))?;
    // libgit2 reports the workdir with a trailing slash; collecting the
    // components drops it, so messages read `.../repo has ...`.
    Utf8PathBuf::from_path_buf(workdir.components().collect())
        .map_err(|p| anyhow::anyhow!("non-UTF-8 repository path: {}", p.display()))
}

/// The repository's default branch on GitHub, for `git branch github`.
pub fn default_branch(project: &Utf8Path) -> Result<String> {
    #[derive(Deserialize)]
    struct Repository {
        default_branch: String,
    }
    let repo = require_repo_id(project)?;
    let gh = github()?;
    let info: Repository = gh.block_on(gh.get(&format!("/repos/{}", repo.slug())))?;
    Ok(info.default_branch)
}

// ── the prune policy ───────────────────────────────────────────────────

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

fn is_failed_conclusion(conclusion: &str) -> bool {
    matches!(
        conclusion,
        "failure" | "cancelled" | "timed_out" | "startup_failure" | "action_required"
    )
}

#[derive(Deserialize)]
struct Id {
    id: u64,
}

#[derive(Deserialize)]
struct WorkflowRuns {
    workflow_runs: Vec<WorkflowRun>,
}

#[derive(Deserialize)]
struct WorkflowRun {
    id: u64,
    status: Option<String>,
    conclusion: Option<String>,
}

/// The workflow runs, newest first, as (id, failed) pairs. REST, since
/// GraphQL cannot list a repo's runs.
pub(crate) async fn fetch_workflow_runs(gh: &Github, repo: &RepoId) -> Result<Vec<(u64, bool)>> {
    let runs = gh
        .get_all(
            &format!("/repos/{}/actions/runs", repo.slug()),
            |page: WorkflowRuns| page.workflow_runs,
        )
        .await?;
    Ok(runs
        .into_iter()
        .map(|r| {
            (
                r.id,
                is_failed_conclusion(r.conclusion.as_deref().unwrap_or("")),
            )
        })
        .collect())
}

// ── prune-ci-rest-obo ──────────────────────────────────────────────────
//
// One request per item, in order, stopping at the first failure: the
// plain REST baseline the faster variants are measured against.

/// Clean up GitHub deployments, releases, and workflow runs for a repository,
/// keeping only the `keep` most recent non-failed of each and deleting the rest.
pub fn prune_ci_rest_obo(project: &Utf8Path, keep: usize) -> Result<()> {
    let repo = require_repo_id(project)?;
    let gh = github()?;
    gh.block_on(async {
        clean_deployments(gh, &repo, keep).await?;
        clean_releases(gh, &repo, keep).await?;
        clean_workflows(gh, &repo, keep).await
    })
}

#[derive(Deserialize)]
struct DeploymentStatus {
    state: String,
}

async fn clean_deployments(gh: &Github, repo: &RepoId, keep: usize) -> Result<()> {
    let base = format!("/repos/{}/deployments", repo.slug());
    let ids = gh.get_all(&base, |page: Vec<Id>| page).await?;

    // A deployment counts as failed when its most recent status is
    // failure/error; those are always deleted, regardless of recency.
    let mut items = Vec::with_capacity(ids.len());
    for Id { id } in ids {
        let statuses: Vec<DeploymentStatus> =
            gh.get(&format!("{base}/{id}/statuses?per_page=1")).await?;
        let failed = statuses
            .first()
            .is_some_and(|s| s.state == "failure" || s.state == "error");
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
        gh.post(
            &format!("{base}/{id}/statuses"),
            &json!({ "state": "inactive" }),
        )
        .await?;
        gh.delete(&format!("{base}/{id}")).await?;
        out_line(&format!("  deleted deployment {id}"));
    }
    Ok(())
}

async fn clean_releases(gh: &Github, repo: &RepoId, keep: usize) -> Result<()> {
    let base = format!("/repos/{}/releases", repo.slug());
    let ids: Vec<u64> = gh
        .get_all(&base, |page: Vec<Id>| page)
        .await?
        .into_iter()
        .map(|r| r.id)
        .collect();
    let to_delete: Vec<u64> = ids.iter().skip(keep).copied().collect();
    out_line(&format!(
        "releases: {} found, deleting {}",
        ids.len(),
        to_delete.len()
    ));
    for id in to_delete {
        gh.delete(&format!("{base}/{id}")).await?;
        out_line(&format!("  deleted release {id}"));
    }
    Ok(())
}

async fn clean_workflows(gh: &Github, repo: &RepoId, keep: usize) -> Result<()> {
    let items = fetch_workflow_runs(gh, repo).await?;
    let to_delete = select_deletions(&items, keep);
    out_line(&format!(
        "workflow runs: {} found, deleting {}",
        items.len(),
        to_delete.len()
    ));
    for id in to_delete {
        gh.delete(&format!("/repos/{}/actions/runs/{id}", repo.slug()))
            .await?;
        out_line(&format!("  deleted workflow run {id}"));
    }
    Ok(())
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
//   listed 100 per page, and the deletes run several at once.
//
// Everything that varies (owner, name, cursors, node ids) travels as
// GraphQL variables, never inside the query text.

/// Deployments per mutation request; each one takes two mutations.
pub(crate) const DEPLOYMENTS_PER_MUTATION: usize = 50;

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

    /// The `after:` variable for this page: null on the first page.
    pub(crate) fn cursor(&self) -> Value {
        match self {
            Page::After(c) => Value::String(c.clone()),
            _ => Value::Null,
        }
    }
}

/// One repository's part of an inventory response. A connection that was
/// not asked for is absent.
#[derive(Debug, Deserialize)]
pub(crate) struct RepoInventory {
    deployments: Option<Connection<DeploymentNode>>,
    releases: Option<Connection<ReleaseNode>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Connection<N> {
    page_info: PageInfo,
    nodes: Vec<N>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PageInfo {
    has_next_page: bool,
    end_cursor: Option<String>,
}

impl PageInfo {
    fn next(&self) -> Result<Page> {
        match (self.has_next_page, &self.end_cursor) {
            (false, _) => Ok(Page::Done),
            (true, Some(c)) => Ok(Page::After(c.clone())),
            (true, None) => anyhow::bail!("next page without a cursor"),
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DeploymentNode {
    id: String,
    database_id: u64,
    state: String,
    latest_status: Option<StatusNode>,
}

#[derive(Debug, Deserialize)]
struct StatusNode {
    state: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReleaseNode {
    database_id: u64,
}

impl From<DeploymentNode> for GqlDeployment {
    fn from(node: DeploymentNode) -> Self {
        let latest = node.latest_status.map(|s| s.state).unwrap_or_default();
        Self {
            node_id: node.id,
            database_id: node.database_id,
            inactive: node.state == "INACTIVE",
            failed: latest == "FAILURE" || latest == "ERROR",
        }
    }
}

/// One repository's deployments and releases, collected page by page.
pub(crate) struct Inventory {
    pub(crate) dpage: Page,
    pub(crate) rpage: Page,
    pub(crate) deployments: Vec<GqlDeployment>,
    pub(crate) releases: Vec<u64>,
}

impl Inventory {
    pub(crate) fn new() -> Self {
        Self {
            dpage: Page::First,
            rpage: Page::First,
            deployments: Vec::new(),
            releases: Vec::new(),
        }
    }

    pub(crate) fn pending(&self) -> bool {
        self.dpage.wanted() || self.rpage.wanted()
    }

    /// Take in one page. A connection that was asked for must be there;
    /// its absence means the repository did not resolve.
    pub(crate) fn absorb(&mut self, page: RepoInventory) -> Result<()> {
        if self.dpage.wanted() {
            let conn = page
                .deployments
                .context("GraphQL query returned no deployments connection")?;
            self.dpage = conn.page_info.next()?;
            self.deployments
                .extend(conn.nodes.into_iter().map(GqlDeployment::from));
        }
        if self.rpage.wanted() {
            let conn = page
                .releases
                .context("GraphQL query returned no releases connection")?;
            self.rpage = conn.page_info.next()?;
            self.releases
                .extend(conn.nodes.into_iter().map(|r| r.database_id));
        }
        Ok(())
    }
}

/// The fields of one repository that the inventory asks for, with the
/// variables named by `suffix` (`$d0`, `$r0` for suffix `0`). Only the
/// connections still wanted are included, since GraphQL rejects a declared
/// variable that is not used.
pub(crate) fn inventory_fields(
    suffix: &str,
    dpage: &Page,
    rpage: &Page,
    vars: &mut Vec<String>,
) -> String {
    let mut body = String::new();
    if dpage.wanted() {
        vars.push(format!("$d{suffix}:String"));
        body.push_str(&format!(
            "deployments(first:{PAGE_SIZE},after:$d{suffix},orderBy:{{field:CREATED_AT,direction:DESC}})\
             {{pageInfo{{hasNextPage endCursor}} nodes{{id databaseId state latestStatus{{state}}}}}} "
        ));
    }
    if rpage.wanted() {
        vars.push(format!("$r{suffix}:String"));
        body.push_str(&format!(
            "releases(first:{PAGE_SIZE},after:$r{suffix},orderBy:{{field:CREATED_AT,direction:DESC}})\
             {{pageInfo{{hasNextPage endCursor}} nodes{{databaseId}}}} "
        ));
    }
    body.trim_end().to_string()
}

/// The inventory query for one repository.
fn inventory_query(dpage: &Page, rpage: &Page) -> String {
    let mut vars = vec!["$owner:String!".to_string(), "$name:String!".to_string()];
    let fields = inventory_fields("", dpage, rpage, &mut vars);
    format!(
        "query({}){{repository(owner:$owner,name:$name){{{fields}}}}}",
        vars.join(",")
    )
}

#[derive(Deserialize)]
struct OneRepository {
    repository: Option<RepoInventory>,
}

/// All deployments and releases of the repo, newest first, paging each
/// connection until it is exhausted.
async fn fetch_inventory(gh: &Github, repo: &RepoId) -> Result<Inventory> {
    let mut inv = Inventory::new();
    while inv.pending() {
        let query = inventory_query(&inv.dpage, &inv.rpage);
        let mut variables = json!({ "owner": repo.owner, "name": repo.name });
        if inv.dpage.wanted() {
            variables["d"] = inv.dpage.cursor();
        }
        if inv.rpage.wanted() {
            variables["r"] = inv.rpage.cursor();
        }
        let response = gh.graphql::<OneRepository>(&query, variables).await?;
        if let Some(e) = response.errors.first() {
            anyhow::bail!("{}", e.message);
        }
        let page = response
            .data
            .and_then(|d| d.repository)
            .context("GraphQL query returned no repository")?;
        inv.absorb(page)?;
    }
    Ok(inv)
}

/// One mutation request that deactivates (where needed) and deletes each of
/// `batch`, in order, with the node ids as variables `$id0`, `$id1`, ...
pub(crate) fn deployment_mutation(batch: &[&GqlDeployment]) -> (String, Value) {
    let mut vars = Vec::new();
    let mut body = String::new();
    let mut variables = serde_json::Map::new();
    for (i, d) in batch.iter().enumerate() {
        vars.push(format!("$id{i}:ID!"));
        variables.insert(format!("id{i}"), Value::String(d.node_id.clone()));
        // GitHub refuses to delete an active deployment.
        if !d.inactive {
            body.push_str(&format!(
                "s{i}:createDeploymentStatus(input:{{deploymentId:$id{i},state:INACTIVE}}){{clientMutationId}} "
            ));
        }
        body.push_str(&format!(
            "d{i}:deleteDeployment(input:{{id:$id{i}}}){{clientMutationId}} "
        ));
    }
    (
        format!("mutation({}){{{}}}", vars.join(","), body.trim_end()),
        Value::Object(variables),
    )
}

/// Run one deployment mutation request and say, per deployment, whether
/// it is gone. Only those whose `d<i>` alias errored failed, or every one
/// when the request never reached GraphQL.
async fn run_deployment_batch(gh: &Github, batch: &[&GqlDeployment]) -> Vec<Result<()>> {
    let (mutation, variables) = deployment_mutation(batch);
    let response = match gh.graphql::<Value>(&mutation, variables).await {
        Ok(r) => r,
        Err(e) => {
            let msg = format!("{e:#}");
            return batch
                .iter()
                .map(|_| Err(anyhow::anyhow!("{msg}")))
                .collect();
        }
    };
    (0..batch.len())
        .map(|i| {
            let alias = format!("d{i}");
            match response
                .errors
                .iter()
                .find(|e| e.alias() == Some(alias.as_str()))
            {
                Some(e) => Err(anyhow::anyhow!("{}", e.message)),
                None => Ok(()),
            }
        })
        .collect()
}

/// Delete the deployments, `DEPLOYMENTS_PER_MUTATION` per request; one
/// result per deployment, in order.
pub(crate) async fn delete_deployments(
    gh: &Github,
    to_delete: &[&GqlDeployment],
) -> Vec<Result<()>> {
    let mut results = Vec::with_capacity(to_delete.len());
    for batch in to_delete.chunks(DEPLOYMENTS_PER_MUTATION) {
        results.extend(run_deployment_batch(gh, batch).await);
    }
    results
}

/// What one repository deletes: indices into its deployments, release ids
/// and workflow run ids.
pub(crate) struct Plan {
    pub(crate) deployments: Vec<usize>,
    pub(crate) releases: Vec<u64>,
    pub(crate) runs: Vec<u64>,
}

impl Plan {
    pub(crate) fn new(inv: &Inventory, runs: &[(u64, bool)], keep: usize) -> Self {
        let items: Vec<(u64, bool)> = inv
            .deployments
            .iter()
            .map(|d| (d.database_id, d.failed))
            .collect();
        let ids: HashSet<u64> = select_deletions(&items, keep).into_iter().collect();
        Self {
            deployments: (0..inv.deployments.len())
                .filter(|&i| ids.contains(&inv.deployments[i].database_id))
                .collect(),
            releases: inv.releases.iter().skip(keep).copied().collect(),
            runs: select_deletions(runs, keep),
        }
    }

    /// The REST routes that delete the plan's releases, then its runs.
    pub(crate) fn rest_routes(&self, repo: &RepoId) -> Vec<String> {
        let slug = repo.slug();
        self.releases
            .iter()
            .map(|id| format!("/repos/{slug}/releases/{id}"))
            .chain(
                self.runs
                    .iter()
                    .map(|id| format!("/repos/{slug}/actions/runs/{id}")),
            )
            .collect()
    }

    /// The output lines and the failures, once the deletes have run.
    /// `rest_results` holds the release results followed by the run
    /// results, as [`Plan::rest_routes`] laid them out.
    pub(crate) fn report(
        &self,
        inv: &Inventory,
        runs_found: usize,
        deployment_results: &[Result<()>],
        rest_results: &[Result<()>],
    ) -> (Vec<String>, Vec<String>) {
        let deployment_ids: Vec<u64> = self
            .deployments
            .iter()
            .map(|&i| inv.deployments[i].database_id)
            .collect();
        let (release_results, run_results) = rest_results.split_at(self.releases.len());
        let mut lines = Vec::new();
        let mut errors = Vec::new();
        for (label, found, ids, results) in [
            (
                "deployments",
                inv.deployments.len(),
                &deployment_ids,
                deployment_results,
            ),
            (
                "releases",
                inv.releases.len(),
                &self.releases,
                release_results,
            ),
            ("workflow runs", runs_found, &self.runs, run_results),
        ] {
            lines.push(format!("{label}: {found} found, deleting {}", ids.len()));
            let singular = label.trim_end_matches('s');
            for (id, result) in ids.iter().zip(results) {
                match result {
                    Ok(()) => lines.push(format!("  deleted {singular} {id}")),
                    Err(e) => errors.push(format!("deleting {singular} {id}: {e:#}")),
                }
            }
        }
        (lines, errors)
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

/// `prune_ci_rest_obo` through GitHub's GraphQL API where it can do the job, with
/// the remaining REST calls run concurrently. Same keep policy, same output.
/// Unlike `prune_ci_rest_obo`, a failed delete does not stop the others; every
/// failure is reported at the end.
pub fn prune_ci_gql_obo(project: &Utf8Path, keep: usize) -> Result<()> {
    let repo = require_repo_id(project)?;
    let gh = github()?;
    let (lines, errors) = gh.block_on(async {
        // The two listings are independent: run them at the same time.
        let (inv, runs) =
            futures_util::join!(fetch_inventory(gh, &repo), fetch_workflow_runs(gh, &repo));
        let (inv, runs) = (inv?, runs?);
        let plan = Plan::new(&inv, &runs, keep);
        let to_delete: Vec<&GqlDeployment> = plan
            .deployments
            .iter()
            .map(|&i| &inv.deployments[i])
            .collect();
        let routes = plan.rest_routes(&repo);
        let (deployment_results, rest_results) =
            futures_util::join!(delete_deployments(gh, &to_delete), gh.delete_all(&routes));
        anyhow::Ok(plan.report(&inv, runs.len(), &deployment_results, &rest_results))
    })?;
    for line in &lines {
        out_line(line);
    }
    combine_errors(errors)
}

// ── artifacts ──────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct Release {
    assets: Vec<Asset>,
}

#[derive(Deserialize)]
struct Asset {
    name: String,
    size: u64,
    download_count: u64,
}

/// The assets of the latest release as an aligned NAME/SIZE/DOWNLOADS table,
/// or None when the repo has no release (or no github.com remote).
pub fn artifacts(project: &Utf8Path) -> Result<Option<String>> {
    let Some(repo) = github_repo_id(project)? else {
        return Ok(None);
    };
    let gh = github()?;
    // A repo that has never had a release is the common case, not an error.
    let Some(release) = gh
        .block_on(gh.get_optional::<Release>(&format!("/repos/{}/releases/latest", repo.slug())))?
    else {
        return Ok(None);
    };
    if release.assets.is_empty() {
        return Ok(None);
    }
    let rows: Vec<[String; 3]> = release
        .assets
        .into_iter()
        .map(|a| [a.name, a.size.to_string(), a.download_count.to_string()])
        .collect();
    Ok(Some(format_table(["NAME", "SIZE", "DOWNLOADS"], &rows)))
}

/// Render rows under a header with every column padded to its widest cell,
/// what `column -t -N ...` does for the shell script.
fn format_table(header: [&str; 3], rows: &[[String; 3]]) -> String {
    let header = header.map(String::from);
    let mut widths = header.each_ref().map(String::len);
    for row in rows {
        for (w, cell) in widths.iter_mut().zip(row) {
            *w = (*w).max(cell.len());
        }
    }
    let render = |row: &[String; 3]| -> String {
        let cells: Vec<String> = row
            .iter()
            .zip(widths)
            .map(|(cell, w)| format!("{cell:<w$}"))
            .collect();
        cells.join("  ").trim_end().to_string()
    };
    let mut lines = vec![render(&header)];
    lines.extend(rows.iter().map(render));
    lines.join("\n")
}

// ── last-workflow-state ────────────────────────────────────────────────

/// The conclusion of the most recent workflow run (`success`, `failure`,
/// ...), or its status (`in_progress`, `queued`) while it has none yet.
/// None when the repo has no runs or no github.com remote.
pub fn last_workflow_state(project: &Utf8Path) -> Result<Option<String>> {
    let Some(repo) = github_repo_id(project)? else {
        return Ok(None);
    };
    let gh = github()?;
    let runs: WorkflowRuns =
        gh.block_on(gh.get(&format!("/repos/{}/actions/runs?per_page=1", repo.slug())))?;
    Ok(runs.workflow_runs.first().and_then(workflow_state))
}

/// The conclusion when the run has one; the status stands in until then.
fn workflow_state(run: &WorkflowRun) -> Option<String> {
    run.conclusion
        .as_deref()
        .filter(|c| !c.is_empty())
        .or(run.status.as_deref())
        .filter(|s| !s.is_empty())
        .map(String::from)
}

// ── open-site ──────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct PagesSite {
    html_url: Option<String>,
}

/// Open the repo's GitHub Pages site in the browser via `xdg-open`. A repo
/// without a Pages site says so and is otherwise left alone.
pub fn open_site(project: &Utf8Path) -> Result<()> {
    let repo = require_repo_id(project)?;
    let gh = github()?;
    let site =
        gh.block_on(gh.get_optional::<PagesSite>(&format!("/repos/{}/pages", repo.slug())))?;
    match site.and_then(|s| s.html_url).filter(|u| !u.is_empty()) {
        Some(url) => {
            out_line(&url);
            check_call(project, "xdg-open", &[&url])
        }
        None => {
            out_line("no GitHub Pages site");
            Ok(())
        }
    }
}

// ── sync-metadata ──────────────────────────────────────────────────────

/// Fixed fleet feature policy, not read from the repo.
const POLICY_WIKI: bool = false;
const POLICY_ISSUES: bool = true;
const POLICY_PROJECTS: bool = false;

/// What GitHub currently has.
#[derive(Debug, PartialEq, Eq, Deserialize)]
pub struct GithubMeta {
    /// `owner/name`, as GitHub spells it now (a renamed repo answers under
    /// its old name too, but edits go to the current one).
    pub full_name: String,
    pub name: String,
    #[serde(default, deserialize_with = "null_as_empty")]
    pub description: String,
    /// Sorted once fetched.
    #[serde(default)]
    pub topics: Vec<String>,
    #[serde(rename = "has_wiki")]
    pub wiki: bool,
    #[serde(rename = "has_issues")]
    pub issues: bool,
    #[serde(rename = "has_projects")]
    pub projects: bool,
}

fn null_as_empty<'de, D: serde::Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    Ok(Option::<String>::deserialize(d)?.unwrap_or_default())
}

/// The differences between local and GitHub, as a report and as the edits
/// that would resolve them.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct SyncPlan {
    pub report: Vec<String>,
    /// New description, when it differs.
    pub description: Option<String>,
    /// The full new topic list, when the topics differ.
    pub topics: Option<Vec<String>>,
    /// The feature flags to change, as (`has_wiki`-style field, value).
    pub features: Vec<(&'static str, bool)>,
}

impl SyncPlan {
    fn has_edits(&self) -> bool {
        self.description.is_some() || self.topics.is_some() || !self.features.is_empty()
    }
}

/// Sync the repo's GitHub description, topics and feature policy from
/// config/project.lua. Returns the report of what differed (and, unless
/// `dry_run`, was changed), or None when GitHub already matches. A repo
/// without a project.lua, or without a github.com remote, is skipped.
pub fn sync_metadata(project: &Utf8Path, dry_run: bool) -> Result<Option<String>> {
    let Some(repo) = github_repo_id(project)? else {
        return Ok(None);
    };
    let lua_path = project.join("config/project.lua");
    if !lua_path.is_file() {
        return Ok(None);
    }
    let text =
        std::fs::read_to_string(&lua_path).with_context(|| format!("failed to read {lua_path}"))?;
    let local = parse_project_lua(&text).with_context(|| format!("failed to parse {lua_path}"))?;
    let gh = github()?;
    gh.block_on(async {
        let mut remote: GithubMeta = gh.get(&format!("/repos/{}", repo.slug())).await?;
        remote.topics.sort();
        let plan = plan_sync(&local, &remote);
        if plan.report.is_empty() {
            return Ok(None);
        }
        if !dry_run && plan.has_edits() {
            apply_sync(gh, &remote.full_name, &plan).await?;
        }
        Ok(Some(plan.report.join("\n")))
    })
}

/// Apply a plan: one PATCH for the description and features, one PUT for
/// the topics (which replaces the whole list).
async fn apply_sync(gh: &Github, full_name: &str, plan: &SyncPlan) -> Result<()> {
    let mut patch = serde_json::Map::new();
    if let Some(description) = &plan.description {
        patch.insert("description".into(), json!(description));
    }
    for (field, value) in &plan.features {
        patch.insert((*field).into(), json!(value));
    }
    if !patch.is_empty() {
        gh.patch(&format!("/repos/{full_name}"), &Value::Object(patch))
            .await?;
    }
    if let Some(topics) = &plan.topics {
        gh.put(
            &format!("/repos/{full_name}/topics"),
            &json!({ "names": topics }),
        )
        .await?;
    }
    Ok(())
}

/// Compare local against GitHub. Only fields the local file declares are
/// compared; the feature policy is fleet-wide and always enforced.
pub fn plan_sync(local: &LocalMeta, remote: &GithubMeta) -> SyncPlan {
    let mut plan = SyncPlan::default();

    // The repo is addressed by its remote, so a NAME that disagrees with it
    // is reported, never acted on.
    if local.name != remote.name {
        plan.report.push("name".to_string());
        plan.report.push(format!("  local:  {}", local.name));
        plan.report.push(format!("  github: {}", remote.name));
    }

    if let Some(want) = &local.description
        && !want.is_empty()
        && *want != remote.description
    {
        plan.report.push("description".to_string());
        plan.report.push(format!("  local:  {want}"));
        plan.report
            .push(format!("  github: {}", remote.description));
        plan.description = Some(want.clone());
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
            plan.topics = Some(want);
        }
    }

    let features = [
        ("has_wiki", remote.wiki, POLICY_WIKI),
        ("has_issues", remote.issues, POLICY_ISSUES),
        ("has_projects", remote.projects, POLICY_PROJECTS),
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
        for (field, have, want) in features {
            if have != want {
                plan.features.push((field, want));
            }
        }
    }

    plan
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

    #[test]
    fn remote_rank_follows_gh() {
        assert!(remote_rank("upstream") > remote_rank("github"));
        assert!(remote_rank("github") > remote_rank("origin"));
        assert!(remote_rank("origin") > remote_rank("mirror"));
    }

    // ── prune-ci-gql-obo ──

    #[test]
    fn inventory_query_first_page_asks_for_both() {
        let q = inventory_query(&Page::First, &Page::First);
        assert!(q.starts_with("query($owner:String!,$name:String!,$d:String,$r:String)"));
        assert!(q.contains("deployments(first:100,after:$d,"));
        assert!(q.contains("releases(first:100,after:$r,"));
        assert!(q.contains("latestStatus{state}"));
    }

    #[test]
    fn inventory_query_leaves_out_finished_connections() {
        // An unused variable is a GraphQL error, so a finished connection
        // takes its variable with it.
        let q = inventory_query(&Page::Done, &Page::After("c".into()));
        assert!(!q.contains("deployments"), "{q}");
        assert!(!q.contains("$d"), "{q}");
        assert!(q.contains("releases("), "{q}");
        let q = inventory_query(&Page::After("c".into()), &Page::Done);
        assert!(!q.contains("releases"), "{q}");
        assert!(!q.contains("$r"), "{q}");
    }

    fn page(json: &str) -> RepoInventory {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn inventory_absorbs_pages_until_done() {
        let mut inv = Inventory::new();
        inv.absorb(page(
            r#"{"deployments":{"pageInfo":{"hasNextPage":true,"endCursor":"Y3Vy=="},
                "nodes":[{"id":"DE_a","databaseId":11,"state":"ACTIVE","latestStatus":{"state":"SUCCESS"}},
                         {"id":"DE_b","databaseId":12,"state":"INACTIVE","latestStatus":{"state":"FAILURE"}},
                         {"id":"DE_c","databaseId":13,"state":"INACTIVE","latestStatus":null}]},
               "releases":{"pageInfo":{"hasNextPage":false,"endCursor":null},
                "nodes":[{"databaseId":21},{"databaseId":22}]}}"#,
        ))
        .unwrap();
        assert_eq!(
            inv.deployments,
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
        assert_eq!(inv.releases, vec![21, 22]);
        assert_eq!(inv.dpage, Page::After("Y3Vy==".into()));
        assert_eq!(inv.rpage, Page::Done);
        assert!(inv.pending());
        // The second page asks only for deployments.
        inv.absorb(page(
            r#"{"deployments":{"pageInfo":{"hasNextPage":false,"endCursor":"x"},
                "nodes":[{"id":"DE_d","databaseId":14,"state":"ERROR","latestStatus":{"state":"ERROR"}}]}}"#,
        ))
        .unwrap();
        assert!(inv.deployments[3].failed);
        assert!(!inv.pending());
    }

    #[test]
    fn inventory_rejects_a_missing_connection_or_cursor() {
        let mut inv = Inventory::new();
        assert!(inv.absorb(page("{}")).is_err());
        let mut inv = Inventory::new();
        inv.rpage = Page::Done;
        let no_cursor =
            r#"{"deployments":{"pageInfo":{"hasNextPage":true,"endCursor":null},"nodes":[]}}"#;
        assert!(inv.absorb(page(no_cursor)).is_err());
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
            node_id: "DE_b\"){evil}".into(),
            database_id: 2,
            inactive: true,
            failed: false,
        };
        let (m, vars) = deployment_mutation(&[&active, &inactive]);
        assert_eq!(
            m,
            "mutation($id0:ID!,$id1:ID!){\
             s0:createDeploymentStatus(input:{deploymentId:$id0,state:INACTIVE}){clientMutationId} \
             d0:deleteDeployment(input:{id:$id0}){clientMutationId} \
             d1:deleteDeployment(input:{id:$id1}){clientMutationId}\
             }"
        );
        // Node ids travel as variables, so even a hostile one stays data.
        assert_eq!(vars, json!({ "id0": "DE_a", "id1": "DE_b\"){evil}" }));
    }

    #[test]
    fn plan_reports_like_rest_obo() {
        let mut inv = Inventory::new();
        inv.deployments = vec![
            GqlDeployment {
                node_id: "a".into(),
                database_id: 1,
                inactive: true,
                failed: false,
            },
            GqlDeployment {
                node_id: "b".into(),
                database_id: 2,
                inactive: true,
                failed: false,
            },
        ];
        inv.releases = vec![10, 11];
        let runs = [(20, false), (21, true)];
        let plan = Plan::new(&inv, &runs, 1);
        assert_eq!(plan.deployments, vec![1]);
        assert_eq!(plan.releases, vec![11]);
        assert_eq!(plan.runs, vec![21]);
        let repo = RepoId::from_slug("o/n").unwrap();
        assert_eq!(
            plan.rest_routes(&repo),
            vec!["/repos/o/n/releases/11", "/repos/o/n/actions/runs/21"]
        );
        let (lines, errors) = plan.report(
            &inv,
            runs.len(),
            &[Ok(())],
            &[Ok(()), Err(anyhow::anyhow!("boom"))],
        );
        assert_eq!(
            lines,
            vec![
                "deployments: 2 found, deleting 1",
                "  deleted deployment 2",
                "releases: 2 found, deleting 1",
                "  deleted release 11",
                "workflow runs: 2 found, deleting 1",
            ]
        );
        assert_eq!(errors, vec!["deleting workflow run 21: boom"]);
    }

    // ── artifacts / last-workflow-state ──

    #[test]
    fn format_table_pads_columns() {
        let rows = [
            ["a-long-name".to_string(), "12".to_string(), "3".to_string()],
            ["b".to_string(), "123456".to_string(), "0".to_string()],
        ];
        let out = format_table(["NAME", "SIZE", "DOWNLOADS"], &rows);
        assert_eq!(
            out,
            "NAME         SIZE    DOWNLOADS\n\
             a-long-name  12      3\n\
             b            123456  0"
        );
    }

    fn run(status: Option<&str>, conclusion: Option<&str>) -> WorkflowRun {
        WorkflowRun {
            id: 1,
            status: status.map(String::from),
            conclusion: conclusion.map(String::from),
        }
    }

    #[test]
    fn workflow_state_prefers_conclusion() {
        assert_eq!(
            workflow_state(&run(Some("completed"), Some("success"))),
            Some("success".to_string())
        );
    }

    #[test]
    fn workflow_state_falls_back_to_status_while_running() {
        assert_eq!(
            workflow_state(&run(Some("in_progress"), None)),
            Some("in_progress".to_string())
        );
        assert_eq!(
            workflow_state(&run(Some("queued"), Some(""))),
            Some("queued".to_string())
        );
        assert_eq!(workflow_state(&run(None, None)), None);
    }

    // ── sync-metadata ──

    #[test]
    fn github_meta_from_the_repo_endpoint() {
        let json = r#"{"id":1,"name":"demo","full_name":"veltzer/demo","description":null,
            "topics":["linux","api"],"has_wiki":false,"has_issues":true,"has_projects":false,
            "stargazers_count":3}"#;
        let meta: GithubMeta = serde_json::from_str(json).unwrap();
        assert_eq!(
            meta,
            GithubMeta {
                full_name: "veltzer/demo".to_string(),
                name: "demo".to_string(),
                description: String::new(),
                topics: vec!["linux".to_string(), "api".to_string()],
                wiki: false,
                issues: true,
                projects: false,
            }
        );
    }

    fn local(desc: Option<&str>, keywords: Option<&[&str]>) -> LocalMeta {
        LocalMeta {
            name: "demo".to_string(),
            description: desc.map(String::from),
            keywords: keywords.map(|k| k.iter().map(|s| s.to_string()).collect()),
        }
    }

    fn remote(desc: &str, topics: &[&str]) -> GithubMeta {
        GithubMeta {
            full_name: "veltzer/demo".to_string(),
            name: "demo".to_string(),
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
        assert_eq!(plan.description.as_deref(), Some("new"));
        assert!(plan.topics.is_none() && plan.features.is_empty());
    }

    #[test]
    fn plan_sync_topics_set_the_whole_list() {
        let plan = plan_sync(
            &local(None, Some(&["shared", "added", "added"])),
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
        assert_eq!(
            plan.topics,
            Some(vec!["added".to_string(), "shared".to_string()])
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
            plan.features,
            vec![("has_wiki", false), ("has_projects", false)]
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
        assert!(!plan.has_edits());
    }
}
