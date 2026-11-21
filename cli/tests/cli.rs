use std::fs;
use std::path::Path;
use std::process::{Command, Output};
use tempfile::tempdir;

fn cli(cwd: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_shinytauri"))
        .current_dir(cwd)
        .args(args)
        .output()
        .unwrap()
}

fn spec(path: &Path) -> toml::Table {
    fs::read_to_string(path.join("shinytauri.toml"))
        .unwrap()
        .parse()
        .unwrap()
}

#[test]
fn init_a_standalone_app_can_use_the_included_windows_shiny_lock() {
    let root = tempdir().unwrap();
    let app = root.path().join("売上 集計");
    fs::create_dir_all(app.join("www")).unwrap();
    fs::create_dir_all(app.join("data")).unwrap();
    for file in [
        "app.R",
        "www/a.css",
        "www/.env",
        "data/n.csv",
        ".Renviron",
        "notes.txt",
    ] {
        fs::write(app.join(file), file).unwrap();
    }
    let result = cli(
        root.path(),
        &["init", "売上 集計", "--id", "org.example.sales"],
    );
    assert!(result.status.success(), "{result:?}");
    let spec = spec(&app);
    assert_eq!(spec["identifier"].as_str(), Some("org.example.sales"));
    assert_eq!(spec["productName"].as_str(), Some("売上 集計"));
    let files: Vec<_> = spec["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x.as_str().unwrap())
        .collect();
    assert_eq!(files, ["app.R", "data/n.csv", "www/a.css"]);
    let lock: toml::Table = fs::read_to_string(app.join(spec["lock"].as_str().unwrap()))
        .unwrap()
        .parse()
        .unwrap();
    assert_eq!(
        lock["targets"].as_array().unwrap()[0]["platform"].as_str(),
        Some("x86_64-w64-mingw32")
    );
    assert!(app.join(".shinytauri/rproj.toml").is_file());
    fs::write(app.join("shinytauri.toml"), "my settings").unwrap();
    assert!(
        !cli(&app, &["init", "--id", "org.example.other"])
            .status
            .success()
    );
    assert_eq!(
        fs::read_to_string(app.join("shinytauri.toml")).unwrap(),
        "my settings"
    );
}

#[test]
fn init_reuses_an_app_lock_on_a_split_layout() {
    let root = tempdir().unwrap();
    let app = root.path();
    for name in ["ui.R", "server.R", "global.R"] {
        fs::write(app.join(name), "").unwrap();
    }
    let lock = include_str!("../../bundles/minimal/rproj.lock");
    fs::write(app.join("rproj.lock"), lock).unwrap();
    assert!(
        cli(app, &["init", "--id", "net.test.split"])
            .status
            .success()
    );
    assert_eq!(spec(app)["lock"].as_str(), Some("rproj.lock"));
    assert_eq!(fs::read_to_string(app.join("rproj.lock")).unwrap(), lock);
    assert!(!app.join(".shinytauri").exists());
}

#[test]
fn init_failure_preserves_existing_dependencies_and_leaves_no_config() {
    let root = tempdir().unwrap();
    let app = root.path();
    fs::write(app.join("ui.R"), "").unwrap();
    assert!(
        !cli(app, &["init", "--id", "org.example.test"])
            .status
            .success()
    );
    fs::write(app.join("app.R"), "").unwrap();
    assert!(!cli(app, &["init", "--id", "has space"]).status.success());
    fs::create_dir(app.join(".shinytauri")).unwrap();
    fs::write(app.join(".shinytauri/rproj.toml"), "custom").unwrap();
    assert!(
        !cli(app, &["init", "--id", "org.example.test"])
            .status
            .success()
    );
    assert_eq!(
        fs::read_to_string(app.join(".shinytauri/rproj.toml")).unwrap(),
        "custom"
    );
    assert!(!app.join("shinytauri.toml").exists());
    assert!(!app.join(".shinytauri/rproj.lock").exists());
}

#[test]
fn init_diagnoses_incompatible_root_lock() {
    let root = tempdir().unwrap();
    fs::write(root.path().join("app.R"), "").unwrap();
    fs::write(
        root.path().join("rproj.lock"),
        "[[targets]]\nplatform='mac'",
    )
    .unwrap();
    let result = cli(root.path(), &["init", "--id", "net.test.case"]);
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("Windows"));
    assert!(!root.path().join("shinytauri.toml").exists());
}

#[cfg(target_os = "linux")]
#[test]
fn build_enters_one_private_shell_and_passes_external_paths_verbatim() {
    use std::os::unix::fs::PermissionsExt;
    let root = tempdir().unwrap();
    let bin = root.path().join("bin");
    let config = root.path().join("外部 $ (spec).toml");
    fs::create_dir(&bin).unwrap();
    fs::write(&config, "").unwrap();
    let fake_nix = bin.join("nix");
    fs::write(
        &fake_nix,
        r#"#!/bin/sh
set -eu
test -e src-tauri/Cargo.toml && test -e rproj.lock
pwd >> "$PROBE/shells"
while [ "$1" != "-c" ]; do shift; done
shift
exec "$@"
"#,
    )
    .unwrap();
    let fake_rig = bin.join("rig");
    fs::write(
        &fake_rig,
        r#"#!/bin/sh
set -eu
case "$1" in
proj)
  test "$2" = sync && test "$3" = --locked
  test ! -e .rvenv && mkdir .rvenv
  ;;
run)
  test -d .rvenv
  test "$2" = --rscript && test "$3" = scripts/build-windows.R
  test "$4" = "$PROBE/外部 $ (spec).toml"
  test "$5" = "$PROBE/成果 物"
  mkdir "$5"
  touch "$5/fixture-setup.exe"
  ;;
*) exit 42 ;;
esac
"#,
    )
    .unwrap();
    for exe in [&fake_nix, &fake_rig] {
        fs::set_permissions(exe, fs::Permissions::from_mode(0o755)).unwrap()
    }
    let output = Command::new(env!("CARGO_BIN_EXE_shinytauri"))
        .current_dir(root.path())
        .args([
            "build",
            "--config",
            "外部 $ (spec).toml",
            "--out-dir",
            "成果 物",
        ])
        .env("PATH", format!("{}:/bin:/usr/bin", bin.display()))
        .env("PROBE", root.path())
        .env_remove("SHINYTAURI_WINDOWS_SHELL")
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    assert!(root.path().join("成果 物/fixture-setup.exe").is_file());
    let logs = fs::read_to_string(root.path().join("shells")).unwrap();
    assert_eq!(logs.lines().count(), 1);
    assert!(!Path::new(logs.trim()).exists());
    assert!(!root.path().join(".rvenv").exists());
}

#[cfg(unix)]
#[test]
fn init_does_not_follow_links_out_of_the_app_or_select_unrelated_config() {
    use std::os::unix::fs::symlink;
    let base = tempdir().unwrap();
    let app = base.path().join("app");
    fs::create_dir_all(app.join("www")).unwrap();
    fs::write(app.join("app.R"), "").unwrap();
    fs::write(app.join("config.yml"), "private: yes").unwrap();
    fs::write(base.path().join("outside"), "private").unwrap();
    symlink(base.path(), app.join("www/cycle")).unwrap();
    symlink(base.path().join("outside"), app.join("www/alias")).unwrap();
    let result = cli(&app, &["init", "--id", "org.example.test"]);
    assert!(result.status.success(), "{result:?}");
    assert_eq!(
        spec(&app)["files"].as_array().unwrap(),
        &[toml::Value::String("app.R".into())]
    );
}

#[cfg(target_os = "linux")]
#[test]
fn a_failed_environment_restore_stops_the_build_and_cleans_the_engine() {
    use std::os::unix::fs::PermissionsExt;
    let root = tempdir().unwrap();
    fs::write(root.path().join("shinytauri.toml"), "app fixture").unwrap();
    let rig = root.path().join("rig");
    fs::write(&rig, "#!/bin/sh\npwd >> \"$PROBE\"\nexit 9\n").unwrap();
    fs::set_permissions(&rig, fs::Permissions::from_mode(0o755)).unwrap();
    let probe = root.path().join("calls");
    let result = Command::new(env!("CARGO_BIN_EXE_shinytauri"))
        .arg("build")
        .current_dir(root.path())
        .env("SHINYTAURI_WINDOWS_SHELL", "1")
        .env("PATH", root.path())
        .env("PROBE", &probe)
        .output()
        .unwrap();
    assert!(!result.status.success());
    let calls = fs::read_to_string(probe).unwrap();
    assert_eq!(calls.lines().count(), 1);
    assert!(!Path::new(calls.trim()).exists());
    assert!(!root.path().join("dist").exists());
}

#[test]
fn init_preserves_the_rhino_demo_static_resource_directory() {
    let demo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../src-shiny");
    let app = tempdir().unwrap();
    let static_dir = app.path().join("app/static");
    fs::create_dir_all(&static_dir).unwrap();
    for file in [
        "app.R",
        "rhino.yml",
        "config.yml",
        "app/static/.gitkeep",
        "rproj.lock",
    ] {
        fs::copy(demo.join(file), app.path().join(file)).unwrap();
    }
    fs::write(static_dir.join(".Renviron"), "PRIVATE=value").unwrap();
    let private = app.path().join("www");
    fs::create_dir(&private).unwrap();
    fs::write(private.join(".gitkeep"), "not a placeholder").unwrap();
    let result = cli(app.path(), &["init", "--id", "org.example.rhino"]);
    assert!(result.status.success(), "{result:?}");
    let settings = spec(app.path());
    let files: Vec<_> = settings["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert_eq!(files, ["app.R", "config.yml", "rhino.yml"]);
    let folders: Vec<_> = settings["emptyDirs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert_eq!(folders, ["app/static", "www"]);
}

#[test]
fn init_records_empty_directories_separately_from_files() {
    let app = tempdir().unwrap();
    fs::write(app.path().join("app.R"), "").unwrap();
    fs::write(app.path().join("rhino.yml"), "sass: r").unwrap();
    for dir in ["app/static", "data/private", "www"] {
        fs::create_dir_all(app.path().join(dir)).unwrap();
    }
    fs::write(app.path().join("data/private/.env"), "credentials").unwrap();
    let init = cli(app.path(), &["init", "--id", "org.example.empty"]);
    assert!(init.status.success(), "{init:?}");
    fs::write(app.path().join("www/future.csv"), "user data").unwrap();
    let settings = spec(app.path());
    let strings = |key: &str| {
        settings[key]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect::<Vec<_>>()
    };
    assert_eq!(strings("files"), ["app.R", "rhino.yml"]);
    assert_eq!(strings("emptyDirs"), ["app/static", "data/private", "www"]);
}
