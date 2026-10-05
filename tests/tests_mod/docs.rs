//! Guard against the command reference drifting from the binary: every
//! subcommand and every global flag the binary reports must be documented
//! in docs/src/commands.md. This is what let the docs describe removed
//! flags and renamed commands for months before anyone noticed.

use crate::common::{run_rsmultigit, stdout_str};

fn commands_md() -> String {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/docs/src/commands.md");
    std::fs::read_to_string(path).expect("docs/src/commands.md is readable")
}

/// The first word of every entry under a `Commands:` / `Options:` heading in
/// clap's help output.
fn help_section(help: &str, heading: &str) -> Vec<String> {
    help.split("\n\n")
        .find(|block| block.starts_with(heading))
        .unwrap_or_else(|| panic!("help has a {heading} block: {help}"))
        .lines()
        .skip(1)
        .filter_map(|line| line.split_whitespace().next().map(str::to_string))
        .collect()
}

#[test]
fn every_subcommand_has_a_heading_in_the_command_reference() {
    let tmp = tempfile::TempDir::new().unwrap();
    let docs = commands_md();
    let headings: Vec<&str> = docs.lines().filter(|l| l.starts_with('#')).collect();
    // The top level, then the `git` group, whose subcommands are documented
    // under `rsmultigit git <name>` headings.
    for (args, prefix, at_least) in [
        (&["--help"][..], "rsmultigit", 20),
        (&["git", "--help"][..], "rsmultigit git", 15),
    ] {
        let output = run_rsmultigit(crate::common::utf8(&tmp), args);
        assert!(output.status.success());
        let subcommands = help_section(&stdout_str(&output), "Commands:");
        assert!(subcommands.len() > at_least, "parsed {subcommands:?}");

        let missing: Vec<&String> = subcommands
            .iter()
            .filter(|name| name.as_str() != "help")
            .filter(|name| {
                // `rsmultigit config ` must not be satisfied by `config-example`.
                let with_space = format!("`{prefix} {name} ");
                let with_tick = format!("`{prefix} {name}`");
                !headings
                    .iter()
                    .any(|h| h.contains(&with_space) || h.contains(&with_tick))
            })
            .collect();
        assert!(
            missing.is_empty(),
            "subcommands without a `{prefix} <name>` heading in docs/src/commands.md: {missing:?}"
        );
    }
}

#[test]
fn every_global_flag_is_in_the_command_reference() {
    let tmp = tempfile::TempDir::new().unwrap();
    // Any subcommand's short help lists the global flags; `age` has none of
    // its own, so everything under Options: is global (plus --help).
    let output = run_rsmultigit(crate::common::utf8(&tmp), &["age", "-h"]);
    assert!(output.status.success());
    let help = stdout_str(&output);
    let options = help
        .split("\n\n")
        .find(|block| block.starts_with("Options:"))
        .unwrap_or_else(|| panic!("help has an Options: block: {help}"));
    let flags: Vec<String> = options
        .lines()
        .filter_map(|line| line.split_once("--"))
        .map(|(_, rest)| {
            let name: String = rest
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '-')
                .collect();
            format!("--{name}")
        })
        .collect();
    assert!(flags.contains(&"--terse".to_string()), "parsed {flags:?}");

    let docs = commands_md();
    let missing: Vec<&String> = flags
        .iter()
        .filter(|f| f.as_str() != "--help")
        // `--jobs <N>` is documented with its value, so accept either a
        // closing backtick or a space after the flag.
        .filter(|f| !docs.contains(&format!("`{f}`")) && !docs.contains(&format!("`{f} ")))
        .collect();
    assert!(
        missing.is_empty(),
        "global flags not mentioned in docs/src/commands.md: {missing:?}"
    );
}
