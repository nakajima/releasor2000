use anyhow::{Context, Result, bail};
use serde_json::Value;
use std::path::Path;
use std::process::Command;

const RELEASE_BASE_URL: &str = "https://github.com";
const API_BASE_URL: &str = "https://api.github.com";
const REPO: &str = "nakajima/releasor2000";
const BINARY: &str = "releasor2000";

fn run_cmd(label: &str, cmd: &str, args: &[&str]) -> Result<String> {
    let output = Command::new(cmd)
        .args(args)
        .output()
        .with_context(|| format!("[{label}] failed to run {cmd}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("[{label}] {cmd} failed: {stderr}");
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn command_exists(cmd: &str) -> bool {
    Command::new("sh")
        .args(["-c", &format!("command -v {cmd}")])
        .output()
        .is_ok_and(|o| o.status.success())
}

fn release_target(os: &str, arch: &str) -> Option<&'static str> {
    match (os, arch) {
        ("linux", "x86_64") => Some("x86_64-unknown-linux-gnu"),
        ("linux", "aarch64") => Some("aarch64-unknown-linux-gnu"),
        ("macos", "x86_64") => Some("x86_64-apple-darwin"),
        ("macos", "aarch64") => Some("aarch64-apple-darwin"),
        _ => None,
    }
}

fn current_target() -> Result<&'static str> {
    release_target(std::env::consts::OS, std::env::consts::ARCH).ok_or_else(|| {
        anyhow::anyhow!(
            "unsupported platform for self-upgrade: os={} arch={}",
            std::env::consts::OS,
            std::env::consts::ARCH
        )
    })
}

fn parse_latest_version(api_response: &str) -> Result<String> {
    let parsed: Value =
        serde_json::from_str(api_response).context("invalid GitHub API response")?;
    let tag = parsed["tag_name"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("latest release response missing tag_name"))?;
    let version = tag.strip_prefix('v').unwrap_or(tag).trim();
    if version.is_empty() {
        bail!("latest release tag_name was empty");
    }
    Ok(version.to_string())
}

fn latest_version() -> Result<String> {
    let url = format!("{API_BASE_URL}/repos/{REPO}/releases/latest");
    let body = run_cmd(
        "upgrade",
        "curl",
        &[
            "-fsSL",
            "-H",
            "Accept: application/vnd.github+json",
            "-H",
            "User-Agent: releasor2000",
            &url,
        ],
    )?;
    parse_latest_version(&body)
}

fn set_executable(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(path)?.permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(path, perms)?;
    }
    Ok(())
}

fn replace_current_binary(new_binary_path: &Path, current_exe: &Path) -> Result<()> {
    let install_dir = current_exe
        .parent()
        .ok_or_else(|| anyhow::anyhow!("could not determine install directory"))?;
    let staged_path = install_dir.join(format!(".{BINARY}.upgrade"));

    std::fs::copy(new_binary_path, &staged_path).with_context(|| {
        format!(
            "failed to stage upgraded binary at {}",
            staged_path.display()
        )
    })?;
    set_executable(&staged_path)?;

    if let Err(rename_err) = std::fs::rename(&staged_path, current_exe) {
        std::fs::remove_file(&staged_path).ok();
        bail!(
            "failed to replace current binary at {}: {rename_err}",
            current_exe.display()
        );
    }

    Ok(())
}

pub fn upgrade() -> Result<()> {
    if !command_exists("curl") {
        bail!("curl is required for upgrade");
    }
    if !command_exists("tar") {
        bail!("tar is required for upgrade");
    }

    let current_exe = std::env::current_exe().context("failed to resolve current executable")?;
    let target = current_target()?;
    let version = latest_version()?;
    let asset_name = format!("{BINARY}-{version}-{target}.tar.gz");
    let download_url =
        format!("{RELEASE_BASE_URL}/{REPO}/releases/download/v{version}/{asset_name}");

    println!("[upgrade] Downloading {BINARY} v{version} for {target}...");

    let temp_dir =
        std::env::temp_dir().join(format!("releasor2000-upgrade-{}", std::process::id()));
    if temp_dir.exists() {
        std::fs::remove_dir_all(&temp_dir).ok();
    }
    std::fs::create_dir_all(&temp_dir)?;

    let archive_path = temp_dir.join(&asset_name);
    run_cmd(
        "upgrade",
        "curl",
        &[
            "-fsSL",
            "-o",
            &archive_path.to_string_lossy(),
            &download_url,
        ],
    )
    .with_context(|| format!("failed to download {download_url}"))?;

    run_cmd(
        "upgrade",
        "tar",
        &[
            "xzf",
            &archive_path.to_string_lossy(),
            "-C",
            &temp_dir.to_string_lossy(),
        ],
    )?;

    let extracted_binary = temp_dir.join(BINARY);
    if !extracted_binary.exists() {
        bail!(
            "downloaded archive did not contain expected binary '{}'",
            extracted_binary.display()
        );
    }

    replace_current_binary(&extracted_binary, &current_exe)?;
    std::fs::remove_dir_all(&temp_dir).ok();

    println!(
        "[upgrade] Upgraded {BINARY} to v{version} at {}",
        current_exe.display()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn release_target_maps_supported_platforms() {
        assert_eq!(
            release_target("linux", "x86_64"),
            Some("x86_64-unknown-linux-gnu")
        );
        assert_eq!(
            release_target("linux", "aarch64"),
            Some("aarch64-unknown-linux-gnu")
        );
        assert_eq!(
            release_target("macos", "x86_64"),
            Some("x86_64-apple-darwin")
        );
        assert_eq!(
            release_target("macos", "aarch64"),
            Some("aarch64-apple-darwin")
        );
    }

    #[test]
    fn release_target_rejects_unsupported_platforms() {
        assert_eq!(release_target("windows", "x86_64"), None);
        assert_eq!(release_target("linux", "arm"), None);
    }

    #[test]
    fn parse_latest_version_strips_v_prefix() {
        let body = r#"{"tag_name":"v1.2.3"}"#;
        assert_eq!(parse_latest_version(body).unwrap(), "1.2.3");
    }

    #[test]
    fn parse_latest_version_accepts_plain_tag() {
        let body = r#"{"tag_name":"1.2.3"}"#;
        assert_eq!(parse_latest_version(body).unwrap(), "1.2.3");
    }

    #[test]
    fn parse_latest_version_rejects_missing_tag_name() {
        let body = r#"{"name":"no-tag"}"#;
        assert!(parse_latest_version(body).is_err());
    }
}
