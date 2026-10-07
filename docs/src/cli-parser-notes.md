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

```text
error[E0599]: no method named `help_heading` found for struct `clap::Command`
```

Upstream tracks this as an unresolved feature request
([clap-rs/clap#4416](https://github.com/clap-rs/clap/issues/4416), open as of
clap 4.6).

**What we do.** Keep the flat alphabetical list in `--help`, and keep it
short by nesting: the git wrappers (`pull`, `push`, `reset`, `restore`, ...)
live under a `git` subcommand group, so the top level holds rsmultigit's own
reports and tool runners. A real subcommand level is the one kind of grouping
clap renders. Document the
intended categories in the README / these docs for human readers. The enum
arms themselves stay ungrouped because adding source-level categories without
corresponding help-output categories would just be noise.

**What we considered and rejected.** Intercepting `--help` to emit a custom
grouped listing (~50 lines): doable but adds a maintenance surface — every
new subcommand has to be added to the grouping table as well as to the enum,
and the table can silently drift out of sync. Not worth it for a cosmetic win
unless the flat help becomes actively painful.

## Commands that demand an operation word

**What we wanted.** `rsmultigit gh` (and `count`, `git clean`, `cargo`, `uv`,
`npm`, ... every command whose first positional is a `ValueEnum` picking the
operation) must list the operations when invoked without one. clap's default
is an error that only says `<WHAT>` is required, which leaves the user to go
and ask `--help` for what `<WHAT>` may be.

**What clap supports.** `#[command(arg_required_else_help = true)]` on the
variant turns the bare invocation into a help display. Two gaps remain:
it renders the *short* help, where the operations appear as a bare
`[possible values: ...]` list without descriptions, and it only fires when
*nothing* followed the command, so `rsmultigit gh --keep 3` still gets the
`<WHAT>` error.

A third gap: either rendering lists every flag as well, and since the
global flags (`--terse`, `--no-header`, `-j`, `--venv`, ...) apply to every
command, that is a dozen entries burying the five operations the user
stopped to see.

**What we do.** Every such variant carries `arg_required_else_help`, and
`main::parse_cli` intercepts both outcomes (the help-on-empty error and a
`MissingRequiredArgument` naming `<WHAT>`) to print the command's help via
`cli::long_help_for`. That renders the command's about and usage, then a
`Commands:` table of its operations - each with its description, no flag
listed - and a trailer pointing at `--help`, which keeps the full output.
The table is built by us (`operations_of` + `operations_list`), not by clap's
`{positionals}`, whose long form is an indented `Possible values:` block that
looks nothing like a subcommand list. Exit status 2 on stderr, as clap would. The positional
is called `what` on every such command; that name is the contract the
intercept relies on. A missing free-form positional (`git blame <FILE>`)
keeps clap's own error.

A group of subcommands (`git`) gets the same treatment: the variant carries
`arg_required_else_help` (and `disable_help_subcommand`, so no `help` entry
pads the list), the intercept also catches `MissingSubcommand` (the
`rsmultigit git --no-stop` form), and `operations_of` takes the subcommands
instead of the `what` values when the command it lands on has subcommands.
Both kinds then go through the same `operations_list`, so `rsmultigit git`
and `rsmultigit npm` print the same form.

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

## Colours in help

`Cli` sets `styles = STYLES` (cargo's palette: green headings, cyan flags and
commands, red errors). clap colours only what it renders itself, so any
heading written as literal text in a `help_template` (`Commands:`,
`Options:`, and the hand-written `-h`/`-V` lines) is put together as a
`StyledStr` from `STYLES` (`top_help_template`, `subcommands_help_template`)
instead of a plain `&str`. `long_help_for` returns that `StyledStr`, and
`parse_cli` prints it with `anstream::eprint!` and `.ansi()`. Calling
`.to_string()` on it would strip the colours, and a plain `eprint!` would
send escape codes into pipes. anstream applies the same rules clap does:
colour on a terminal, none when the output is piped, and it respects
`NO_COLOR` and `CLICOLOR_FORCE`.

## `--version` vs `version` subcommand

Both exist. `--version` is the clap-derived flag and prints a one-line
`rsmultigit x.y.z by Author`. The `version` subcommand prints the richer
version block with git SHA / branch / dirty / rustc / build timestamp (populated
by `build.rs`). Keep both — they serve different use cases (scripts that parse
a version string vs humans debugging an install).

## Global flags on subcommands

`--terse`, `--no-header`, `--no-output`, `--verbose`, `--print-not`, `--no-stop`,
`--short-circuit`, `-j/--jobs`, `--venv`/`--no-venv` are all declared with
`global = true` on the top-level `Cli`, so
they can appear *before or after* the subcommand:

```bash
rsmultigit --jobs 8 git pull       # works
rsmultigit git pull --jobs 8        # also works
```

Subcommand-specific flags (e.g. `pull --quiet`, `grep -l`) are declared on the
subcommand variant and only work after the subcommand name. That split is
intentional and matches git's own conventions.
