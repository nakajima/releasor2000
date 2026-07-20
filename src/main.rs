mod channels;
mod config;
mod upgrade;

use anyhow::{Result, bail};
use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "releasor2000",
    about = "Release your software everywhere",
    version = env!("RELEASOR2000_VERSION")
)]
struct Cli {
    #[arg(short, long, default_value = "releasor2000.toml")]
    config: PathBuf,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Generate a releasor2000.toml config file
    Init,
    /// Upgrade releasor2000 in place to the latest release
    Upgrade,
    /// Run a full release across all enabled channels
    Release {
        #[arg(long)]
        version: Option<String>,
        /// Channels to release to (defaults to all enabled channels)
        channels: Vec<String>,
    },
    /// Validate the config file without doing anything
    Validate,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match &cli.command {
        Command::Init => {
            if cli.config.exists() {
                bail!("{} already exists", cli.config.display());
            }
            let current_dir = std::env::current_dir()?;
            let name = current_dir
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| "myproject".to_string());
            let git_info = config::GitInfo::from_config(&current_dir.join(".git/config"));
            std::fs::write(
                &cli.config,
                config::generate_template(&name, git_info.as_ref()),
            )?;
            println!("Created {}", cli.config.display());
            return Ok(());
        }
        Command::Upgrade => return upgrade::upgrade(),
        _ => {}
    }

    let config = config::Config::load(&cli.config)?;

    match cli.command {
        Command::Init | Command::Upgrade => unreachable!(),
        Command::Validate => {
            println!("Config is valid.");
            println!("Enabled channels: {:?}", config.enabled_channels());
            Ok(())
        }
        Command::Release { version, channels } => {
            let channels = if channels.is_empty() {
                None
            } else {
                Some(channels)
            };
            channels::release(&config, version.as_deref(), channels.as_deref())
        }
    }
}
