use anyhow::{Context, Result, ensure};
use clap::ValueEnum;
use std::{
    env::consts::EXE_SUFFIX,
    ffi::OsStr,
    fs,
    path::{Path, PathBuf},
    process::Command,
};

#[derive(Clone, Copy, ValueEnum)]
pub(crate) enum Fixture {
    Minimal,
    Rhino,
}

fn copy_tree(source: &Path, destination: &Path) -> Result<()> {
    fs::create_dir(destination)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        // A local Rhino checkout can contain rig's generated R environment.
        if matches!(
            entry.file_name().to_str(),
            Some(".rvenv" | ".rvenvlib" | ".Renviron")
        ) {
            continue;
        }
        let target = destination.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else {
            fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

fn invoke(cli: &Path, caller: &Path, args: &[&OsStr]) -> Result<()> {
    let status = Command::new(cli).current_dir(caller).args(args).status()?;
    ensure!(status.success(), "shinytauri {args:?} exited with {status}");
    Ok(())
}

fn edit_profile(app: &Path, file: &str, fields: &[(&str, toml::Value)]) -> Result<()> {
    let mut settings: toml::Table = fs::read_to_string(app.join("shinytauri.toml"))?.parse()?;
    for &(key, ref value) in fields {
        settings.insert(key.into(), value.clone());
    }
    fs::write(app.join(file), toml::to_string(&settings)?)?;
    Ok(())
}

fn artifact(app: &Path, suffix: &str) -> Result<PathBuf> {
    let mut matches = Vec::new();
    for entry in fs::read_dir(app.join("dist"))? {
        let entry = entry?;
        if entry.file_type()?.is_file() && entry.file_name().to_string_lossy().ends_with(suffix) {
            matches.push(entry.path());
        }
    }
    ensure!(
        matches.len() == 1,
        "Expected one {suffix} in {}, found {matches:?}",
        app.display()
    );
    Ok(matches.pop().unwrap())
}

fn collect_setup(app: &Path, destination: &Path) -> Result<()> {
    fs::copy(artifact(app, "-setup.exe")?, destination)?;
    Ok(())
}

pub(crate) fn run(fixture: Fixture, cli: &Path, repo: &Path, temp: &Path) -> Result<()> {
    let temp = std::path::absolute(temp).context("resolve runner temporary directory")?;
    let caller = temp.join("利用者の 作業");
    fs::create_dir(&caller)?;
    // Exercise the distributed binary outside its checkout, as a user would.
    let external_cli = temp.join(format!("shinytauri{EXE_SUFFIX}"));
    fs::copy(cli, &external_cli)?;
    let cli = external_cli.as_path();
    invoke(cli, &caller, &["doctor".as_ref()])?;
    let fixtures = repo.join("scripts/tests/fixtures");
    match fixture {
        Fixture::Minimal => {
            let app = caller.join("shinytauri-smoke-minimal");
            copy_tree(&fixtures.join("minimal"), &app)?;
            copy_tree(&repo.join("examples/minimal/www"), &app.join("www"))?;
            let init = |project: &Path| {
                invoke(
                    cli,
                    &caller,
                    &[
                        "init".as_ref(),
                        project.as_os_str(),
                        "--id".as_ref(),
                        "org.example.minimal".as_ref(),
                        "--r-version".as_ref(),
                        "4.5.3".as_ref(),
                    ],
                )
            };
            init(&app)?;
            invoke(cli, &caller, &["lock".as_ref(), app.as_os_str()])?;
            invoke(cli, &caller, &["build".as_ref(), app.as_os_str()])?;
            collect_setup(&app, &temp.join("minimal-setup.exe"))?;

            let next_parent = temp.join("新版");
            fs::create_dir(&next_parent)?;
            let next = next_parent.join("shinytauri-smoke-minimal");
            copy_tree(&fixtures.join("upgrade"), &next)?;
            fs::copy(app.join("DESCRIPTION"), next.join("DESCRIPTION"))?;
            init(&next)?;
            // Both fixtures have the same R version and package requirements.
            fs::create_dir_all(next.join(".shinytauri"))?;
            fs::copy(
                app.join(".shinytauri/windows.lock"),
                next.join(".shinytauri/windows.lock"),
            )?;
            edit_profile(&next, "shinytauri.toml", &[("version", "0.2.0".into())])?;
            invoke(cli, &caller, &["build".as_ref(), next.as_os_str()])?;
            collect_setup(&next, &temp.join("minimal-upgrade-setup.exe"))
        }
        Fixture::Rhino => {
            let app = caller.join("shinytauri-smoke-rhino");
            copy_tree(&repo.join("src-shiny"), &app)?;
            copy_tree(&fixtures.join("source-bundle"), &caller.join("配布資料"))?;
            copy_tree(&fixtures.join("branding"), &app.join("配布 素材"))?;
            invoke(
                cli,
                &caller,
                &[
                    "init".as_ref(),
                    app.as_os_str(),
                    "--id".as_ref(),
                    "org.example.rhino".as_ref(),
                ],
            )?;
            edit_profile(
                &app,
                "配布.toml",
                &[
                    ("version", "1.2.3".into()),
                    ("title", "Rhino Smoke".into()),
                    ("quarto", true.into()),
                    ("sourceBundle", "../配布資料".into()),
                    ("publisher", "販売 株式会社".into()),
                    ("icon", "配布 素材/製品.ico".into()),
                    ("licenseFile", "配布 素材/利用規約.txt".into()),
                ],
            )?;
            invoke(
                cli,
                &caller,
                &[
                    "build".as_ref(),
                    "--config".as_ref(),
                    app.join("配布.toml").as_os_str(),
                    "--out-dir".as_ref(),
                    app.join("dist").as_os_str(),
                ],
            )?;
            collect_setup(&app, &temp.join("rhino-setup.exe"))?;
            // Preserve the content-addressed basename referenced by the installed manifest.
            let sidecar = artifact(&app, ".tar.gz")?;
            fs::copy(&sidecar, temp.join(sidecar.file_name().unwrap()))?;
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn overrides_keep_init_file_list_and_types() -> Result<()> {
        let dir = tempfile::tempdir()?;
        fs::write(
            dir.path().join("shinytauri.toml"),
            "files = ['app.R', 'app/static']\nversion = '0.1.0'",
        )?;
        edit_profile(
            dir.path(),
            "配布.toml",
            &[("quarto", true.into()), ("version", "1.2.3".into())],
        )?;
        let result: toml::Table = fs::read_to_string(dir.path().join("配布.toml"))?.parse()?;
        assert_eq!(
            result["files"].as_array().unwrap()[1].as_str(),
            Some("app/static")
        );
        assert_eq!(result["quarto"].as_bool(), Some(true));
        assert_eq!(result["version"].as_str(), Some("1.2.3"));
        Ok(())
    }
}
