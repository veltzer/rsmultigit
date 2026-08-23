# CLI Parser Notes

rsmultigit uses [clap 4](https://docs.rs/clap/) (derive macros) for command-line
parsing. This page documents parser limitations we've hit and the workarounds
we've chosen, so that future contributors don't re-investigate the same dead
ends.

## Command categories in `--help`

**What we wanted.** Group the ~30 subcommands under headings
(`Inspection`, `Sync`, `Mutating`, `Build`, `Maintenance`, `Correctness`, `Meta`)
so `rsmultigit --help` is scannable instead of a flat alphabetical wall.

**What clap actually supports.** In clap 4, `#[command(help_heading = "...")]`
(and the builder equivalent `Command::help_heading`) only group **arguments**,
not **subcommands**. Per-subcommand-variant `help_heading` doesn't compile:

```
error[E0599]: no method named `help_heading` found for struct `clap::Command`
```

Upstream tracks this as an unresolved feature request
([clap-rs/clap#4416](https://github.com/clap-rs/clap/issues/4416), open as of
clap 4.6).

**What we do.** Keep the flat alphabetical list in `--help`. Document the
intended categories in the README / these docs for human readers. The enum
arms themselves stay ungrouped because adding source-level categories without
corresponding help-output categories would just be noise.

**What we considered and rejected.** Intercepting `--help` to emit a custom
grouped listing (~50 lines): doable but adds a maintenance surface — every
new subcommand has to be added to the grouping table as well as to the enum,
and the table can silently drift out of sync. Not worth it for a cosmetic win
unless the flat help becomes actively painful.

## Aliases and shell completion

**What we wanted.** `alias mg=rsmultigit` should preserve tab completion.

**What actually happens.** Shell completions are bound to a specific program
name — the name is baked into the generated completion script. Generating with
`rsmultigit complete bash` produces a `_rsmultigit` function registered against
the literal word `rsmultigit`. Typing `mg <TAB>` doesn't trigger it.

**Workaround.** Reuse the existing completion function by telling the shell:

```bash
alias mg=rsmultigit
complete -F _rsmultigit mg   # bash
compdef mg=rsmultigit        # zsh
# fish aliases inherit completions automatically
```

rsmultigit could also be extended to take a program name: e.g.
`rsmultigit complete bash --name mg` would emit completions bound to `mg`.
Not currently implemented — the `complete` subcommand hardcodes `"rsmultigit"`
in `cli::print_completions`.

## Top-level `help_template` and subcommand heading interaction

The top-level `Cli` uses a custom `help_template` containing a literal
`Commands:` line above `{subcommands}`. If subcommand `help_heading` support
ever lands, the template needs to drop that literal — otherwise you'd see
`Commands:` above clap's auto-emitted per-group headings. Flagging it here so
it's not forgotten.

## `--version` vs `version` subcommand

Both exist. `--version` is the clap-derived flag and prints a one-line
`rsmultigit x.y.z by Author`. The `version` subcommand prints the richer
version block with git SHA / branch / dirty / rustc / build timestamp (populated
by `build.rs`). Keep both — they serve different use cases (scripts that parse
a version string vs humans debugging an install).

## Global flags on subcommands

`--terse`, `--no-header`, `--no-output`, `--verbose`, `--print-not`, `--no-stop`,
`--short-circuit`, `-j/--jobs` are all declared with `global = true` on the top-level `Cli`, so
they can appear *before or after* the subcommand:

```bash
rsmultigit --jobs 8 pull           # works
rsmultigit pull --jobs 8            # also works
```

Subcommand-specific flags (e.g. `pull --quiet`, `grep -l`) are declared on the
subcommand variant and only work after the subcommand name. That split is
intentional and matches git's own conventions.
