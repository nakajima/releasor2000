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

# Review the generated config and enable any additional channels
$EDITOR releasor2000.toml

# Optional: enable [project].auto-tag in releasor2000.toml
# auto-tag = true

# Release
releasor2000 release

# Upgrade releasor2000 itself in place
releasor2000 upgrade
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

`releasor2000 init` generates a `releasor2000.toml`. When `.git/config` contains an `origin` remote, it derives `project.repo` and the git host settings from that URL. `github.com` is treated as GitHub; other network hosts are configured as Gitea.

```toml
[project]
name = "myapp"
# auto-tag = true  # read Cargo.toml version and keep v<version> only after a successful release
# binary = "myapp"  # defaults to project name
# package = "myapp"  # optional workspace package override; auto-detected from binary when unique
# binaries = ["myapp", "myapp-cli"]  # optional extra release assets
repo = "owner/myapp"
# version-command = "git describe --tags --abbrev=0"

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
# base-url = "https://git.example.com"
# api-base-url = "https://git.example.com/api/v1"  # optional override
# token-env = "GITEA_TOKEN"       # defaults: GITHUB_TOKEN or GITEA_TOKEN

[channels.git]
enabled = true

# [channels.homebrew]
# tap = "owner/homebrew-tap"
# formula-name = "myapp"

# [channels.cargo]
# crate-name = "myapp"

# [channels.curl]

# [channels.nix]
# flake-repo = "owner/nix-repo"  # defaults to project repo
```

Underscore config keys are still accepted for compatibility, but they are deprecated in favor of dashed keys and emit warnings.

### Project fields

| Field | Required | Description |
|---|---|---|
| `name` | yes | Project name |
| `binary` | no | Primary binary for single-binary channels (defaults to `name`) |
| `package` | no | Cargo workspace package override (auto-detected from `binary` when unique) |
| `binaries` | no | Additional binaries to package/upload as release assets |
| `repo` | yes | Repository path (`owner/repo`) |
| `auto-tag` | no | If true, use `Cargo.toml` package version and create/push annotated git tag `v<version>` when releasing to `git`; try to remove it if the release fails |
| `version-command` | no | Shell command to detect version (defaults to `git describe --tags --abbrev=0`) |

If `binaries` is set, releasor2000 builds/packages each `{binary}` per target and uploads each archive as its own release asset. If both `binary` and `binaries` are set, `binary` must be included in `binaries`.
When `auto-tag = true`, releasor2000 supports Rust packages and Cargo workspace roots. In workspaces, it uses the same binary-to-package detection as release builds, or `[project].package` if set. It fails if tag `v<version>` already exists locally or on `origin`. The tag is created only after artifacts build successfully; if a later release step fails, releasor2000 tries to remove the tag locally and from `origin`.

### Git fields

| Field | Required | Description |
|---|---|---|
| `type` | no | Git type: `github` (default) or `gitea` |
| `base-url` | no | Web base URL (defaults to `https://github.com` for GitHub) |
| `api-base-url` | no | API base URL override (Gitea defaults to `{base-url}/api/v1`) |
| `token-env` | no | Token env var override (defaults to `GITHUB_TOKEN` or `GITEA_TOKEN`) |

### Build fields

| Field | Required | Description |
|---|---|---|
| `command` | yes* | Build command template. Supports `{target}`, `{binary}`, `{package}`, `{version}` placeholders |
| `artifact` | yes* | Path to built artifact. Same placeholders as `command` |
| `pre-built-dir` | yes* | Directory with pre-built binaries (mutually exclusive with `command`) |
| `targets` | yes | List of Rust target triples to build for |

*Either `command`+`artifact` or `pre-built-dir` is required.

## Channels

The `git` channel always runs first — it creates the release on your git host and uploads the build artifacts that the other channels (homebrew, curl, nix) depend on.

### Release (`git` channel)

Creates a release and uploads `.tar.gz` archives for each binary/target combination.

```toml
[channels.git]
enabled = true
```

On GitHub, release notes are auto-generated. On Gitea, a basic release is created.

Archives are named `{binary}-{version}-{target}.tar.gz`.

### Homebrew

Generates a Homebrew formula and pushes it to your tap repository. Only includes macOS targets.
Download URLs in the formula are built from your git host `base-url`.
Uses the primary binary (`project.binary`, or the first item in `project.binaries`).

```toml
[channels.homebrew]
tap = "owner/homebrew-tap"       # required
formula-name = "myapp"           # defaults to project name
```

### Cargo

Publishes the crate to crates.io via `cargo publish`.

```toml
[channels.cargo]
crate-name = "myapp"  # defaults to project name
```

Requires prior `cargo login`.

In Cargo workspaces, releasor2000 auto-detects the package when exactly one workspace member provides the configured binary target. Set `[project].package` if the binary name is ambiguous.

### Curl

Generates an `install.sh` script that detects OS/arch and downloads the right binary from your configured git host, then uploads it to the release.
Uses the primary binary (`project.binary`, or the first item in `project.binaries`).

```toml
[channels.curl]
```

The generated script has the version baked in and is uploaded to the release as `install.sh`.

### Nix

Generates a `flake.nix` and `flake.lock` and pushes them to a repository.
Source URLs are built from your git host `base-url`.
Uses the primary binary (`project.binary`, or the first item in `project.binaries`).

```toml
[channels.nix]
flake-repo = "owner/nix-repo"  # defaults to project repo
```

Requires the `nix` command to be available.

## Cross-compilation

When building for a target that differs from the host, releasor2000 prefers:

- `cargo-zigbuild` (if installed)
- `cross` (if installed, as fallback)

If neither is installed, it falls back to plain `cargo build` (which may still work if you configured `CARGO_TARGET_<TRIPLE>_LINKER`).

macOS targets can cross-compile between x86_64 and aarch64 natively on macOS. Building macOS targets from non-macOS hosts usually requires Apple SDK tooling, so use a macOS builder for those targets.

## Requirements

- **Git token env var** — required for channels that interact with git APIs (`git`, `homebrew`, `curl`, `nix`); defaults are `GITHUB_TOKEN` (GitHub) and `GITEA_TOKEN` (Gitea), and you can override with `[git].token-env`
- **rustup targets** — install targets with `rustup target add <target>`
- **cargo-zigbuild** (optional) — preferred cross-compilation backend
- **cross** (optional) — fallback cross-compilation backend
- **nix** (optional) — required only for the nix channel
