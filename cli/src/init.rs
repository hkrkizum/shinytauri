use crate::Result;
use std::{fs, io::Write, path::Path};
use toml::{Table, Value};

const PROFILE: &str = "shinytauri.toml";
const MINIMAL_MANIFEST: &str = include_str!("../../bundles/minimal/rproj.toml");
const MINIMAL_LOCK: &str = include_str!("../../bundles/minimal/rproj.lock");

fn entry(path: &Path) -> Result<Option<fs::FileType>> {
    match fs::symlink_metadata(path) {
        Ok(meta) => Ok(Some(meta.file_type())),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(err.into()),
    }
}

fn check_identifier(id: &str) -> Result<()> {
    if !id.contains('.')
        || id.split('.').any(|part| {
            part.is_empty()
                || !part.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
                || part.starts_with('-')
                || part.ends_with('-')
        })
    {
        return Err("Specify a reverse-domain --id, such as org.example.dashboard.".into());
    }
    Ok(())
}

#[derive(Default)]
struct Resources {
    files: Vec<String>,
    empty_dirs: Vec<String>,
}

fn collect_tree(app: &Path, folder: &str, resources: &mut Resources) -> Result<()> {
    let mut empty = true;
    for child in fs::read_dir(app.join(folder))? {
        let child = child?;
        let name = child.file_name();
        let name = name.to_str().ok_or("App resources must have UTF-8 names")?;
        if name.starts_with('.') || matches!(name, "node_modules" | "renv" | "packrat") {
            continue;
        }
        let kind = child.file_type()?;
        let relative = format!("{folder}/{name}");
        if kind.is_dir() {
            collect_tree(app, &relative, resources)?;
            empty = false;
        } else if kind.is_file() {
            resources.files.push(relative);
            empty = false;
        }
    }
    if empty {
        resources.empty_dirs.push(folder.to_owned());
    }
    Ok(())
}

fn resources(app: &Path) -> Result<Resources> {
    let mut resources = Resources::default();
    let files = &mut resources.files;
    for name in ["app.R", "ui.R", "server.R", "global.R", "rhino.yml"] {
        if entry(&app.join(name))?.is_some_and(|kind| kind.is_file()) {
            files.push(name.to_owned());
        }
    }
    let has = |name: &str| files.iter().any(|f| f == name);
    if !(has("app.R") || has("ui.R") && has("server.R")) {
        return Err(format!("{} needs app.R or both ui.R and server.R.", app.display()).into());
    }
    let rhino = has("rhino.yml");
    if rhino && entry(&app.join("config.yml"))?.is_some_and(|kind| kind.is_file()) {
        files.push("config.yml".to_owned());
    }
    for folder in ["R", "www", "data", "app"] {
        if folder == "app" && !rhino {
            continue;
        }
        let path = app.join(folder);
        if entry(&path)?.is_some_and(|kind| kind.is_dir()) {
            collect_tree(app, folder, &mut resources)?;
        }
    }
    resources.files.sort();
    resources.empty_dirs.sort();
    Ok(resources)
}

fn validate_lock(path: &Path, version: Option<&str>) -> Result<()> {
    let message = || {
        format!(
            "{} needs a Windows x64 Shiny runtime lock. Declare shiny in [dependencies], set a Windows rig target, then run rig proj lock.",
            path.display()
        )
    };
    let lock: Table = fs::read_to_string(path)?
        .parse()
        .map_err(|err| format!("{} ({err})", message()))?;
    let mut targets = lock
        .get("targets")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|target| {
            target.get("platform").and_then(Value::as_str) == Some("x86_64-w64-mingw32")
                && version
                    .is_none_or(|v| target.get("r_version").and_then(Value::as_str) == Some(v))
        });
    let target = targets.next().ok_or_else(|| match version {
        Some(v) => format!("{} has no Windows x64 target for R {v}.", path.display()),
        None => message(),
    })?;
    if targets.next().is_some() {
        return Err(format!("{} has multiple matching Windows x64 targets. Select a unique target with --r-version.", path.display()).into());
    }
    let packages = target.get("packages").and_then(Value::as_array);
    if packages.into_iter().flatten().any(|package| {
        package.get("package").and_then(Value::as_str) == Some("shiny")
            && package
                .get("groups")
                .and_then(Value::as_array)
                .is_some_and(|groups| groups.iter().any(|g| g.as_str() == Some("main")))
    }) {
        return Ok(());
    }
    Err(message().into())
}

fn write_new(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut pending = tempfile::NamedTempFile::new_in(path.parent().unwrap())?;
    pending.write_all(bytes)?;
    pending
        .persist_noclobber(path)
        .map_err(|err| format!("Cannot create {}: {}", path.display(), err.error))?;
    Ok(())
}

pub(crate) fn initialize(
    app: &Path,
    id: &str,
    packages: Vec<String>,
    r_version: Option<String>,
) -> Result<()> {
    check_identifier(id)?;
    let app = fs::canonicalize(app).map_err(|err| format!("{}: {err}", app.display()))?;
    let config = app.join(PROFILE);
    if entry(&config)?.is_some() {
        return Err(format!("{} already exists.", config.display()).into());
    }
    let resources = resources(&app)?;
    let name = app
        .file_name()
        .and_then(|f| f.to_str())
        .ok_or("App directory needs a UTF-8 name.")?;
    let root_lock = app.join("rproj.lock");
    let has_lock = entry(&root_lock)?.is_some();
    let has_manifest = entry(&app.join("rproj.toml"))?.is_some();
    if has_lock {
        validate_lock(&root_lock, r_version.as_deref())?;
    }
    if (has_lock || has_manifest) && !packages.is_empty() {
        return Err("The app already has rig metadata. Declare additional packages there.".into());
    }
    let mut dependency_file = None;
    if !has_lock && !has_manifest {
        for name in ["renv.lock", "DESCRIPTION"] {
            if entry(&app.join(name))?.is_some() {
                dependency_file = Some(name);
                break;
            }
        }
    }
    let shiny_only = !has_lock && !has_manifest && dependency_file.is_none() && packages.is_empty();
    let use_minimal = shiny_only && r_version.is_none();
    let managed = !has_lock && !has_manifest && !use_minimal;
    let lock = if managed {
        ".shinytauri/windows.lock"
    } else if use_minimal {
        ".shinytauri/rproj.lock"
    } else {
        "rproj.lock"
    };
    let mut table = Table::new();
    if let Some(version) = r_version {
        table.insert("rVersion".into(), version.into());
    }
    if let Some(source) = dependency_file {
        table.insert("dependencyFile".into(), source.into());
    }
    if !packages.is_empty() {
        table.insert("packages".into(), packages.into());
    }
    if shiny_only {
        table.insert("checkMinimal".into(), true.into());
    }
    for (key, value) in [
        ("app", "."),
        ("lock", lock),
        ("productName", name),
        ("title", name),
        ("version", "0.1.0"),
        ("identifier", id),
    ] {
        table.insert(key.into(), value.into());
    }
    table.insert("files".into(), resources.files.into());
    if !resources.empty_dirs.is_empty() {
        table.insert("emptyDirs".into(), resources.empty_dirs.into());
    }
    let document = toml::to_string_pretty(&table)?;
    let support = app.join(".shinytauri");
    if use_minimal {
        // Reserve this namespace for this init. Never merge with existing user files.
        fs::create_dir(&support)
            .map_err(|err| format!("Cannot create {}: {err}", support.display()))?;
        let mut created = Vec::new();
        let result = (|| -> Result<()> {
            for (file, data) in [
                ("rproj.toml", MINIMAL_MANIFEST),
                ("rproj.lock", MINIMAL_LOCK),
            ] {
                let path = support.join(file);
                write_new(&path, data.as_bytes())?;
                created.push(path);
            }
            write_new(&config, document.as_bytes())
        })();
        if result.is_err() {
            for path in created {
                let _ = fs::remove_file(path);
            }
            let _ = fs::remove_dir(&support);
        }
        result?;
        println!(
            "The bundled lock contains shiny and its dependencies. For other dependencies, set packages in shinytauri.toml, set lock to .shinytauri/windows.lock, remove checkMinimal, then run shinytauri lock."
        );
    } else {
        write_new(&config, document.as_bytes())?;
        if managed {
            println!(
                "Run shinytauri lock with this app directory to resolve its Windows runtime packages. Commit the resulting .shinytauri/windows.lock with your app."
            );
        } else if !has_lock {
            println!(
                "Before building, configure your Windows target and run rig proj lock beside rproj.toml."
            );
        }
    }
    println!(
        "Created {}. Review the files list and runtime dependencies before building.",
        config.display()
    );
    Ok(())
}
