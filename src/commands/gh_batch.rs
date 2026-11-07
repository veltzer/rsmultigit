//! `gh prune-ci-gql-batch`: the work of `gh prune-ci-gql-obo`, done for the
//! whole fleet at once instead of repo by repo.
//!
//! - The GraphQL inventory (deployments with their latest status, releases)
//!   is fetched for `REPOS_PER_QUERY` repos per request, by giving each
//!   repo its own alias in one query (`r0: repository(...) r1: ...`).
//! - The deployment mutations are batched across repos the same way.
//! - The workflow-run listings and the release/run deletes stay on REST,
//!   since GraphQL can neither list runs nor delete releases or runs, but
//!   they share fleet-wide concurrency limits instead of one per repo.
//! - The repositories are identified from their remotes with git2, so no
//!   request is spent finding out which repo a checkout is.
//!
//! All the network work happens first; only then is the output printed,
//! per repo and in config order, through the usual runner, so headers,
//! `--terse` and `--no-output` behave as for every other command.

use std::collections::HashMap;

use anyhow::Result;
use camino::Utf8PathBuf;
use futures_util::stream::{self, StreamExt};
use serde_json::{Value, json};

use crate::commands::gh::{
    GqlDeployment, Inventory, Plan, PruneSummary, RepoInventory, RepoTally, combine_errors,
    delete_deployments, fetch_workflow_runs, github_repo_id, inventory_fields,
};
use crate::config::AppConfig;
use crate::git_url::RepoId;
use crate::github::{Github, REST_CONCURRENCY, github};
use crate::runner;
use crate::subprocess_utils::out_line;

/// Repos per inventory query. Each brings up to 100 deployments (with a
/// status each) and 100 releases, which keeps a query well inside GitHub's
/// node limit and its 10-second query timeout.
const REPOS_PER_QUERY: usize = 20;
/// Inventory queries in flight at once.
const QUERY_CONCURRENCY: usize = 4;

// ── inventory ──────────────────────────────────────────────────────────

/// One query with an aliased `repository` per entry (`r<i>`), asking each
/// only for the connections it still has pages of. Owner, name and cursors
/// are variables (`$o<i>`, `$n<i>`, `$d<i>`, `$r<i>`), never query text.
fn batch_inventory_query(entries: &[(usize, &RepoId, &Inventory)]) -> (String, Value) {
    let mut vars = Vec::new();
    let mut body = String::new();
    let mut variables = serde_json::Map::new();
    for &(i, id, inv) in entries {
        vars.push(format!("$o{i}:String!"));
        vars.push(format!("$n{i}:String!"));
        variables.insert(format!("o{i}"), json!(id.owner));
        variables.insert(format!("n{i}"), json!(id.name));
        if inv.dpage.wanted() {
            variables.insert(format!("d{i}"), inv.dpage.cursor());
        }
        if inv.rpage.wanted() {
            variables.insert(format!("r{i}"), inv.rpage.cursor());
        }
        let fields = inventory_fields(&i.to_string(), &inv.dpage, &inv.rpage, &mut vars);
        body.push_str(&format!(
            "r{i}:repository(owner:$o{i},name:$n{i}){{{fields}}} "
        ));
    }
    (
        format!("query({}){{{}}}", vars.join(","), body.trim_end()),
        Value::Object(variables),
    )
}

/// The inventory of every repo in `ids`, one result per repo, in order.
/// GitHub answers a batch with data for the repos that resolved and an
/// error per repo that did not, so a bad repo fails alone and the rest
/// carry on.
async fn fetch_batch(gh: &Github, ids: &[&RepoId]) -> Vec<Result<Inventory>> {
    let mut invs: Vec<Inventory> = ids.iter().map(|_| Inventory::new()).collect();
    let mut errors: Vec<Option<String>> = vec![None; ids.len()];
    loop {
        let active: Vec<usize> = (0..ids.len())
            .filter(|&i| errors[i].is_none() && invs[i].pending())
            .collect();
        if active.is_empty() {
            break;
        }
        let entries: Vec<(usize, &RepoId, &Inventory)> =
            active.iter().map(|&i| (i, ids[i], &invs[i])).collect();
        let (query, variables) = batch_inventory_query(&entries);
        let response = match gh
            .graphql::<HashMap<String, Option<RepoInventory>>>(&query, variables)
            .await
        {
            Ok(r) => r,
            Err(e) => {
                let msg = format!("{e:#}");
                for &i in &active {
                    errors[i] = Some(msg.clone());
                }
                break;
            }
        };
        // An error that names one of the repos fails that repo; one that
        // names none of them fails the whole batch.
        let mut named_any = false;
        for e in &response.errors {
            if let Some(i) = e
                .alias()
                .and_then(|a| a.strip_prefix('r'))
                .and_then(|n| n.parse::<usize>().ok())
                .filter(|i| active.contains(i))
            {
                errors[i] = Some(e.message.clone());
                named_any = true;
            }
        }
        if let Some(e) = response.errors.first()
            && !named_any
        {
            for &i in &active {
                errors[i] = Some(e.message.clone());
            }
            break;
        }
        let mut data = response.data.unwrap_or_default();
        for &i in &active {
            if errors[i].is_some() {
                continue;
            }
            let result = match data.remove(&format!("r{i}")).flatten() {
                Some(page) => invs[i].absorb(page),
                None => Err(anyhow::anyhow!("GraphQL query returned no repository")),
            };
            if let Err(e) = result {
                errors[i] = Some(format!("{e:#}"));
            }
        }
    }
    invs.into_iter()
        .zip(errors)
        .map(|(inv, error)| match error {
            Some(e) => Err(anyhow::anyhow!("listing deployments and releases: {e}")),
            None => Ok(inv),
        })
        .collect()
}

// ── the command ────────────────────────────────────────────────────────

/// One GitHub repo of the fleet, as it moves through the phases.
struct FleetRepo {
    /// Absolute checkout path: the runner's key.
    path: Utf8PathBuf,
    id: RepoId,
    inventory: Inventory,
    runs: Vec<(u64, bool)>,
    errors: Vec<String>,
}

/// What gets printed for a repo once all the work is done.
struct Outcome {
    lines: Vec<String>,
    error: Option<String>,
}

/// `gh prune-ci-gql-obo` for every repo in `projects` at once; see the
/// module documentation. Every repo's work is done before anything is
/// printed, so every failure is reported, under its repo, whether or not
/// `--no-stop` was given, and the exit status is non-zero if any repo
/// failed. Every repo's counts go into `summary`.
pub fn prune_ci_gql_batch(
    config: &AppConfig,
    projects: &[Utf8PathBuf],
    keep: usize,
    summary: &PruneSummary,
) -> Result<()> {
    let base = runner::current_dir()?;

    // Phase 1: which repos are on GitHub, and as which owner/name. Local
    // git2 reads only.
    let mut outcomes: HashMap<Utf8PathBuf, Outcome> = HashMap::new();
    let mut repos: Vec<FleetRepo> = Vec::new();
    for project in projects {
        let path = runner::absolute(project, &base);
        match github_repo_id(&path) {
            Ok(None) => {}
            Ok(Some(id)) => repos.push(FleetRepo {
                path,
                id,
                inventory: Inventory::new(),
                runs: Vec::new(),
                errors: Vec::new(),
            }),
            Err(e) => {
                summary.record(&RepoTally::default(), false);
                outcomes.insert(
                    path,
                    Outcome {
                        lines: Vec::new(),
                        error: Some(format!("{e:#}")),
                    },
                );
            }
        }
    }

    if !repos.is_empty() {
        let gh = github()?;
        gh.block_on(prune_fleet(gh, &mut repos, keep, &mut outcomes, summary));
    }

    // Print through the runner, which owns headers, --terse, --no-output
    // and the error report. The work is all done by now, so stopping at
    // the first failing repo would only hide the rest: show every repo,
    // and still exit non-zero if any failed.
    let mut render = config.clone();
    render.no_stop = true;
    runner::do_for_all_projects_with_check(
        &render,
        projects,
        |p| Ok(outcomes.contains_key(p)),
        |p| {
            let outcome = &outcomes[p];
            for line in &outcome.lines {
                out_line(line);
            }
            match &outcome.error {
                Some(e) => Err(anyhow::anyhow!("{e}")),
                None => Ok(()),
            }
        },
    )
}

/// Phases 2 to 5 for the repos on GitHub: list, decide, delete, report.
async fn prune_fleet(
    gh: &Github,
    repos: &mut [FleetRepo],
    keep: usize,
    outcomes: &mut HashMap<Utf8PathBuf, Outcome>,
    summary: &PruneSummary,
) {
    // Phase 2: list everything. The batched GraphQL inventory and the
    // per-repo REST run listings run side by side.
    let ids: Vec<&RepoId> = repos.iter().map(|r| &r.id).collect();
    let inventories = stream::iter(ids.chunks(REPOS_PER_QUERY))
        .map(|batch| fetch_batch(gh, batch))
        .buffered(QUERY_CONCURRENCY)
        .collect::<Vec<_>>();
    let runs = stream::iter(&ids)
        .map(|id| fetch_workflow_runs(gh, id))
        .buffered(REST_CONCURRENCY)
        .collect::<Vec<_>>();
    let (inventories, runs) = futures_util::join!(inventories, runs);
    for ((repo, inventory), runs) in repos
        .iter_mut()
        .zip(inventories.into_iter().flatten())
        .zip(runs)
    {
        match inventory {
            Ok(inv) => repo.inventory = inv,
            Err(e) => repo.errors.push(format!("{e:#}")),
        }
        match runs {
            Ok(r) => repo.runs = r,
            Err(e) => repo.errors.push(format!("listing workflow runs: {e:#}")),
        }
    }

    // Phase 3: decide. A repo whose listing failed deletes nothing, rather
    // than acting on half a picture.
    let plans: Vec<Plan> = repos
        .iter()
        .map(|r| {
            if r.errors.is_empty() {
                Plan::new(&r.inventory, &r.runs, keep)
            } else {
                Plan {
                    deployments: Vec::new(),
                    releases: Vec::new(),
                    runs: Vec::new(),
                }
            }
        })
        .collect();

    // Phase 4: delete. Deployment mutations batched across the fleet,
    // alongside the REST deletes of every release and run.
    let to_delete: Vec<&GqlDeployment> = repos
        .iter()
        .zip(&plans)
        .flat_map(|(r, p)| p.deployments.iter().map(|&i| &r.inventory.deployments[i]))
        .collect();
    let routes: Vec<String> = repos
        .iter()
        .zip(&plans)
        .flat_map(|(r, p)| p.rest_routes(&r.id))
        .collect();
    let (deployment_results, rest_results) =
        futures_util::join!(delete_deployments(gh, &to_delete), gh.delete_all(&routes));

    // Phase 5: hand each repo its results back, in the order it planned
    // them (the jobs were laid out repo by repo, so a cursor per list
    // walks them in step).
    let mut deployment_results = deployment_results.into_iter();
    let mut rest_results = rest_results.into_iter();
    for (repo, plan) in repos.iter_mut().zip(&plans) {
        let dres: Vec<Result<()>> = deployment_results
            .by_ref()
            .take(plan.deployments.len())
            .collect();
        let rres: Vec<Result<()>> = rest_results
            .by_ref()
            .take(plan.releases.len() + plan.runs.len())
            .collect();
        let mut errors = std::mem::take(&mut repo.errors);
        let (lines, tally) = if errors.is_empty() {
            let (lines, failures, tally) =
                plan.report(&repo.inventory, repo.runs.len(), &dres, &rres);
            errors.extend(failures);
            (lines, tally)
        } else {
            (Vec::new(), RepoTally::default())
        };
        summary.record(&tally, errors.is_empty());
        outcomes.insert(
            repo.path.clone(),
            Outcome {
                lines,
                error: combine_errors(errors).err().map(|e| format!("{e:#}")),
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::gh::Page;

    fn id(o: &str, n: &str) -> RepoId {
        RepoId {
            owner: o.to_string(),
            name: n.to_string(),
        }
    }

    #[test]
    fn batch_query_aliases_each_repo_and_skips_finished_connections() {
        let a = id("o", "a");
        let b = id("o\"", "b");
        let first = Inventory::new();
        let mut later = Inventory::new();
        later.dpage = Page::Done;
        later.rpage = Page::After("Y3Vy==".into());
        let (q, vars) = batch_inventory_query(&[(0, &a, &first), (3, &b, &later)]);
        assert!(
            q.starts_with(
                "query($o0:String!,$n0:String!,$d0:String,$r0:String,$o3:String!,$n3:String!,$r3:String)"
            ),
            "{q}"
        );
        assert!(
            q.contains("r0:repository(owner:$o0,name:$n0){deployments(first:100,after:$d0,"),
            "{q}"
        );
        assert!(
            q.contains("r3:repository(owner:$o3,name:$n3){releases(first:100,after:$r3,"),
            "{q}"
        );
        assert_eq!(q.matches("deployments(").count(), 1, "{q}");
        assert_eq!(q.matches("releases(").count(), 2, "{q}");
        // Names and cursors are variables, so even a hostile one stays data.
        assert_eq!(
            vars,
            json!({
                "o0": "o", "n0": "a", "d0": null, "r0": null,
                "o3": "o\"", "n3": "b", "r3": "Y3Vy==",
            })
        );
    }
}
