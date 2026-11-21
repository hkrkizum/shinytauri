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
        /// Additional CRAN runtime package, optionally pinned as NAME=VERSION. Repeat as needed.
        #[arg(long = "package")]
        packages: Vec<String>,
        /// Exact portable R version for the Windows app, for example 4.5.3.
        #[arg(long, value_name = "X.Y.Z", value_parser = parse_r_version)]
        r_version: Option<String>,
    },
    /// Resolve Windows runtime dependencies and write the reviewable Windows lock.
    Lock {
        /// Directory containing shinytauri.toml (defaults to the current directory).
        #[arg(conflicts_with = "config")]
        app: Option<PathBuf>,
        /// Explicit path to a bundle TOML, relative to the current directory.
        #[arg(long)]
        config: Option<PathBuf>,
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

fn parse_r_version(value: &str) -> std::result::Result<String, String> {
    let parts: Vec<_> = value.split('.').collect();
    if parts.len() == 3
        && parts.iter().all(|part| {
            !part.is_empty()
                && part.bytes().all(|b| b.is_ascii_digit())
                && (part.len() == 1 || !part.starts_with('0'))
        })
    {
        Ok(value.to_owned())
    } else {
        Err("expected an exact R version such as 4.5.3".into())
    }
}

fn main() -> ExitCode {
    let result = match Cli::parse().command {
        Commands::Init {
            app,
            id,
            packages,
            r_version,
        } => init::initialize(&app, &id, packages, r_version),
        Commands::Lock { app, config } => build::execute(app, config, build::Action::Lock),
        Commands::Build {
            app,
            config,
            out_dir,
        } => build::execute(app, config, build::Action::Build(out_dir)),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("shinytauri: {err}");
            ExitCode::FAILURE
        }
    }
}
