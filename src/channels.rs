use anyhow::{Context, Result, bail};
use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::config::{Config, GitType};

// --- Shared infrastructure ---

#[derive(Debug, Clone)]
struct GitContext {
    web_base_url: String,
    api_base_url: String,
    token_env: String,
    auth_prefix: &'static str,
    accept_header: &'static str,
    is_github: bool,
}

impl GitContext {
    fn from_config(config: &Config) -> Self {
        let (auth_prefix, accept_header, is_github) = match config.git.r#type {
            GitType::Github => ("Bearer", "application/vnd.github+json", true),
            GitType::Gitea => ("token", "application/json", false),
        };
        Self {
            web_base_url: config.git.web_base_url(),
            api_base_url: config.git.api_base_url(),
            token_env: config.git.token_env().to_string(),
            auth_prefix,
            accept_header,
            is_github,
        }
    }

    fn token(&self) -> Result<String> {
        std::env::var(&self.token_env)
            .with_context(|| format!("{} environment variable not set", self.token_env))
    }

    fn auth_header(&self, token: &str) -> String {
        format!("Authorization: {} {token}", self.auth_prefix)
    }

    fn repo_api_url(&self, repo: &str, suffix: &str) -> String {
        format!("{}/repos/{repo}{suffix}", self.api_base_url)
    }

    fn contents_api_url(&self, repo: &str, path: &str) -> String {
        self.repo_api_url(repo, &format!("/contents/{path}"))
    }

    fn release_download_url(&self, repo: &str, version: &str, asset_name: &str) -> String {
        format!(
            "{}/{repo}/releases/download/v{version}/{asset_name}",
            self.web_base_url
        )
    }

    fn upload_url_with_name(upload_url: &str, name: &str) -> String {
        let base = upload_url.split('{').next().unwrap_or(upload_url);
        if base.contains('?') {
            format!("{base}&name={name}")
        } else {
            format!("{base}?name={name}")
        }
    }
}

fn run_cmd(label: &str, dir: Option<&Path>, cmd: &str, args: &[&str]) -> Result<String> {
    println!("[{label}] Running: {cmd} {}", args.join(" "));
    let mut command = Command::new(cmd);
    command.args(args);
    if let Some(d) = dir {
        command.current_dir(d);
    }
    let output = command
        .output()
        .with_context(|| format!("[{label}] failed to run {cmd}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("[{label}] {cmd} failed: {stderr}");
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn git_api(
    git: &GitContext,
    label: &str,
    method: &str,
    url: &str,
    json_body: Option<&str>,
) -> Result<serde_json::Value> {
    let token = git.token()?;
    let auth = git.auth_header(&token);
    let accept = format!("Accept: {}", git.accept_header);
    println!("[{label}] {method} {url}");
    let mut cmd = Command::new("curl");
    cmd.args(["-fsSL", "-X", method]);
    cmd.args(["-H", &accept]);
    cmd.args(["-H", &auth]);
    if let Some(body) = json_body {
        cmd.args(["-H", "Content-Type: application/json"]);
        cmd.args(["-d", body]);
    }
    cmd.arg(url);
    let output = cmd
        .output()
        .with_context(|| format!("[{label}] failed to run curl"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("[{label}] API request failed: {stderr}");
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    if stdout.trim().is_empty() {
        return Ok(serde_json::Value::Null);
    }
    serde_json::from_str(stdout.trim())
        .with_context(|| format!("[{label}] failed to parse API response"))
}

fn git_upload_asset(
    git: &GitContext,
    label: &str,
    upload_url: &str,
    file_path: &Path,
    name: &str,
    content_type: &str,
) -> Result<()> {
    let token = git.token()?;
    let auth = git.auth_header(&token);
    let accept = format!("Accept: {}", git.accept_header);
    let ct = format!("Content-Type: {content_type}");
    let url = GitContext::upload_url_with_name(upload_url, name);
    let data_arg = format!("@{}", file_path.to_string_lossy());
    let attach_arg = format!("attachment=@{}", file_path.to_string_lossy());
    println!("[{label}] Uploading {name}");
    let mut cmd = Command::new("curl");
    cmd.args(["-fsSL", "-X", "POST"]);
    cmd.args(["-H", &accept]);
    cmd.args(["-H", &auth]);
    if git.is_github {
        cmd.args(["-H", &ct]);
        cmd.args(["--data-binary", &data_arg]);
    } else {
        cmd.args(["-F", &attach_arg]);
    }
    cmd.arg(&url);
    let output = cmd
        .output()
        .with_context(|| format!("[{label}] failed to upload {name}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("[{label}] upload of {name} failed: {stderr}");
    }
    Ok(())
}

fn substitute(template: &str, vars: &[(&str, &str)]) -> String {
    let mut result = template.to_string();
    for (key, value) in vars {
        result = result.replace(&format!("{{{key}}}"), value);
    }
    result
}

fn detect_version(config: &Config, version_override: Option<&str>) -> Result<String> {
    if let Some(v) = version_override {
        return Ok(v.strip_prefix('v').unwrap_or(v).to_string());
    }
    let raw = if let Some(cmd) = &config.project.version_command {
        let parts: Vec<&str> = cmd.split_whitespace().collect();
        let (bin, args) = parts
            .split_first()
            .ok_or_else(|| anyhow::anyhow!("empty version_command"))?;
        run_cmd("version", None, bin, args).context("version_command failed")?
    } else {
        run_cmd("version", None, "git", &["describe", "--tags", "--abbrev=0"])
            .context("could not detect version from git tags — use --version or set version_command in config")?
    };
    Ok(raw.strip_prefix('v').unwrap_or(&raw).to_string())
}

fn confirm(prompt: &str) -> Result<bool> {
    eprint!("{prompt} [y/N] ");
    let mut input = String::new();
    std::io::stdin().read_line(&mut input)?;
    Ok(input.trim().eq_ignore_ascii_case("y"))
}

fn parse_host_target(rustc_output: &str) -> Option<String> {
    rustc_output
        .lines()
        .find_map(|line| line.strip_prefix("host: "))
        .map(|s| s.trim().to_string())
}

fn host_target() -> Option<String> {
    Command::new("rustc")
        .args(["-vV"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| parse_host_target(&String::from_utf8_lossy(&o.stdout)))
}

fn needs_cross_linker(host: &str, target: &str) -> bool {
    if host == target {
        return false;
    }
    // macOS toolchain handles both x86_64 and aarch64 natively
    if host.contains("darwin") && target.contains("darwin") {
        return false;
    }
    true
}

fn has_cargo_zigbuild() -> bool {
    Command::new("cargo-zigbuild")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
}

fn parse_installed_targets(output: &str) -> HashSet<String> {
    output
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect()
}

fn installed_targets() -> HashSet<String> {
    Command::new("rustup")
        .args(["target", "list", "--installed"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| parse_installed_targets(&String::from_utf8_lossy(&o.stdout)))
        .unwrap_or_default()
}

fn build_artifacts(config: &Config, version: &str) -> Result<Vec<(String, PathBuf)>> {
    let binary = config.project.binary();
    let staging = PathBuf::from("target/release-staging");
    std::fs::create_dir_all(&staging)?;

    let host = host_target().unwrap_or_default();
    let zigbuild_available = has_cargo_zigbuild();

    let mut archives = Vec::new();
    let mut failed = Vec::new();
    for target in &config.build.targets {
        let vars = &[
            ("target", target.as_str()),
            ("binary", binary),
            ("version", version),
        ];

        let artifact_path = if let Some(cmd_template) = &config.build.command {
            let cmd_str = substitute(cmd_template, vars);
            let cmd_str = if cmd_str.contains("cargo build")
                && zigbuild_available
                && needs_cross_linker(&host, target)
            {
                eprintln!("[build] Using cargo-zigbuild for cross-compilation target {target}");
                cmd_str.replace("cargo build", "cargo zigbuild")
            } else {
                cmd_str
            };
            let parts: Vec<&str> = cmd_str.split_whitespace().collect();
            let (bin, args) = parts
                .split_first()
                .ok_or_else(|| anyhow::anyhow!("empty build command"))?;
            if let Err(e) = run_cmd("build", None, bin, args) {
                eprintln!("[build] Warning: target {target} failed: {e}");
                failed.push(target.clone());
                continue;
            }

            let artifact_template = config
                .build
                .artifact
                .as_ref()
                .expect("artifact required with command");
            PathBuf::from(substitute(artifact_template, vars))
        } else {
            let dir = config
                .build
                .pre_built_dir
                .as_ref()
                .expect("pre_built_dir required");
            PathBuf::from(substitute(dir, vars)).join(format!("{binary}-{target}"))
        };

        if !artifact_path.exists() {
            eprintln!(
                "[build] Warning: target {target} failed: artifact not found at {}",
                artifact_path.display()
            );
            failed.push(target.clone());
            continue;
        }

        let archive_name = format!("{binary}-{version}-{target}.tar.gz");
        let archive_path = staging.join(&archive_name);

        let artifact_dir = artifact_path.parent().unwrap_or_else(|| Path::new("."));
        let artifact_file = artifact_path
            .file_name()
            .ok_or_else(|| anyhow::anyhow!("artifact has no filename"))?
            .to_string_lossy();

        run_cmd(
            "build",
            Some(artifact_dir),
            "tar",
            &[
                "czf",
                &archive_path
                    .canonicalize()
                    .unwrap_or(std::fs::canonicalize(&staging)?.join(&archive_name))
                    .to_string_lossy(),
                &artifact_file,
            ],
        )?;

        archives.push((target.clone(), archive_path));
    }

    if archives.is_empty() {
        bail!("all build targets failed");
    }

    if !failed.is_empty() {
        eprintln!(
            "\n{}/{} targets failed:",
            failed.len(),
            config.build.targets.len()
        );
        for t in &failed {
            eprintln!("  - {t}");
        }
        if config
            .build
            .command
            .as_ref()
            .is_some_and(|c| c.contains("cargo"))
        {
            let installed = installed_targets();
            let (installed_failed, missing): (Vec<_>, Vec<_>) =
                failed.iter().partition(|t| installed.contains(t.as_str()));

            if !missing.is_empty() {
                eprintln!("\nMissing targets (install with rustup):");
                for t in &missing {
                    eprintln!("  rustup target add {t}");
                }
            }
            if !installed_failed.is_empty() {
                eprintln!("\nInstalled but failed to build (missing cross-compilation linker):");
                for t in &installed_failed {
                    eprintln!("  - {t}");
                }
                eprintln!(
                    "  Tip: install `cross` (uses Docker) or `cargo-zigbuild` (uses zig) for cross-compilation"
                );
            }
        }
        if config.build.pre_built_dir.is_some() {
            let dir = config.build.pre_built_dir.as_ref().unwrap();
            eprintln!("\nExpected pre-built artifacts in {dir}:");
            for t in &failed {
                eprintln!("  {dir}{binary}-{t}");
            }
        }
        eprintln!();
        let succeeded: Vec<&str> = archives.iter().map(|(t, _)| t.as_str()).collect();
        eprintln!("Succeeded: {}", succeeded.join(", "));
        if !confirm("Continue with successful targets?")? {
            bail!("aborted by user");
        }
    }

    Ok(archives)
}

fn sha256(path: &Path) -> Result<String> {
    let path_str = path.to_string_lossy();
    let output = run_cmd("sha256", None, "shasum", &["-a", "256", &path_str])
        .or_else(|_| run_cmd("sha256", None, "sha256sum", &[&path_str]))?;
    output
        .split_whitespace()
        .next()
        .map(|s| s.to_string())
        .ok_or_else(|| anyhow::anyhow!("unexpected sha256 output"))
}

fn to_pascal_case(s: &str) -> String {
    s.split(|c| c == '-' || c == '_')
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                None => String::new(),
                Some(first) => {
                    let mut result = first.to_uppercase().to_string();
                    result.extend(chars);
                    result
                }
            }
        })
        .collect()
}

fn command_exists(cmd: &str) -> bool {
    Command::new("sh")
        .args(["-c", &format!("command -v {cmd}")])
        .output()
        .is_ok_and(|o| o.status.success())
}

fn preflight(git: &GitContext, selected: &[&str]) -> Result<()> {
    let mut missing = Vec::new();

    let needs_git_api = selected
        .iter()
        .any(|ch| matches!(*ch, "git" | "homebrew" | "curl" | "nix"));

    if needs_git_api {
        if std::env::var(&git.token_env).is_err() {
            missing.push(format!(
                "{} env var is required for: git, homebrew, curl, nix",
                git.token_env
            ));
        }
        if !command_exists("curl") {
            missing.push("curl command is required for: git, homebrew, curl, nix".to_string());
        }
    }

    if selected.contains(&"nix") && !command_exists("nix") {
        missing.push("nix command is required for: nix".to_string());
    }

    if selected.contains(&"cargo") && !command_exists("cargo") {
        missing.push("cargo command is required for: cargo".to_string());
    }

    let depends_on_git = selected
        .iter()
        .any(|ch| matches!(*ch, "homebrew" | "curl" | "nix"));
    if depends_on_git && !selected.contains(&"git") {
        missing.push("git channel must be selected when using: homebrew, curl, nix".to_string());
    }

    if !missing.is_empty() {
        bail!("preflight check failed:\n  - {}", missing.join("\n  - "));
    }

    Ok(())
}

// --- Public entry point ---

const KNOWN_CHANNELS: &[&str] = &["git", "homebrew", "cargo", "curl", "nix"];

pub fn release(
    config: &Config,
    version_override: Option<&str>,
    channels: Option<&[String]>,
) -> Result<()> {
    let enabled = config.enabled_channels();
    let git = GitContext::from_config(config);

    let selected: Vec<&str> = match channels {
        Some(requested) => {
            for ch in requested {
                if !KNOWN_CHANNELS.contains(&ch.as_str()) {
                    bail!(
                        "unknown channel: {ch} (known: {})",
                        KNOWN_CHANNELS.join(", ")
                    );
                }
                if !enabled.contains(&ch.as_str()) {
                    bail!("channel {ch} is not enabled in config");
                }
            }
            requested.iter().map(|s| s.as_str()).collect()
        }
        None => enabled.clone(),
    };

    if selected.is_empty() {
        println!("No channels enabled.");
        return Ok(());
    }

    preflight(&git, &selected)?;

    let version = detect_version(config, version_override)?;
    println!(
        "Releasing {} v{version} via: {}",
        config.project.name,
        selected.join(", ")
    );

    let archives = build_artifacts(config, &version)?;

    // Run git first so other channels can reference release URLs
    let ordered: Vec<&str> = {
        let mut v = Vec::new();
        if selected.contains(&"git") {
            v.push("git");
        }
        for ch in &selected {
            if *ch != "git" {
                v.push(ch);
            }
        }
        v
    };

    for channel in &ordered {
        match *channel {
            "git" => release_git(config, &git, &version, &archives)?,
            "homebrew" => release_homebrew(config, &git, &version, &archives)?,
            "cargo" => release_cargo(config)?,
            "curl" => release_curl(config, &git, &version)?,
            "nix" => release_nix(config, &git, &version, &archives)?,
            _ => unreachable!(),
        }
    }

    println!("Done.");
    Ok(())
}

// --- Channel implementations ---

fn release_upload_url(
    git: &GitContext,
    channel: &str,
    repo: &str,
    release: &serde_json::Value,
) -> Result<String> {
    if let Some(upload_url) = release["upload_url"].as_str() {
        return Ok(upload_url.to_string());
    }
    // Older Gitea releases may omit upload_url in the response.
    if let Some(release_id) = release["id"].as_i64() {
        return Ok(git.repo_api_url(repo, &format!("/releases/{release_id}/assets")));
    }
    bail!("[{channel}] missing upload_url/id in release response");
}

fn create_release(git: &GitContext, repo: &str, version: &str) -> Result<String> {
    let url = git.repo_api_url(repo, "/releases");
    let body = if git.is_github {
        serde_json::json!({
            "tag_name": format!("v{version}"),
            "name": format!("v{version}"),
            "generate_release_notes": true,
        })
    } else {
        serde_json::json!({
            "tag_name": format!("v{version}"),
            "name": format!("v{version}"),
        })
    };
    let resp = git_api(git, "git", "POST", &url, Some(&body.to_string()))?;
    release_upload_url(git, "git", repo, &resp)
}

fn release_git(
    config: &Config,
    git: &GitContext,
    version: &str,
    archives: &[(String, PathBuf)],
) -> Result<()> {
    let upload_url = create_release(git, &config.project.repo, version)?;
    for (_, path) in archives {
        let name = path.file_name().unwrap().to_string_lossy();
        git_upload_asset(git, "git", &upload_url, path, &name, "application/gzip")?;
    }
    println!("[git] Created release v{version}");
    Ok(())
}

fn release_homebrew(
    config: &Config,
    git: &GitContext,
    version: &str,
    archives: &[(String, PathBuf)],
) -> Result<()> {
    let ch = config.channels.homebrew.as_ref().unwrap();
    let formula_name = ch.formula_name.as_deref().unwrap_or(&config.project.name);
    let binary = config.project.binary();
    let repo = &config.project.repo;

    let release_url = git.repo_api_url(repo, &format!("/releases/tags/v{version}"));
    git_api(git, "homebrew", "GET", &release_url, None).with_context(|| {
        format!("[homebrew] release v{version} not found — run the git channel first")
    })?;

    let mut darwin_arm_sha = String::new();
    let mut darwin_intel_sha = String::new();

    for (target, path) in archives {
        if target.contains("aarch64") && target.contains("apple-darwin") {
            darwin_arm_sha = sha256(path)?;
        } else if target.contains("x86_64") && target.contains("apple-darwin") {
            darwin_intel_sha = sha256(path)?;
        }
    }

    let formula = generate_formula(
        formula_name,
        binary,
        repo,
        version,
        &darwin_arm_sha,
        &darwin_intel_sha,
        &git.web_base_url,
    );

    let file_path = format!("Formula/{formula_name}.rb");
    let api_url = git.contents_api_url(&ch.tap, &file_path);

    // Get current file SHA if it exists (required for updates)
    let existing_sha = git_api(git, "homebrew", "GET", &api_url, None)
        .ok()
        .and_then(|resp| resp["sha"].as_str().map(|s| s.to_string()));

    let mut body = serde_json::json!({
        "message": format!("Update {formula_name} to {version}"),
        "content": BASE64.encode(formula.as_bytes()),
    });
    if let Some(sha) = existing_sha {
        body["sha"] = serde_json::Value::String(sha);
    }

    git_api(git, "homebrew", "PUT", &api_url, Some(&body.to_string()))?;
    println!("[homebrew] Updated formula {formula_name} in {}", ch.tap);
    Ok(())
}

fn generate_formula(
    name: &str,
    binary: &str,
    repo: &str,
    version: &str,
    arm_sha: &str,
    intel_sha: &str,
    web_base_url: &str,
) -> String {
    let class_name = to_pascal_case(name);
    format!(
        r#"class {class_name} < Formula
  desc "{name}"
  homepage "{web_base_url}/{repo}"
  version "{version}"

  on_macos do
    on_arm do
      url "{web_base_url}/{repo}/releases/download/v{version}/{binary}-{version}-aarch64-apple-darwin.tar.gz"
      sha256 "{arm_sha}"
    end
    on_intel do
      url "{web_base_url}/{repo}/releases/download/v{version}/{binary}-{version}-x86_64-apple-darwin.tar.gz"
      sha256 "{intel_sha}"
    end
  end

  def install
    bin.install "{binary}"
  end
end
"#
    )
}

fn release_cargo(config: &Config) -> Result<()> {
    let ch = config.channels.cargo.as_ref().unwrap();
    let crate_name = ch.crate_name.as_deref().unwrap_or(&config.project.name);
    run_cmd("cargo", None, "cargo", &["publish"])?;
    println!("[cargo] Published crate {crate_name}");
    Ok(())
}

fn release_curl(config: &Config, git: &GitContext, version: &str) -> Result<()> {
    let binary = config.project.binary();
    let repo = &config.project.repo;

    let script = generate_install_script(binary, repo, version, &git.web_base_url);

    let script_path = PathBuf::from("target/release-staging/install.sh");
    std::fs::write(&script_path, &script)?;

    // Get the release to find its upload URL
    let url = git.repo_api_url(repo, &format!("/releases/tags/v{version}"));
    let resp = git_api(git, "curl", "GET", &url, None)?;
    let upload_url = release_upload_url(git, "curl", repo, &resp).map_err(|_| {
        anyhow::anyhow!("[curl] could not find release v{version} — is the git channel enabled?")
    })?;

    git_upload_asset(
        git,
        "curl",
        &upload_url,
        &script_path,
        "install.sh",
        "text/plain",
    )?;
    println!("[curl] Uploaded install.sh to release v{version}");
    Ok(())
}

fn generate_install_script(binary: &str, repo: &str, version: &str, web_base_url: &str) -> String {
    format!(
        r#"#!/bin/sh
set -eu

BINARY="{binary}"
REPO="{repo}"
VERSION="{version}"
RELEASE_BASE_URL="{web_base_url}"

OS="$(uname -s)"
ARCH="$(uname -m)"

case "$OS" in
  Linux)  OS_TARGET="unknown-linux-gnu" ;;
  Darwin) OS_TARGET="apple-darwin" ;;
  *)      echo "Unsupported OS: $OS" >&2; exit 1 ;;
esac

case "$ARCH" in
  x86_64|amd64) ARCH_TARGET="x86_64" ;;
  arm64|aarch64) ARCH_TARGET="aarch64" ;;
  *)             echo "Unsupported architecture: $ARCH" >&2; exit 1 ;;
esac

TARGET="${{ARCH_TARGET}}-${{OS_TARGET}}"
URL="${{RELEASE_BASE_URL}}/${{REPO}}/releases/download/v${{VERSION}}/${{BINARY}}-${{VERSION}}-${{TARGET}}.tar.gz"

TMPDIR="$(mktemp -d)"
trap 'rm -rf "$TMPDIR"' EXIT

echo "Downloading $BINARY v$VERSION for $TARGET..."
curl -fsSL "$URL" | tar xz -C "$TMPDIR"

if [ -z "${{INSTALL_DIR:-}}" ]; then
  printf "Install directory [/usr/local/bin]: "
  read -r INSTALL_DIR
  INSTALL_DIR="${{INSTALL_DIR:-/usr/local/bin}}"
fi
install -d "$INSTALL_DIR"
install "$TMPDIR/$BINARY" "$INSTALL_DIR/$BINARY"
echo "Installed $BINARY to $INSTALL_DIR/$BINARY"
"#
    )
}

fn nix_system(target: &str) -> Option<&'static str> {
    if target.contains("x86_64") && target.contains("linux") {
        Some("x86_64-linux")
    } else if target.contains("aarch64") && target.contains("linux") {
        Some("aarch64-linux")
    } else if target.contains("x86_64") && target.contains("darwin") {
        Some("x86_64-darwin")
    } else if target.contains("aarch64") && target.contains("darwin") {
        Some("aarch64-darwin")
    } else {
        None
    }
}

fn generate_flake(
    name: &str,
    binary: &str,
    repo: &str,
    version: &str,
    system_hashes: &[(&str, &str, &str)],
    web_base_url: &str,
) -> String {
    let pkg_entries: Vec<String> = system_hashes
        .iter()
        .map(|(nix_sys, rust_target, sha256_hex)| {
            let entry = r#"      "NIXSYSTEM" = let
        pkgs = nixpkgs.legacyPackages.NIXSYSTEM;
        pkg = pkgs.stdenv.mkDerivation {
          pname = "BINARY";
          version = "VERSION";
          src = pkgs.fetchurl {
            url = "BASEURL/REPO/releases/download/vVERSION/BINARY-VERSION-RUSTTARGET.tar.gz";
            sha256 = "SHA256HEX";
          };
          sourceRoot = ".";
          installPhase = ''
            install -m755 -D BINARY $out/bin/BINARY
          '';
        };
      in { BINARY = pkg; default = pkg; };"#;
            entry
                .replace("NIXSYSTEM", nix_sys)
                .replace("RUSTTARGET", rust_target)
                .replace("SHA256HEX", sha256_hex)
                .replace("BINARY", binary)
                .replace("REPO", repo)
                .replace("BASEURL", web_base_url)
                .replace("VERSION", version)
        })
        .collect();

    let template = r#"{
  description = "DESCRIPTION";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";

  outputs = { self, nixpkgs }: {
    packages = {
PKGENTRIES
    };
  };
}
"#;

    template
        .replace("DESCRIPTION", name)
        .replace("PKGENTRIES", &pkg_entries.join("\n"))
}

fn release_nix(
    config: &Config,
    git: &GitContext,
    version: &str,
    archives: &[(String, PathBuf)],
) -> Result<()> {
    let ch = config.channels.nix.as_ref().unwrap();
    let binary = config.project.binary();
    let repo = &config.project.repo;
    let flake_repo = ch.flake_repo.as_deref().unwrap_or(repo);

    // Download release assets from git release and hash them (local archives may differ)
    let release_url = git.repo_api_url(repo, &format!("/releases/tags/v{version}"));
    let release = git_api(git, "nix", "GET", &release_url, None).with_context(|| {
        format!("[nix] release v{version} not found — run the git channel first")
    })?;

    let staging = PathBuf::from("target/release-staging");
    std::fs::create_dir_all(&staging)?;

    let mut system_hashes = Vec::new();
    for (target, _) in archives {
        let nix_sys = match nix_system(target) {
            Some(s) => s,
            None => continue,
        };
        let asset_name = format!("{binary}-{version}-{target}.tar.gz");
        let download_url = git.release_download_url(repo, version, &asset_name);

        // Verify asset exists in the release
        let assets = release["assets"].as_array();
        let asset_exists = assets.is_some_and(|a| {
            a.iter()
                .any(|asset| asset["name"].as_str() == Some(&asset_name))
        });
        if !asset_exists {
            eprintln!("[nix] Warning: asset {asset_name} not found in release, skipping");
            continue;
        }

        let tmp_path = staging.join(format!("nix-{asset_name}"));
        run_cmd(
            "nix",
            None,
            "curl",
            &["-fsSL", "-o", &tmp_path.to_string_lossy(), &download_url],
        )?;
        let hash = sha256(&tmp_path)?;
        std::fs::remove_file(&tmp_path).ok();
        system_hashes.push((nix_sys, target.as_str(), hash));
    }

    let system_hash_refs: Vec<(&str, &str, &str)> = system_hashes
        .iter()
        .map(|(s, t, h)| (*s, *t, h.as_str()))
        .collect();

    let flake = generate_flake(
        binary,
        binary,
        repo,
        version,
        &system_hash_refs,
        &git.web_base_url,
    );

    // Push file via Contents API, returns Ok(true) if pushed, Ok(false) if skipped
    let push_file = |file: &str, content: &str, msg: &str| -> Result<()> {
        let api_url = git.contents_api_url(flake_repo, file);
        let existing_sha = git_api(git, "nix", "GET", &api_url, None)
            .ok()
            .and_then(|resp| resp["sha"].as_str().map(|s| s.to_string()));
        let mut body = serde_json::json!({
            "message": msg,
            "content": BASE64.encode(content.as_bytes()),
        });
        if let Some(sha) = existing_sha {
            body["sha"] = serde_json::Value::String(sha);
        }
        git_api(git, "nix", "PUT", &api_url, Some(&body.to_string()))?;
        Ok(())
    };

    // Generate flake.lock
    let tmp_dir = std::env::temp_dir().join(format!("releasor2000-nix-{version}"));
    std::fs::create_dir_all(&tmp_dir)?;
    std::fs::write(tmp_dir.join("flake.nix"), &flake)?;
    let lock_cmd = format!("cd '{}' && nix flake lock", tmp_dir.display());
    run_cmd("nix", None, "sh", &["-lc", &lock_cmd])?;
    let flake_lock = std::fs::read_to_string(tmp_dir.join("flake.lock"))
        .context("[nix] failed to read generated flake.lock")?;
    std::fs::remove_dir_all(&tmp_dir).ok();

    push_file(
        "flake.nix",
        &flake,
        &format!("Update {binary} to {version}"),
    )?;
    push_file(
        "flake.lock",
        &flake_lock,
        &format!("Update flake.lock for {binary} {version}"),
    )?;
    println!("[nix] Updated flake.nix and flake.lock in {flake_repo}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const GITHUB_BASE_URL: &str = "https://github.com";

    fn github_git() -> GitContext {
        GitContext {
            web_base_url: GITHUB_BASE_URL.to_string(),
            api_base_url: "https://api.github.com".to_string(),
            token_env: "GITHUB_TOKEN".to_string(),
            auth_prefix: "Bearer",
            accept_header: "application/vnd.github+json",
            is_github: true,
        }
    }

    // --- substitute tests ---

    #[test]
    fn substitute_basic_replacement() {
        let result = substitute("hello {name}", &[("name", "world")]);
        assert_eq!(result, "hello world");
    }

    #[test]
    fn substitute_no_op_when_no_match() {
        let result = substitute("no placeholders here", &[("name", "world")]);
        assert_eq!(result, "no placeholders here");
    }

    #[test]
    fn substitute_multiple_occurrences() {
        let result = substitute("{x} and {x} and {y}", &[("x", "a"), ("y", "b")]);
        assert_eq!(result, "a and a and b");
    }

    // --- to_pascal_case tests ---

    #[test]
    fn to_pascal_case_hyphenated() {
        assert_eq!(to_pascal_case("my-cool-tool"), "MyCoolTool");
    }

    #[test]
    fn to_pascal_case_underscored() {
        assert_eq!(to_pascal_case("my_cool_tool"), "MyCoolTool");
    }

    #[test]
    fn to_pascal_case_single_word() {
        assert_eq!(to_pascal_case("hello"), "Hello");
    }

    // --- generate_formula tests ---

    #[test]
    fn generate_formula_correct_class_name() {
        let formula = generate_formula(
            "my-tool",
            "my-tool",
            "owner/repo",
            "1.0.0",
            "abc",
            "def",
            GITHUB_BASE_URL,
        );
        assert!(formula.starts_with("class MyTool < Formula"));
    }

    #[test]
    fn generate_formula_contains_version() {
        let formula = generate_formula(
            "tool",
            "tool",
            "owner/repo",
            "2.3.4",
            "abc",
            "def",
            GITHUB_BASE_URL,
        );
        assert!(formula.contains("version \"2.3.4\""));
    }

    #[test]
    fn generate_formula_contains_arch_blocks() {
        let formula = generate_formula(
            "tool",
            "tool",
            "owner/repo",
            "1.0.0",
            "armsha",
            "intelsha",
            GITHUB_BASE_URL,
        );
        assert!(formula.contains("on_macos do"));
        assert!(formula.contains("on_arm do"));
        assert!(formula.contains("on_intel do"));
        assert!(formula.contains("sha256 \"armsha\""));
        assert!(formula.contains("sha256 \"intelsha\""));
    }

    #[test]
    fn generate_formula_contains_download_urls() {
        let formula = generate_formula(
            "tool",
            "tool",
            "owner/repo",
            "1.0.0",
            "a",
            "b",
            GITHUB_BASE_URL,
        );
        assert!(formula.contains("https://github.com/owner/repo/releases/download/v1.0.0/tool-1.0.0-aarch64-apple-darwin.tar.gz"));
        assert!(formula.contains("https://github.com/owner/repo/releases/download/v1.0.0/tool-1.0.0-x86_64-apple-darwin.tar.gz"));
    }

    #[test]
    fn generate_formula_contains_binary_install() {
        let formula = generate_formula(
            "tool",
            "mybinary",
            "owner/repo",
            "1.0.0",
            "a",
            "b",
            GITHUB_BASE_URL,
        );
        assert!(formula.contains("bin.install \"mybinary\""));
    }

    #[test]
    fn generate_formula_supports_custom_base_url() {
        let formula = generate_formula(
            "tool",
            "tool",
            "owner/repo",
            "1.0.0",
            "a",
            "b",
            "https://git.example.com",
        );
        assert!(formula.contains("https://git.example.com/owner/repo/releases/download"));
    }

    // --- parse_host_target tests ---

    #[test]
    fn parse_host_target_extracts_host_line() {
        let output = "rustc 1.77.0 (aedd173a2 2024-03-17)\nbinary: rustc\nhost: aarch64-apple-darwin\nrelease: 1.77.0\n";
        assert_eq!(
            parse_host_target(output),
            Some("aarch64-apple-darwin".to_string())
        );
    }

    #[test]
    fn parse_host_target_missing() {
        assert_eq!(parse_host_target("no host here\n"), None);
    }

    // --- needs_cross_linker tests ---

    #[test]
    fn needs_cross_linker_same_os_different_arch() {
        assert!(!needs_cross_linker(
            "aarch64-apple-darwin",
            "x86_64-apple-darwin"
        ));
    }

    #[test]
    fn needs_cross_linker_different_os() {
        assert!(needs_cross_linker(
            "aarch64-apple-darwin",
            "x86_64-unknown-linux-gnu"
        ));
    }

    #[test]
    fn needs_cross_linker_identical() {
        assert!(!needs_cross_linker(
            "aarch64-apple-darwin",
            "aarch64-apple-darwin"
        ));
    }

    #[test]
    fn needs_cross_linker_linux_different_arch() {
        assert!(needs_cross_linker(
            "x86_64-unknown-linux-gnu",
            "aarch64-unknown-linux-gnu"
        ));
    }

    // --- parse_installed_targets tests ---

    #[test]
    fn parse_installed_targets_typical_output() {
        let output = "aarch64-apple-darwin\nx86_64-apple-darwin\nx86_64-unknown-linux-gnu\n";
        let result = parse_installed_targets(output);
        assert_eq!(result.len(), 3);
        assert!(result.contains("aarch64-apple-darwin"));
        assert!(result.contains("x86_64-apple-darwin"));
        assert!(result.contains("x86_64-unknown-linux-gnu"));
    }

    #[test]
    fn parse_installed_targets_empty_output() {
        assert!(parse_installed_targets("").is_empty());
        assert!(parse_installed_targets("  \n  \n").is_empty());
    }

    #[test]
    fn parse_installed_targets_trims_whitespace() {
        let output = "  aarch64-apple-darwin  \n  x86_64-apple-darwin \n";
        let result = parse_installed_targets(output);
        assert_eq!(result.len(), 2);
        assert!(result.contains("aarch64-apple-darwin"));
        assert!(result.contains("x86_64-apple-darwin"));
    }

    // --- generate_install_script tests ---

    #[test]
    fn generate_install_script_starts_with_shebang() {
        let script = generate_install_script("tool", "owner/repo", "1.0.0", GITHUB_BASE_URL);
        assert!(script.starts_with("#!/bin/sh"));
    }

    #[test]
    fn generate_install_script_contains_repo_binary_version() {
        let script = generate_install_script("mytool", "cool/repo", "3.2.1", GITHUB_BASE_URL);
        assert!(script.contains("BINARY=\"mytool\""));
        assert!(script.contains("REPO=\"cool/repo\""));
        assert!(script.contains("VERSION=\"3.2.1\""));
        assert!(script.contains("RELEASE_BASE_URL=\"https://github.com\""));
    }

    #[test]
    fn generate_install_script_handles_all_arch_os_combos() {
        let script = generate_install_script("tool", "owner/repo", "1.0.0", GITHUB_BASE_URL);
        assert!(script.contains("Linux)"));
        assert!(script.contains("Darwin)"));
        assert!(script.contains("x86_64|amd64)"));
        assert!(script.contains("arm64|aarch64)"));
    }

    #[test]
    fn generate_install_script_prompts_for_install_dir() {
        let script = generate_install_script("tool", "owner/repo", "1.0.0", GITHUB_BASE_URL);
        assert!(script.contains("printf \"Install directory [/usr/local/bin]: \""));
        assert!(script.contains("read -r INSTALL_DIR"));
    }

    // --- nix_system tests ---

    #[test]
    fn nix_system_x86_64_linux() {
        assert_eq!(nix_system("x86_64-unknown-linux-gnu"), Some("x86_64-linux"));
    }

    #[test]
    fn nix_system_aarch64_linux() {
        assert_eq!(
            nix_system("aarch64-unknown-linux-gnu"),
            Some("aarch64-linux")
        );
    }

    #[test]
    fn nix_system_x86_64_darwin() {
        assert_eq!(nix_system("x86_64-apple-darwin"), Some("x86_64-darwin"));
    }

    #[test]
    fn nix_system_aarch64_darwin() {
        assert_eq!(nix_system("aarch64-apple-darwin"), Some("aarch64-darwin"));
    }

    #[test]
    fn nix_system_unknown_target() {
        assert_eq!(nix_system("wasm32-unknown-unknown"), None);
    }

    // --- generate_flake tests ---

    #[test]
    fn generate_flake_contains_description() {
        let flake = generate_flake(
            "mytool",
            "mytool",
            "owner/repo",
            "1.0.0",
            &[("x86_64-linux", "x86_64-unknown-linux-gnu", "abc123")],
            GITHUB_BASE_URL,
        );
        assert!(flake.contains(r#"description = "mytool""#));
    }

    #[test]
    fn generate_flake_contains_version() {
        let flake = generate_flake(
            "mytool",
            "mytool",
            "owner/repo",
            "2.3.4",
            &[("x86_64-linux", "x86_64-unknown-linux-gnu", "abc123")],
            GITHUB_BASE_URL,
        );
        assert!(flake.contains(r#"version = "2.3.4""#));
    }

    #[test]
    fn generate_flake_contains_sha256_values() {
        let flake = generate_flake(
            "mytool",
            "mytool",
            "owner/repo",
            "1.0.0",
            &[
                ("x86_64-linux", "x86_64-unknown-linux-gnu", "deadbeef"),
                ("aarch64-darwin", "aarch64-apple-darwin", "cafebabe"),
            ],
            GITHUB_BASE_URL,
        );
        assert!(flake.contains(r#""deadbeef""#));
        assert!(flake.contains(r#""cafebabe""#));
    }

    #[test]
    fn generate_flake_contains_binary_name() {
        let flake = generate_flake(
            "mytool",
            "mybinary",
            "owner/repo",
            "1.0.0",
            &[("x86_64-linux", "x86_64-unknown-linux-gnu", "abc")],
            GITHUB_BASE_URL,
        );
        assert!(flake.contains(r#"pname = "mybinary""#));
        assert!(flake.contains("install -m755 -D mybinary $out/bin/mybinary"));
    }

    #[test]
    fn generate_flake_contains_download_urls() {
        let flake = generate_flake(
            "mytool",
            "mytool",
            "owner/repo",
            "1.0.0",
            &[
                ("x86_64-linux", "x86_64-unknown-linux-gnu", "abc"),
                ("aarch64-darwin", "aarch64-apple-darwin", "def"),
            ],
            GITHUB_BASE_URL,
        );
        assert!(
            flake.contains("https://github.com/owner/repo/releases/download/v1.0.0/mytool-1.0.0-")
        );
    }

    #[test]
    fn generate_flake_contains_system_entries() {
        let flake = generate_flake(
            "mytool",
            "mytool",
            "owner/repo",
            "1.0.0",
            &[
                ("x86_64-linux", "x86_64-unknown-linux-gnu", "abc"),
                ("aarch64-darwin", "aarch64-apple-darwin", "def"),
            ],
            GITHUB_BASE_URL,
        );
        assert!(flake.contains(r#""x86_64-linux" = let"#));
        assert!(flake.contains(r#""aarch64-darwin" = let"#));
    }

    #[test]
    fn generate_flake_supports_custom_base_url() {
        let flake = generate_flake(
            "mytool",
            "mytool",
            "owner/repo",
            "1.0.0",
            &[("x86_64-linux", "x86_64-unknown-linux-gnu", "abc")],
            "https://git.example.com",
        );
        assert!(flake.contains("https://git.example.com/owner/repo/releases/download"));
    }

    // --- preflight tests ---

    #[test]
    fn preflight_ok_with_no_channels() {
        let git = github_git();
        assert!(preflight(&git, &[]).is_ok());
    }

    #[test]
    fn preflight_requires_default_github_token() {
        // Remove GITHUB_TOKEN to ensure the check triggers
        let saved = std::env::var("GITHUB_TOKEN").ok();
        unsafe { std::env::remove_var("GITHUB_TOKEN") };

        let git = github_git();
        let err = preflight(&git, &["git"]).unwrap_err();
        assert!(err.to_string().contains("GITHUB_TOKEN"), "got: {err}");

        if let Some(val) = saved {
            unsafe { std::env::set_var("GITHUB_TOKEN", val) };
        }
    }

    #[test]
    fn preflight_uses_git_token_env_name() {
        let saved = std::env::var("GITEA_TOKEN").ok();
        unsafe { std::env::remove_var("GITEA_TOKEN") };
        let git = GitContext {
            web_base_url: "https://git.example.com".to_string(),
            api_base_url: "https://git.example.com/api/v1".to_string(),
            token_env: "GITEA_TOKEN".to_string(),
            auth_prefix: "token",
            accept_header: "application/json",
            is_github: false,
        };
        let err = preflight(&git, &["git"]).unwrap_err();
        assert!(err.to_string().contains("GITEA_TOKEN"), "got: {err}");
        if let Some(val) = saved {
            unsafe { std::env::set_var("GITEA_TOKEN", val) };
        }
    }

    #[test]
    fn preflight_requires_nix() {
        // This test assumes `nix` is not installed in the test environment,
        // which is typical for CI. If nix IS installed, we can't test the
        // negative case, so skip.
        if command_exists("nix") {
            return;
        }

        // Ensure GITHUB_TOKEN is set so that check doesn't also fail
        let saved = std::env::var("GITHUB_TOKEN").ok();
        unsafe { std::env::set_var("GITHUB_TOKEN", "fake-token-for-test") };

        let git = github_git();
        let err = preflight(&git, &["git", "nix"]).unwrap_err();
        assert!(err.to_string().contains("nix command"), "got: {err}");

        match saved {
            Some(val) => unsafe { std::env::set_var("GITHUB_TOKEN", val) },
            None => unsafe { std::env::remove_var("GITHUB_TOKEN") },
        }
    }

    #[test]
    fn preflight_requires_git_for_dependent_channels() {
        // Ensure GITHUB_TOKEN is set so only the dependency check triggers
        let saved = std::env::var("GITHUB_TOKEN").ok();
        unsafe { std::env::set_var("GITHUB_TOKEN", "fake-token-for-test") };

        let git = github_git();
        for ch in &["homebrew", "curl", "nix"] {
            let err = preflight(&git, &[*ch]).unwrap_err();
            assert!(
                err.to_string().contains("git channel must be selected"),
                "channel {ch}: got: {err}"
            );
        }

        match saved {
            Some(val) => unsafe { std::env::set_var("GITHUB_TOKEN", val) },
            None => unsafe { std::env::remove_var("GITHUB_TOKEN") },
        }
    }
}
