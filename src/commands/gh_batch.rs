//! `gh prune-ci-gql-batch`: the work of `gh prune-ci-gql-obo`, done for the
//! whole fleet at once instead of repo by repo.
//!
//! - The GraphQL inventory (deployments with their latest status, releases)
//!   is fetched for `REPOS_PER_QUERY` repos per request, by giving each
//!   repo its own alias in one query (`r0: repository(...) r1: ...`).
//! - The deployment mutations are batched across repos the same way.
//! - The workflow-run listings and the release/run deletes stay on REST,
//!   since GraphQL can neither list runs nor delete releases or runs, but
//!   they share fleet-wide thread pools instead of a pool per repo.
//! - The repositories are identified from their remotes with git2, so no
//!   `gh` process is spent finding out which repo a checkout is.
//!
//! All the network work happens first; only then is the output printed,
//! per repo and in config order, through the usual runner, so headers,
//! `--terse` and `--no-output` behave as for every other command.

use std::collections::HashMap;

use anyhow::{Context, Result};
use camino::{Utf8Path, Utf8PathBuf};

use crate::commands::gh::{
    DEPLOYMENTS_PER_MUTATION, GRAPHQL_PAGE_SIZE, GqlDeployment, Page, REST_DELETE_THREADS,
    check_github, combine_errors, fetch_workflow_runs_of, graphql_errors, is_safe_node_id,
    parallel_map, parse_inventory_page, report_section, rest_delete_parallel, run_deployment_batch,
    select_deletions,
};
use crate::config::AppConfig;
use crate::runner;
use crate::subprocess_utils::{capture_output_allow_failure, out_line};

/// Repos per inventory query. Each brings up to 100 deployments (with a
/// status each) and 100 releases, which keeps a query well inside GitHub's
/// node limit and its 10-second query timeout.
const REPOS_PER_QUERY: usize = 20;
/// Inventory queries in flight at once.
const QUERY_THREADS: usize = 4;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RepoId {
    pub(crate) owner: String,
    pub(crate) name: String,
}

impl RepoId {
    fn slug(&self) -> String {
        format!("{}/{}", self.owner, self.name)
    }
}

/// GitHub owner and repo names are made of these; anything else is not a
/// repo name, and would not be safe inside the query's string literals.
fn is_safe_name(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

fn parse_slug(slug: &str) -> Option<RepoId> {
    let slug = slug.trim_end_matches('/');
    let slug = slug.strip_suffix(".git").unwrap_or(slug);
    let (owner, name) = slug.split_once('/')?;
    (is_safe_name(owner) && is_safe_name(name)).then(|| RepoId {
        owner: owner.to_string(),
        name: name.to_string(),
    })
}

/// owner/name from a github.com remote URL: `git@github.com:o/n.git`,
/// `https://github.com/o/n`, `ssh://git@github.com/o/n.git` and the like.
fn parse_github_url(url: &str) -> Option<RepoId> {
    let path = if let Some(rest) = url.strip_prefix("git@github.com:") {
        rest
    } else {
        let (_, after_scheme) = url.split_once("://")?;
        let (authority, path) = after_scheme.split_once('/')?;
        let host = authority.rsplit('@').next()?;
        let host = host.split(':').next()?;
        if !host.eq_ignore_ascii_case("github.com") && !host.eq_ignore_ascii_case("www.github.com")
        {
            return None;
        }
        path
    };
    parse_slug(path)
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

/// The GitHub repository `gh` would act on in this checkout: the one set by
/// `gh repo set-default` (`remote.<name>.gh-resolved`), else the github.com
/// remote gh prefers by name (upstream, github, origin, then the rest).
pub(crate) fn github_repo_id(project: &Utf8Path) -> Result<Option<RepoId>> {
    let repo = crate::commands::count::open_repo(project)?;
    let config = repo
        .config()
        .with_context(|| format!("failed to read the git config of {project}"))?;
    let remotes = repo
        .remotes()
        .with_context(|| format!("failed to list remotes for {project}"))?;
    let mut best: Option<(u8, RepoId)> = None;
    for name in remotes.iter() {
        let Ok(Some(name)) = name else {
            continue;
        };
        let Ok(remote) = repo.find_remote(name) else {
            continue;
        };
        let Ok(url) = remote.url() else {
            continue;
        };
        let Some(id) = parse_github_url(url) else {
            continue;
        };
        let rank = match config.get_string(&format!("remote.{name}.gh-resolved")) {
            // `base` marks this remote; a slug names the repo outright.
            Ok(v) if v == "base" => 4,
            Ok(v) => match parse_slug(&v) {
                Some(id) => return Ok(Some(id)),
                None => remote_rank(name),
            },
            Err(_) => remote_rank(name),
        };
        if best.as_ref().is_none_or(|(r, _)| rank > *r) {
            best = Some((rank, id));
        }
    }
    Ok(best.map(|(_, id)| id))
}

// ── inventory ──────────────────────────────────────────────────────────

/// Pagination state of one repo in a batch.
struct RepoPages {
    dpage: Page,
    rpage: Page,
    deployments: Vec<GqlDeployment>,
    releases: Vec<u64>,
    error: Option<String>,
}

impl RepoPages {
    fn pending(&self) -> bool {
        self.error.is_none() && (self.dpage.wanted() || self.rpage.wanted())
    }
}

/// Like `INVENTORY_JQ`, with every line prefixed by its repo's alias.
const BATCH_INVENTORY_JQ: &str = r#".data | to_entries[] | .key as $k | .value // empty
  | (.deployments // empty
      | (.nodes[] | "\($k) D \(.id) \(.databaseId) \(.state) \(.latestStatus.state // "-")"),
        "\($k) DP \(.pageInfo.hasNextPage) \(.pageInfo.endCursor // "-")"),
    (.releases // empty
      | (.nodes[] | "\($k) R \(.databaseId)"),
        "\($k) RP \(.pageInfo.hasNextPage) \(.pageInfo.endCursor // "-")")"#;

fn after_arg(page: &Page) -> Result<String> {
    match page.cursor() {
        None => Ok(String::new()),
        Some(c) if is_safe_node_id(c) => Ok(format!("after:\"{c}\",")),
        Some(c) => anyhow::bail!("unexpected pagination cursor {c:?}"),
    }
}

/// One query with an aliased `repository` per entry, asking each only for
/// the connections it still has pages of. Names and cursors are checked
/// before they are put into the query text.
fn batch_inventory_query(entries: &[(String, &RepoId, &Page, &Page)]) -> Result<String> {
    let mut q = String::from("query{");
    for (alias, id, dpage, rpage) in entries {
        if !is_safe_name(&id.owner) || !is_safe_name(&id.name) {
            anyhow::bail!("unexpected repository name {:?}", id.slug());
        }
        q.push_str(&format!(
            "{alias}:repository(owner:\"{}\",name:\"{}\"){{",
            id.owner, id.name
        ));
        if dpage.wanted() {
            q.push_str(&format!(
                "deployments(first:{GRAPHQL_PAGE_SIZE},{}orderBy:{{field:CREATED_AT,direction:DESC}})\
                 {{pageInfo{{hasNextPage endCursor}} nodes{{id databaseId state latestStatus{{state}}}}}} ",
                after_arg(dpage)?
            ));
        }
        if rpage.wanted() {
            q.push_str(&format!(
                "releases(first:{GRAPHQL_PAGE_SIZE},{}orderBy:{{field:CREATED_AT,direction:DESC}})\
                 {{pageInfo{{hasNextPage endCursor}} nodes{{databaseId}}}} ",
                after_arg(rpage)?
            ));
        }
        q.push_str("} ");
    }
    q.push('}');
    Ok(q)
}

/// Split the jq output by alias.
fn group_by_alias(out: &str) -> HashMap<&str, String> {
    let mut groups: HashMap<&str, String> = HashMap::new();
    for line in out.lines().map(str::trim).filter(|l| !l.is_empty()) {
        let (alias, rest) = line.split_once(' ').unwrap_or((line, ""));
        let g = groups.entry(alias).or_default();
        g.push_str(rest);
        g.push('\n');
    }
    groups
}

/// The inventory of every repo in `ids`, one result per repo, in order.
/// A repo GitHub cannot resolve fails alone: its alias is dropped and the
/// query is re-sent for the others.
fn fetch_batch(cwd: &Utf8Path, ids: &[&RepoId]) -> Vec<Result<(Vec<GqlDeployment>, Vec<u64>)>> {
    let mut states: Vec<RepoPages> = ids
        .iter()
        .map(|_| RepoPages {
            dpage: Page::First,
            rpage: Page::First,
            deployments: Vec::new(),
            releases: Vec::new(),
            error: None,
        })
        .collect();
    loop {
        let active: Vec<usize> = (0..ids.len()).filter(|&i| states[i].pending()).collect();
        if active.is_empty() {
            break;
        }
        let entries: Vec<(String, &RepoId, &Page, &Page)> = active
            .iter()
            .map(|&i| (format!("r{i}"), ids[i], &states[i].dpage, &states[i].rpage))
            .collect();
        let fail_all = |states: &mut [RepoPages], msg: &str| {
            for &i in &active {
                states[i].error = Some(msg.to_string());
            }
        };
        let query = match batch_inventory_query(&entries) {
            Ok(q) => q,
            Err(e) => {
                fail_all(&mut states, &format!("{e:#}"));
                break;
            }
        };
        let result = capture_output_allow_failure(
            cwd,
            "gh",
            &[
                "api",
                "graphql",
                "-f",
                &format!("query={query}"),
                "--jq",
                BATCH_INVENTORY_JQ,
            ],
        );
        let (code, stdout, stderr) = match result {
            Ok(r) => r,
            Err(e) => {
                fail_all(&mut states, &format!("{e:#}"));
                break;
            }
        };
        if code != 0 {
            // Drop the repos the errors name and try again without them;
            // an error that names none of them sinks the whole batch.
            let errors = graphql_errors(&stdout).unwrap_or_default();
            let mut named_any = false;
            for (alias, msg) in &errors {
                if let Some(i) = alias
                    .as_deref()
                    .and_then(|a| a.strip_prefix('r'))
                    .and_then(|n| n.parse::<usize>().ok())
                    .filter(|i| active.contains(i))
                {
                    states[i].error = Some(msg.clone());
                    named_any = true;
                }
            }
            if !named_any {
                fail_all(
                    &mut states,
                    &format!("gh api graphql failed: {}", stderr.trim()),
                );
            }
            continue;
        }
        let groups = group_by_alias(&stdout);
        for &i in &active {
            let alias = format!("r{i}");
            let text = groups.get(alias.as_str()).map(String::as_str).unwrap_or("");
            let st = &mut states[i];
            let page = match parse_inventory_page(text) {
                Ok(p) => p,
                Err(e) => {
                    st.error = Some(format!("{e:#}"));
                    continue;
                }
            };
            if st.dpage.wanted() {
                match page.deployments_next {
                    Some(next) => st.dpage = next,
                    None => {
                        st.error = Some("GraphQL query returned no deployments".to_string());
                        continue;
                    }
                }
                st.deployments.extend(page.deployments);
            }
            if st.rpage.wanted() {
                match page.releases_next {
                    Some(next) => st.rpage = next,
                    None => {
                        st.error = Some("GraphQL query returned no releases".to_string());
                        continue;
                    }
                }
                st.releases.extend(page.releases);
            }
        }
    }
    states
        .into_iter()
        .map(|st| match st.error {
            Some(e) => Err(anyhow::anyhow!("listing deployments and releases: {e}")),
            None => Ok((st.deployments, st.releases)),
        })
        .collect()
}

// ── the command ────────────────────────────────────────────────────────

/// One GitHub repo of the fleet, as it moves through the phases.
struct FleetRepo {
    /// Absolute checkout path: where `gh` runs, and the runner's key.
    path: Utf8PathBuf,
    id: RepoId,
    deployments: Vec<GqlDeployment>,
    releases: Vec<u64>,
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
/// failed.
pub fn prune_ci_gql_batch(config: &AppConfig, projects: &[Utf8PathBuf], keep: usize) -> Result<()> {
    let base = runner::current_dir()?;
    let paths: Vec<Utf8PathBuf> = projects
        .iter()
        .map(|p| runner::absolute(p, &base))
        .collect();

    // Phase 1: which repos are on GitHub, and as which owner/name. Local
    // git2 reads only.
    let identified = parallel_map(&paths, REST_DELETE_THREADS, |path| {
        if !check_github(path)? {
            return Ok(None);
        }
        github_repo_id(path)?.map(Some).with_context(|| {
            format!("cannot tell the GitHub repository of {path} from its remotes")
        })
    });
    let mut outcomes: HashMap<Utf8PathBuf, Outcome> = HashMap::new();
    let mut repos: Vec<FleetRepo> = Vec::new();
    for (path, id) in paths.iter().zip(identified) {
        match id {
            Ok(None) => {}
            Ok(Some(id)) => repos.push(FleetRepo {
                path: path.clone(),
                id,
                deployments: Vec::new(),
                releases: Vec::new(),
                runs: Vec::new(),
                errors: Vec::new(),
            }),
            Err(e) => {
                outcomes.insert(
                    path.clone(),
                    Outcome {
                        lines: Vec::new(),
                        error: Some(format!("{e:#}")),
                    },
                );
            }
        }
    }

    // Phase 2: list everything. The batched GraphQL inventory and the
    // per-repo REST run listings run side by side.
    let ids: Vec<&RepoId> = repos.iter().map(|r| &r.id).collect();
    let batches: Vec<&[&RepoId]> = ids.chunks(REPOS_PER_QUERY).collect();
    let (inventories, runs) = std::thread::scope(|scope| {
        let inv = scope.spawn(|| {
            parallel_map(&batches, QUERY_THREADS, |batch| fetch_batch(&base, batch))
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
        });
        let runs = parallel_map(&repos, REST_DELETE_THREADS, |r| {
            fetch_workflow_runs_of(&r.path, &r.id.slug())
        });
        (inv.join().expect("inventory listing panicked"), runs)
    });
    for ((repo, inventory), runs) in repos.iter_mut().zip(inventories).zip(runs) {
        match inventory {
            Ok((d, r)) => {
                repo.deployments = d;
                repo.releases = r;
            }
            Err(e) => repo.errors.push(format!("{e:#}")),
        }
        match runs {
            Ok(r) => repo.runs = r,
            Err(e) => repo.errors.push(format!("listing workflow runs: {e:#}")),
        }
    }

    // Phase 3: decide. A repo whose listing failed deletes nothing, rather
    // than acting on half a picture.
    struct Plan {
        deployments: Vec<usize>,
        releases: Vec<u64>,
        runs: Vec<u64>,
    }
    let plans: Vec<Plan> = repos
        .iter()
        .map(|r| {
            if !r.errors.is_empty() {
                return Plan {
                    deployments: Vec::new(),
                    releases: Vec::new(),
                    runs: Vec::new(),
                };
            }
            let items: Vec<(u64, bool)> = r
                .deployments
                .iter()
                .map(|d| (d.database_id, d.failed))
                .collect();
            let ids = select_deletions(&items, keep);
            Plan {
                deployments: (0..r.deployments.len())
                    .filter(|&i| ids.contains(&r.deployments[i].database_id))
                    .collect(),
                releases: r.releases.iter().skip(keep).copied().collect(),
                runs: select_deletions(&r.runs, keep),
            }
        })
        .collect();

    // Phase 4: delete. Deployment mutations batched across the fleet,
    // alongside one REST pool for every release and run.
    let repos_ref = &repos;
    let deployment_jobs: Vec<(usize, &GqlDeployment)> = plans
        .iter()
        .enumerate()
        .flat_map(|(ri, p)| {
            p.deployments
                .iter()
                .map(move |&di| (ri, &repos_ref[ri].deployments[di]))
        })
        .collect();
    let mut rest_jobs: Vec<(usize, String)> = Vec::new();
    for (ri, p) in plans.iter().enumerate() {
        let slug = repos[ri].id.slug();
        for id in &p.releases {
            rest_jobs.push((ri, format!("repos/{slug}/releases/{id}")));
        }
        for id in &p.runs {
            rest_jobs.push((ri, format!("repos/{slug}/actions/runs/{id}")));
        }
    }
    let rest_paths: Vec<String> = rest_jobs.iter().map(|(_, p)| p.clone()).collect();
    let (deployment_results, rest_results) = std::thread::scope(|scope| {
        let d = scope.spawn(|| {
            deployment_jobs
                .chunks(DEPLOYMENTS_PER_MUTATION)
                .flat_map(|chunk| {
                    let batch: Vec<&GqlDeployment> = chunk.iter().map(|(_, d)| *d).collect();
                    run_deployment_batch(&base, &batch)
                })
                .collect::<Vec<_>>()
        });
        let rest = rest_delete_parallel(&base, &rest_paths);
        (d.join().expect("deployment deletion panicked"), rest)
    });

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
        let rres: Vec<Result<()>> = rest_results.by_ref().take(plan.releases.len()).collect();
        let runres: Vec<Result<()>> = rest_results.by_ref().take(plan.runs.len()).collect();
        let mut lines = Vec::new();
        let mut errors = std::mem::take(&mut repo.errors);
        if errors.is_empty() {
            let dids: Vec<u64> = plan
                .deployments
                .iter()
                .map(|&i| repo.deployments[i].database_id)
                .collect();
            report_section(
                "deployments",
                repo.deployments.len(),
                &dids,
                &dres,
                &mut lines,
                &mut errors,
            );
            report_section(
                "releases",
                repo.releases.len(),
                &plan.releases,
                &rres,
                &mut lines,
                &mut errors,
            );
            report_section(
                "workflow runs",
                repo.runs.len(),
                &plan.runs,
                &runres,
                &mut lines,
                &mut errors,
            );
        }
        outcomes.insert(
            repo.path.clone(),
            Outcome {
                lines,
                error: combine_errors(errors).err().map(|e| format!("{e:#}")),
            },
        );
    }

    // Phase 6: print through the runner, which owns headers, --terse,
    // --no-output and the error report. The work is all done by now, so
    // stopping at the first failing repo would only hide the rest: show
    // every repo, and still exit non-zero if any failed.
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

#[cfg(test)]
mod tests {
    use super::*;

    fn id(o: &str, n: &str) -> RepoId {
        RepoId {
            owner: o.to_string(),
            name: n.to_string(),
        }
    }

    #[test]
    fn parse_github_url_forms() {
        for url in [
            "git@github.com:veltzer/rsmultigit.git",
            "git@github.com:veltzer/rsmultigit",
            "https://github.com/veltzer/rsmultigit",
            "https://github.com/veltzer/rsmultigit.git",
            "https://github.com/veltzer/rsmultigit/",
            "https://user@github.com/veltzer/rsmultigit.git",
            "ssh://git@github.com/veltzer/rsmultigit.git",
            "ssh://git@github.com:22/veltzer/rsmultigit.git",
            "git://github.com/veltzer/rsmultigit.git",
        ] {
            assert_eq!(
                parse_github_url(url),
                Some(id("veltzer", "rsmultigit")),
                "{url}"
            );
        }
    }

    #[test]
    fn parse_github_url_rejects_others() {
        for url in [
            "git@gitlab.com:veltzer/rsmultigit.git",
            "https://gitlab.com/veltzer/rsmultigit",
            "https://github.com/veltzer",
            "https://github.com/veltzer/rsmultigit/tree/master",
            "/home/mark/git/rsmultigit",
            "https://github.com/ve\"ltzer/x",
        ] {
            assert_eq!(parse_github_url(url), None, "{url}");
        }
    }

    #[test]
    fn remote_rank_follows_gh() {
        assert!(remote_rank("upstream") > remote_rank("github"));
        assert!(remote_rank("github") > remote_rank("origin"));
        assert!(remote_rank("origin") > remote_rank("mirror"));
    }

    #[test]
    fn batch_query_aliases_each_repo_and_skips_finished_connections() {
        let a = id("o", "a");
        let b = id("o", "b");
        let q = batch_inventory_query(&[
            ("r0".to_string(), &a, &Page::First, &Page::First),
            (
                "r1".to_string(),
                &b,
                &Page::Done,
                &Page::After("Y3Vy==".into()),
            ),
        ])
        .unwrap();
        assert!(
            q.starts_with(
                "query{r0:repository(owner:\"o\",name:\"a\"){deployments(first:100,orderBy"
            ),
            "{q}"
        );
        assert!(
            q.contains(
                "r1:repository(owner:\"o\",name:\"b\"){releases(first:100,after:\"Y3Vy==\",orderBy"
            ),
            "{q}"
        );
        assert_eq!(q.matches("deployments(").count(), 1, "{q}");
        assert_eq!(q.matches("releases(").count(), 2, "{q}");
    }

    #[test]
    fn batch_query_rejects_unsafe_input() {
        let bad = id("o\"", "a");
        assert!(batch_inventory_query(&[("r0".into(), &bad, &Page::First, &Page::First)]).is_err());
        let good = id("o", "a");
        let cursor = Page::After("x\"}".into());
        assert!(batch_inventory_query(&[("r0".into(), &good, &cursor, &Page::Done)]).is_err());
    }

    #[test]
    fn group_by_alias_splits_lines() {
        let out = "r0 D DE_a 1 ACTIVE SUCCESS\nr1 R 5\nr0 DP false -\n";
        let g = group_by_alias(out);
        assert_eq!(g["r0"], "D DE_a 1 ACTIVE SUCCESS\nDP false -\n");
        assert_eq!(g["r1"], "R 5\n");
        let page = parse_inventory_page(&g["r0"]).unwrap();
        assert_eq!(page.deployments.len(), 1);
        assert_eq!(page.deployments_next, Some(Page::Done));
    }
}
