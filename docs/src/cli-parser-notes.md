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
`cli::long_help_for`. That renders the command with a `help_template` of
`{positionals}` only - so each operation comes with its long description and
no flag is listed - and an `after_help` trailer pointing at `--help`, which
keeps the full output. Exit status 2 on stderr, as clap would. The positional
is called `what` on every such command; that name is the contract the
intercept relies on. A missing free-form positional (`git blame <FILE>`)
keeps clap's own error.

A group of subcommands (`git`) gets the same treatment: the variant carries
`arg_required_else_help` (and `disable_help_subcommand`, so no `help` entry
pads the list), the intercept also catches `MissingSubcommand` (the
`rsmultigit git --no-stop` form), and `long_help_for` switches to a
`{subcommands}` template when the command it lands on has subcommands.

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
