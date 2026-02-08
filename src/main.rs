mod channels;
mod config;

use anyhow::{Result, bail};
use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "releasor2000", about = "Release your software everywhere")]
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

    if let Command::Init = &cli.command {
        if cli.config.exists() {
            bail!("{} already exists", cli.config.display());
        }
        let name = std::env::current_dir()?
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "myproject".to_string());
        std::fs::write(&cli.config, config::generate_template(&name))?;
        println!("Created {}", cli.config.display());
        return Ok(());
    }

    let config = config::Config::load(&cli.config)?;

    match cli.command {
        Command::Init => unreachable!(),
        Command::Validate => {
            println!("Config is valid.");
            println!("Enabled channels: {:?}", config.enabled_channels());
            Ok(())
        }
        Command::Release { version, channels } => {
            let channels = if channels.is_empty() { None } else { Some(channels) };
            channels::release(&config, version.as_deref(), channels.as_deref())
        }
    }
}
