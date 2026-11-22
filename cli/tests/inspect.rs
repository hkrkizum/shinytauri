use std::fs;
use std::path::Path;
use std::process::{Command, Output};
use tempfile::tempdir;

const LOCK: &str = include_str!("../../bundles/minimal/rproj.lock");

#[test]
fn reports_effective_timeout_and_rejects_non_integer_or_out_of_range_values() {
    let root = tempdir().unwrap();
    let spec = root.path().join("shinytauri.toml");
    fs::write(root.path().join("app.R"), "").unwrap();
    fs::write(root.path().join("test.lock"), LOCK).unwrap();
    let base = "app='.'\nfiles=['app.R']\nlock='test.lock'\n";
    for (key, expected) in [("", 90), ("startupTimeoutSeconds=0x78", 120)] {
        fs::write(&spec, format!("{base}{key}")).unwrap();
        let result = inspect(root.path(), &[]);
        assert!(result.status.success(), "{result:?}");
        assert!(
            String::from_utf8_lossy(&result.stdout)
                .lines()
                .any(|line| { line == format!("Startup timeout: {expected} seconds") })
        );
    }
    for invalid in [
        "0",
        "-1",
        "1.0",
        "'2'",
        "true",
        "[1]",
        "2147483648",
        "-4294967295",
        "4294967386",
    ] {
        fs::write(&spec, format!("{base}startupTimeoutSeconds={invalid}")).unwrap();
        let result = inspect(root.path(), &[]);
        assert!(!result.status.success(), "{invalid}");
        assert!(String::from_utf8_lossy(&result.stderr).contains("startupTimeoutSeconds"));
        for command in ["build", "lock"] {
            let result = Command::new(env!("CARGO_BIN_EXE_shinytauri"))
                .env("PATH", "")
                .current_dir(root.path())
                .arg(command)
                .output()
                .unwrap();
            assert!(!result.status.success());
            assert!(
                String::from_utf8_lossy(&result.stderr).contains("startupTimeoutSeconds"),
                "{command}: {result:?}"
            );
        }
    }
}

fn inspect(cwd: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_shinytauri"))
        .env("PATH", "")
        .current_dir(cwd)
        .arg("inspect")
        .args(args)
        .output()
        .unwrap()
}

fn section<'a>(report: &'a str, name: &str) -> Vec<&'a str> {
    report
        .lines()
        .skip_while(|line| *line != name)
        .skip(1)
        .map_while(|line| line.strip_prefix("  "))
        .collect()
}

#[test]
fn expands_the_staged_tree_and_only_suggests_conventional_resources() {
    let root = tempdir().unwrap();
    let app = root.path().join("売上");
    let config = root.path().join("profiles");
    for dir in ["www/site", "www/empty", "data", "R", "private"] {
        fs::create_dir_all(app.join(dir)).unwrap();
    }
    for file in [
        "app.R",
        "www/site/a.css",
        "www/.explicit",
        "data/new.csv",
        "R/new.R",
        ".Renviron",
        "private/secret",
        "data/.password",
    ] {
        fs::write(app.join(file), "").unwrap();
    }
    fs::create_dir(&config).unwrap();
    fs::write(config.join("app.lock"), LOCK).unwrap();
    fs::write(config.join("sales.toml"),
        "app='../売上'\nlock='app.lock'\nfiles=['app.R','www','www/site/a.css']\nemptyDirs=['data','www','www/site']").unwrap();
    let result = inspect(root.path(), &["--config", "profiles/sales.toml"]);
    assert!(result.status.success(), "{result:?}");
    let report = String::from_utf8(result.stdout).unwrap();
    assert_eq!(
        section(&report, "Included files:"),
        ["app.R", "www/.explicit", "www/site/a.css"]
    );
    assert_eq!(
        section(&report, "Empty directories:"),
        ["data", "www/empty"]
    );
    assert_eq!(
        section(&report, "Not included (init candidates):"),
        ["R/new.R", "data/new.csv"]
    );
    assert!(report.contains("Windows R: 4.6.2"));
    assert!(report.contains(&format!(
        "Main packages: {}",
        LOCK.matches("groups = [\"main\"]").count()
    )));
    assert!(
        !report.contains("private")
            && !report.contains("password")
            && !report.contains(".Renviron")
    );
    assert!(!root.path().join("dist").exists() && !app.join(".shinytauri").exists());
}

#[test]
fn missing_entries_and_lock_are_reported_together() {
    let root = tempdir().unwrap();
    let app = root.path().join("app");
    fs::create_dir(&app).unwrap();
    fs::write(app.join("app.R"), "").unwrap();
    fs::write(
        app.join("shinytauri.toml"),
        "app='.'\nlock='rproj.lock'\nfiles=['app.R','first.txt']\nemptyDirs=['last']",
    )
    .unwrap();
    let result = inspect(root.path(), &["app"]);
    assert!(!result.status.success());
    let report = String::from_utf8(result.stdout).unwrap();
    assert_eq!(section(&report, "Missing entries:"), ["first.txt", "last"]);
    assert!(
        String::from_utf8(result.stderr)
            .unwrap()
            .contains("rproj.lock")
    );
    assert!(!app.join("dist").exists());
}

#[test]
fn rejects_invalid_metadata_and_ineligible_paths() {
    let root = tempdir().unwrap();
    fs::write(root.path().join("app.R"), "").unwrap();
    fs::write(root.path().join("rproj.lock"), LOCK).unwrap();
    for (files, extra, expected) in [
        ("['../other']", "", "files"),
        ("['app.R']", "quarto='yes'", "quarto"),
        ("['app.R']", "version='alpha'", "version"),
        ("['app.R']", "productName='NUL.txt'", "productName"),
        ("['app.R']", "emptyDirs=['app.R']", "directory"),
        ("['app.R']", "rVersion='4.0.1'", "4.0.1"),
        ("['app.R']", "packages=['shiny']", "windows.lock"),
    ] {
        fs::write(
            root.path().join("shinytauri.toml"),
            format!("app='.'\nlock='rproj.lock'\nfiles={files}\n{extra}"),
        )
        .unwrap();
        let result = inspect(root.path(), &[]);
        assert!(!result.status.success(), "{result:?}");
        assert!(
            String::from_utf8_lossy(&result.stderr).contains(expected),
            "{result:?}"
        );
    }
}

#[cfg(unix)]
#[test]
fn rejects_link_ancestors_without_reading_other_projects() {
    let root = tempdir().unwrap();
    fs::create_dir(root.path().join("elsewhere")).unwrap();
    fs::write(root.path().join("elsewhere/confidential"), "").unwrap();
    std::os::unix::fs::symlink(root.path().join("elsewhere"), root.path().join("alias")).unwrap();
    fs::write(root.path().join("app.R"), "").unwrap();
    fs::write(root.path().join("rproj.lock"), LOCK).unwrap();
    fs::write(
        root.path().join("shinytauri.toml"),
        "app='.'\nlock='rproj.lock'\nfiles=['app.R','alias/confidential']",
    )
    .unwrap();
    let result = inspect(root.path(), &[]);
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("link"));
    assert!(!String::from_utf8_lossy(&result.stdout).contains("confidential"));
}

#[cfg(unix)]
#[test]
fn a_link_to_a_config_uses_the_target_configs_directory() {
    let root = tempdir().unwrap();
    let app = root.path().join("project");
    fs::create_dir(&app).unwrap();
    fs::write(app.join("app.R"), "").unwrap();
    fs::write(app.join("rproj.lock"), LOCK).unwrap();
    fs::write(
        app.join("shinytauri.toml"),
        "app='.'\nlock='rproj.lock'\nfiles='app.R'",
    )
    .unwrap();
    std::os::unix::fs::symlink(app.join("shinytauri.toml"), root.path().join("alias.toml"))
        .unwrap();
    let output = inspect(root.path(), &["--config", "alias.toml"]);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        section(
            &String::from_utf8(output.stdout).unwrap(),
            "Included files:"
        ),
        ["app.R"]
    );
}

#[cfg(target_os = "linux")]
#[test]
fn checks_name_collisions_only_in_the_selected_tree() {
    let root = tempdir().unwrap();
    let app = root.path();
    fs::create_dir(app.join("WWW")).unwrap();
    for name in ["SERVER.r", "server.R", "WWW/INDEX.HTML", "WWW/index.html"] {
        fs::write(app.join(name), "").unwrap();
    }
    fs::write(app.join("rproj.lock"), LOCK).unwrap();
    let config = app.join("shinytauri.toml");
    let set_files = |selection: &str| {
        fs::write(
            &config,
            format!("app='.'\nlock='rproj.lock'\nfiles={selection}"),
        )
        .unwrap();
    };
    set_files("['SERVER.r', 'WWW/INDEX.HTML']");
    let result = inspect(app, &[]);
    assert!(result.status.success(), "{result:?}");
    let report = String::from_utf8(result.stdout).unwrap();
    assert_eq!(
        section(&report, "Not included (init candidates):"),
        ["WWW/index.html", "server.R"]
    );
    set_files("['SERVER.r', 'WWW']");
    let result = inspect(app, &[]);
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("collide"));
}

#[test]
fn reports_explicit_installer_inputs_relative_to_the_config() {
    let root = tempdir().unwrap();
    let config_dir = root.path().join("packaging");
    let assets = root.path().join("branding");
    fs::create_dir(&config_dir).unwrap();
    fs::create_dir(&assets).unwrap();
    let icon = assets.join("app.ico");
    let license = assets.join("利用条件.txt");
    fs::write(&icon, include_bytes!("../../src-tauri/icons/icon.ico")).unwrap();
    fs::write(&license, "取扱規約の本文は表示しない").unwrap();
    fs::write(root.path().join("app.R"), "").unwrap();
    fs::write(config_dir.join("runtime.lock"), LOCK).unwrap();
    let config = config_dir.join("spec.toml");
    let base = "app='..'\nlock='runtime.lock'\nfiles=['app.R']";
    let args = ["--config", "packaging/spec.toml"];
    fs::write(&config, base).unwrap();
    let plain = inspect(root.path(), &args);
    assert!(plain.status.success());
    let plain_text = String::from_utf8(plain.stdout).unwrap();
    for label in ["Publisher:", "Installer icon:", "Installer license:"] {
        assert!(!plain_text.contains(label));
    }

    fs::write(&config, format!("{base}\npublisher='集計部門'\nicon='../branding/app.ico'\nlicenseFile='../branding/利用条件.txt'")).unwrap();
    let branded = inspect(root.path(), &args);
    assert!(branded.status.success(), "{branded:?}");
    let report = String::from_utf8(branded.stdout).unwrap();
    assert_eq!(section(&report, "Included files:"), ["app.R"]);
    assert!(report.contains("Publisher: 集計部門"));
    assert!(report.contains(&format!(
        "Installer icon: {}",
        icon.canonicalize().unwrap().display()
    )));
    assert!(report.contains(&format!(
        "Installer license: {}",
        license.canonicalize().unwrap().display()
    )));
    assert!(!report.contains("取扱規約の本文"));

    for (extra, key) in [
        ("icon='lost.ico'", "icon"),
        ("licenseFile='lost.txt'", "licenseFile"),
        ("publisher=' '", "publisher"),
        ("icon='../branding'", "icon"),
    ] {
        fs::write(&config, format!("{base}\n{extra}")).unwrap();
        let invalid = inspect(root.path(), &args);
        assert!(!invalid.status.success(), "{invalid:?}");
        assert!(
            String::from_utf8_lossy(&invalid.stderr).contains(key),
            "{invalid:?}"
        );
    }
}

#[test]
fn reports_source_bundles_relative_to_the_config_before_building() {
    let root = tempdir().unwrap();
    let profiles = root.path().join("profiles");
    let materials = root.path().join("配布用ソース");
    fs::create_dir(&profiles).unwrap();
    fs::create_dir(&materials).unwrap();
    fs::write(materials.join("build.txt"), "build instructions").unwrap();
    fs::write(root.path().join("sources.tar.gz"), "example archive").unwrap();
    fs::write(root.path().join("app.R"), "").unwrap();
    fs::write(profiles.join("runtime.lock"), LOCK).unwrap();
    let args = ["--config", "profiles/spec.toml"];
    let write_config = |source: &str| {
        fs::write(
            profiles.join("spec.toml"),
            format!("app='..'\nlock='runtime.lock'\nfiles=['app.R']\nsourceBundle='../{source}'"),
        )
        .unwrap();
    };

    for source in ["sources.tar.gz", "配布用ソース"] {
        write_config(source);
        let output = inspect(root.path(), &args);
        assert!(output.status.success(), "{output:?}");
        let canonical = root.path().join(source).canonicalize().unwrap();
        let report = String::from_utf8(output.stdout).unwrap();
        assert!(report.contains(&format!("Source bundle: {}", canonical.display())));
        assert_eq!(section(&report, "Included files:"), ["app.R"]);
    }
    write_config("missing.tar.gz");
    let output = inspect(root.path(), &args);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("sourceBundle"));
}

#[cfg(target_os = "linux")]
#[test]
fn checks_all_selected_windows_paths_and_ancestors() {
    let root = tempdir().unwrap();
    let app = root.path();
    for name in ["www/Chart", "www/chart", "www/chart.", "www/empty"] {
        fs::create_dir_all(app.join(name)).unwrap();
    }
    for name in [
        "app.R",
        "www/Chart/é.svg",
        "www/Chart/É.svg",
        "www/chart/other",
        "www/EMPTY",
        "www/straße.txt",
        "www/strasse.txt",
        "www/é",
        "www/e\u{301}",
    ] {
        fs::write(app.join(name), "").unwrap();
    }
    fs::write(app.join("rproj.lock"), LOCK).unwrap();
    let check = |files: &str, extra: &str| {
        fs::write(
            app.join("shinytauri.toml"),
            format!("app='.'\nlock='rproj.lock'\nfiles={files}\n{extra}"),
        )
        .unwrap();
        inspect(app, &[])
    };
    for (files, extra, first, second) in [
        (
            "['app.R','www/Chart']",
            "",
            "www/Chart/é.svg",
            "www/Chart/É.svg",
        ),
        (
            "['app.R','www/Chart/é.svg','www/chart/other']",
            "",
            "www/Chart",
            "www/chart",
        ),
        (
            "['app.R','www/EMPTY']",
            "emptyDirs=['www/empty']",
            "www/EMPTY",
            "www/empty",
        ),
    ] {
        let result = check(files, extra);
        let message = String::from_utf8_lossy(&result.stderr);
        assert!(
            !result.status.success() && message.contains(first) && message.contains(second),
            "{result:?}"
        );
    }
    let invalid = check("['app.R','www/chart.']", "");
    assert!(
        !invalid.status.success() && String::from_utf8_lossy(&invalid.stderr).contains("Windows")
    );
    let result = check(
        "['app.R','www/straße.txt','www/strasse.txt','www/é','www/e\u{301}','www/Chart/é.svg']",
        "emptyDirs=['www/Chart']",
    );
    assert!(result.status.success(), "{result:?}");
}

#[test]
fn missing_literal_sources_report_the_call_site_and_include_remedy_offline() {
    let fixture = tempdir().unwrap();
    let root = fixture.path();
    fs::write(
        root.join("app.R"),
        "base::source(file = 'helpers.R', local = TRUE)\n",
    )
    .unwrap();
    fs::write(
        root.join("helpers.R"),
        "stop('app code must never be evaluated')\n",
    )
    .unwrap();
    fs::write(root.join("rproj.lock"), LOCK).unwrap();
    fs::write(
        root.join("shinytauri.toml"),
        "app='.'\nlock='rproj.lock'\nfiles=['app.R']",
    )
    .unwrap();
    let result = inspect(root, &[]);
    assert!(!result.status.success(), "{result:?}");
    let error = String::from_utf8(result.stderr).unwrap();
    for expected in ["app.R:1", "helpers.R", "files"] {
        assert!(error.contains(expected), "{error}");
    }
    fs::write(
        root.join("shinytauri.toml"),
        "app='.'\nlock='rproj.lock'\nfiles=['app.R', 'helpers.R']",
    )
    .unwrap();
    assert!(inspect(root, &[]).status.success());
}

#[test]
fn child_working_directory_and_source_bindings_affect_the_calling_script() {
    for change in [
        "setwd('data')",
        "source <- function(...) NULL",
        "if (ready) source <- function(...) NULL",
    ] {
        let fixture = tempdir().unwrap();
        let root = fixture.path();
        fs::write(root.join("app.R"), "source('setup.R')\nsource('another.R')").unwrap();
        fs::write(root.join("setup.R"), change).unwrap();
        fs::write(root.join("another.R"), "").unwrap();
        fs::write(root.join("rproj.lock"), LOCK).unwrap();
        fs::write(
            root.join("shinytauri.toml"),
            "app='.'\nlock='rproj.lock'\nfiles=['app.R','setup.R']",
        )
        .unwrap();
        let result = inspect(root, &[]);
        assert!(result.status.success(), "{result:?}");
    }
}

#[test]
fn a_dynamic_encoding_does_not_hide_a_known_source_path() {
    let fixture = tempdir().unwrap();
    let root = fixture.path();
    fs::write(
        root.join("app.R"),
        "source('helper.R', encoding = preferred)",
    )
    .unwrap();
    fs::write(root.join("helper.R"), "").unwrap();
    fs::write(root.join("rproj.lock"), LOCK).unwrap();
    fs::write(
        root.join("shinytauri.toml"),
        "app='.'\nlock='rproj.lock'\nfiles=['app.R']",
    )
    .unwrap();
    let result = inspect(root, &[]);
    assert!(
        !result.status.success() && String::from_utf8_lossy(&result.stderr).contains("helper.R"),
        "{result:?}"
    );
}

#[test]
fn follows_chdir_for_each_call_context_but_ignores_deferred_code() {
    let root = tempdir().unwrap();
    let app = root.path();
    fs::create_dir(app.join("lib")).unwrap();
    fs::write(
        app.join("app.R"),
        r#"# source("decoy.R")
quoted <- r"(source('decoy.R'))"
quote(source("decoy.R"))
if (FALSE) source("decoy.R")
on_event <- function() source("decoy.R")
source(ch = FALSE, "lib/load.R")
{ base :: source(fil = "lib/load.R", chdir = TRUE) }
"#,
    )
    .unwrap();
    fs::write(app.join("lib/load.R"), "source('./child.R')").unwrap();
    for p in ["child.R", "lib/child.R", "decoy.R"] {
        fs::write(app.join(p), "").unwrap();
    }
    fs::write(
        app.join("shinytauri.toml"),
        "app='.'\nlock='rproj.lock'\nfiles=['app.R','lib/load.R','child.R']",
    )
    .unwrap();
    fs::write(app.join("rproj.lock"), LOCK).unwrap();
    let result = inspect(app, &[]);
    let error = String::from_utf8(result.stderr).unwrap();
    assert!(!result.status.success(), "{error}");
    assert!(
        error.contains("lib/load.R:1") && error.contains("lib/child.R") && !error.contains("decoy"),
        "{error}"
    );
}

#[test]
fn checks_shiny_autoload_and_split_entry_points_and_warns_on_absent_originals() {
    let root = tempdir().unwrap();
    let app = root.path();
    fs::create_dir(app.join("R")).unwrap();
    fs::write(app.join("app.R"), "source('never.R')").unwrap();
    fs::write(app.join("server.R"), "").unwrap();
    fs::write(app.join("global.r"), "source('global-helper.R')").unwrap();
    fs::write(app.join("ui.R"), "source('created.R')").unwrap();
    fs::write(app.join("R/a.R"), "source('auto-helper.R')").unwrap();
    for p in ["global-helper.R", "auto-helper.R", "never.R"] {
        fs::write(app.join(p), "").unwrap();
    }
    fs::write(app.join("rproj.lock"), LOCK).unwrap();
    fs::write(
        app.join("shinytauri.toml"),
        "app='.'\nlock='rproj.lock'\nfiles=['server.R','ui.R','global.r','app.R','R']",
    )
    .unwrap();
    let result = inspect(app, &[]);
    let error = String::from_utf8(result.stderr).unwrap();
    assert!(!result.status.success());
    for expected in [
        "global.r:1",
        "global-helper.R",
        "R/a.R:1",
        "auto-helper.R",
        "Warning",
        "created.R",
    ] {
        assert!(error.contains(expected), "{error}");
    }
    assert!(!error.contains("never.R"));
    fs::write(app.join("R/_disable_autoload.R"), "").unwrap();
    fs::write(app.join("global.r"), "").unwrap();
    let result = inspect(app, &[]);
    assert!(
        result.status.success() && String::from_utf8_lossy(&result.stderr).contains("created.R"),
        "{result:?}"
    );
}
