#[cfg(windows)]
mod branding;
#[cfg(any(windows, test))]
mod browser;
#[cfg(windows)]
mod desktop;
#[cfg(any(windows, test))]
mod logs;
#[cfg(any(windows, test))]
mod notices;
mod package;
#[cfg(windows)]
mod processes;
#[cfg(windows)]
mod suite;
#[cfg(windows)]
mod upgrade;
#[cfg(windows)]
mod winui;

use anyhow::Result;
use clap::Parser;
use std::path::PathBuf;

/// Build and exercise real shinytauri installers for the acceptance suite.
#[derive(Parser)]
enum Command {
    Package {
        #[arg(value_enum)]
        fixture: package::Fixture,
        cli: PathBuf,
        repo: PathBuf,
        temp: PathBuf,
    },
    #[cfg(windows)]
    InstalledApps,
    #[cfg(windows)]
    BrandingSetup { exe: PathBuf, screenshot: PathBuf },
    #[cfg(windows)]
    BrandingHost { exe: PathBuf },
    #[cfg(windows)]
    Upgrade { setup: PathBuf, evidence: PathBuf },
    #[cfg(windows)]
    Desktop {
        #[arg(value_parser = ["minimal", "upgrade", "rhino"])]
        kind: String,
        exe: PathBuf,
        evidence: PathBuf,
    },
}

fn main() -> Result<()> {
    match Command::parse() {
        Command::Package {
            fixture,
            cli,
            repo,
            temp,
        } => package::run(fixture, &cli, &repo, &temp),
        #[cfg(windows)]
        Command::InstalledApps => suite::run(),
        #[cfg(windows)]
        Command::BrandingSetup { exe, screenshot } => branding::check_setup(&exe, &screenshot),
        #[cfg(windows)]
        Command::BrandingHost { exe } => branding::check_host(&exe),
        #[cfg(windows)]
        Command::Upgrade { setup, evidence } => upgrade::run(&setup, &evidence),
        #[cfg(windows)]
        Command::Desktop {
            kind,
            exe,
            evidence,
        } => desktop::run(&kind, &exe, &evidence),
    }
}
