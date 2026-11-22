use crate::Result;
use std::{env, ffi::OsStr, process::Command};

/// Version and configuration queries only. In particular rustup must never
/// install an absent toolchain as a side effect of running doctor.
fn query(program: impl AsRef<OsStr>, args: &[&str]) -> Option<String> {
    let output = Command::new(program)
        .args(args)
        .env("RUSTUP_AUTO_INSTALL", "0")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    (!text.is_empty()).then_some(text)
}

#[derive(Default)]
struct Checks {
    failures: usize,
}

impl Checks {
    fn report(&mut self, label: &str, value: Option<String>, fix: &str) {
        match value {
            Some(value) => println!("[ok] {label}: {value}"),
            None => {
                self.failures += 1;
                println!("[missing] {label}: {fix}");
            }
        }
    }

    fn rig(&mut self) {
        self.report(
            "rig",
            query("rig", &["--version"]),
            "Install rig 0.11.1 and add it to PATH.",
        );
    }
}

#[cfg(target_os = "linux")]
fn linux(checks: &mut Checks) {
    if env::var_os("SHINYTAURI_WINDOWS_SHELL").as_deref() == Some(OsStr::new("1")) {
        checks.rig();
        checks.report(
            "Cargo",
            query("cargo", &["--version"]),
            "Re-enter the windows Nix shell.",
        );
        let tauri = env::var_os("SHINYTAURI_CARGO_TAURI");
        let version = match tauri {
            Some(path) => query(path, &["--version"]),
            None => query("cargo", &["tauri", "--version"]),
        };
        checks.report("Tauri CLI", version, "Re-enter the windows Nix shell.");
        checks.report(
            "MinGW-w64",
            query("x86_64-w64-mingw32-gcc", &["-dumpmachine"]),
            "Re-enter the windows Nix shell.",
        );
        checks.report(
            "NSIS",
            query("makensis", &["-VERSION"]),
            "Re-enter the windows Nix shell.",
        );
        return;
    }
    checks.report(
        "Git",
        query("git", &["--version"]),
        "Install Git and add git to PATH.",
    );
    let version = query("nix", &["--version"]);
    let features = version.as_ref().and_then(|_| {
        // Older Nix releases expose the same setting through show-config.
        query("nix", &["config", "show", "experimental-features"]).or_else(|| {
            query("nix", &["show-config"])?.lines().find_map(|line| {
                let (name, value) = line.split_once('=')?;
                (name.trim() == "experimental-features").then(|| value.trim().to_owned())
            })
        })
    });
    checks.report("Nix", version, "Install Nix and add nix to PATH.");
    let features = features.filter(|value| {
        ["nix-command", "flakes"]
            .iter()
            .all(|required| value.split_whitespace().any(|part| part == *required))
    });
    checks.report(
        "Nix features",
        features,
        "Set extra-experimental-features = nix-command flakes in nix.conf.",
    );
}

#[cfg(windows)]
fn windows(checks: &mut Checks) -> Result<()> {
    const TARGET: &str = "x86_64-pc-windows-msvc";
    let toolchain: toml::Table = include_str!("../../rust-toolchain.toml").parse()?;
    let channel = toolchain["toolchain"]["channel"]
        .as_str()
        .ok_or("Missing host Rust channel")?;
    let toolchain = format!("{channel}-{TARGET}");
    let cargo = query("rustup", &["which", "--toolchain", &toolchain, "cargo"]);
    checks.report(
        "Rust (MSVC x64)",
        cargo,
        &format!("Install rustup and run rustup toolchain install {toolchain}."),
    );
    checks.rig();
    let tauri = match env::var_os("SHINYTAURI_CARGO_TAURI") {
        Some(path) => query(path, &["--version"]),
        None => query(
            "rustup",
            &["run", &toolchain, "cargo", "tauri", "--version"],
        ),
    };
    checks.report(
        "Tauri CLI",
        tauri,
        "Install cargo-tauri 2.12.6 or run cargo install tauri-cli --version 2.12.6 --locked.",
    );
    // Use the tool discovery library shared with cc-rs; a normal PowerShell
    // session need not have cl.exe, rc.exe or the SDK libraries in PATH.
    let compiler = find_msvc_tools::find_tool(TARGET, "cl.exe")
        .filter(|tool| tool.path().is_file())
        .map(|tool| tool.path().display().to_string());
    checks.report(
        "MSVC C++",
        compiler,
        "Install Desktop development with C++ in Visual Studio Build Tools.",
    );
    let sdk = find_msvc_tools::find_windows_sdk("x86_64")
        .filter(|sdk| {
            sdk.libs().any(|dir| dir.join("kernel32.lib").is_file())
                && sdk.path().any(|dir| dir.join("rc.exe").is_file())
        })
        .map(|sdk| sdk.sdk_version().to_owned());
    checks.report(
        "Windows SDK",
        sdk,
        "Install the Windows 10 or 11 SDK in Visual Studio Installer.",
    );
    Ok(())
}

pub(crate) fn run() -> Result<()> {
    println!("Build host: {} {}", env::consts::OS, env::consts::ARCH);
    if !cfg!(all(
        target_arch = "x86_64",
        any(target_os = "linux", windows)
    )) {
        return Err("Use an x86_64 Linux or Windows build machine.".into());
    }
    let mut checks = Checks::default();
    #[cfg(target_os = "linux")]
    linux(&mut checks);
    #[cfg(windows)]
    windows(&mut checks)?;
    if checks.failures != 0 {
        println!("Setup guide: https://hkrkizum.github.io/shinytauri/installation.html");
        return Err(format!(
            "{} build environment check(s) need attention.",
            checks.failures
        )
        .into());
    }
    Ok(())
}
