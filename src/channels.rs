use anyhow::{Context, Result, bail};
use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::config::Config;

// --- Shared infrastructure ---

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

fn github_token() -> Result<String> {
    std::env::var("GITHUB_TOKEN").context("GITHUB_TOKEN environment variable not set")
}

fn github_api(label: &str, method: &str, url: &str, json_body: Option<&str>) -> Result<serde_json::Value> {
    let token = github_token()?;
    let auth = format!("Authorization: Bearer {token}");
    println!("[{label}] {method} {url}");
    let mut cmd = Command::new("curl");
    cmd.args(["-fsSL", "-X", method]);
    cmd.args(["-H", "Accept: application/vnd.github+json"]);
    cmd.args(["-H", &auth]);
    if let Some(body) = json_body {
        cmd.args(["-H", "Content-Type: application/json"]);
        cmd.args(["-d", body]);
    }
    cmd.arg(url);
    let output = cmd.output().with_context(|| format!("[{label}] failed to run curl"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("[{label}] API request failed: {stderr}");
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    if stdout.trim().is_empty() {
        return Ok(serde_json::Value::Null);
    }
    serde_json::from_str(stdout.trim()).with_context(|| format!("[{label}] failed to parse API response"))
}

fn github_upload_asset(label: &str, upload_url: &str, file_path: &Path, name: &str, content_type: &str) -> Result<()> {
    let token = github_token()?;
    let auth = format!("Authorization: Bearer {token}");
    let ct = format!("Content-Type: {content_type}");
    let url = format!("{upload_url}?name={name}");
    let data_arg = format!("@{}", file_path.to_string_lossy());
    println!("[{label}] Uploading {name}");
    let mut cmd = Command::new("curl");
    cmd.args(["-fsSL", "-X", "POST"]);
    cmd.args(["-H", "Accept: application/vnd.github+json"]);
    cmd.args(["-H", &auth]);
    cmd.args(["-H", &ct]);
    cmd.args(["--data-binary", &data_arg]);
    cmd.arg(&url);
    let output = cmd.output().with_context(|| format!("[{label}] failed to upload {name}"))?;
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
        run_cmd("version", None, bin, args)
            .context("version_command failed")?
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
    let host_suffix = host.splitn(2, '-').nth(1).unwrap_or(host);
    let target_suffix = target.splitn(2, '-').nth(1).unwrap_or(target);
    host_suffix != target_suffix
}

fn has_cargo_zigbuild() -> bool {
    Command::new("cargo-zigbuild")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
}

fn parse_installed_targets(output: &str) -> HashSet<String> {
    output.lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect()
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
            eprintln!("[build] Warning: target {target} failed: artifact not found at {}", artifact_path.display());
            failed.push(target.clone());
            continue;
        }

        let archive_name = format!("{binary}-{version}-{target}.tar.gz");
        let archive_path = staging.join(&archive_name);

        let artifact_dir = artifact_path
            .parent()
            .unwrap_or_else(|| Path::new("."));
        let artifact_file = artifact_path
            .file_name()
            .ok_or_else(|| anyhow::anyhow!("artifact has no filename"))?
            .to_string_lossy();

        run_cmd(
            "build",
            Some(artifact_dir),
            "tar",
            &["czf", &archive_path.canonicalize().unwrap_or(std::fs::canonicalize(&staging)?.join(&archive_name)).to_string_lossy(), &artifact_file],
        )?;

        archives.push((target.clone(), archive_path));
    }

    if archives.is_empty() {
        bail!("all build targets failed");
    }

    if !failed.is_empty() {
        eprintln!("\n{}/{} targets failed:", failed.len(), config.build.targets.len());
        for t in &failed {
            eprintln!("  - {t}");
        }
        if config.build.command.as_ref().is_some_and(|c| c.contains("cargo")) {
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
                eprintln!("  Tip: install `cross` (uses Docker) or `cargo-zigbuild` (uses zig) for cross-compilation");
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
    let output = run_cmd("sha256", None, "shasum", &["-a", "256", &path.to_string_lossy()])?;
    output
        .split_whitespace()
        .next()
        .map(|s| s.to_string())
        .ok_or_else(|| anyhow::anyhow!("unexpected shasum output"))
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

// --- Public entry point ---

const KNOWN_CHANNELS: &[&str] = &["github", "homebrew", "cargo", "curl", "nix"];

pub fn release(config: &Config, version_override: Option<&str>, channels: Option<&[String]>) -> Result<()> {
    let enabled = config.enabled_channels();

    let selected: Vec<&str> = match channels {
        Some(requested) => {
            for ch in requested {
                if !KNOWN_CHANNELS.contains(&ch.as_str()) {
                    bail!("unknown channel: {ch} (known: {})", KNOWN_CHANNELS.join(", "));
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

    let version = detect_version(config, version_override)?;
    println!(
        "Releasing {} v{version} via: {}",
        config.project.name,
        selected.join(", ")
    );

    let archives = build_artifacts(config, &version)?;

    // Run github first so other channels can reference release URLs
    let ordered: Vec<&str> = {
        let mut v = Vec::new();
        if selected.contains(&"github") {
            v.push("github");
        }
        for ch in &selected {
            if *ch != "github" {
                v.push(ch);
            }
        }
        v
    };

    for channel in &ordered {
        match *channel {
            "github" => release_github(config, &version, &archives)?,
            "homebrew" => release_homebrew(config, &version, &archives)?,
            "cargo" => release_cargo(config)?,
            "curl" => release_curl(config, &version)?,
            "nix" => release_nix(config, &version)?,
            _ => unreachable!(),
        }
    }

    println!("Done.");
    Ok(())
}

// --- Channel implementations ---

fn create_github_release(repo: &str, version: &str) -> Result<String> {
    let url = format!("https://api.github.com/repos/{repo}/releases");
    let body = serde_json::json!({
        "tag_name": format!("v{version}"),
        "name": format!("v{version}"),
        "generate_release_notes": true,
    });
    let resp = github_api("github", "POST", &url, Some(&body.to_string()))?;
    let upload_url = resp["upload_url"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("[github] missing upload_url in response"))?;
    // Strip the {?name,label} URI template suffix
    Ok(upload_url.split('{').next().unwrap_or(upload_url).to_string())
}

fn release_github(config: &Config, version: &str, archives: &[(String, PathBuf)]) -> Result<()> {
    let upload_url = create_github_release(&config.project.repo, version)?;
    for (_, path) in archives {
        let name = path.file_name().unwrap().to_string_lossy();
        github_upload_asset("github", &upload_url, path, &name, "application/gzip")?;
    }
    println!("[github] Created release v{version}");
    Ok(())
}

fn release_homebrew(
    config: &Config,
    version: &str,
    archives: &[(String, PathBuf)],
) -> Result<()> {
    let ch = config.channels.homebrew.as_ref().unwrap();
    let formula_name = ch.formula_name.as_deref().unwrap_or(&config.project.name);
    let binary = config.project.binary();
    let repo = &config.project.repo;

    let release_url = format!("https://api.github.com/repos/{repo}/releases/tags/v{version}");
    github_api("homebrew", "GET", &release_url, None)
        .with_context(|| format!("[homebrew] GitHub release v{version} not found — run the github channel first"))?;

    let mut darwin_arm_sha = String::new();
    let mut darwin_intel_sha = String::new();

    for (target, path) in archives {
        if target.contains("aarch64") && target.contains("apple-darwin") {
            darwin_arm_sha = sha256(path)?;
        } else if target.contains("x86_64") && target.contains("apple-darwin") {
            darwin_intel_sha = sha256(path)?;
        }
    }

    let formula = generate_formula(formula_name, binary, repo, version, &darwin_arm_sha, &darwin_intel_sha);

    let file_path = format!("Formula/{formula_name}.rb");
    let api_url = format!("https://api.github.com/repos/{}/contents/{}", ch.tap, file_path);

    // Get current file SHA if it exists (required for updates)
    let existing_sha = github_api("homebrew", "GET", &api_url, None)
        .ok()
        .and_then(|resp| resp["sha"].as_str().map(|s| s.to_string()));

    let mut body = serde_json::json!({
        "message": format!("Update {formula_name} to {version}"),
        "content": BASE64.encode(formula.as_bytes()),
    });
    if let Some(sha) = existing_sha {
        body["sha"] = serde_json::Value::String(sha);
    }

    github_api("homebrew", "PUT", &api_url, Some(&body.to_string()))?;
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
) -> String {
    let class_name = to_pascal_case(name);
    format!(
        r#"class {class_name} < Formula
  desc "{name}"
  homepage "https://github.com/{repo}"
  version "{version}"

  on_macos do
    on_arm do
      url "https://github.com/{repo}/releases/download/v{version}/{binary}-{version}-aarch64-apple-darwin.tar.gz"
      sha256 "{arm_sha}"
    end
    on_intel do
      url "https://github.com/{repo}/releases/download/v{version}/{binary}-{version}-x86_64-apple-darwin.tar.gz"
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

fn release_curl(config: &Config, version: &str) -> Result<()> {
    let binary = config.project.binary();
    let repo = &config.project.repo;

    let script = generate_install_script(binary, repo, version);

    let script_path = PathBuf::from("target/release-staging/install.sh");
    std::fs::write(&script_path, &script)?;

    // Get the release to find its upload URL
    let url = format!("https://api.github.com/repos/{repo}/releases/tags/v{version}");
    let resp = github_api("curl", "GET", &url, None)?;
    let upload_url = resp["upload_url"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("[curl] could not find release v{version} — is the github channel enabled?"))?;
    let upload_url = upload_url.split('{').next().unwrap_or(upload_url);

    github_upload_asset("curl", upload_url, &script_path, "install.sh", "text/plain")?;
    println!("[curl] Uploaded install.sh to release v{version}");
    Ok(())
}

fn generate_install_script(binary: &str, repo: &str, version: &str) -> String {
    format!(
        r#"#!/bin/sh
set -eu

BINARY="{binary}"
REPO="{repo}"
VERSION="{version}"

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
URL="https://github.com/${{REPO}}/releases/download/v${{VERSION}}/${{BINARY}}-${{VERSION}}-${{TARGET}}.tar.gz"

TMPDIR="$(mktemp -d)"
trap 'rm -rf "$TMPDIR"' EXIT

echo "Downloading $BINARY v$VERSION for $TARGET..."
curl -fsSL "$URL" | tar xz -C "$TMPDIR"

INSTALL_DIR="${{INSTALL_DIR:-/usr/local/bin}}"
install -d "$INSTALL_DIR"
install "$TMPDIR/$BINARY" "$INSTALL_DIR/$BINARY"
echo "Installed $BINARY to $INSTALL_DIR/$BINARY"
"#
    )
}

fn release_nix(config: &Config, version: &str) -> Result<()> {
    let ch = config.channels.nix.as_ref().unwrap();
    let repo = &config.project.repo;

    let source_url = format!(
        "https://github.com/{repo}/archive/refs/tags/v{version}.tar.gz"
    );
    let hash = run_cmd(
        "nix",
        None,
        "nix-prefetch-url",
        &["--unpack", &source_url],
    )?;

    let url = format!("https://api.github.com/repos/{}/issues", ch.flake_repo);
    let body = serde_json::json!({
        "title": format!("Update to v{version}"),
        "body": format!("New release v{version}\n\nSource: {source_url}\nSHA256: {hash}"),
    });
    github_api("nix", "POST", &url, Some(&body.to_string()))?;
    println!("[nix] Created update issue on {}", ch.flake_repo);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let formula = generate_formula("my-tool", "my-tool", "owner/repo", "1.0.0", "abc", "def");
        assert!(formula.starts_with("class MyTool < Formula"));
    }

    #[test]
    fn generate_formula_contains_version() {
        let formula = generate_formula("tool", "tool", "owner/repo", "2.3.4", "abc", "def");
        assert!(formula.contains("version \"2.3.4\""));
    }

    #[test]
    fn generate_formula_contains_arch_blocks() {
        let formula = generate_formula("tool", "tool", "owner/repo", "1.0.0", "armsha", "intelsha");
        assert!(formula.contains("on_macos do"));
        assert!(formula.contains("on_arm do"));
        assert!(formula.contains("on_intel do"));
        assert!(formula.contains("sha256 \"armsha\""));
        assert!(formula.contains("sha256 \"intelsha\""));
    }

    #[test]
    fn generate_formula_contains_download_urls() {
        let formula = generate_formula("tool", "tool", "owner/repo", "1.0.0", "a", "b");
        assert!(formula.contains("https://github.com/owner/repo/releases/download/v1.0.0/tool-1.0.0-aarch64-apple-darwin.tar.gz"));
        assert!(formula.contains("https://github.com/owner/repo/releases/download/v1.0.0/tool-1.0.0-x86_64-apple-darwin.tar.gz"));
    }

    #[test]
    fn generate_formula_contains_binary_install() {
        let formula = generate_formula("tool", "mybinary", "owner/repo", "1.0.0", "a", "b");
        assert!(formula.contains("bin.install \"mybinary\""));
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
        let script = generate_install_script("tool", "owner/repo", "1.0.0");
        assert!(script.starts_with("#!/bin/sh"));
    }

    #[test]
    fn generate_install_script_contains_repo_binary_version() {
        let script = generate_install_script("mytool", "cool/repo", "3.2.1");
        assert!(script.contains("BINARY=\"mytool\""));
        assert!(script.contains("REPO=\"cool/repo\""));
        assert!(script.contains("VERSION=\"3.2.1\""));
    }

    #[test]
    fn generate_install_script_handles_all_arch_os_combos() {
        let script = generate_install_script("tool", "owner/repo", "1.0.0");
        assert!(script.contains("Linux)"));
        assert!(script.contains("Darwin)"));
        assert!(script.contains("x86_64|amd64)"));
        assert!(script.contains("arm64|aarch64)"));
    }
}
