# releasor2000

Release your software everywhere.

A CLI tool that builds your Rust project for multiple targets and publishes releases across GitHub, Homebrew, Cargo, curl-installable scripts, and Nix flakes — all from a single config file.

## Install

### Homebrew

```sh
brew install nakajima/tap/releasor2000
```

### Cargo

```sh
cargo install releasor2000
```

### Curl

```sh
curl -fsSL https://github.com/nakajima/releasor2000/releases/download/v0.1.1/install.sh | sh
```

### Nix

```sh
nix run github:nakajima/releasor2000
```

### GitHub releases

Download a prebuilt binary from the [releases page](https://github.com/nakajima/releasor2000/releases).

## Quick start

```sh
# Generate a config file
releasor2000 init

# Edit releasor2000.toml to set your repo and enable channels
$EDITOR releasor2000.toml

# Tag a version and release
git tag v0.1.0
releasor2000 release
```

You can also pass `--version` directly:

```sh
releasor2000 release --version 0.1.0
```

Or release to specific channels:

```sh
releasor2000 release github homebrew
```

Use `releasor2000 validate` to check your config without releasing.

## Configuration

`releasor2000 init` generates a `releasor2000.toml`:

```toml
[project]
name = "myapp"
# binary = "myapp"  # defaults to project name
repo = "owner/myapp"
# version_command = "git describe --tags --abbrev=0"

[build]
command = "cargo build --release --target {target}"
artifact = "target/{target}/release/{binary}"
targets = [
    "x86_64-apple-darwin",
    "aarch64-apple-darwin",
    "x86_64-unknown-linux-gnu",
    "aarch64-unknown-linux-gnu",
]

[channels.github]
enabled = true

# [channels.homebrew]
# tap = "owner/homebrew-tap"
# formula_name = "myapp"

# [channels.cargo]
# crate_name = "myapp"

# [channels.curl]

# [channels.nix]
# flake_repo = "owner/nix-repo"  # defaults to project repo
```

### Project fields

| Field | Required | Description |
|---|---|---|
| `name` | yes | Project name |
| `binary` | no | Binary name (defaults to `name`) |
| `repo` | yes | GitHub repository (`owner/repo`) |
| `version_command` | no | Shell command to detect version (defaults to `git describe --tags --abbrev=0`) |

### Build fields

| Field | Required | Description |
|---|---|---|
| `command` | yes* | Build command template. Supports `{target}`, `{binary}`, `{version}` placeholders |
| `artifact` | yes* | Path to built artifact. Same placeholders as `command` |
| `pre_built_dir` | yes* | Directory with pre-built binaries (mutually exclusive with `command`) |
| `targets` | yes | List of Rust target triples to build for |

*Either `command`+`artifact` or `pre_built_dir` is required.

## Channels

The GitHub channel always runs first — it creates the release and uploads the build artifacts that the other channels (homebrew, curl, nix) depend on.

### GitHub

Creates a GitHub release with auto-generated release notes and uploads `.tar.gz` archives for each target.

```toml
[channels.github]
enabled = true
```

Archives are named `{binary}-{version}-{target}.tar.gz`.

### Homebrew

Generates a Homebrew formula and pushes it to your tap repository. Only includes macOS targets.

```toml
[channels.homebrew]
tap = "owner/homebrew-tap"       # required
formula_name = "myapp"           # defaults to project name
```

### Cargo

Publishes the crate to crates.io via `cargo publish`.

```toml
[channels.cargo]
crate_name = "myapp"  # defaults to project name
```

Requires prior `cargo login`.

### Curl

Generates an `install.sh` script that detects OS/arch and downloads the right binary from GitHub, then uploads it to the release.

```toml
[channels.curl]
```

The generated script has the version baked in and is uploaded to the GitHub release as `install.sh`.

### Nix

Generates a `flake.nix` and `flake.lock` and pushes them to a repository.

```toml
[channels.nix]
flake_repo = "owner/nix-repo"  # defaults to project repo
```

Requires the `nix` command to be available.

## Cross-compilation

When building for a target that differs from the host, releasor2000 automatically detects `cargo-zigbuild` and uses it in place of `cargo build`. macOS targets can cross-compile between x86_64 and aarch64 natively without extra tooling.

## Requirements

- **`GITHUB_TOKEN`** — environment variable required for all channels that interact with GitHub (github, homebrew, curl, nix)
- **rustup targets** — install targets with `rustup target add <target>`
- **cargo-zigbuild** (optional) — for cross-compiling Linux targets from macOS
- **nix** (optional) — required only for the nix channel
