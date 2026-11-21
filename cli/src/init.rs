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

fn collect_tree(app: &Path, folder: &Path, files: &mut Vec<String>) -> Result<()> {
    for child in fs::read_dir(folder)? {
        let child = child?;
        let name = child.file_name();
        let name = name.to_str().ok_or("App resources must have UTF-8 names")?;
        if name.starts_with('.') || matches!(name, "node_modules" | "renv" | "packrat") {
            continue;
        }
        let kind = child.file_type()?;
        let file = child.path();
        if kind.is_dir() {
            collect_tree(app, &file, files)?;
        } else if kind.is_file() {
            let relative = file.strip_prefix(app)?;
            let components: Vec<_> = relative.iter().map(|part| part.to_str().unwrap()).collect();
            files.push(components.join("/"));
        }
    }
    Ok(())
}

fn resources(app: &Path) -> Result<Vec<String>> {
    let mut files = Vec::new();
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
            collect_tree(app, &path, &mut files)?;
        }
    }
    files.sort();
    Ok(files)
}

fn validate_lock(path: &Path) -> Result<()> {
    let message = || {
        format!(
            "{} needs a Windows x64 Shiny runtime lock. Declare shiny in [dependencies], set a Windows rig target, then run rig proj lock.",
            path.display()
        )
    };
    let lock: Table = fs::read_to_string(path)?
        .parse()
        .map_err(|err| format!("{} ({err})", message()))?;
    let targets = lock.get("targets").and_then(Value::as_array);
    for target in targets.into_iter().flatten() {
        if target.get("platform").and_then(Value::as_str) != Some("x86_64-w64-mingw32") {
            continue;
        }
        let packages = target.get("packages").and_then(Value::as_array);
        for package in packages.into_iter().flatten() {
            let groups = package.get("groups").and_then(Value::as_array);
            if package.get("package").and_then(Value::as_str) == Some("shiny")
                && groups
                    .into_iter()
                    .flatten()
                    .any(|g| g.as_str() == Some("main"))
            {
                return Ok(());
            }
        }
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

pub(crate) fn initialize(app: &Path, id: &str) -> Result<()> {
    check_identifier(id)?;
    let app = fs::canonicalize(app).map_err(|err| format!("{}: {err}", app.display()))?;
    let config = app.join(PROFILE);
    if entry(&config)?.is_some() {
        return Err(format!("{} already exists.", config.display()).into());
    }
    let files = resources(&app)?;
    let name = app
        .file_name()
        .and_then(|f| f.to_str())
        .ok_or("App directory needs a UTF-8 name.")?;
    let root_lock = app.join("rproj.lock");
    let has_lock = entry(&root_lock)?.is_some();
    let has_manifest = entry(&app.join("rproj.toml"))?.is_some();
    if has_lock {
        validate_lock(&root_lock)?;
    }
    let use_minimal = !has_lock && !has_manifest;
    let lock = if use_minimal {
        ".shinytauri/rproj.lock"
    } else {
        "rproj.lock"
    };
    let mut table = Table::new();
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
    table.insert("files".into(), files.into());
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
            "The bundled lock contains shiny and its dependencies. For other packages, edit .shinytauri/rproj.toml and run rig proj lock in .shinytauri/."
        );
    } else {
        write_new(&config, document.as_bytes())?;
        if !has_lock {
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
