use anyhow::{Context, Result, bail};
use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Command;
use toml::Value as TomlValue;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProjectKind {
    Rust,
}

fn normalize_version(v: &str) -> String {
    v.strip_prefix('v').unwrap_or(v).trim().to_string()
}

fn detect_project_kind() -> Result<ProjectKind> {
    if Path::new("Cargo.toml").exists() {
        return Ok(ProjectKind::Rust);
    }
    bail!(
        "auto-tag is enabled, but no supported manifest was found in the current directory (expected Cargo.toml for Rust projects)"
    )
}

fn rust_manifest_package_version(manifest_path: &Path) -> Result<Option<String>> {
    let manifest = std::fs::read_to_string(manifest_path)
        .with_context(|| format!("failed reading {}", manifest_path.display()))?;
    let parsed: TomlValue = toml::from_str(&manifest)
        .with_context(|| format!("failed parsing {}", manifest_path.display()))?;
    let version = parsed
        .get("package")
        .and_then(TomlValue::as_table)
        .and_then(|package| package.get("version"))
        .and_then(TomlValue::as_str)
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(str::to_string);
    Ok(version)
}

#[cfg(test)]
fn rust_manifest_version(manifest_path: &Path) -> Result<String> {
    rust_manifest_package_version(manifest_path)?
        .ok_or_else(|| anyhow::anyhow!("{} is missing package.version", manifest_path.display()))
}

fn rust_workspace_package_version(
    config: &Config,
    cargo_package: Option<&CargoPackageSelection>,
    workspace: &CargoWorkspace,
) -> Result<Option<String>> {
    if let Some(package) = cargo_package {
        return workspace
            .version_for_package(&package.name)?
            .map(Some)
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "project.package {} was not found in cargo workspace",
                    package.name
                )
            });
    }

    let binary = config.project.primary_binary();
    if let Some(package) = workspace.package_for_binary(binary)? {
        return workspace.version_for_package(&package.name);
    }

    if workspace.packages.len() == 1 {
        return Ok(Some(workspace.packages[0].version.clone()));
    }

    Ok(None)
}

fn detect_manifest_version(
    config: &Config,
    cargo_package: Option<&CargoPackageSelection>,
) -> Result<String> {
    match detect_project_kind()? {
        ProjectKind::Rust => {
            if cargo_package.is_some() {
                let workspace =
                    CargoWorkspace::load().context("detecting Cargo workspace package version")?;
                if let Some(version) =
                    rust_workspace_package_version(config, cargo_package, &workspace)?
                {
                    return Ok(version);
                }
            }

            if let Some(version) = rust_manifest_package_version(Path::new("Cargo.toml"))? {
                return Ok(version);
            }

            let workspace =
                CargoWorkspace::load().context("detecting Cargo workspace package version")?;
            if let Some(version) =
                rust_workspace_package_version(config, cargo_package, &workspace)?
            {
                return Ok(version);
            }

            let binary = config.project.primary_binary();
            bail!(
                "Cargo.toml is a workspace root without package.version, and no workspace package provides binary {binary}; set project.package or disable project.auto-tag"
            )
        }
    }
}

fn local_tag_exists(tag: &str) -> Result<bool> {
    let ref_name = format!("refs/tags/{tag}");
    let output = Command::new("git")
        .args(["rev-parse", "-q", "--verify", &ref_name])
        .output()
        .context("[tag] failed to run git rev-parse")?;
    if output.status.success() {
        return Ok(true);
    }
    if output.status.code() == Some(1) {
        return Ok(false);
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    bail!("[tag] could not check local tag {tag}: {stderr}");
}

fn remote_tag_exists(tag: &str) -> Result<bool> {
    let ref_name = format!("refs/tags/{tag}");
    let output = Command::new("git")
        .args(["ls-remote", "--exit-code", "--tags", "origin", &ref_name])
        .output()
        .context("[tag] failed to run git ls-remote")?;
    if output.status.success() {
        return Ok(true);
    }
    if output.status.code() == Some(2) {
        return Ok(false);
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    bail!("[tag] could not check remote tag {tag} on origin: {stderr}");
}

fn create_and_push_tag(version: &str) -> Result<()> {
    let tag = format!("v{version}");
    if local_tag_exists(&tag)? {
        bail!("[tag] tag {tag} already exists locally");
    }
    if remote_tag_exists(&tag)? {
        bail!("[tag] tag {tag} already exists on origin");
    }

    let message = format!("Release {tag}");
    run_cmd("tag", None, "git", &["tag", "-a", &tag, "-m", &message])?;
    run_cmd("tag", None, "git", &["push", "origin", &tag])?;
    println!("[tag] Created and pushed {tag}");
    Ok(())
}

fn detect_version(config: &Config, version_override: Option<&str>) -> Result<String> {
    if let Some(v) = version_override {
        return Ok(normalize_version(v));
    }
    let raw = if let Some(cmd) = &config.project.version_command {
        let parts: Vec<&str> = cmd.split_whitespace().collect();
        let (bin, args) = parts
            .split_first()
            .ok_or_else(|| anyhow::anyhow!("empty version-command"))?;
        run_cmd("version", None, bin, args).context("version-command failed")?
    } else {
        run_cmd("version", None, "git", &["describe", "--tags", "--abbrev=0"])
            .context("could not detect version from git tags — use --version or set version-command in config")?
    };
    Ok(normalize_version(&raw))
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

fn has_cross() -> bool {
    Command::new("cross")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
}

fn linker_env_var(target: &str) -> String {
    format!(
        "CARGO_TARGET_{}_LINKER",
        target.to_ascii_uppercase().replace('-', "_")
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CargoPackageSelection {
    name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CargoWorkspacePackage {
    name: String,
    version: String,
    binary_targets: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CargoWorkspace {
    packages: Vec<CargoWorkspacePackage>,
}

impl CargoWorkspace {
    fn load() -> Result<Self> {
        let output = Command::new("cargo")
            .args(["metadata", "--no-deps", "--format-version", "1"])
            .output()
            .context("failed to run cargo metadata")?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("cargo metadata failed: {stderr}");
        }
        Self::from_metadata_json(&String::from_utf8_lossy(&output.stdout))
    }

    fn from_metadata_json(metadata_json: &str) -> Result<Self> {
        let metadata: serde_json::Value =
            serde_json::from_str(metadata_json).context("parsing cargo metadata")?;
        let workspace_members = metadata["workspace_members"]
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("cargo metadata missing workspace_members"))?;
        let member_ids: HashSet<String> = workspace_members
            .iter()
            .filter_map(|member| member.as_str().map(str::to_string))
            .collect();
        let packages = metadata["packages"]
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("cargo metadata missing packages"))?;

        let mut workspace_packages = Vec::new();
        for package in packages {
            let Some(id) = package["id"].as_str() else {
                continue;
            };
            if !member_ids.contains(id) {
                continue;
            }
            let Some(name) = package["name"].as_str() else {
                continue;
            };
            let Some(version) = package["version"].as_str() else {
                continue;
            };

            let mut binary_targets = Vec::new();
            if let Some(targets) = package["targets"].as_array() {
                for target in targets {
                    let is_bin = target["kind"]
                        .as_array()
                        .is_some_and(|kinds| kinds.iter().any(|kind| kind.as_str() == Some("bin")));
                    if !is_bin {
                        continue;
                    }
                    let Some(target_name) = target["name"].as_str() else {
                        continue;
                    };
                    if !binary_targets.iter().any(|name| name == target_name) {
                        binary_targets.push(target_name.to_string());
                    }
                }
            }

            workspace_packages.push(CargoWorkspacePackage {
                name: name.to_string(),
                version: version.to_string(),
                binary_targets,
            });
        }

        Ok(Self {
            packages: workspace_packages,
        })
    }

    fn package_for_binary(&self, binary: &str) -> Result<Option<CargoPackageSelection>> {
        let matches: Vec<&str> = self
            .packages
            .iter()
            .filter(|package| package.binary_targets.iter().any(|target| target == binary))
            .map(|package| package.name.as_str())
            .collect();

        match matches.as_slice() {
            [] => Ok(None),
            [name] => Ok(Some(CargoPackageSelection {
                name: (*name).to_string(),
            })),
            names => bail!(
                "could not infer cargo package: binary {binary} is provided by multiple workspace packages ({}); set project.package",
                names.join(", ")
            ),
        }
    }

    fn version_for_package(&self, package_name: &str) -> Result<Option<String>> {
        let matches: Vec<&CargoWorkspacePackage> = self
            .packages
            .iter()
            .filter(|package| package.name == package_name)
            .collect();

        match matches.as_slice() {
            [] => Ok(None),
            [package] => Ok(Some(package.version.clone())),
            packages => bail!(
                "could not infer cargo package version: multiple workspace packages are named {package_name} ({})",
                packages.len()
            ),
        }
    }

    fn is_multi_package(&self) -> bool {
        self.packages.len() > 1
    }
}

impl CargoPackageSelection {
    fn resolve(config: &Config, selected: &[&str]) -> Result<Option<Self>> {
        if let Some(package) = &config.project.package {
            return Ok(Some(Self {
                name: package.clone(),
            }));
        }

        if !Self::should_detect(config, selected) {
            return Ok(None);
        }

        let package_required =
            selected.contains(&"cargo") || Self::templates_require_package(config);
        let workspace = match CargoWorkspace::load() {
            Ok(workspace) => workspace,
            Err(err) => {
                if package_required {
                    return Err(err.context("detecting cargo workspace package"));
                }
                return Ok(None);
            }
        };

        let binary = config.project.primary_binary();
        if let Some(package) = workspace.package_for_binary(binary)? {
            return Ok(Some(package));
        }

        if Self::templates_require_package(config) {
            bail!(
                "could not resolve {{package}}: no workspace package provides binary {binary}; set project.package"
            );
        }

        if selected.contains(&"cargo") && workspace.is_multi_package() {
            bail!(
                "could not infer cargo package: no workspace package provides binary {binary}; set project.package"
            );
        }

        Ok(None)
    }

    fn should_detect(config: &Config, selected: &[&str]) -> bool {
        selected.contains(&"cargo")
            || Self::templates_require_package(config)
            || config
                .build
                .command
                .as_ref()
                .is_some_and(|command| is_cargo_build_like_command(command))
    }

    fn templates_require_package(config: &Config) -> bool {
        config
            .build
            .command
            .as_ref()
            .is_some_and(|template| template.contains("{package}"))
            || config
                .build
                .artifact
                .as_ref()
                .is_some_and(|template| template.contains("{package}"))
            || config
                .build
                .pre_built_dir
                .as_ref()
                .is_some_and(|template| template.contains("{package}"))
    }

    fn augment_build_command(&self, cmd_str: &str, binary: &str) -> String {
        if !is_cargo_build_like_command(cmd_str) {
            return cmd_str.to_string();
        }

        let parts: Vec<&str> = cmd_str.split_whitespace().collect();
        if parts
            .iter()
            .any(|part| *part == "--workspace" || *part == "--all")
        {
            return cmd_str.to_string();
        }

        let mut additions = Vec::new();
        if !has_package_selector(&parts) {
            additions.push(format!("--package {}", self.name));
        }
        if !has_binary_selector(&parts) {
            additions.push(format!("--bin {binary}"));
        }

        if additions.is_empty() {
            cmd_str.to_string()
        } else {
            format!("{cmd_str} {}", additions.join(" "))
        }
    }
}

fn is_cargo_build_like_command(cmd_str: &str) -> bool {
    let parts: Vec<&str> = cmd_str.split_whitespace().collect();
    matches!(
        parts.as_slice(),
        ["cargo", "build", ..] | ["cargo", "zigbuild", ..] | ["cross", "build", ..]
    )
}

fn has_package_selector(parts: &[&str]) -> bool {
    parts.iter().any(|part| {
        *part == "--package"
            || part.starts_with("--package=")
            || *part == "-p"
            || part.starts_with("-p")
    })
}

fn has_binary_selector(parts: &[&str]) -> bool {
    parts.iter().any(|part| {
        *part == "--bin"
            || part.starts_with("--bin=")
            || *part == "--bins"
            || *part == "--all-targets"
            || *part == "--lib"
            || *part == "--example"
            || part.starts_with("--example=")
    })
}

enum CargoBuildPlan {
    RunAsIs,
    ReplaceCommand {
        replacement: &'static str,
        tool_name: &'static str,
    },
    Skip(&'static str),
}

fn plan_cargo_build(
    host: &str,
    target: &str,
    zigbuild_available: bool,
    cross_available: bool,
) -> CargoBuildPlan {
    if !needs_cross_linker(host, target) {
        return CargoBuildPlan::RunAsIs;
    }

    if !host.contains("darwin") && target.contains("apple-darwin") {
        return CargoBuildPlan::Skip(
            "macOS targets require Apple SDK/toolchains; build these on a macOS runner",
        );
    }

    if zigbuild_available && !target.contains("apple-darwin") {
        return CargoBuildPlan::ReplaceCommand {
            replacement: "cargo zigbuild",
            tool_name: "cargo-zigbuild",
        };
    }

    if cross_available && !target.contains("apple-darwin") {
        return CargoBuildPlan::ReplaceCommand {
            replacement: "cross build",
            tool_name: "cross",
        };
    }

    CargoBuildPlan::RunAsIs
}

fn parse_installed_targets(output: &str) -> HashSet<String> {
    output
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect()
}

fn parse_command(cmd_str: &str) -> Result<(&str, Vec<&str>)> {
    let parts: Vec<&str> = cmd_str.split_whitespace().collect();
    let (bin, args) = parts
        .split_first()
        .ok_or_else(|| anyhow::anyhow!("empty command"))?;
    Ok((bin, args.to_vec()))
}

fn is_process_fd_quota_exceeded(err: &str) -> bool {
    err.contains("ProcessFdQuotaExceeded")
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

#[derive(Debug, Clone)]
struct BuiltArchive {
    binary: String,
    target: String,
    asset_name: String,
    archive_path: PathBuf,
}

fn build_artifacts(
    config: &Config,
    version: &str,
    cargo_package: Option<&CargoPackageSelection>,
) -> Result<Vec<BuiltArchive>> {
    let binaries = config.project.release_binaries();
    let staging = PathBuf::from("target/release-staging");
    std::fs::create_dir_all(&staging)?;

    let host = host_target().unwrap_or_default();
    let zigbuild_available = has_cargo_zigbuild();
    let cross_available = has_cross();

    let mut archives = Vec::new();
    let mut failed_pairs: Vec<(String, String)> = Vec::new();
    let total_attempts = config.build.targets.len() * binaries.len();

    for target in &config.build.targets {
        for binary in &binaries {
            let binary = *binary;
            let package = cargo_package
                .map(|package| package.name.as_str())
                .unwrap_or("");
            let vars = &[
                ("target", target.as_str()),
                ("binary", binary),
                ("package", package),
                ("version", version),
            ];

            let artifact_path = if let Some(cmd_template) = &config.build.command {
                let mut cmd_str = substitute(cmd_template, vars);
                let cross_needed = needs_cross_linker(&host, target);
                let mut using_zigbuild = false;

                if cmd_str.contains("cargo build") {
                    match plan_cargo_build(&host, target, zigbuild_available, cross_available) {
                        CargoBuildPlan::RunAsIs => {
                            if cross_needed {
                                eprintln!(
                                    "[build] No cross helper detected for {target}; trying plain cargo build (set {} if linker errors occur)",
                                    linker_env_var(target)
                                );
                            }
                        }
                        CargoBuildPlan::ReplaceCommand {
                            replacement,
                            tool_name,
                        } => {
                            eprintln!(
                                "[build] Using {tool_name} for cross-compilation target {target}"
                            );
                            cmd_str = cmd_str.replacen("cargo build", replacement, 1);
                            using_zigbuild = tool_name == "cargo-zigbuild";
                        }
                        CargoBuildPlan::Skip(reason) => {
                            eprintln!("[build] Warning: {binary} ({target}) skipped: {reason}");
                            failed_pairs.push((binary.to_string(), target.clone()));
                            continue;
                        }
                    }
                }

                if let Some(package) = cargo_package {
                    cmd_str = package.augment_build_command(&cmd_str, binary);
                }

                let build_err = {
                    let (bin, args) = parse_command(&cmd_str).map_err(|_| {
                        anyhow::anyhow!("empty build command for {binary} ({target})")
                    })?;
                    run_cmd("build", None, bin, &args).err()
                };

                if let Some(err) = build_err {
                    let err_msg = err.to_string();
                    if using_zigbuild && is_process_fd_quota_exceeded(&err_msg) {
                        if cross_available {
                            let fallback = cmd_str.replacen("cargo zigbuild", "cross build", 1);
                            eprintln!(
                                "[build] cargo-zigbuild hit file descriptor quota for {binary} ({target}); retrying with cross"
                            );
                            let (bin, args) = parse_command(&fallback)
                                .map_err(|_| anyhow::anyhow!("empty cross fallback command"))?;
                            if let Err(retry_err) = run_cmd("build", None, bin, &args) {
                                eprintln!(
                                    "[build] Warning: {binary} ({target}) failed: {retry_err}"
                                );
                                failed_pairs.push((binary.to_string(), target.clone()));
                                continue;
                            }
                        } else {
                            eprintln!(
                                "[build] Warning: {binary} ({target}) failed: {err_msg}\n[build] Hint: zig hit the open-file limit. Try `ulimit -n 65536` before running release, or install `cross` to avoid this zig linker path."
                            );
                            failed_pairs.push((binary.to_string(), target.clone()));
                            continue;
                        }
                    } else {
                        eprintln!("[build] Warning: {binary} ({target}) failed: {err}");
                        failed_pairs.push((binary.to_string(), target.clone()));
                        continue;
                    }
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
                    "[build] Warning: {binary} ({target}) failed: artifact not found at {}",
                    artifact_path.display()
                );
                failed_pairs.push((binary.to_string(), target.clone()));
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

            archives.push(BuiltArchive {
                binary: binary.to_string(),
                target: target.clone(),
                asset_name: archive_name,
                archive_path,
            });
        }
    }

    if archives.is_empty() {
        bail!("all build target/binary combinations failed");
    }

    if !failed_pairs.is_empty() {
        eprintln!(
            "\n{}/{} target/binary combinations failed:",
            failed_pairs.len(),
            total_attempts
        );
        for (binary, target) in &failed_pairs {
            eprintln!("  - {binary} ({target})");
        }
        if config
            .build
            .command
            .as_ref()
            .is_some_and(|c| c.contains("cargo"))
        {
            let installed = installed_targets();
            let mut failed_targets = Vec::new();
            for (_, target) in &failed_pairs {
                if !failed_targets.contains(target) {
                    failed_targets.push(target.clone());
                }
            }
            let (installed_failed, missing): (Vec<_>, Vec<_>) = failed_targets
                .into_iter()
                .partition(|target| installed.contains(target.as_str()));

            if !missing.is_empty() {
                eprintln!("\nMissing targets (install with rustup):");
                for target in &missing {
                    eprintln!("  rustup target add {target}");
                }
            }
            if !installed_failed.is_empty() {
                eprintln!(
                    "\nInstalled but failed to build (missing cross-compilation linker/SDK):"
                );
                for target in &installed_failed {
                    eprintln!("  - {target}");
                }
                eprintln!(
                    "  Tip: install `cross` (uses Docker) or `cargo-zigbuild` (uses zig) for cross-compilation"
                );
                eprintln!("  Or configure a target linker, for example:");
                for target in installed_failed.iter().take(3) {
                    eprintln!("    export {}=<path-to-linker>", linker_env_var(target));
                }
                if installed_failed.len() > 3 {
                    eprintln!("    ...");
                }
                if installed_failed
                    .iter()
                    .any(|target| target.contains("apple-darwin"))
                {
                    eprintln!("  Note: Apple targets usually require building on macOS.");
                }
            }
        }
        if config.build.pre_built_dir.is_some() {
            let dir = config.build.pre_built_dir.as_ref().unwrap();
            eprintln!("\nExpected pre-built artifacts in {dir}:");
            for (binary, target) in &failed_pairs {
                eprintln!("  {dir}{binary}-{target}");
            }
        }
        eprintln!();
        let succeeded: Vec<String> = archives
            .iter()
            .map(|archive| format!("{} ({})", archive.binary, archive.target))
            .collect();
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

fn preflight(git: &GitContext, selected: &[&str], auto_tag_enabled: bool) -> Result<()> {
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

    if auto_tag_enabled && !command_exists("git") {
        missing.push("git command is required when project.auto-tag is enabled".to_string());
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

    let auto_tag_enabled = config.project.auto_tag && selected.contains(&"git");
    preflight(&git, &selected, auto_tag_enabled)?;
    let cargo_package = CargoPackageSelection::resolve(config, &selected)?;

    let version = if auto_tag_enabled {
        let manifest_version = detect_manifest_version(config, cargo_package.as_ref())?;
        if let Some(override_version) = version_override {
            let normalized_override = normalize_version(override_version);
            if normalized_override != manifest_version {
                bail!(
                    "--version ({normalized_override}) does not match manifest version ({manifest_version}) while project.auto-tag is enabled"
                );
            }
        }
        println!("[tag] Using manifest version {manifest_version} from Cargo.toml");
        manifest_version
    } else {
        detect_version(config, version_override)?
    };

    if auto_tag_enabled {
        create_and_push_tag(&version)?;
    }

    println!(
        "Releasing {} v{version} via: {}",
        config.project.name,
        selected.join(", ")
    );
    let release_binaries = config.project.release_binaries();
    if release_binaries.len() > 1 {
        println!("Release binaries: {}", release_binaries.join(", "));
        if selected
            .iter()
            .any(|ch| matches!(*ch, "homebrew" | "curl" | "nix"))
        {
            println!(
                "Note: homebrew/curl/nix use primary binary '{}'; all binaries are uploaded as git release assets.",
                config.project.primary_binary()
            );
        }
    }

    let archives = build_artifacts(config, &version, cargo_package.as_ref())?;

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
            "cargo" => release_cargo(config, cargo_package.as_ref())?,
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

fn fetch_release_by_tag(git: &GitContext, repo: &str, version: &str) -> Result<serde_json::Value> {
    let url = git.repo_api_url(repo, &format!("/releases/tags/v{version}"));
    git_api(git, "git", "GET", &url, None)
}

fn create_release(git: &GitContext, repo: &str, version: &str) -> Result<String> {
    if let Ok(existing) = fetch_release_by_tag(git, repo, version) {
        eprintln!("[git] Release v{version} already exists; reusing it");
        return release_upload_url(git, "git", repo, &existing);
    }

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
    match git_api(git, "git", "POST", &url, Some(&body.to_string())) {
        Ok(resp) => release_upload_url(git, "git", repo, &resp),
        Err(create_err) => {
            if let Ok(existing) = fetch_release_by_tag(git, repo, version) {
                eprintln!(
                    "[git] Reusing existing release v{version} after create returned an error"
                );
                return release_upload_url(git, "git", repo, &existing);
            }
            Err(create_err)
        }
    }
}

fn delete_existing_asset_if_any(
    git: &GitContext,
    repo: &str,
    version: &str,
    asset_name: &str,
) -> Result<()> {
    let release = match fetch_release_by_tag(git, repo, version) {
        Ok(r) => r,
        Err(_) => return Ok(()),
    };

    let release_id = release["id"].as_i64();
    let assets = match release["assets"].as_array() {
        Some(a) => a,
        None => return Ok(()),
    };

    for asset in assets {
        if asset["name"].as_str() != Some(asset_name) {
            continue;
        }
        let Some(asset_id) = asset["id"].as_i64() else {
            continue;
        };

        let delete_url = if git.is_github {
            git.repo_api_url(repo, &format!("/releases/assets/{asset_id}"))
        } else if let Some(release_id) = release_id {
            git.repo_api_url(repo, &format!("/releases/{release_id}/assets/{asset_id}"))
        } else {
            eprintln!(
                "[git] Warning: could not replace existing asset {asset_name}: missing release id"
            );
            return Ok(());
        };

        eprintln!("[git] Removing existing asset {asset_name} before upload");
        if let Err(err) = git_api(git, "git", "DELETE", &delete_url, None) {
            eprintln!("[git] Warning: failed to delete existing asset {asset_name}: {err}");
        }
        break;
    }

    Ok(())
}

fn release_git(
    config: &Config,
    git: &GitContext,
    version: &str,
    archives: &[BuiltArchive],
) -> Result<()> {
    let upload_url = create_release(git, &config.project.repo, version)?;
    for archive in archives {
        delete_existing_asset_if_any(git, &config.project.repo, version, &archive.asset_name)?;
        git_upload_asset(
            git,
            "git",
            &upload_url,
            &archive.archive_path,
            &archive.asset_name,
            "application/gzip",
        )?;
    }
    println!("[git] Created release v{version}");
    Ok(())
}

fn release_homebrew(
    config: &Config,
    git: &GitContext,
    version: &str,
    archives: &[BuiltArchive],
) -> Result<()> {
    let ch = config.channels.homebrew.as_ref().unwrap();
    let formula_name = ch.formula_name.as_deref().unwrap_or(&config.project.name);
    let binary = config.project.primary_binary();
    let repo = &config.project.repo;

    let release_url = git.repo_api_url(repo, &format!("/releases/tags/v{version}"));
    git_api(git, "homebrew", "GET", &release_url, None).with_context(|| {
        format!("[homebrew] release v{version} not found — run the git channel first")
    })?;

    let (darwin_arm_sha, darwin_intel_sha) = homebrew_macos_shas(archives, binary)?;

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

fn homebrew_macos_shas(archives: &[BuiltArchive], binary: &str) -> Result<(String, String)> {
    let mut darwin_arm: Option<&PathBuf> = None;
    let mut darwin_intel: Option<&PathBuf> = None;

    for archive in archives {
        if archive.binary != binary {
            continue;
        }
        if archive.target == "aarch64-apple-darwin" {
            darwin_arm = Some(&archive.archive_path);
        } else if archive.target == "x86_64-apple-darwin" {
            darwin_intel = Some(&archive.archive_path);
        }
    }

    let darwin_arm = darwin_arm.ok_or_else(|| {
        anyhow::anyhow!(
            "[homebrew] missing artifact for binary {binary} target aarch64-apple-darwin; build it or disable channels.homebrew"
        )
    })?;
    let darwin_intel = darwin_intel.ok_or_else(|| {
        anyhow::anyhow!(
            "[homebrew] missing artifact for binary {binary} target x86_64-apple-darwin; build it or disable channels.homebrew"
        )
    })?;

    Ok((sha256(darwin_arm)?, sha256(darwin_intel)?))
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

fn release_cargo(config: &Config, cargo_package: Option<&CargoPackageSelection>) -> Result<()> {
    let ch = config.channels.cargo.as_ref().unwrap();
    let crate_name = ch
        .crate_name
        .as_deref()
        .or_else(|| cargo_package.map(|package| package.name.as_str()))
        .unwrap_or(&config.project.name);
    let mut args = vec!["publish"];
    if let Some(package) = cargo_package {
        args.push("--package");
        args.push(package.name.as_str());
    }
    run_cmd("cargo", None, "cargo", &args)?;
    println!("[cargo] Published crate {crate_name}");
    Ok(())
}

fn release_curl(config: &Config, git: &GitContext, version: &str) -> Result<()> {
    let binary = config.project.primary_binary();
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

    delete_existing_asset_if_any(git, repo, version, "install.sh")?;
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
  if [ -t 0 ] && [ -r /dev/tty ]; then
    printf "Install directory [/usr/local/bin]: " > /dev/tty
    read -r INSTALL_DIR < /dev/tty || true
    INSTALL_DIR="${{INSTALL_DIR:-/usr/local/bin}}"
  else
    INSTALL_DIR="/usr/local/bin"
  fi
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
    archives: &[BuiltArchive],
) -> Result<()> {
    let ch = config.channels.nix.as_ref().unwrap();
    let binary = config.project.primary_binary();
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
    for archive in archives {
        if archive.binary != binary {
            continue;
        }

        let target = &archive.target;
        let nix_sys = match nix_system(target) {
            Some(s) => s,
            None => continue,
        };
        let asset_name = archive.asset_name.as_str();
        let download_url = git.release_download_url(repo, version, asset_name);

        // Verify asset exists in the release
        let assets = release["assets"].as_array();
        let asset_exists = assets.is_some_and(|a| {
            a.iter()
                .any(|asset| asset["name"].as_str() == Some(asset_name))
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

    // --- homebrew archive validation tests ---

    #[test]
    fn homebrew_macos_shas_requires_arm_archive() {
        let archives = vec![BuiltArchive {
            binary: "tool".to_string(),
            target: "x86_64-unknown-linux-gnu".to_string(),
            asset_name: "unused.tar.gz".to_string(),
            archive_path: PathBuf::from("unused"),
        }];
        let err = homebrew_macos_shas(&archives, "tool").unwrap_err();
        assert!(err.to_string().contains("aarch64-apple-darwin"));
    }

    #[test]
    fn homebrew_macos_shas_requires_intel_archive() {
        let archives = vec![BuiltArchive {
            binary: "tool".to_string(),
            target: "aarch64-apple-darwin".to_string(),
            asset_name: "unused.tar.gz".to_string(),
            archive_path: PathBuf::from("unused"),
        }];
        let err = homebrew_macos_shas(&archives, "tool").unwrap_err();
        assert!(err.to_string().contains("x86_64-apple-darwin"));
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

    #[test]
    fn normalize_version_strips_v_prefix() {
        assert_eq!(normalize_version("v1.2.3"), "1.2.3");
        assert_eq!(normalize_version("1.2.3"), "1.2.3");
    }

    fn write_temp_manifest(content: &str) -> PathBuf {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("releasor2000-test-{stamp}"));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("Cargo.toml");
        std::fs::write(&path, content).unwrap();
        path
    }

    #[test]
    fn rust_manifest_version_reads_package_version() {
        let path = write_temp_manifest(
            r#"
[package]
name = "mytool"
version = "1.2.3"
"#,
        );
        let version = rust_manifest_version(&path).unwrap();
        assert_eq!(version, "1.2.3");
        std::fs::remove_dir_all(path.parent().unwrap()).ok();
    }

    #[test]
    fn rust_manifest_version_rejects_missing_package_version() {
        let path = write_temp_manifest(
            r#"
[workspace]
members = ["app"]
"#,
        );
        let err = rust_manifest_version(&path).unwrap_err();
        assert!(
            err.to_string().contains("missing package.version"),
            "got: {err}"
        );
        std::fs::remove_dir_all(path.parent().unwrap()).ok();
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

    // --- cross build planning tests ---

    #[test]
    fn linker_env_var_formats_target() {
        assert_eq!(
            linker_env_var("aarch64-unknown-linux-gnu"),
            "CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER"
        );
    }

    #[test]
    fn plan_cargo_build_uses_zigbuild_for_linux_cross_target() {
        let plan = plan_cargo_build(
            "x86_64-unknown-linux-gnu",
            "aarch64-unknown-linux-gnu",
            true,
            false,
        );
        assert!(matches!(
            plan,
            CargoBuildPlan::ReplaceCommand {
                replacement: "cargo zigbuild",
                tool_name: "cargo-zigbuild"
            }
        ));
    }

    #[test]
    fn plan_cargo_build_falls_back_to_cross() {
        let plan = plan_cargo_build(
            "x86_64-unknown-linux-gnu",
            "aarch64-unknown-linux-gnu",
            false,
            true,
        );
        assert!(matches!(
            plan,
            CargoBuildPlan::ReplaceCommand {
                replacement: "cross build",
                tool_name: "cross"
            }
        ));
    }

    #[test]
    fn plan_cargo_build_skips_darwin_on_non_darwin_hosts() {
        let plan = plan_cargo_build(
            "x86_64-unknown-linux-gnu",
            "aarch64-apple-darwin",
            true,
            true,
        );
        assert!(matches!(plan, CargoBuildPlan::Skip(_)));
    }

    #[test]
    fn plan_cargo_build_runs_as_is_for_native_target() {
        let plan = plan_cargo_build(
            "x86_64-unknown-linux-gnu",
            "x86_64-unknown-linux-gnu",
            true,
            true,
        );
        assert!(matches!(plan, CargoBuildPlan::RunAsIs));
    }

    // --- command parsing / linker error tests ---

    #[test]
    fn parse_command_splits_binary_and_args() {
        let (bin, args) =
            parse_command("cargo build --release --target x86_64-unknown-linux-gnu").unwrap();
        assert_eq!(bin, "cargo");
        assert_eq!(
            args,
            vec!["build", "--release", "--target", "x86_64-unknown-linux-gnu"]
        );
    }

    #[test]
    fn parse_command_rejects_empty_command() {
        assert!(parse_command("   ").is_err());
    }

    #[test]
    fn is_process_fd_quota_exceeded_detects_zig_error() {
        assert!(is_process_fd_quota_exceeded(
            "error: unable to search for static library: ProcessFdQuotaExceeded"
        ));
        assert!(!is_process_fd_quota_exceeded("some other linker error"));
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

    // --- cargo workspace package detection tests ---

    #[test]
    fn cargo_workspace_detects_unique_binary_package() {
        let metadata = r#"
{
  "workspace_members": ["path+file:///repo/app-cli#0.1.0", "path+file:///repo/helper#0.1.0"],
  "packages": [
    {
      "id": "path+file:///repo/app-cli#0.1.0",
      "name": "app-cli",
      "version": "0.1.0",
      "targets": [
        { "name": "coolapp", "kind": ["bin"] },
        { "name": "app_cli", "kind": ["lib"] }
      ]
    },
    {
      "id": "path+file:///repo/helper#0.1.0",
      "name": "helper",
      "version": "0.1.0",
      "targets": [
        { "name": "helper", "kind": ["bin"] }
      ]
    }
  ]
}
"#;
        let workspace = CargoWorkspace::from_metadata_json(metadata).unwrap();
        let package = workspace.package_for_binary("coolapp").unwrap().unwrap();
        assert_eq!(package.name, "app-cli");
        assert_eq!(
            workspace.version_for_package("app-cli").unwrap(),
            Some("0.1.0".to_string())
        );
    }

    #[test]
    fn rust_workspace_package_version_uses_detected_binary_package() {
        let config = Config::parse(
            r#"
[project]
name = "coolapp"
repo = "owner/repo"

[build]
command = "cargo build --release --target {target}"
artifact = "target/{target}/release/{binary}"
targets = ["x86_64-apple-darwin"]
"#,
        )
        .unwrap();
        let metadata = r#"
{
  "workspace_members": ["path+file:///repo/app-cli#1.2.3"],
  "packages": [
    {
      "id": "path+file:///repo/app-cli#1.2.3",
      "name": "app-cli",
      "version": "1.2.3",
      "targets": [{ "name": "coolapp", "kind": ["bin"] }]
    }
  ]
}
"#;
        let workspace = CargoWorkspace::from_metadata_json(metadata).unwrap();
        let version = rust_workspace_package_version(&config, None, &workspace)
            .unwrap()
            .unwrap();
        assert_eq!(version, "1.2.3");
    }

    #[test]
    fn rust_workspace_package_version_uses_project_package_override() {
        let config = Config::parse(
            r#"
[project]
name = "coolapp"
package = "app-cli"
repo = "owner/repo"

[build]
command = "make"
artifact = "out/{binary}"
targets = ["x86_64-apple-darwin"]
"#,
        )
        .unwrap();
        let package = CargoPackageSelection {
            name: "app-cli".to_string(),
        };
        let metadata = r#"
{
  "workspace_members": ["path+file:///repo/app-cli#2.3.4"],
  "packages": [
    {
      "id": "path+file:///repo/app-cli#2.3.4",
      "name": "app-cli",
      "version": "2.3.4",
      "targets": [{ "name": "other-bin", "kind": ["bin"] }]
    }
  ]
}
"#;
        let workspace = CargoWorkspace::from_metadata_json(metadata).unwrap();
        let version = rust_workspace_package_version(&config, Some(&package), &workspace)
            .unwrap()
            .unwrap();
        assert_eq!(version, "2.3.4");
    }

    #[test]
    fn cargo_workspace_ignores_non_workspace_packages() {
        let metadata = r#"
{
  "workspace_members": ["path+file:///repo/app-cli#0.1.0"],
  "packages": [
    {
      "id": "path+file:///repo/app-cli#0.1.0",
      "name": "app-cli",
      "version": "0.1.0",
      "targets": [
        { "name": "coolapp", "kind": ["bin"] }
      ]
    },
    {
      "id": "path+file:///repo/dep#0.1.0",
      "name": "dep",
      "version": "0.1.0",
      "targets": [
        { "name": "dep-bin", "kind": ["bin"] }
      ]
    }
  ]
}
"#;
        let workspace = CargoWorkspace::from_metadata_json(metadata).unwrap();
        assert!(workspace.package_for_binary("dep-bin").unwrap().is_none());
    }

    #[test]
    fn cargo_workspace_rejects_ambiguous_binary_package() {
        let metadata = r#"
{
  "workspace_members": ["path+file:///repo/one#0.1.0", "path+file:///repo/two#0.1.0"],
  "packages": [
    {
      "id": "path+file:///repo/one#0.1.0",
      "name": "one",
      "version": "0.1.0",
      "targets": [{ "name": "tool", "kind": ["bin"] }]
    },
    {
      "id": "path+file:///repo/two#0.1.0",
      "name": "two",
      "version": "0.1.0",
      "targets": [{ "name": "tool", "kind": ["bin"] }]
    }
  ]
}
"#;
        let workspace = CargoWorkspace::from_metadata_json(metadata).unwrap();
        let err = workspace.package_for_binary("tool").unwrap_err();
        assert!(err.to_string().contains("multiple workspace packages"));
        assert!(err.to_string().contains("project.package"));
    }

    #[test]
    fn cargo_package_selection_uses_project_package_override() {
        let config = Config::parse(
            r#"
[project]
name = "coolapp"
package = "app-cli"
repo = "owner/repo"

[build]
command = "make"
artifact = "out/{binary}"
targets = ["x86_64-apple-darwin"]
"#,
        )
        .unwrap();
        let package = CargoPackageSelection::resolve(&config, &["cargo"])
            .unwrap()
            .unwrap();
        assert_eq!(package.name, "app-cli");
    }

    #[test]
    fn cargo_package_selection_augments_cargo_build() {
        let package = CargoPackageSelection {
            name: "app-cli".to_string(),
        };
        let command = package.augment_build_command(
            "cargo build --release --target x86_64-unknown-linux-gnu",
            "coolapp",
        );
        assert_eq!(
            command,
            "cargo build --release --target x86_64-unknown-linux-gnu --package app-cli --bin coolapp"
        );
    }

    #[test]
    fn cargo_package_selection_augments_cross_build() {
        let package = CargoPackageSelection {
            name: "app-cli".to_string(),
        };
        let command = package.augment_build_command("cross build --release", "coolapp");
        assert_eq!(
            command,
            "cross build --release --package app-cli --bin coolapp"
        );
    }

    #[test]
    fn cargo_package_selection_does_not_duplicate_selectors() {
        let package = CargoPackageSelection {
            name: "app-cli".to_string(),
        };
        let command = package.augment_build_command(
            "cargo build --package app-cli --bin coolapp --release",
            "coolapp",
        );
        assert_eq!(
            command,
            "cargo build --package app-cli --bin coolapp --release"
        );
    }

    #[test]
    fn cargo_package_selection_ignores_non_cargo_commands() {
        let package = CargoPackageSelection {
            name: "app-cli".to_string(),
        };
        let command = package.augment_build_command("make release", "coolapp");
        assert_eq!(command, "make release");
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
        assert!(script.contains("read -r INSTALL_DIR < /dev/tty || true"));
    }

    #[test]
    fn generate_install_script_defaults_install_dir_when_non_interactive() {
        let script = generate_install_script("tool", "owner/repo", "1.0.0", GITHUB_BASE_URL);
        assert!(script.contains("if [ -t 0 ] && [ -r /dev/tty ]; then"));
        assert!(script.contains("INSTALL_DIR=\"/usr/local/bin\""));
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
        assert!(preflight(&git, &[], false).is_ok());
    }

    #[test]
    fn preflight_requires_default_github_token() {
        // Remove GITHUB_TOKEN to ensure the check triggers
        let saved = std::env::var("GITHUB_TOKEN").ok();
        unsafe { std::env::remove_var("GITHUB_TOKEN") };

        let git = github_git();
        let err = preflight(&git, &["git"], false).unwrap_err();
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
        let err = preflight(&git, &["git"], false).unwrap_err();
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
        let err = preflight(&git, &["git", "nix"], false).unwrap_err();
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
            let err = preflight(&git, &[*ch], false).unwrap_err();
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

    #[test]
    fn preflight_requires_git_command_when_auto_tag_enabled() {
        if command_exists("git") {
            return;
        }
        let git = github_git();
        let err = preflight(&git, &["git"], true).unwrap_err();
        assert!(
            err.to_string().contains("git command is required"),
            "got: {err}"
        );
    }
}
