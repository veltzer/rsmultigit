//! Drivers for `check-same`, `check-exists` and `check-all`: rule selection,
//! reporting, exit codes, and the interactive `--diff` / `--copy` /
//! `--fix-missing` flows.
//!
//! Everything here writes to a `Write` and reads prompts from a `BufRead`,
//! so the tests drive whole commands with in-memory buffers. Rule
//! evaluation itself lives in [`check`]; this module decides which rules
//! run and what is said about the results.

use std::collections::HashSet;
use std::io::{BufRead, Write};

use anyhow::{Context, Result};
use camino::{Utf8Path, Utf8PathBuf};

use crate::commands::check::{self, CheckConfig, ExistsRule, Rule, RuleResult};
use crate::commands::interactive::{Choice, confirm, group_label, pick_group};
use crate::config::AppConfig;

/// What `--checks` / `--checks-re` resolution needs to know about a rule.
pub trait NamedRule {
    fn name(&self) -> &str;
    fn enabled(&self) -> bool;
}

impl NamedRule for Rule {
    fn name(&self) -> &str {
        &self.name
    }
    fn enabled(&self) -> bool {
        self.enabled
    }
}

impl NamedRule for ExistsRule {
    fn name(&self) -> &str {
        &self.name
    }
    fn enabled(&self) -> bool {
        self.enabled
    }
}

/// Decide which of `rules` to run.
///
/// Both lists empty → every enabled rule, in config order. Otherwise →
/// exactly the rules named in `requested` (in request order) plus, in config
/// order, every rule whose name one of the `requested_re` regexes matches
/// (unanchored search), even if they are `enabled = false`. An unknown name,
/// an invalid regex, or a regex matching no rule is a hard error; `kind`
/// names the rule type in those messages ("check", "exists rule").
pub fn select_rules<'a, R: NamedRule>(
    rules: &'a [R],
    requested: &[String],
    requested_re: &[String],
    kind: &str,
) -> Result<Vec<&'a R>> {
    if requested.is_empty() && requested_re.is_empty() {
        return Ok(rules.iter().filter(|r| r.enabled()).collect());
    }

    let known: HashSet<&str> = rules.iter().map(NamedRule::name).collect();
    let unknown: Vec<&String> = requested
        .iter()
        .filter(|name| !known.contains(name.as_str()))
        .collect();
    if !unknown.is_empty() {
        anyhow::bail!("unknown {kind} name(s): {}", quoted_list(unknown));
    }

    let mut selected: Vec<&R> = requested
        .iter()
        .filter_map(|name| rules.iter().find(|r| r.name() == name))
        .collect();
    for pattern in requested_re {
        let re = regex_lite::Regex::new(pattern)
            .with_context(|| format!("invalid {kind} regex {pattern:?}"))?;
        let mut matched_any = false;
        for rule in rules {
            if re.is_match(rule.name()) {
                matched_any = true;
                if !selected.iter().any(|r| r.name() == rule.name()) {
                    selected.push(rule);
                }
            }
        }
        if !matched_any {
            anyhow::bail!("{kind} regex {pattern:?} matches no {kind} name");
        }
    }
    Ok(selected)
}

/// `"a", "b", "c"` for error messages.
fn quoted_list<S: std::fmt::Debug>(items: impl IntoIterator<Item = S>) -> String {
    items
        .into_iter()
        .map(|s| format!("{s:?}"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Print the `[name]` line that introduces a rule's report, unless
/// `--no-header` suppressed it.
fn rule_header<W: Write>(app: &AppConfig, out: &mut W, name: &str) -> Result<()> {
    if !app.no_header {
        writeln!(out, "[{name}]")?;
    }
    Ok(())
}

/// The end-of-run hard error for rules that matched nothing. The inline
/// report lines scroll away in a long run, so the run finishes with an
/// error on stderr, the last thing on screen, naming every empty rule. It
/// takes precedence over the exit-0 modes (`--copy`, `--fix-missing`): an
/// empty rule is a config bug, not drift those modes could fix.
fn fail_on_empty_rules(command: &str, what: &str, hint: &str, empty: &[String]) -> Result<()> {
    if empty.is_empty() {
        return Ok(());
    }
    let noun = if empty.len() == 1 { "rule" } else { "rules" };
    anyhow::bail!(
        "{command}: {} {noun} {what}: {} ({hint} pass --allow-empty to accept)",
        empty.len(),
        quoted_list(empty),
    )
}

/// The check-same flag set, bundled to keep `run_check_same`'s signature small.
pub struct CheckSameOpts<'a> {
    pub requested: &'a [String],
    pub requested_re: &'a [String],
    pub only_failed: bool,
    pub show_diff: bool,
    pub do_copy: bool,
    pub allow_empty: bool,
    pub do_fix_missing: bool,
}

/// Run the check-same command. Returns the process exit code.
///
/// Rule selection is [`select_rules`]. Passing rules print an `ok (N files)`
/// line by default; `only_failed` (and `--terse`, whose output is a
/// machine-readable list of failing rule names) restrict the output to
/// failing rules.
///
/// A rule that matches no files at all fails ("no files matched") unless
/// `allow_empty` is set, in which case it passes as `ok (0 files)`; see
/// [`fail_on_empty_rules`] for the end-of-run error.
///
/// When `do_copy` or `do_fix_missing` is set, interactive prompts are served
/// from `input`, and the exit code is 0 regardless of mismatches: these are
/// tools to fix drift, not pass/fail checks.
///
/// With the global `--short-circuit` flag, evaluation stops as soon as the
/// first rule is found broken: the remaining rules are neither evaluated nor
/// reported.
pub fn run_check_same<R: BufRead, W: Write>(
    app: &AppConfig,
    file_config: &CheckConfig,
    projects: &[Utf8PathBuf],
    opts: &CheckSameOpts<'_>,
    input: &mut R,
    out: &mut W,
) -> Result<i32> {
    let &CheckSameOpts {
        requested,
        requested_re,
        only_failed,
        show_diff,
        do_copy,
        allow_empty,
        do_fix_missing,
    } = opts;

    let rules = select_rules(&file_config.check, requested, requested_re, "check")?;
    if rules.is_empty() {
        if app.verbose {
            writeln!(out, "no rules to check")?;
        }
        return Ok(0);
    }

    let mut any_mismatch = false;
    let mut empty_rules: Vec<String> = Vec::new();

    for rule in rules {
        let result = check::evaluate_rule(rule, projects)?;

        if result.matched_nothing() && !allow_empty {
            // A rule that checked nothing is almost always a stale select/path,
            // so it fails by default; --allow-empty restores the old "ok (0
            // files)" behavior.
            any_mismatch = true;
            empty_rules.push(result.name.clone());
            if app.terse {
                writeln!(out, "{}", result.name)?;
            } else {
                rule_header(app, out, &result.name)?;
                let suffix = if result.skipped.is_empty() {
                    String::new()
                } else {
                    format!(" ({} skipped)", result.skipped.len())
                };
                writeln!(out, "no files matched{suffix}")?;
            }
            if app.short_circuit {
                break;
            }
            continue;
        }

        if result.is_consistent() {
            if !only_failed && !app.terse {
                rule_header(app, out, &result.name)?;
                writeln!(out, "ok ({} files)", result.total_files)?;
            }
            continue;
        }

        any_mismatch = true;

        if app.terse {
            writeln!(out, "{}", result.name)?;
            if app.short_circuit {
                break;
            }
            continue;
        }

        rule_header(app, out, &result.name)?;
        let mut suffix_parts: Vec<String> = Vec::new();
        if !result.must_have_violations.is_empty() {
            suffix_parts.push(format!("{} missing", result.must_have_violations.len()));
        }
        if !result.skipped.is_empty() {
            suffix_parts.push(format!("{} skipped", result.skipped.len()));
        }
        let suffix = if suffix_parts.is_empty() {
            String::new()
        } else {
            format!(" ({})", suffix_parts.join(", "))
        };
        writeln!(
            out,
            "{} files, {} groups{suffix}",
            result.total_files,
            result.groups.len(),
        )?;

        if !app.no_output {
            for (i, group) in result.groups.iter().enumerate() {
                writeln!(out, "  group {} ({} files):", group_label(i), group.len())?;
                for file in group {
                    writeln!(out, "    {file}")?;
                }
            }
            if !result.must_have_violations.is_empty() {
                writeln!(out, "  missing in:")?;
                for repo in &result.must_have_violations {
                    writeln!(out, "    {repo}")?;
                }
            }

            let mut quit = false;
            if show_diff {
                quit |= run_diff(&result, &mut *input, &mut *out)? == FlowControl::Quit;
            }
            if do_copy && !quit {
                quit |= run_copy(&result, &mut *input, &mut *out)? == FlowControl::Quit;
            }
            if do_fix_missing && !quit && !result.must_have_violations.is_empty() {
                quit |= run_fix_missing(&result, &mut *input, &mut *out)? == FlowControl::Quit;
            }
            if quit {
                break;
            }
        }

        if app.short_circuit {
            break;
        }
    }

    fail_on_empty_rules(
        "check-same",
        "matched no files",
        "stale select/path?",
        &empty_rules,
    )?;

    if do_copy || do_fix_missing {
        Ok(0)
    } else {
        Ok(if any_mismatch { 1 } else { 0 })
    }
}

/// The check-exists flag set, mirroring [`CheckSameOpts`].
pub struct CheckExistsOpts<'a> {
    pub requested: &'a [String],
    pub requested_re: &'a [String],
    pub only_failed: bool,
    pub allow_empty: bool,
}

/// Run the `[[exists]]` rules: assert presence, never compare content.
///
/// Output and flag handling deliberately mirror [`run_check_same`] so the
/// two commands read the same way: `--terse` prints bare failing rule
/// names, `--only-failed` drops the `ok` lines, `--short-circuit` stops at
/// the first failure, and an empty rule is a hard error unless
/// `--allow-empty`. There are no interactive modes.
pub fn run_check_exists<W: Write>(
    app: &AppConfig,
    file_config: &CheckConfig,
    projects: &[Utf8PathBuf],
    opts: &CheckExistsOpts<'_>,
    out: &mut W,
) -> Result<i32> {
    let CheckExistsOpts {
        requested,
        requested_re,
        only_failed,
        allow_empty,
    } = *opts;

    let rules = select_rules(&file_config.exists, requested, requested_re, "exists rule")?;
    if rules.is_empty() {
        if app.verbose {
            writeln!(out, "no exists rules to check")?;
        }
        return Ok(0);
    }

    let mut any_missing = false;
    let mut empty_rules: Vec<String> = Vec::new();

    for rule in rules {
        let result = check::evaluate_exists_rule(rule, projects)?;

        if result.matched_nothing() && !allow_empty {
            any_missing = true;
            empty_rules.push(result.name.clone());
            if app.terse {
                writeln!(out, "{}", result.name)?;
            } else {
                rule_header(app, out, &result.name)?;
                writeln!(out, "no repos selected")?;
            }
            if app.short_circuit {
                break;
            }
            continue;
        }

        if result.is_satisfied() {
            if !only_failed && !app.terse {
                rule_header(app, out, &result.name)?;
                writeln!(out, "ok ({} repos)", result.total_repos())?;
            }
            continue;
        }

        any_missing = true;

        if app.terse {
            writeln!(out, "{}", result.name)?;
            if app.short_circuit {
                break;
            }
            continue;
        }

        rule_header(app, out, &result.name)?;
        writeln!(
            out,
            "{} repos, {} missing {}",
            result.total_repos(),
            result.missing.len(),
            result.path,
        )?;
        if !app.no_output {
            writeln!(out, "  missing in:")?;
            for repo in &result.missing {
                writeln!(out, "    {repo}")?;
            }
        }

        if app.short_circuit {
            break;
        }
    }

    fail_on_empty_rules(
        "check-exists",
        "selected no repos",
        "stale select?",
        &empty_rules,
    )?;

    Ok(if any_missing { 1 } else { 0 })
}

#[derive(Debug, PartialEq, Eq)]
enum FlowControl {
    Continue,
    Quit,
}

/// Run the diff flow for a rule with at least two content groups.
/// - 2 groups: auto-pair and diff, no prompting.
/// - 3+ groups: prompt for from/to, diff, then offer to diff another pair.
fn run_diff<R: BufRead, W: Write>(
    result: &RuleResult,
    reader: &mut R,
    writer: &mut W,
) -> Result<FlowControl> {
    let n = result.groups.len();
    if n == 2 {
        emit_pair_diff(result, 0, 1, writer);
        return Ok(FlowControl::Continue);
    }

    loop {
        let from = match pick_group(&mut *reader, &mut *writer, "diff from group?", n, None)? {
            Choice::Value(i) => i,
            Choice::Skip => return Ok(FlowControl::Continue),
            Choice::Quit => return Ok(FlowControl::Quit),
        };
        let to = match pick_group(&mut *reader, &mut *writer, "diff to group?", n, Some(from))? {
            Choice::Value(i) => i,
            Choice::Skip => return Ok(FlowControl::Continue),
            Choice::Quit => return Ok(FlowControl::Quit),
        };
        emit_pair_diff(result, from, to, writer);

        if !confirm(&mut *reader, &mut *writer, "diff another pair?")? {
            return Ok(FlowControl::Continue);
        }
    }
}

/// Write a unified diff between representatives of `groups[a]` and `groups[b]`
/// to `writer`. Handles I/O errors and non-UTF-8 content gracefully.
fn emit_pair_diff<W: Write>(result: &RuleResult, a_idx: usize, b_idx: usize, writer: &mut W) {
    let a = &result.groups[a_idx][0];
    let b = &result.groups[b_idx][0];

    let a_bytes = match std::fs::read(a) {
        Ok(b) => b,
        Err(e) => {
            let _ = writeln!(writer, "  (could not read {a}: {e})");
            return;
        }
    };
    let b_bytes = match std::fs::read(b) {
        Ok(b) => b,
        Err(e) => {
            let _ = writeln!(writer, "  (could not read {b}: {e})");
            return;
        }
    };
    let (a_text, b_text) = match (std::str::from_utf8(&a_bytes), std::str::from_utf8(&b_bytes)) {
        (Ok(a), Ok(b)) => (a, b),
        _ => {
            let _ = writeln!(writer, "  (binary files differ, not shown)");
            return;
        }
    };

    let diff = similar::TextDiff::from_lines(a_text, b_text);
    let _ = write!(
        writer,
        "{}",
        diff.unified_diff()
            .context_radius(3)
            .header(a.as_str(), b.as_str())
    );
}

/// Run the interactive copy flow for a rule: prompt for "from" and "to" groups,
/// confirm, then overwrite every file in the "to" group with the content of a
/// representative from the "from" group (preserving the destination's mode).
fn run_copy<R: BufRead, W: Write>(
    result: &RuleResult,
    reader: &mut R,
    writer: &mut W,
) -> Result<FlowControl> {
    let n = result.groups.len();
    let from = match pick_group(&mut *reader, &mut *writer, "copy from group?", n, None)? {
        Choice::Value(i) => i,
        Choice::Skip => return Ok(FlowControl::Continue),
        Choice::Quit => return Ok(FlowControl::Quit),
    };
    let to = match pick_group(&mut *reader, &mut *writer, "copy to group?", n, Some(from))? {
        Choice::Value(i) => i,
        Choice::Skip => return Ok(FlowControl::Continue),
        Choice::Quit => return Ok(FlowControl::Quit),
    };

    let src = &result.groups[from][0];
    let dst_group = &result.groups[to];
    let prompt = format!(
        "overwrite {} file(s) in group {} with content from {src}?",
        dst_group.len(),
        group_label(to),
    );
    if !confirm(&mut *reader, &mut *writer, &prompt)? {
        let _ = writeln!(writer, "  (skipped)");
        return Ok(FlowControl::Continue);
    }

    for dst in dst_group {
        if let Err(e) = copy_preserving_mode(src, dst) {
            let _ = writeln!(writer, "  error: {src} -> {dst}: {e}");
        } else {
            let _ = writeln!(writer, "  copied -> {dst}");
        }
    }
    Ok(FlowControl::Continue)
}

/// `fs::copy` replaces the destination's permissions with the source's. We want
/// the opposite: overwrite the *content* but keep the destination's mode.
fn copy_preserving_mode(src: &Utf8Path, dst: &Utf8Path) -> Result<()> {
    let original_mode = std::fs::metadata(dst)
        .with_context(|| format!("failed to stat {dst}"))?
        .permissions();
    std::fs::copy(src, dst).with_context(|| format!("failed to copy {src} -> {dst}"))?;
    std::fs::set_permissions(dst, original_mode)
        .with_context(|| format!("failed to restore permissions on {dst}"))?;
    Ok(())
}

/// Run the interactive fix-missing flow for a rule that has `must_have`
/// violations. Prompts for a "seed" group to copy from, then creates the file
/// in each violating repo, using plain `fs::copy` (so the new file inherits the
/// source's mode). Parent directories are created as needed.
///
/// When the rule has zero content groups (no repo has the file at all) there's
/// nothing to seed from: print a note and skip.
fn run_fix_missing<R: BufRead, W: Write>(
    result: &RuleResult,
    reader: &mut R,
    writer: &mut W,
) -> Result<FlowControl> {
    if result.groups.is_empty() {
        let _ = writeln!(
            writer,
            "  (cannot --fix-missing: no repo has {} — nothing to seed from)",
            result.path
        );
        return Ok(FlowControl::Continue);
    }

    let n = result.groups.len();
    let from = match pick_group(
        &mut *reader,
        &mut *writer,
        "seed missing files from which group?",
        n,
        None,
    )? {
        Choice::Value(i) => i,
        Choice::Skip => return Ok(FlowControl::Continue),
        Choice::Quit => return Ok(FlowControl::Quit),
    };

    let src = &result.groups[from][0];
    let violators = &result.must_have_violations;
    let prompt = format!(
        "create {} file(s) using content from {src}?",
        violators.len(),
    );
    if !confirm(&mut *reader, &mut *writer, &prompt)? {
        let _ = writeln!(writer, "  (skipped)");
        return Ok(FlowControl::Continue);
    }

    for repo in violators {
        let dst = repo.join(&result.path);
        if let Some(parent) = dst.parent()
            && let Err(e) = std::fs::create_dir_all(parent)
        {
            let _ = writeln!(writer, "  error: failed to create directory {parent}: {e}");
            continue;
        }
        match std::fs::copy(src, &dst) {
            Ok(_) => {
                let _ = writeln!(writer, "  created -> {dst}");
            }
            Err(e) => {
                let _ = writeln!(writer, "  error: {src} -> {dst}: {e}");
            }
        }
    }
    Ok(FlowControl::Continue)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Cursor;
    use tempfile::TempDir;

    fn rule(name: &str, enabled: bool) -> Rule {
        Rule {
            name: name.into(),
            select: "*".into(),
            exclude: None,
            marker: None,
            marker_absent: None,
            path: "f".into(),
            enabled,
            must_have: false,
        }
    }

    fn names<R: NamedRule>(rules: &[&R]) -> Vec<String> {
        rules.iter().map(|r| r.name().to_string()).collect()
    }

    fn strings(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn select_rules_defaults_to_enabled_in_config_order() {
        let rules = [rule("a", true), rule("b", false), rule("c", true)];
        let got = select_rules(&rules, &[], &[], "check").unwrap();
        assert_eq!(names(&got), ["a", "c"]);
    }

    #[test]
    fn select_rules_by_name_keeps_request_order_and_forces_disabled() {
        let rules = [rule("a", true), rule("b", false), rule("c", true)];
        let got = select_rules(&rules, &strings(&["c", "b"]), &[], "check").unwrap();
        assert_eq!(names(&got), ["c", "b"]);
    }

    #[test]
    fn select_rules_unknown_name_is_an_error() {
        let rules = [rule("a", true)];
        let err = select_rules(&rules, &strings(&["zz"]), &[], "check").unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("unknown check name(s)"), "{msg}");
        assert!(msg.contains("\"zz\""), "{msg}");
    }

    #[test]
    fn select_rules_regex_adds_matches_without_duplicates() {
        let rules = [rule("rs-a", true), rule("py-b", false), rule("rs-c", false)];
        let got = select_rules(&rules, &strings(&["rs-c"]), &strings(&["^rs-"]), "check").unwrap();
        // rs-c came first by name; the regex adds rs-a and does not repeat rs-c.
        assert_eq!(names(&got), ["rs-c", "rs-a"]);
    }

    #[test]
    fn select_rules_regex_errors() {
        let rules = [rule("a", true)];
        let bad = select_rules(&rules, &[], &strings(&["("]), "exists rule").unwrap_err();
        assert!(format!("{bad:#}").contains("invalid exists rule regex"));
        let none = select_rules(&rules, &[], &strings(&["^zz"]), "exists rule").unwrap_err();
        assert!(format!("{none:#}").contains("matches no exists rule name"));
    }

    // ---- whole-command tests, driven through in-memory buffers ----

    struct Fleet {
        _tmp: TempDir,
        repos: Vec<Utf8PathBuf>,
    }

    /// `n` repo directories, each with `path` holding the matching entry of
    /// `contents` (`None` = file absent).
    fn make_fleet(path: &str, contents: &[Option<&str>]) -> Fleet {
        let tmp = TempDir::new().unwrap();
        let root = Utf8Path::from_path(tmp.path()).unwrap();
        let mut repos = Vec::new();
        for (i, content) in contents.iter().enumerate() {
            let repo = root.join(format!("r{i}"));
            fs::create_dir_all(&repo).unwrap();
            if let Some(text) = content {
                let file = repo.join(path);
                fs::create_dir_all(file.parent().unwrap()).unwrap();
                fs::write(&file, text).unwrap();
            }
            repos.push(repo);
        }
        Fleet { _tmp: tmp, repos }
    }

    fn config(toml: &str) -> CheckConfig {
        toml::from_str(toml).unwrap()
    }

    fn same_opts<'a>() -> CheckSameOpts<'a> {
        CheckSameOpts {
            requested: &[],
            requested_re: &[],
            only_failed: false,
            show_diff: false,
            do_copy: false,
            allow_empty: false,
            do_fix_missing: false,
        }
    }

    fn exists_opts<'a>() -> CheckExistsOpts<'a> {
        CheckExistsOpts {
            requested: &[],
            requested_re: &[],
            only_failed: false,
            allow_empty: false,
        }
    }

    fn run_same(
        app: &AppConfig,
        cfg: &CheckConfig,
        fleet: &Fleet,
        opts: &CheckSameOpts<'_>,
        stdin: &str,
    ) -> (Result<i32>, String) {
        let mut input = Cursor::new(stdin.as_bytes().to_vec());
        let mut out = Vec::new();
        let code = run_check_same(app, cfg, &fleet.repos, opts, &mut input, &mut out);
        (code, String::from_utf8(out).unwrap())
    }

    fn run_exists(
        app: &AppConfig,
        cfg: &CheckConfig,
        fleet: &Fleet,
        opts: &CheckExistsOpts<'_>,
    ) -> (Result<i32>, String) {
        let mut out = Vec::new();
        let code = run_check_exists(app, cfg, &fleet.repos, opts, &mut out);
        (code, String::from_utf8(out).unwrap())
    }

    const ONE_CHECK: &str = "[[check]]\nname = \"gi\"\nselect = \"*\"\npath = \".gitignore\"\n";
    const ONE_EXISTS: &str = "[[exists]]\nname = \"rd\"\nselect = \"*\"\npath = \"README.md\"\n";

    #[test]
    fn check_same_reports_ok_for_identical_files() {
        let fleet = make_fleet(".gitignore", &[Some("x\n"), Some("x\n")]);
        let (code, out) = run_same(
            &AppConfig::default(),
            &config(ONE_CHECK),
            &fleet,
            &same_opts(),
            "",
        );
        assert_eq!(code.unwrap(), 0);
        assert_eq!(out, "[gi]\nok (2 files)\n");
    }

    #[test]
    fn check_same_reports_groups_and_exits_one() {
        let fleet = make_fleet(".gitignore", &[Some("x\n"), Some("x\n"), Some("y\n")]);
        let (code, out) = run_same(
            &AppConfig::default(),
            &config(ONE_CHECK),
            &fleet,
            &same_opts(),
            "",
        );
        assert_eq!(code.unwrap(), 1);
        assert!(
            out.starts_with("[gi]\n3 files, 2 groups\n  group A (2 files):\n"),
            "{out}"
        );
        assert!(out.contains("  group B (1 files):\n"), "{out}");
        assert!(out.contains(fleet.repos[2].as_str()), "{out}");
    }

    #[test]
    fn check_same_terse_prints_failing_names_only() {
        let fleet = make_fleet(".gitignore", &[Some("x\n"), Some("y\n")]);
        let app = AppConfig {
            terse: true,
            ..AppConfig::default()
        };
        let (code, out) = run_same(&app, &config(ONE_CHECK), &fleet, &same_opts(), "");
        assert_eq!(code.unwrap(), 1);
        assert_eq!(out, "gi\n");
    }

    #[test]
    fn check_same_empty_rule_is_a_hard_error_unless_allowed() {
        let fleet = make_fleet("other", &[Some("x\n")]);
        let (code, out) = run_same(
            &AppConfig::default(),
            &config(ONE_CHECK),
            &fleet,
            &same_opts(),
            "",
        );
        assert_eq!(out, "[gi]\nno files matched (1 skipped)\n");
        let msg = format!("{:#}", code.unwrap_err());
        assert!(
            msg.contains("check-same: 1 rule matched no files: \"gi\""),
            "{msg}"
        );

        let opts = CheckSameOpts {
            allow_empty: true,
            ..same_opts()
        };
        let (code, out) = run_same(&AppConfig::default(), &config(ONE_CHECK), &fleet, &opts, "");
        assert_eq!(code.unwrap(), 0);
        assert_eq!(out, "[gi]\nok (0 files)\n");
    }

    #[test]
    fn check_same_diff_with_two_groups_is_automatic() {
        let fleet = make_fleet(".gitignore", &[Some("x\n"), Some("y\n")]);
        let opts = CheckSameOpts {
            show_diff: true,
            ..same_opts()
        };
        let (code, out) = run_same(&AppConfig::default(), &config(ONE_CHECK), &fleet, &opts, "");
        assert_eq!(code.unwrap(), 1);
        // Equal-sized groups are ordered by digest, so either direction is fine.
        assert!(
            out.contains("-x\n+y\n") || out.contains("-y\n+x\n"),
            "{out}"
        );
    }

    #[test]
    fn check_same_copy_overwrites_the_chosen_group_and_exits_zero() {
        let fleet = make_fleet(".gitignore", &[Some("x\n"), Some("x\n"), Some("y\n")]);
        let opts = CheckSameOpts {
            do_copy: true,
            ..same_opts()
        };
        // from A (the majority), to B, confirm.
        let (code, out) = run_same(
            &AppConfig::default(),
            &config(ONE_CHECK),
            &fleet,
            &opts,
            "A\nB\ny\n",
        );
        assert_eq!(code.unwrap(), 0);
        assert!(out.contains("copied -> "), "{out}");
        assert_eq!(
            fs::read_to_string(fleet.repos[2].join(".gitignore")).unwrap(),
            "x\n"
        );
    }

    #[test]
    fn check_same_quit_at_a_prompt_stops_further_rules() {
        let fleet = make_fleet(".gitignore", &[Some("x\n"), Some("y\n")]);
        // Same divergence declared twice; quitting the first prompt must
        // keep the second rule from being reported at all.
        let cfg = config(&format!("{ONE_CHECK}\n{}", ONE_CHECK.replace("gi", "gi2")));
        let opts = CheckSameOpts {
            do_copy: true,
            ..same_opts()
        };
        let (code, out) = run_same(&AppConfig::default(), &cfg, &fleet, &opts, "q\n");
        assert_eq!(code.unwrap(), 0);
        assert!(out.contains("[gi]"), "{out}");
        assert!(!out.contains("[gi2]"), "{out}");
    }

    #[test]
    fn check_same_fix_missing_creates_the_file() {
        let fleet = make_fleet(".gitignore", &[Some("x\n"), None]);
        let cfg = config(&format!("{ONE_CHECK}must_have = true\n"));
        let opts = CheckSameOpts {
            do_fix_missing: true,
            ..same_opts()
        };
        let (code, out) = run_same(&AppConfig::default(), &cfg, &fleet, &opts, "A\ny\n");
        assert_eq!(code.unwrap(), 0);
        assert!(out.contains("1 files, 1 groups (1 missing)"), "{out}");
        assert!(out.contains("created -> "), "{out}");
        assert_eq!(
            fs::read_to_string(fleet.repos[1].join(".gitignore")).unwrap(),
            "x\n"
        );
    }

    #[test]
    fn check_exists_reports_ok_and_missing() {
        let fleet = make_fleet("README.md", &[Some("a"), Some("b")]);
        let (code, out) = run_exists(
            &AppConfig::default(),
            &config(ONE_EXISTS),
            &fleet,
            &exists_opts(),
        );
        assert_eq!(code.unwrap(), 0);
        assert_eq!(out, "[rd]\nok (2 repos)\n");

        let fleet = make_fleet("README.md", &[Some("a"), None]);
        let (code, out) = run_exists(
            &AppConfig::default(),
            &config(ONE_EXISTS),
            &fleet,
            &exists_opts(),
        );
        assert_eq!(code.unwrap(), 1);
        assert_eq!(
            out,
            format!(
                "[rd]\n2 repos, 1 missing README.md\n  missing in:\n    {}\n",
                fleet.repos[1]
            )
        );
    }

    #[test]
    fn check_exists_only_failed_terse_and_no_header() {
        let fleet = make_fleet("README.md", &[Some("a"), None]);
        let cfg = config(&format!(
            "{ONE_EXISTS}\n{}",
            ONE_EXISTS.replace("rd", "rd2")
        ));

        let opts = CheckExistsOpts {
            only_failed: true,
            ..exists_opts()
        };
        let passing = make_fleet("README.md", &[Some("a")]);
        let (code, out) = run_exists(&AppConfig::default(), &cfg, &passing, &opts);
        assert_eq!(code.unwrap(), 0);
        assert_eq!(out, "");

        let app = AppConfig {
            terse: true,
            ..AppConfig::default()
        };
        let (code, out) = run_exists(&app, &cfg, &fleet, &exists_opts());
        assert_eq!(code.unwrap(), 1);
        assert_eq!(out, "rd\nrd2\n");

        let app = AppConfig {
            no_header: true,
            no_output: true,
            ..AppConfig::default()
        };
        let (code, out) = run_exists(&app, &cfg, &fleet, &exists_opts());
        assert_eq!(code.unwrap(), 1);
        assert_eq!(
            out,
            "2 repos, 1 missing README.md\n2 repos, 1 missing README.md\n"
        );
    }

    #[test]
    fn check_exists_short_circuit_stops_at_first_failure() {
        let fleet = make_fleet("README.md", &[None]);
        let cfg = config(&format!(
            "{ONE_EXISTS}\n{}",
            ONE_EXISTS.replace("rd", "rd2")
        ));
        let app = AppConfig {
            terse: true,
            short_circuit: true,
            ..AppConfig::default()
        };
        let (code, out) = run_exists(&app, &cfg, &fleet, &exists_opts());
        assert_eq!(code.unwrap(), 1);
        assert_eq!(out, "rd\n");
    }

    #[test]
    fn check_exists_empty_rule_is_a_hard_error_unless_allowed() {
        let fleet = make_fleet("README.md", &[Some("a")]);
        let cfg = config(&ONE_EXISTS.replace("select = \"*\"", "select = \"zz*\""));
        let (code, out) = run_exists(&AppConfig::default(), &cfg, &fleet, &exists_opts());
        assert_eq!(out, "[rd]\nno repos selected\n");
        let msg = format!("{:#}", code.unwrap_err());
        assert!(
            msg.contains("check-exists: 1 rule selected no repos: \"rd\""),
            "{msg}"
        );

        let opts = CheckExistsOpts {
            allow_empty: true,
            ..exists_opts()
        };
        let (code, out) = run_exists(&AppConfig::default(), &cfg, &fleet, &opts);
        assert_eq!(code.unwrap(), 0);
        assert_eq!(out, "[rd]\nok (0 repos)\n");
    }

    #[test]
    fn no_rules_selected_is_quiet_unless_verbose() {
        let fleet = make_fleet("README.md", &[Some("a")]);
        let cfg = config("");
        let (code, out) = run_exists(&AppConfig::default(), &cfg, &fleet, &exists_opts());
        assert_eq!(code.unwrap(), 0);
        assert_eq!(out, "");
        let app = AppConfig {
            verbose: true,
            ..AppConfig::default()
        };
        let (_, out) = run_same(&app, &cfg, &fleet, &same_opts(), "");
        assert_eq!(out, "no rules to check\n");
    }
}
