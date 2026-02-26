# releasor2000

Release everywhere.

A CLI tool that builds a Rust project for multiple targets and publishes releases across GitHub/Gitea, Homebrew, Cargo, curl-installable scripts, and Nix flakes.

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
curl -fsSL https://github.com/nakajima/releasor2000/releases/latest/download/install.sh | sh
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
releasor2000 release git homebrew
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

[git]
# type = "gitea"                  # defaults to "github"
# base_url = "https://git.example.com"
# api_base_url = "https://git.example.com/api/v1"  # optional override
# token_env = "GITEA_TOKEN"       # defaults: GITHUB_TOKEN or GITEA_TOKEN

[channels.git]
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
| `repo` | yes | Repository path (`owner/repo`) |
| `version_command` | no | Shell command to detect version (defaults to `git describe --tags --abbrev=0`) |

### Git fields

| Field | Required | Description |
|---|---|---|
| `type` | no | Git type: `github` (default) or `gitea` |
| `base_url` | no | Web base URL (defaults to `https://github.com` for GitHub) |
| `api_base_url` | no | API base URL override (Gitea defaults to `{base_url}/api/v1`) |
| `token_env` | no | Token env var override (defaults to `GITHUB_TOKEN` or `GITEA_TOKEN`) |

### Build fields

| Field | Required | Description |
|---|---|---|
| `command` | yes* | Build command template. Supports `{target}`, `{binary}`, `{version}` placeholders |
| `artifact` | yes* | Path to built artifact. Same placeholders as `command` |
| `pre_built_dir` | yes* | Directory with pre-built binaries (mutually exclusive with `command`) |
| `targets` | yes | List of Rust target triples to build for |

*Either `command`+`artifact` or `pre_built_dir` is required.

## Channels

The `git` channel always runs first — it creates the release on your git host and uploads the build artifacts that the other channels (homebrew, curl, nix) depend on.

### Release (`git` channel)

Creates a release and uploads `.tar.gz` archives for each target.

```toml
[channels.git]
enabled = true
```

On GitHub, release notes are auto-generated. On Gitea, a basic release is created.

Archives are named `{binary}-{version}-{target}.tar.gz`.

### Homebrew

Generates a Homebrew formula and pushes it to your tap repository. Only includes macOS targets.
Download URLs in the formula are built from your git host `base_url`.

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

Generates an `install.sh` script that detects OS/arch and downloads the right binary from your configured git host, then uploads it to the release.

```toml
[channels.curl]
```

The generated script has the version baked in and is uploaded to the release as `install.sh`.

### Nix

Generates a `flake.nix` and `flake.lock` and pushes them to a repository.
Source URLs are built from your git host `base_url`.

```toml
[channels.nix]
flake_repo = "owner/nix-repo"  # defaults to project repo
```

Requires the `nix` command to be available.

## Cross-compilation

When building for a target that differs from the host, releasor2000 prefers:

- `cargo-zigbuild` (if installed)
- `cross` (if installed, as fallback)

If neither is installed, it falls back to plain `cargo build` (which may still work if you configured `CARGO_TARGET_<TRIPLE>_LINKER`).

macOS targets can cross-compile between x86_64 and aarch64 natively on macOS. Building macOS targets from non-macOS hosts usually requires Apple SDK tooling, so use a macOS builder for those targets.

## Requirements

- **Git token env var** — required for channels that interact with git APIs (`git`, `homebrew`, `curl`, `nix`); defaults are `GITHUB_TOKEN` (GitHub) and `GITEA_TOKEN` (Gitea), and you can override with `[git].token_env`
- **rustup targets** — install targets with `rustup target add <target>`
- **cargo-zigbuild** (optional) — preferred cross-compilation backend
- **cross** (optional) — fallback cross-compilation backend
- **nix** (optional) — required only for the nix channel
