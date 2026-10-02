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
pub fn clean_all(project: &Utf8Path, keep: usize) -> Result<()> {
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
fn api_lines(project: &Utf8Path, endpoint: &str, jq: &str) -> Result<Vec<String>> {
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
