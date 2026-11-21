mod build;
mod init;
mod template;

use clap::{Parser, Subcommand};
use std::{error::Error, path::PathBuf, process::ExitCode};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

#[derive(Parser)]
#[command(
    name = "shinytauri",
    version,
    about = "Package a Shiny app as an offline Windows x64 installer"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Create shinytauri.toml in an existing Shiny app directory.
    Init {
        #[arg(default_value = ".")]
        app: PathBuf,
        /// Stable reverse-domain identifier for installation and upgrades.
        #[arg(long)]
        id: String,
    },
    /// Build the Windows installer from an app directory or a bundle TOML.
    Build {
        /// Directory containing shinytauri.toml (defaults to the current directory).
        #[arg(conflicts_with = "config")]
        app: Option<PathBuf>,
        /// Explicit path to a bundle TOML, relative to the current directory.
        #[arg(long)]
        config: Option<PathBuf>,
        /// Destination directory (defaults to dist/ beside the config).
        #[arg(long)]
        out_dir: Option<PathBuf>,
    },
}

fn main() -> ExitCode {
    let result = match Cli::parse().command {
        Commands::Init { app, id } => init::initialize(&app, &id),
        Commands::Build {
            app,
            config,
            out_dir,
        } => build::build(app, config, out_dir),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("shinytauri: {err}");
            ExitCode::FAILURE
        }
    }
}
