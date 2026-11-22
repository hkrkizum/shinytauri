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
pub(crate) struct Resources {
    pub(crate) files: Vec<String>,
    pub(crate) empty_dirs: Vec<String>,
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

pub(crate) fn resources(app: &Path) -> Result<Resources> {
    let mut resources = Resources::default();
    // Read actual directory entries: looking up "app.R" loses the spelling on Windows.
    for child in fs::read_dir(app)? {
        let child = child?;
        let name = child.file_name();
        let Some(name) = name.to_str() else { continue };
        match name.to_ascii_lowercase().as_str() {
            "app.r" | "ui.r" | "server.r" | "global.r" if child.file_type()?.is_file() => {
                resources.files.push(name.to_owned());
            }
            "www" if child.file_type()?.is_dir() => collect_tree(app, name, &mut resources)?,
            _ => {}
        }
    }
    let rhino = entry(&app.join("rhino.yml"))?.is_some_and(|kind| kind.is_file());
    if rhino {
        resources.files.push("rhino.yml".into());
        if entry(&app.join("config.yml"))?.is_some_and(|kind| kind.is_file()) {
            resources.files.push("config.yml".into());
        }
    }
    for folder in ["R", "data", "app"] {
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

pub(crate) fn read_lock(path: &Path) -> Result<Table> {
    let text = fs::read_to_string(path).map_err(|err| format!("{}: {err}", path.display()))?;
    text.parse().map_err(|err: toml::de::Error| {
        format!("Invalid Windows lock {}: {}", path.display(), err.message()).into()
    })
}

pub(crate) fn windows_target<'a>(
    lock: &'a Table,
    path: &Path,
    version: Option<&str>,
) -> Result<&'a Value> {
    let message = || {
        format!(
            "{} needs a Windows x64 Shiny runtime lock. Declare shiny in [dependencies], set a Windows rig target, then run rig proj lock.",
            path.display()
        )
    };
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
        return Err(format!("{} has multiple matching Windows x64 targets. Set rVersion in the bundle config (or use --r-version with init).", path.display()).into());
    }
    if main_packages(target)
        .any(|package| package.get("package").and_then(Value::as_str) == Some("shiny"))
    {
        return Ok(target);
    }
    Err(message().into())
}

pub(crate) fn main_packages(target: &Value) -> impl Iterator<Item = &Value> {
    target
        .get("packages")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|p| {
            p.get("groups")
                .and_then(Value::as_array)
                .is_some_and(|g| g.iter().any(|v| v.as_str() == Some("main")))
        })
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
    crate::windows_names::check(
        resources
            .files
            .iter()
            .chain(&resources.empty_dirs)
            .map(String::as_str),
    )?;
    if !resources
        .files
        .iter()
        .any(|name| name.eq_ignore_ascii_case("app.r") || name.eq_ignore_ascii_case("server.r"))
    {
        return Err(format!(
            "{} needs a Shiny entry point: app.R or server.R (with ui.R or www/index.html for the UI).",
            app.display()
        ).into());
    }
    let name = app
        .file_name()
        .and_then(|f| f.to_str())
        .ok_or("App directory needs a UTF-8 name.")?;
    let root_lock = app.join("rproj.lock");
    let has_lock = entry(&root_lock)?.is_some();
    let has_manifest = entry(&app.join("rproj.toml"))?.is_some();
    if has_lock {
        windows_target(&read_lock(&root_lock)?, &root_lock, r_version.as_deref())?;
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
