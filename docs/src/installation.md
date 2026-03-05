# Installation

## Build from source

```bash
git clone https://github.com/veltzer/rmg.git
cd rmg
cargo build --release
```

The binary will be at `target/release/rsmultigit`.

To install it system-wide:

```bash
sudo cp target/release/rsmultigit /usr/local/bin/
```

## Dependencies

RSMultiGit links against libgit2 (via the `git2` crate) for native git repo inspection. The C library is compiled from source during the build, so no system packages are required beyond a C compiler and CMake (provided by your Rust toolchain).

## Release profile

For an optimized binary, add the following to `Cargo.toml`:

```toml
[profile.release]
strip = true        # Remove debug symbols
lto = true          # Link-time optimization across all crates
codegen-units = 1   # Single codegen unit for better optimization
```
