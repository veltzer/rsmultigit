# Installation

## Install from crates.io

```bash
cargo install rsmultigit
```

This downloads, compiles, and installs the latest published version into
`~/.cargo/bin/`.

## Pre-built binaries

Every release on GitHub ships a statically named binary per platform:

| Platform | Asset |
|----------|-------|
| Linux x86_64 | `rsmultigit-linux-x86_64` |
| Linux aarch64 | `rsmultigit-linux-aarch64` |
| macOS x86_64 | `rsmultigit-macos-x86_64` |
| macOS aarch64 | `rsmultigit-macos-aarch64` |

With the GitHub CLI (downloads from the latest release when no tag is given):

```bash
gh release download --repo veltzer/rsmultigit --pattern rsmultigit-linux-x86_64 --output rsmultigit --clobber
chmod +x rsmultigit
sudo mv rsmultigit /usr/local/bin/
```

Or with curl, using GitHub's stable latest-release URL:

```bash
curl -Lo rsmultigit https://github.com/veltzer/rsmultigit/releases/latest/download/rsmultigit-linux-x86_64
chmod +x rsmultigit
sudo mv rsmultigit /usr/local/bin/
```

Replace the asset name with the one for your platform.

## Build from source

```bash
git clone https://github.com/veltzer/rsmultigit.git
cd rsmultigit
cargo build --release
```

The binary will be at `target/release/rsmultigit`. The release profile in
`Cargo.toml` already strips symbols and enables LTO, so no extra flags are
needed.

To install it system-wide:

```bash
sudo cp target/release/rsmultigit /usr/local/bin/
```

## Dependencies

RSMultiGit links against libgit2 (via the `git2` crate) for native git repo
inspection. libgit2 and OpenSSL are compiled from source during the build, so
no system packages are required beyond a C compiler, CMake and Perl.

At run time, some commands shell out to external tools that must be on
`PATH`: `git` for network and formatting commands, `gh` for `git branch github`
and `gh clean-all`, `uv` for the `uv` commands, `cargo` and
[cargo-release](https://crates.io/crates/cargo-release) for the Rust
commands, and whichever build tool a `build` method names.

## First run

rsmultigit needs a config file before it will do anything. Create one
interactively:

```bash
rsmultigit setup interactive
```

or from the built-in example, then edit the `repos` list:

```bash
mkdir -p ~/.config/rsmultigit
rsmultigit setup config-sample > ~/.config/rsmultigit/config.toml
```

See [Getting Started](getting-started.md) for what to run next.
