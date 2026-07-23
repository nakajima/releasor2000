use anyhow::{Context, Result, bail};
use serde::Deserialize;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Command;
use toml::Value;

#[derive(Debug, Deserialize)]
pub struct Config {
    pub project: Project,
    pub build: Build,
    #[serde(default)]
    pub git: Git,
    #[serde(default)]
    pub forge: Option<toml::Value>,
    #[serde(default)]
    pub channels: Channels,
    #[serde(default)]
    pub macos: Macos,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct Project {
    pub name: String,
    pub binary: Option<String>,
    pub package: Option<String>,
    pub binaries: Option<Vec<String>>,
    pub repo: String,
    #[serde(alias = "version_command")]
    pub version_command: Option<String>,
    #[serde(default, alias = "auto_tag")]
    pub auto_tag: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct Build {
    pub command: Option<String>,
    pub artifact: Option<String>,
    #[serde(alias = "pre_built_dir")]
    pub pre_built_dir: Option<String>,
    pub targets: Vec<String>,
}

#[derive(Debug, Default, Deserialize)]
pub struct Macos {
    pub codesign: Option<Codesign>,
    pub notarization: Option<Notarization>,
}

#[derive(Debug, Deserialize)]
pub struct Codesign {
    pub identity: String,
    pub entitlements: Option<PathBuf>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct Notarization {
    #[serde(alias = "keychain_profile")]
    pub keychain_profile: String,
}

#[derive(Debug, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum GitType {
    Github,
    Gitea,
}

impl Default for GitType {
    fn default() -> Self {
        Self::Github
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct Git {
    #[serde(default)]
    pub r#type: GitType,
    #[serde(alias = "base_url")]
    pub base_url: Option<String>,
    #[serde(alias = "api_base_url")]
    pub api_base_url: Option<String>,
    #[serde(alias = "token_env")]
    pub token_env: Option<String>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct GitInfo {
    repo: String,
    r#type: GitType,
    base_url: String,
}

impl GitInfo {
    pub fn from_config(path: &Path) -> Option<Self> {
        if !path.is_file() {
            return None;
        }

        let output = Command::new("git")
            .arg("config")
            .arg("--file")
            .arg(path)
            .arg("--get")
            .arg("remote.origin.url")
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }

        let remote_url = String::from_utf8(output.stdout).ok()?;
        Self::from_remote_url(remote_url.trim())
    }

    fn from_remote_url(remote_url: &str) -> Option<Self> {
        let remote_url = remote_url.trim().trim_end_matches('/');
        let (web_scheme, web_authority, host, path) =
            if let Some((raw_scheme, remainder)) = remote_url.split_once("://") {
                let scheme = raw_scheme.to_ascii_lowercase();
                if !matches!(scheme.as_str(), "http" | "https" | "ssh" | "git") {
                    return None;
                }
                let (raw_authority, path) = remainder.split_once('/')?;
                let authority = raw_authority.rsplit('@').next()?.trim();
                if authority.is_empty() {
                    return None;
                }
                let host = if let Some(bracketed) = authority.strip_prefix('[') {
                    bracketed.split_once(']')?.0
                } else {
                    authority.split(':').next()?
                };
                if host.is_empty() {
                    return None;
                }

                let is_web_scheme = matches!(scheme.as_str(), "http" | "https");
                let web_scheme = if is_web_scheme {
                    scheme
                } else {
                    "https".to_string()
                };
                let web_authority = if is_web_scheme {
                    authority.to_string()
                } else if host.contains(':') {
                    format!("[{host}]")
                } else {
                    host.to_string()
                };

                (web_scheme, web_authority, host.to_string(), path)
            } else {
                let (raw_authority, path) = remote_url.split_once(':')?;
                if raw_authority.contains('/') || raw_authority.contains('\\') {
                    return None;
                }
                let host = raw_authority.rsplit('@').next()?.trim();
                if host.is_empty() {
                    return None;
                }
                (
                    "https".to_string(),
                    host.to_string(),
                    host.to_string(),
                    path,
                )
            };

        let mut segments: Vec<&str> = path
            .trim_matches('/')
            .split('/')
            .filter(|segment| !segment.is_empty())
            .collect();
        let repo_segment = segments.pop()?;
        let repo_name = repo_segment.strip_suffix(".git").unwrap_or(repo_segment);
        let owner = segments.pop()?;
        if repo_name.is_empty()
            || [owner, repo_name].iter().any(|value| {
                value.chars().any(|character| {
                    character.is_whitespace()
                        || character.is_control()
                        || matches!(character, '"' | '\\' | '#' | '?')
                })
            })
        {
            return None;
        }

        let mut base_url = format!("{web_scheme}://{web_authority}");
        if !segments.is_empty() {
            base_url.push('/');
            base_url.push_str(&segments.join("/"));
        }
        if base_url.chars().any(|character| {
            character.is_whitespace()
                || character.is_control()
                || matches!(character, '"' | '\\' | '#' | '?')
        }) {
            return None;
        }

        let r#type = if host.eq_ignore_ascii_case("github.com") {
            GitType::Github
        } else {
            GitType::Gitea
        };

        Some(Self {
            repo: format!("{owner}/{repo_name}"),
            r#type,
            base_url,
        })
    }
}

#[derive(Debug, Default, Deserialize)]
pub struct Channels {
    #[serde(default)]
    pub github: Option<toml::Value>,
    #[serde(default)]
    pub forge: Option<toml::Value>,
    pub git: Option<GitChannel>,
    pub homebrew: Option<HomebrewChannel>,
    pub cargo: Option<CargoChannel>,
    pub curl: Option<CurlChannel>,
    pub nix: Option<NixChannel>,
}

#[derive(Debug, Deserialize)]
pub struct GitChannel {
    #[serde(default = "default_true")]
    pub enabled: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct HomebrewChannel {
    #[serde(default = "default_true")]
    pub enabled: bool,
    pub tap: String,
    #[serde(alias = "formula_name")]
    pub formula_name: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct CargoChannel {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(alias = "crate_name")]
    pub crate_name: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CurlChannel {
    #[serde(default = "default_true")]
    pub enabled: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct NixChannel {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(alias = "flake_repo")]
    pub flake_repo: Option<String>,
}

fn default_true() -> bool {
    true
}

impl Project {
    pub fn primary_binary(&self) -> &str {
        if let Some(binary) = self.binary.as_deref() {
            binary
        } else if let Some(binaries) = &self.binaries {
            binaries.first().map(String::as_str).unwrap_or(&self.name)
        } else {
            &self.name
        }
    }

    pub fn release_binaries(&self) -> Vec<&str> {
        if let Some(binaries) = &self.binaries {
            binaries.iter().map(String::as_str).collect()
        } else {
            vec![self.primary_binary()]
        }
    }
}

impl Git {
    fn normalized_url(url: Option<&str>, default: &str) -> String {
        let selected = url
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or(default);
        selected.trim_end_matches('/').to_string()
    }

    pub fn web_base_url(&self) -> String {
        match self.r#type {
            GitType::Github => Self::normalized_url(self.base_url.as_deref(), "https://github.com"),
            GitType::Gitea => {
                Self::normalized_url(self.base_url.as_deref(), "https://gitea.example.com")
            }
        }
    }

    pub fn api_base_url(&self) -> String {
        match self.r#type {
            GitType::Github => {
                Self::normalized_url(self.api_base_url.as_deref(), "https://api.github.com")
            }
            GitType::Gitea => {
                let default = format!("{}/api/v1", self.web_base_url());
                Self::normalized_url(self.api_base_url.as_deref(), &default)
            }
        }
    }

    pub fn token_env(&self) -> &str {
        self.token_env.as_deref().unwrap_or(match self.r#type {
            GitType::Github => "GITHUB_TOKEN",
            GitType::Gitea => "GITEA_TOKEN",
        })
    }
}

pub fn generate_template(project_name: &str, git_info: Option<&GitInfo>) -> String {
    let default_repo = format!("owner/{project_name}");
    let repo = git_info
        .map(|info| info.repo.as_str())
        .unwrap_or(&default_repo);
    let git_settings = match git_info {
        Some(info) if info.r#type == GitType::Gitea => format!(
            "type = \"gitea\"\nbase-url = \"{}\"\n# api-base-url = \"{}/api/v1\"  # defaults from type/base-url\n# token-env = \"GITEA_TOKEN\"       # defaults: GITHUB_TOKEN or GITEA_TOKEN\n",
            info.base_url, info.base_url
        ),
        Some(info) if info.base_url != "https://github.com" => format!(
            "# type = \"gitea\"                  # defaults to \"github\"\nbase-url = \"{}\"\n# api-base-url = \"https://api.github.com\"\n# token-env = \"GITHUB_TOKEN\"      # defaults: GITHUB_TOKEN or GITEA_TOKEN\n",
            info.base_url
        ),
        _ => "# type = \"gitea\"                  # defaults to \"github\"\n# base-url = \"https://git.example.com\"\n# api-base-url = \"https://git.example.com/api/v1\"  # defaults from type/base-url\n# token-env = \"GITEA_TOKEN\"       # defaults: GITHUB_TOKEN or GITEA_TOKEN\n"
            .to_string(),
    };

    format!(
        r#"[project]
name = "{project_name}"
# auto-tag = true  # detect Cargo.toml version and keep git tag v<version> only after a successful release
# binary = "{project_name}"  # defaults to project name
# package = "{project_name}"  # optional workspace package override; auto-detected from binary when unique
# binaries = ["{project_name}", "{project_name}-cli"]  # optional extra release assets
repo = "{repo}"
# version-command = "git describe --tags --abbrev=0"

[build]
command = "cargo build --release --target {{target}}"
artifact = "target/{{target}}/release/{{binary}}"
targets = [
    "x86_64-apple-darwin",
    "aarch64-apple-darwin",
    "x86_64-unknown-linux-gnu",
    "aarch64-unknown-linux-gnu",
]

# [macos.codesign]
# identity = "Developer ID Application: Example Corp (TEAMID)"
# entitlements = "entitlements.plist"  # optional

# [macos.notarization]
# keychain-profile = "releasor2000"  # created with: xcrun notarytool store-credentials releasor2000

[git]
{git_settings}
[channels.git]
enabled = true

# [channels.homebrew]
# tap = "owner/homebrew-tap"
# formula-name = "{project_name}"

# [channels.cargo]
# crate-name = "{project_name}"

# [channels.curl]

# [channels.nix]
# flake-repo = "owner/nix-repo"  # defaults to project repo
"#
    )
}

#[derive(Debug, Clone, Copy)]
struct KeyMigration {
    table_path: &'static [&'static str],
    dashed: &'static str,
    underscored: &'static str,
}

const KEY_MIGRATIONS: &[KeyMigration] = &[
    KeyMigration {
        table_path: &["project"],
        dashed: "auto-tag",
        underscored: "auto_tag",
    },
    KeyMigration {
        table_path: &["project"],
        dashed: "version-command",
        underscored: "version_command",
    },
    KeyMigration {
        table_path: &["build"],
        dashed: "pre-built-dir",
        underscored: "pre_built_dir",
    },
    KeyMigration {
        table_path: &["git"],
        dashed: "base-url",
        underscored: "base_url",
    },
    KeyMigration {
        table_path: &["git"],
        dashed: "api-base-url",
        underscored: "api_base_url",
    },
    KeyMigration {
        table_path: &["git"],
        dashed: "token-env",
        underscored: "token_env",
    },
    KeyMigration {
        table_path: &["channels", "homebrew"],
        dashed: "formula-name",
        underscored: "formula_name",
    },
    KeyMigration {
        table_path: &["channels", "cargo"],
        dashed: "crate-name",
        underscored: "crate_name",
    },
    KeyMigration {
        table_path: &["channels", "nix"],
        dashed: "flake-repo",
        underscored: "flake_repo",
    },
    KeyMigration {
        table_path: &["macos", "notarization"],
        dashed: "keychain-profile",
        underscored: "keychain_profile",
    },
];

fn table_at_path<'a>(root: &'a Value, path: &[&str]) -> Option<&'a toml::map::Map<String, Value>> {
    let mut value = root;
    for segment in path {
        value = value.get(*segment)?;
    }
    value.as_table()
}

fn full_key(path: &[&str], key: &str) -> String {
    if path.is_empty() {
        key.to_string()
    } else {
        format!("{}.{}", path.join("."), key)
    }
}

fn check_key_migrations(raw: &Value) -> Result<()> {
    for mapping in KEY_MIGRATIONS {
        let Some(table) = table_at_path(raw, mapping.table_path) else {
            continue;
        };
        let dashed = table.get(mapping.dashed);
        let underscored = table.get(mapping.underscored);

        if let Some(old_value) = underscored {
            if let Some(new_value) = dashed {
                if new_value != old_value {
                    bail!(
                        "conflicting config keys '{}' and '{}' have different values",
                        full_key(mapping.table_path, mapping.dashed),
                        full_key(mapping.table_path, mapping.underscored)
                    );
                }
            }
            eprintln!(
                "Warning: config key '{}' is deprecated; use '{}'",
                full_key(mapping.table_path, mapping.underscored),
                full_key(mapping.table_path, mapping.dashed)
            );
        }
    }
    Ok(())
}

impl Config {
    pub fn load(path: &Path) -> Result<Self> {
        let content =
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        Self::parse(&content)
    }

    pub fn parse(content: &str) -> Result<Self> {
        let raw: Value = toml::from_str(content).context("parsing config")?;
        check_key_migrations(&raw)?;
        let config: Config = raw.try_into().context("parsing config")?;
        config.validate()?;
        Ok(config)
    }

    fn validate(&self) -> Result<()> {
        if self.forge.is_some() {
            bail!("forge section was removed; use git");
        }
        if self.channels.github.is_some() {
            bail!("channels.github was removed; use channels.git");
        }
        if self.channels.forge.is_some() {
            bail!("channels.forge was removed; use channels.git");
        }
        if let Some(binary) = self.project.binary.as_deref() {
            if binary.trim().is_empty() {
                bail!("project.binary must not be empty");
            }
        }
        if let Some(package) = self.project.package.as_deref() {
            if package.trim().is_empty() {
                bail!("project.package must not be empty");
            }
        }
        if let Some(binaries) = &self.project.binaries {
            if binaries.is_empty() {
                bail!("project.binaries must not be empty");
            }
            let mut seen: HashSet<&str> = HashSet::new();
            for binary in binaries {
                if binary.trim().is_empty() {
                    bail!("project.binaries must not contain empty values");
                }
                if !seen.insert(binary.as_str()) {
                    bail!("project.binaries must not contain duplicate values");
                }
            }
            if let Some(binary) = self.project.binary.as_deref() {
                if !binaries.iter().any(|b| b == binary) {
                    bail!("project.binary must be included in project.binaries when both are set");
                }
            }
        }
        if self.build.command.is_some() && self.build.pre_built_dir.is_some() {
            bail!("build.command and build.pre-built-dir are mutually exclusive");
        }
        if self.build.command.is_none() && self.build.pre_built_dir.is_none() {
            bail!("one of build.command or build.pre-built-dir is required");
        }
        if self.build.command.is_some() && self.build.artifact.is_none() {
            bail!("build.artifact is required when build.command is set");
        }
        if self.build.targets.is_empty() {
            bail!("build.targets must not be empty");
        }
        if let Some(codesign) = &self.macos.codesign {
            if codesign.identity.trim().is_empty() {
                bail!("macos.codesign.identity must not be empty");
            }
            if codesign
                .entitlements
                .as_ref()
                .is_some_and(|path| path.to_string_lossy().trim().is_empty())
            {
                bail!("macos.codesign.entitlements must not be empty");
            }
        }
        if self
            .macos
            .notarization
            .as_ref()
            .is_some_and(|notarization| notarization.keychain_profile.trim().is_empty())
        {
            bail!("macos.notarization.keychain-profile must not be empty");
        }
        Ok(())
    }

    pub fn enabled_channels(&self) -> Vec<&str> {
        let mut names = Vec::new();
        if let Some(ch) = &self.channels.git {
            if ch.enabled {
                names.push("git");
            }
        }
        if let Some(ch) = &self.channels.homebrew {
            if ch.enabled {
                names.push("homebrew");
            }
        }
        if let Some(ch) = &self.channels.cargo {
            if ch.enabled {
                names.push("cargo");
            }
        }
        if let Some(ch) = &self.channels.curl {
            if ch.enabled {
                names.push("curl");
            }
        }
        if let Some(ch) = &self.channels.nix {
            if ch.enabled {
                names.push("nix");
            }
        }
        names
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn minimal_toml() -> String {
        r#"
[project]
name = "myapp"
repo = "owner/repo"

[build]
command = "cargo build --release --target {target}"
artifact = "target/{target}/release/{binary}"
targets = ["x86_64-apple-darwin"]
"#
        .to_string()
    }

    #[test]
    fn parse_minimal_config() {
        let config = Config::parse(&minimal_toml()).unwrap();
        assert_eq!(config.project.name, "myapp");
        assert_eq!(config.project.repo, "owner/repo");
        assert_eq!(config.project.primary_binary(), "myapp");
        assert!(config.project.binary.is_none());
        assert!(config.project.package.is_none());
        assert!(config.project.binaries.is_none());
        assert!(config.project.version_command.is_none());
        assert!(!config.project.auto_tag);
        assert_eq!(config.build.targets.len(), 1);
        assert!(config.enabled_channels().is_empty());
    }

    #[test]
    fn git_defaults_to_github() {
        let config = Config::parse(&minimal_toml()).unwrap();
        assert_eq!(config.git.r#type, GitType::Github);
        assert_eq!(config.git.web_base_url(), "https://github.com");
        assert_eq!(config.git.api_base_url(), "https://api.github.com");
        assert_eq!(config.git.token_env(), "GITHUB_TOKEN");
    }

    #[test]
    fn git_gitea_defaults_from_base_url() {
        let toml = format!(
            "{}\n[git]\ntype = \"gitea\"\nbase-url = \"https://git.example.com\"\n",
            minimal_toml()
        );
        let config = Config::parse(&toml).unwrap();
        assert_eq!(config.git.r#type, GitType::Gitea);
        assert_eq!(config.git.web_base_url(), "https://git.example.com");
        assert_eq!(config.git.api_base_url(), "https://git.example.com/api/v1");
        assert_eq!(config.git.token_env(), "GITEA_TOKEN");
    }

    #[test]
    fn git_allows_custom_api_and_token() {
        let toml = format!(
            "{}\n[git]\ntype = \"gitea\"\nbase-url = \"https://git.example.com\"\napi-base-url = \"https://git.example.com/custom-api\"\ntoken-env = \"MY_TOKEN\"\n",
            minimal_toml()
        );
        let config = Config::parse(&toml).unwrap();
        assert_eq!(
            config.git.api_base_url(),
            "https://git.example.com/custom-api"
        );
        assert_eq!(config.git.token_env(), "MY_TOKEN");
    }

    #[test]
    fn git_blank_api_base_url_uses_default() {
        let toml = format!(
            "{}\n[git]\ntype = \"gitea\"\nbase-url = \"https://git.example.com\"\napi-base-url = \"   \"\n",
            minimal_toml()
        );
        let config = Config::parse(&toml).unwrap();
        assert_eq!(config.git.api_base_url(), "https://git.example.com/api/v1");
    }

    #[test]
    fn binary_defaults_to_name() {
        let config = Config::parse(&minimal_toml()).unwrap();
        assert_eq!(config.project.primary_binary(), "myapp");
        assert_eq!(config.project.release_binaries(), vec!["myapp"]);
    }

    #[test]
    fn binary_override() {
        let toml = r#"
[project]
name = "myapp"
binary = "myapp-bin"
repo = "owner/repo"

[build]
command = "make"
artifact = "out/{binary}"
targets = ["x86_64-apple-darwin"]
"#;
        let config = Config::parse(toml).unwrap();
        assert_eq!(config.project.primary_binary(), "myapp-bin");
        assert_eq!(config.project.release_binaries(), vec!["myapp-bin"]);
    }

    #[test]
    fn binaries_override_defaults_when_binary_is_unset() {
        let toml = r#"
[project]
name = "myapp"
binaries = ["myapp-server", "myapp-cli"]
repo = "owner/repo"

[build]
command = "make"
artifact = "out/{binary}"
targets = ["x86_64-apple-darwin"]
"#;
        let config = Config::parse(toml).unwrap();
        assert_eq!(config.project.primary_binary(), "myapp-server");
        assert_eq!(
            config.project.release_binaries(),
            vec!["myapp-server", "myapp-cli"]
        );
    }

    #[test]
    fn binaries_and_binary_can_be_set_together() {
        let toml = r#"
[project]
name = "myapp"
binary = "myapp-cli"
binaries = ["myapp-server", "myapp-cli"]
repo = "owner/repo"

[build]
command = "make"
artifact = "out/{binary}"
targets = ["x86_64-apple-darwin"]
"#;
        let config = Config::parse(toml).unwrap();
        assert_eq!(config.project.primary_binary(), "myapp-cli");
        assert_eq!(
            config.project.release_binaries(),
            vec!["myapp-server", "myapp-cli"]
        );
    }

    #[test]
    fn binaries_reject_empty_list() {
        let toml = r#"
[project]
name = "myapp"
binaries = []
repo = "owner/repo"

[build]
command = "make"
artifact = "out/{binary}"
targets = ["x86_64-apple-darwin"]
"#;
        let err = Config::parse(toml).unwrap_err();
        assert!(
            err.to_string()
                .contains("project.binaries must not be empty"),
            "got: {err}"
        );
    }

    #[test]
    fn binaries_reject_duplicates() {
        let toml = r#"
[project]
name = "myapp"
binaries = ["myapp", "myapp"]
repo = "owner/repo"

[build]
command = "make"
artifact = "out/{binary}"
targets = ["x86_64-apple-darwin"]
"#;
        let err = Config::parse(toml).unwrap_err();
        assert!(
            err.to_string()
                .contains("project.binaries must not contain duplicate values"),
            "got: {err}"
        );
    }

    #[test]
    fn binary_must_exist_in_binaries_when_both_are_set() {
        let toml = r#"
[project]
name = "myapp"
binary = "myapp-cli"
binaries = ["myapp-server"]
repo = "owner/repo"

[build]
command = "make"
artifact = "out/{binary}"
targets = ["x86_64-apple-darwin"]
"#;
        let err = Config::parse(toml).unwrap_err();
        assert!(
            err.to_string()
                .contains("project.binary must be included in project.binaries"),
            "got: {err}"
        );
    }

    #[test]
    fn package_override() {
        let toml = r#"
[project]
name = "myapp"
package = "myapp-cli"
repo = "owner/repo"

[build]
command = "make"
artifact = "out/{binary}"
targets = ["x86_64-apple-darwin"]
"#;
        let config = Config::parse(toml).unwrap();
        assert_eq!(config.project.package.as_deref(), Some("myapp-cli"));
    }

    #[test]
    fn pre_built_dir_instead_of_command() {
        let toml = r#"
[project]
name = "myapp"
repo = "owner/repo"

[build]
pre-built-dir = "dist/"
targets = ["x86_64-apple-darwin"]
"#;
        let config = Config::parse(toml).unwrap();
        assert_eq!(config.build.pre_built_dir.as_deref(), Some("dist/"));
        assert!(config.build.command.is_none());
    }

    #[test]
    fn parse_macos_codesign_and_notarization() {
        let toml = format!(
            "{}\n[macos.codesign]\nidentity = \"Developer ID Application: Example Corp (TEAMID)\"\nentitlements = \"entitlements.plist\"\n\n[macos.notarization]\nkeychain-profile = \"release-profile\"\n",
            minimal_toml()
        );
        let config = Config::parse(&toml).unwrap();
        let codesign = config.macos.codesign.unwrap();
        assert_eq!(
            codesign.identity,
            "Developer ID Application: Example Corp (TEAMID)"
        );
        assert_eq!(
            codesign.entitlements.as_deref(),
            Some(Path::new("entitlements.plist"))
        );
        assert_eq!(
            config.macos.notarization.unwrap().keychain_profile,
            "release-profile"
        );
    }

    #[test]
    fn notarization_can_use_pre_signed_artifacts() {
        let toml = format!(
            "{}\n[macos.notarization]\nkeychain-profile = \"release-profile\"\n",
            minimal_toml()
        );
        let config = Config::parse(&toml).unwrap();
        assert!(config.macos.codesign.is_none());
        assert!(config.macos.notarization.is_some());
    }

    #[test]
    fn macos_rejects_empty_credentials() {
        let codesign_toml = format!("{}\n[macos.codesign]\nidentity = \"  \"\n", minimal_toml());
        let codesign_err = Config::parse(&codesign_toml).unwrap_err();
        assert!(
            codesign_err
                .to_string()
                .contains("macos.codesign.identity must not be empty")
        );

        let notarization_toml = format!(
            "{}\n[macos.notarization]\nkeychain-profile = \"  \"\n",
            minimal_toml()
        );
        let notarization_err = Config::parse(&notarization_toml).unwrap_err();
        assert!(
            notarization_err
                .to_string()
                .contains("macos.notarization.keychain-profile must not be empty")
        );
    }

    #[test]
    fn command_and_pre_built_dir_mutually_exclusive() {
        let toml = r#"
[project]
name = "myapp"
repo = "owner/repo"

[build]
command = "make"
artifact = "out/bin"
pre-built-dir = "dist/"
targets = ["x86_64-apple-darwin"]
"#;
        let err = Config::parse(toml).unwrap_err();
        assert!(err.to_string().contains("mutually exclusive"), "got: {err}");
    }

    #[test]
    fn missing_both_command_and_pre_built_dir() {
        let toml = r#"
[project]
name = "myapp"
repo = "owner/repo"

[build]
targets = ["x86_64-apple-darwin"]
"#;
        let err = Config::parse(toml).unwrap_err();
        assert!(
            err.to_string()
                .contains("one of build.command or build.pre-built-dir is required"),
            "got: {err}"
        );
    }

    #[test]
    fn artifact_required_with_command() {
        let toml = r#"
[project]
name = "myapp"
repo = "owner/repo"

[build]
command = "make"
targets = ["x86_64-apple-darwin"]
"#;
        let err = Config::parse(toml).unwrap_err();
        assert!(
            err.to_string()
                .contains("build.artifact is required when build.command is set"),
            "got: {err}"
        );
    }

    #[test]
    fn empty_targets_rejected() {
        let toml = r#"
[project]
name = "myapp"
repo = "owner/repo"

[build]
command = "make"
artifact = "out/bin"
targets = []
"#;
        let err = Config::parse(toml).unwrap_err();
        assert!(
            err.to_string().contains("targets must not be empty"),
            "got: {err}"
        );
    }

    #[test]
    fn channel_presence_means_enabled() {
        let toml = r#"
[project]
name = "myapp"
repo = "owner/repo"

[build]
command = "make"
artifact = "out/bin"
targets = ["x86_64-apple-darwin"]

[channels.git]

[channels.homebrew]
tap = "owner/homebrew-tap"
"#;
        let config = Config::parse(toml).unwrap();
        assert_eq!(config.enabled_channels(), vec!["git", "homebrew"]);
    }

    #[test]
    fn git_channel_only_is_enabled() {
        let toml = r#"
[project]
name = "myapp"
repo = "owner/repo"

[build]
command = "make"
artifact = "out/bin"
targets = ["x86_64-apple-darwin"]

[channels.git]
enabled = true
"#;
        let config = Config::parse(toml).unwrap();
        assert_eq!(config.enabled_channels(), vec!["git"]);
    }

    #[test]
    fn github_channel_is_rejected() {
        let toml = r#"
[project]
name = "myapp"
repo = "owner/repo"

[build]
command = "make"
artifact = "out/bin"
targets = ["x86_64-apple-darwin"]

[channels.github]
enabled = true
"#;
        let err = Config::parse(toml).unwrap_err();
        assert!(
            err.to_string()
                .contains("channels.github was removed; use channels.git"),
            "got: {err}"
        );
    }

    #[test]
    fn forge_channel_is_rejected() {
        let toml = r#"
[project]
name = "myapp"
repo = "owner/repo"

[build]
command = "make"
artifact = "out/bin"
targets = ["x86_64-apple-darwin"]

[channels.forge]
enabled = true
"#;
        let err = Config::parse(toml).unwrap_err();
        assert!(
            err.to_string()
                .contains("channels.forge was removed; use channels.git"),
            "got: {err}"
        );
    }

    #[test]
    fn forge_section_is_rejected() {
        let toml = format!(
            "{}\n[forge]\ntype = \"gitea\"\nbase-url = \"https://git.example.com\"\n",
            minimal_toml()
        );
        let err = Config::parse(&toml).unwrap_err();
        assert!(
            err.to_string()
                .contains("forge section was removed; use git"),
            "got: {err}"
        );
    }

    #[test]
    fn channel_explicitly_disabled() {
        let toml = r#"
[project]
name = "myapp"
repo = "owner/repo"

[build]
command = "make"
artifact = "out/bin"
targets = ["x86_64-apple-darwin"]

[channels.cargo]
enabled = false
crate-name = "myapp"
"#;
        let config = Config::parse(toml).unwrap();
        assert!(config.enabled_channels().is_empty());
    }

    #[test]
    fn git_info_derives_github_repo_from_scp_remote() {
        let info = GitInfo::from_remote_url("git@github.com:nakajima/releasor2000.git").unwrap();
        assert_eq!(info.repo, "nakajima/releasor2000");
        assert_eq!(info.r#type, GitType::Github);
        assert_eq!(info.base_url, "https://github.com");
    }

    #[test]
    fn git_info_derives_gitea_settings_from_https_remote() {
        let info = GitInfo::from_remote_url("https://git.example.com/gitea/team/releasor2000.git")
            .unwrap();
        assert_eq!(info.repo, "team/releasor2000");
        assert_eq!(info.r#type, GitType::Gitea);
        assert_eq!(info.base_url, "https://git.example.com/gitea");
    }

    #[test]
    fn git_info_ignores_local_remotes() {
        assert!(GitInfo::from_remote_url("../releasor2000.git").is_none());
    }

    #[test]
    fn generate_template_parses_successfully() {
        let template = generate_template("myapp", None);
        Config::parse(&template).unwrap();
    }

    #[test]
    fn generate_template_interpolates_project_name() {
        let template = generate_template("cool-tool", None);
        let config = Config::parse(&template).unwrap();
        assert_eq!(config.project.name, "cool-tool");
        assert_eq!(config.project.repo, "owner/cool-tool");
    }

    #[test]
    fn generate_template_uses_git_info() {
        let info =
            GitInfo::from_remote_url("ssh://git@git.example.com/team/cool-tool.git").unwrap();
        let template = generate_template("cool-tool", Some(&info));
        let config = Config::parse(&template).unwrap();
        assert_eq!(config.project.repo, "team/cool-tool");
        assert_eq!(config.git.r#type, GitType::Gitea);
        assert_eq!(config.git.web_base_url(), "https://git.example.com");
    }

    #[test]
    fn full_config_roundtrip() {
        let toml = r#"
[project]
name = "myapp"
binary = "myapp"
repo = "owner/repo"
version-command = "git describe --tags --abbrev=0"

[build]
command = "cargo build --release --target {target}"
artifact = "target/{target}/release/{binary}"
targets = [
    "x86_64-apple-darwin",
    "aarch64-apple-darwin",
    "x86_64-unknown-linux-gnu",
    "aarch64-unknown-linux-gnu",
]

[channels.git]
enabled = true

[channels.homebrew]
tap = "owner/homebrew-tap"
formula-name = "myapp"

[channels.cargo]
enabled = false
crate-name = "myapp"

[channels.curl]
url = "https://myapp.dev/install.sh"

[channels.nix]
flake-repo = "owner/nix-repo"
"#;
        let config = Config::parse(toml).unwrap();
        assert_eq!(config.project.name, "myapp");
        assert_eq!(config.build.targets.len(), 4);
        assert_eq!(
            config.enabled_channels(),
            vec!["git", "homebrew", "curl", "nix"]
        );
        assert_eq!(
            config
                .channels
                .homebrew
                .as_ref()
                .unwrap()
                .formula_name
                .as_deref(),
            Some("myapp")
        );
    }

    #[test]
    fn channels_git_is_enabled_in_realistic_example() {
        let toml = r#"
[project]
name = "releasor2000"
repo = "nakajima/releasor2000"

[build]
command = "cargo build --release --target {target}"
artifact = "target/{target}/release/{binary}"
targets = [
    "x86_64-apple-darwin",
    "aarch64-apple-darwin",
    "x86_64-unknown-linux-gnu",
    "aarch64-unknown-linux-gnu",
]

[channels.git]
enabled = true

[channels.homebrew]
tap = "nakajima/homebrew-tap"
formula-name = "releasor2000"

# [channels.cargo]
crate-name = "releasor2000"

[channels.curl]

[channels.nix]
"#;
        let config = Config::parse(toml).unwrap();
        assert_eq!(
            config.enabled_channels(),
            vec!["git", "homebrew", "curl", "nix"]
        );
    }

    #[test]
    fn parse_auto_tag_from_kebab_case() {
        let toml = r#"
[project]
name = "myapp"
repo = "owner/repo"
auto-tag = true

[build]
command = "make"
artifact = "out/{binary}"
targets = ["x86_64-apple-darwin"]
"#;
        let config = Config::parse(toml).unwrap();
        assert!(config.project.auto_tag);
    }

    #[test]
    fn parse_auto_tag_from_underscore_alias() {
        let toml = r#"
[project]
name = "myapp"
repo = "owner/repo"
auto_tag = true

[build]
command = "make"
artifact = "out/{binary}"
targets = ["x86_64-apple-darwin"]
"#;
        let config = Config::parse(toml).unwrap();
        assert!(config.project.auto_tag);
    }

    #[test]
    fn parse_conflicting_key_styles_fails() {
        let toml = r#"
[project]
name = "myapp"
repo = "owner/repo"
version-command = "cargo metadata --no-deps"
version_command = "git describe --tags --abbrev=0"

[build]
command = "make"
artifact = "out/{binary}"
targets = ["x86_64-apple-darwin"]
"#;
        let err = Config::parse(toml).unwrap_err();
        assert!(
            err.to_string().contains(
                "conflicting config keys 'project.version-command' and 'project.version_command'"
            ),
            "got: {err}"
        );
    }
}
