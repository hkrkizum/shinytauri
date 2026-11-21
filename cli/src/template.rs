use std::error::Error;
use std::fs;
use std::path::Path;

// An explicit list excludes local build output, user files, and demo sources.
// include_bytes! also makes Cargo rebuild the CLI when a template input changes.
macro_rules! engine_files {
    ($($name:literal),+ $(,)?) => {
        &[$(($name, include_bytes!(concat!("../../", $name)) as &[u8])),+]
    };
}

const ENGINE_FILES: &[(&str, &[u8])] = engine_files![
    "LICENSE",
    "flake.nix",
    "flake.lock",
    "nix/rig.nix",
    "rust-toolchain.toml",
    "rproj.toml",
    "rproj.lock",
    "scripts/build-windows.R",
    "scripts/serve-bundled.R",
    "ui/index.html",
    "src-tauri/build.rs",
    "src-tauri/Cargo.toml",
    "src-tauri/Cargo.lock",
    "src-tauri/.cargo/config.toml",
    "src-tauri/tauri.conf.json",
    "src-tauri/tauri.windows.conf.json",
    "src-tauri/src/main.rs",
    "src-tauri/src/offline.rs",
    "src-tauri/src/quarto.rs",
    "src-tauri/icons/32x32.png",
    "src-tauri/icons/128x128.png",
    "src-tauri/icons/128x128@2x.png",
    "src-tauri/icons/icon.png",
    "src-tauri/icons/icon.ico",
    "src-tauri/icons/icon.icns",
];

pub fn extract(destination: &Path) -> Result<(), Box<dyn Error>> {
    if fs::read_dir(destination)?.next().transpose()?.is_some() {
        return Err("engine destination is not empty".into());
    }
    for (name, contents) in ENGINE_FILES {
        let path = destination.join(name);
        fs::create_dir_all(path.parent().expect("template file has a parent"))?;
        fs::write(path, contents)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    #[test]
    fn extracted_engine_is_a_locked_cargo_project_outside_the_repository() {
        let engine = tempfile::tempdir().unwrap();
        extract(engine.path()).unwrap();

        let output = Command::new(env!("CARGO"))
            .current_dir(engine.path().join("src-tauri"))
            .args([
                "metadata",
                "--format-version",
                "1",
                "--no-deps",
                "--locked",
                "--offline",
            ])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(engine.path().join("scripts/build-windows.R").is_file());
        assert!(engine.path().join("ui/index.html").is_file());
        assert!(engine.path().join("rproj.lock").is_file());
        assert!(!engine.path().join("src-shiny").exists());
    }

    #[test]
    fn occupied_destination_is_preserved() {
        let engine = tempfile::tempdir().unwrap();
        let existing = engine.path().join("LICENSE");
        fs::write(&existing, "already here").unwrap();
        assert!(extract(engine.path()).is_err());
        assert_eq!(fs::read_to_string(existing).unwrap(), "already here");
        assert_eq!(fs::read_dir(engine.path()).unwrap().count(), 1);
    }
}
