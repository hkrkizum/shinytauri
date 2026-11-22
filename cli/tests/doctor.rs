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
