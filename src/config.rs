use anyhow::{Context, Result, bail};
use serde::Deserialize;
use std::path::Path;

#[derive(Debug, Deserialize)]
pub struct Config {
    pub project: Project,
    pub build: Build,
    #[serde(default)]
    pub channels: Channels,
}

#[derive(Debug, Deserialize)]
pub struct Project {
    pub name: String,
    pub binary: Option<String>,
    pub repo: String,
    pub version_command: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct Build {
    pub command: Option<String>,
    pub artifact: Option<String>,
    pub pre_built_dir: Option<String>,
    pub targets: Vec<String>,
}

#[derive(Debug, Default, Deserialize)]
pub struct Channels {
    pub github: Option<GitHubChannel>,
    pub homebrew: Option<HomebrewChannel>,
    pub cargo: Option<CargoChannel>,
    pub curl: Option<CurlChannel>,
    pub nix: Option<NixChannel>,
}

#[derive(Debug, Deserialize)]
pub struct GitHubChannel {
    #[serde(default = "default_true")]
    pub enabled: bool,
}

#[derive(Debug, Deserialize)]
pub struct HomebrewChannel {
    #[serde(default = "default_true")]
    pub enabled: bool,
    pub tap: String,
    pub formula_name: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CargoChannel {
    #[serde(default = "default_true")]
    pub enabled: bool,
    pub crate_name: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CurlChannel {
    #[serde(default = "default_true")]
    pub enabled: bool,
}

#[derive(Debug, Deserialize)]
pub struct NixChannel {
    #[serde(default = "default_true")]
    pub enabled: bool,
    pub flake_repo: Option<String>,
}

fn default_true() -> bool {
    true
}

impl Project {
    pub fn binary(&self) -> &str {
        self.binary.as_deref().unwrap_or(&self.name)
    }
}

pub fn generate_template(project_name: &str) -> String {
    format!(
        r#"[project]
name = "{project_name}"
# binary = "{project_name}"  # defaults to project name
repo = "owner/{project_name}"
# version_command = "git describe --tags --abbrev=0"

[build]
command = "cargo build --release --target {{target}}"
artifact = "target/{{target}}/release/{{binary}}"
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
# formula_name = "{project_name}"

# [channels.cargo]
# crate_name = "{project_name}"

# [channels.curl]

# [channels.nix]
# flake_repo = "owner/nix-repo"  # defaults to project repo
"#
    )
}

impl Config {
    pub fn load(path: &Path) -> Result<Self> {
        let content =
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        Self::parse(&content)
    }

    pub fn parse(content: &str) -> Result<Self> {
        let config: Config = toml::from_str(content).context("parsing config")?;
        config.validate()?;
        Ok(config)
    }

    fn validate(&self) -> Result<()> {
        if self.build.command.is_some() && self.build.pre_built_dir.is_some() {
            bail!("build.command and build.pre_built_dir are mutually exclusive");
        }
        if self.build.command.is_none() && self.build.pre_built_dir.is_none() {
            bail!("one of build.command or build.pre_built_dir is required");
        }
        if self.build.command.is_some() && self.build.artifact.is_none() {
            bail!("build.artifact is required when build.command is set");
        }
        if self.build.targets.is_empty() {
            bail!("build.targets must not be empty");
        }
        Ok(())
    }

    pub fn enabled_channels(&self) -> Vec<&str> {
        let mut names = Vec::new();
        if let Some(ch) = &self.channels.github {
            if ch.enabled {
                names.push("github");
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
        assert_eq!(config.project.binary(), "myapp");
        assert!(config.project.binary.is_none());
        assert!(config.project.version_command.is_none());
        assert_eq!(config.build.targets.len(), 1);
        assert!(config.enabled_channels().is_empty());
    }

    #[test]
    fn binary_defaults_to_name() {
        let config = Config::parse(&minimal_toml()).unwrap();
        assert_eq!(config.project.binary(), "myapp");
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
        assert_eq!(config.project.binary(), "myapp-bin");
    }

    #[test]
    fn pre_built_dir_instead_of_command() {
        let toml = r#"
[project]
name = "myapp"
repo = "owner/repo"

[build]
pre_built_dir = "dist/"
targets = ["x86_64-apple-darwin"]
"#;
        let config = Config::parse(toml).unwrap();
        assert_eq!(config.build.pre_built_dir.as_deref(), Some("dist/"));
        assert!(config.build.command.is_none());
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
pre_built_dir = "dist/"
targets = ["x86_64-apple-darwin"]
"#;
        let err = Config::parse(toml).unwrap_err();
        assert!(
            err.to_string().contains("mutually exclusive"),
            "got: {err}"
        );
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
                .contains("one of build.command or build.pre_built_dir is required"),
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

[channels.github]

[channels.homebrew]
tap = "owner/homebrew-tap"
"#;
        let config = Config::parse(toml).unwrap();
        assert_eq!(config.enabled_channels(), vec!["github", "homebrew"]);
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
crate_name = "myapp"
"#;
        let config = Config::parse(toml).unwrap();
        assert!(config.enabled_channels().is_empty());
    }

    #[test]
    fn generate_template_parses_successfully() {
        let template = generate_template("myapp");
        Config::parse(&template).unwrap();
    }

    #[test]
    fn generate_template_interpolates_project_name() {
        let template = generate_template("cool-tool");
        let config = Config::parse(&template).unwrap();
        assert_eq!(config.project.name, "cool-tool");
        assert_eq!(config.project.repo, "owner/cool-tool");
    }

    #[test]
    fn full_config_roundtrip() {
        let toml = r#"
[project]
name = "myapp"
binary = "myapp"
repo = "owner/repo"
version_command = "git describe --tags --abbrev=0"

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

[channels.homebrew]
tap = "owner/homebrew-tap"
formula_name = "myapp"

[channels.cargo]
enabled = false
crate_name = "myapp"

[channels.curl]
url = "https://myapp.dev/install.sh"

[channels.nix]
flake_repo = "owner/nix-repo"
"#;
        let config = Config::parse(toml).unwrap();
        assert_eq!(config.project.name, "myapp");
        assert_eq!(config.build.targets.len(), 4);
        assert_eq!(
            config.enabled_channels(),
            vec!["github", "homebrew", "curl", "nix"]
        );
        assert_eq!(
            config.channels.homebrew.as_ref().unwrap().formula_name.as_deref(),
            Some("myapp")
        );
    }
}
