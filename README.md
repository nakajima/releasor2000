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

Without `auto-tag`, the version comes from `--version`, then `version-command`, then a `.version` file in the project root (plain text, e.g. `1.2.3`), then `git describe --tags --abbrev=0`.

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

# [macos.codesign]
# p12-file = "developer-id.p12"
# p12-password-file = "developer-id.p12.password"
# entitlements = "entitlements.plist"  # optional
# p12-env = "MACOS_P12_BASE64"           # instead of p12-file
# p12-password-env = "MACOS_P12_PASSWORD"  # instead of p12-password-file

# [macos.notarization]
# api-key-file = "app-store-connect-key.json"
# api-key-env = "APP_STORE_CONNECT_KEY"  # instead of api-key-file

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
| `repo` | yes* | Repository path (`owner/repo`); required for `git`, `homebrew`, `nix`, and `curl` without `dest` |
| `auto-tag` | no | If true, use `Cargo.toml` package version and create/push annotated git tag `v<version>` when releasing to `git`; try to remove it if the release fails |
| `version-command` | no | Shell command to detect version (defaults to the `.version` file, then `git describe --tags --abbrev=0`) |

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

### macOS code signing and notarization

Both sections are optional and apply only to `apple-darwin` targets. Signing runs after the build and before the release archive is created. Both use [`rcodesign`](https://github.com/indygreg/apple-platform-rs/tree/main/apple-codesign), so signing and notarization work from Linux as well as macOS. Install it with `cargo install apple-codesign`.

```toml
[macos.codesign]
p12-file = "developer-id.p12"
p12-password-file = "developer-id.p12.password"
entitlements = "entitlements.plist"  # optional

[macos.notarization]
api-key-file = "app-store-connect-key.json"
```

Code signing runs `rcodesign sign` with the hardened runtime and a secure timestamp. `p12-file` is a Developer ID Application certificate and private key exported from Keychain Access, and `p12-password-file` holds its password on the first line.

Notarization zips the binary, submits it with `rcodesign notary-submit --wait`, and publishes the existing `.tar.gz` only after Apple accepts the submission. A rejected submission fails the release and prints Apple's notarization log. `api-key-file` is an App Store Connect API key encoded once with:

```sh
rcodesign encode-app-store-connect-api-key -o app-store-connect-key.json <issuer-id> <key-id> AuthKey_<key-id>.p8
```

All paths are relative to the directory releasor2000 runs in. Keep these files out of version control.

In CI, each credential can come from an env var instead of a file by using the `-env` form of its key (`p12-env`, `p12-password-env`, `api-key-env`). Set exactly one of `-file` or `-env` per credential.

```toml
[macos.codesign]
p12-env = "MACOS_P12_BASE64"            # base64 of the .p12: base64 < developer-id.p12
p12-password-env = "MACOS_P12_PASSWORD"

[macos.notarization]
api-key-env = "APP_STORE_CONNECT_KEY"   # contents of app-store-connect-key.json
```

Releasor2000 writes env var secrets to a private temporary directory for `rcodesign` and deletes it after each artifact is processed.

The `[macos.notarization]` section can also notarize pre-signed artifacts when `[macos.codesign]` is omitted. Apple rejects artifacts without a valid Developer ID signature.

Apple creates an online ticket for standalone binaries, but does not support stapling tickets to them. Gatekeeper retrieves the ticket when the binary is first opened.

## Channels

The `git` channel always runs first — it creates the release on your git host and uploads the build artifacts that the other channels (homebrew, nix, and curl without `dest`) depend on.

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

Generates an `install.sh` script that detects OS/arch, downloads the right archive, and verifies its SHA-256 checksum before installing.
It installs to `$INSTALL_DIR` if set; otherwise it asks on the terminal (also under `curl | sh`), defaulting to `~/.local/bin`, and says so if that directory is not on `PATH`.
Uses the primary binary (`project.binary`, or the first item in `project.binaries`).

```toml
[channels.curl]
```

By default the script downloads from your configured git host and is uploaded to the release as `install.sh`, with the version baked in.

To release without a git host, set `dest` and `url`:

```toml
[channels.curl]
dest = "deploy@example.com:/var/www/dl/myapp"  # sftp destination; the directory must exist
url = "https://example.com/dl/myapp"           # where dest is served
```

Each release goes into its own directory, and `latest` links to the newest:

```
myapp/
  1.2.3/
    myapp-1.2.3-aarch64-apple-darwin.tar.gz
    install.sh
    version.json
  latest -> 1.2.3
```

One `sftp` session creates `<version>/`, uploads the archives, `install.sh`, and finally `version.json` (the version and the SHA-256 of each file), then swaps `latest` to the new directory with a rename. If any upload fails, `latest` is left alone. Install the latest release, or a specific one:

```sh
curl -fsSL https://example.com/dl/myapp/latest/install.sh | sh
curl -fsSL https://example.com/dl/myapp/1.2.3/install.sh | sh
```

Before building, releasor2000 checks `<url>/<version>/version.json` and stops if it exists. Because `version.json` is uploaded last, a failed upload can simply be rerun. `project.repo`, a git token, and the `git` channel are not needed in this mode. `sftp` runs in batch mode, so `dest` needs key-based SSH authentication, and the web server must follow symlinks (nginx and Caddy do by default).

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

macOS targets can cross-compile between x86_64 and aarch64 natively on macOS. On other hosts, macOS targets are built with `cargo-zigbuild` and skipped when it isn't installed (`cross` has no macOS images). Crates that link Apple frameworks also need a macOS SDK, set through `SDKROOT`.

## Requirements

- **Git token env var** — required for channels that interact with git APIs (`git`, `homebrew`, `nix`, and `curl` without `dest`); defaults are `GITHUB_TOKEN` (GitHub) and `GITEA_TOKEN` (Gitea), and you can override with `[git].token-env`
- **rustup targets** — install targets with `rustup target add <target>`
- **cargo-zigbuild** (optional) — preferred cross-compilation backend
- **cross** (optional) — fallback cross-compilation backend
- **nix** (optional) — required only for the nix channel
- **sftp** (optional) — required only for the curl channel with `dest`
- **rcodesign** (optional) — required for macOS signing and notarization; install with `cargo install apple-codesign`
- **Developer ID Application certificate** (optional) — exported as a `.p12` for `[macos.codesign]`
- **App Store Connect API key** (optional) — required when `[macos.notarization]` is configured
- **zip** (optional) — required for macOS notarization
