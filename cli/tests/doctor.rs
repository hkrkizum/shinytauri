#![cfg(target_os = "linux")]

use std::{fs, os::unix::fs::PermissionsExt, path::Path, process::Command};
use tempfile::tempdir;

fn tool(path: &Path, name: &str, script: &str) {
    let file = path.join(name);
    fs::write(&file, format!("#!/bin/sh\n{script}\n")).unwrap();
    fs::set_permissions(file, fs::Permissions::from_mode(0o700)).unwrap();
}

fn doctor(path: &Path, project: &Path) -> (bool, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_shinytauri"))
        .arg("doctor")
        .env("PATH", path)
        .env_remove("SHINYTAURI_WINDOWS_SHELL")
        .current_dir(project)
        .output()
        .unwrap();
    (
        output.status.success(),
        String::from_utf8(output.stdout).unwrap(),
    )
}

#[test]
fn reports_all_missing_tools_and_the_fix_without_creating_a_project() {
    let empty = tempdir().unwrap();
    let (ok, text) = doctor(empty.path(), empty.path());
    assert!(!ok);
    for hint in ["Git", "Nix", "flakes", "installation.html"] {
        assert!(text.contains(hint), "{text}");
    }
    assert_eq!(fs::read_dir(empty.path()).unwrap().count(), 0);
}

#[test]
fn reads_effective_nix_features_and_never_starts_a_build() {
    let bin = tempdir().unwrap();
    let app = tempdir().unwrap();
    tool(
        bin.path(),
        "git",
        "test \"$*\" = '--version' && echo 'git version 2.50'",
    );
    for (features, expected) in [
        ("nix-command", false),
        ("ca-derivations flakes nix-command", true),
        ("no-flakes nix-command", false),
    ] {
        let script = format!(
            "case \"$*\" in\n'--version') echo 'nix (Nix) 2.30';;\n'config show experimental-features') echo '{features}';;\n*) exit 1;;\nesac"
        );
        tool(bin.path(), "nix", &script);
        let (ok, text) = doctor(bin.path(), app.path());
        assert_eq!(ok, expected, "{text}");
        assert!(text.contains("flakes"), "{text}");
    }
    assert_eq!(fs::read_dir(app.path()).unwrap().count(), 0);
}

#[test]
fn cross_shell_requires_the_build_tools_used_by_the_host() {
    let bin = tempdir().unwrap();
    let app = tempdir().unwrap();
    tool(
        bin.path(),
        "cargo",
        "case \"$*\" in 'tauri --version') echo 'tauri-cli 2.12.6';; '--version') echo 'cargo 1.100.0';; esac",
    );
    tool(
        bin.path(),
        "rig",
        "case \"$*\" in 'proj sync --help') echo '--locked';; '--version') echo 'rig 0.11.1';; esac",
    );
    tool(
        bin.path(),
        "x86_64-w64-mingw32-gcc",
        "echo x86_64-w64-mingw32",
    );
    tool(bin.path(), "makensis", "echo v3.12");
    let run = || {
        Command::new(env!("CARGO_BIN_EXE_shinytauri"))
            .arg("doctor")
            .env("PATH", bin.path())
            .env("SHINYTAURI_WINDOWS_SHELL", "1")
            .env("SHINYTAURI_CARGO_TAURI", "")
            .current_dir(app.path())
            .output()
            .unwrap()
    };
    assert!(run().status.success());
    tool(bin.path(), "cargo", "echo tauri-cli 1.6.0");
    tool(bin.path(), "rig", "echo 'rig 0.6.2'");
    tool(
        bin.path(),
        "x86_64-w64-mingw32-gcc",
        "echo x86_64-linux-gnu",
    );
    let failed = run();
    assert!(!failed.status.success());
    let report = String::from_utf8(failed.stdout).unwrap();
    for label in ["rig", "Tauri CLI", "MinGW-w64"] {
        assert!(report.contains(&format!("[missing] {label}")), "{report}");
    }
}
