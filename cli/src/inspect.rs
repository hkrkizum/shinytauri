use crate::{Result, init, parse_r_version};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use toml::{Table, Value};

fn string<'a>(spec: &'a Table, key: &str) -> Result<Option<&'a str>> {
    spec.get(key)
        .map(|value| {
            value
                .as_str()
                .filter(|s| !s.trim().is_empty())
                .ok_or_else(|| format!("Expected a nonempty string for {key}").into())
        })
        .transpose()
}

fn strings<'a>(value: &'a Value, key: &str) -> Result<Vec<&'a str>> {
    // The R backend also accepts a single TOML string as a one-element vector.
    if let Some(item) = value.as_str() {
        return Ok(vec![item]);
    }
    let items = value
        .as_array()
        .filter(|a| !a.is_empty())
        .ok_or_else(|| format!("Expected a nonempty array for {key}"))?;
    items
        .iter()
        .map(|item| {
            item.as_str()
                .ok_or_else(|| format!("Invalid {key} entry").into())
        })
        .collect()
}

fn app_paths<'a>(spec: &'a Table, key: &str) -> Result<Vec<&'a str>> {
    let Some(value) = spec.get(key) else {
        return if key == "emptyDirs" {
            Ok(Vec::new())
        } else {
            Err(format!("Missing {key}").into())
        };
    };
    let entries = strings(value, key)?;
    for entry in &entries {
        if entry.contains([':', '\\']) || entry.split('/').any(|p| matches!(p, "" | "." | "..")) {
            return Err(format!("Expected relative app paths for {key}: {entry:?}").into());
        }
    }
    Ok(entries)
}

fn metadata(spec: &Table) -> Result<()> {
    for key in ["quarto", "checkMinimal"] {
        if spec.get(key).is_some_and(|value| !value.is_bool()) {
            return Err(format!("Expected a boolean for {key}").into());
        }
    }
    string(spec, "title")?;
    if let Some(name) = string(spec, "productName")? {
        let stem = name.split('.').next().unwrap().to_ascii_lowercase();
        let reserved = matches!(stem.as_str(), "con" | "prn" | "aux" | "nul")
            || stem
                .strip_prefix("com")
                .or_else(|| stem.strip_prefix("lpt"))
                .is_some_and(|n| matches!(n.as_bytes(), [b'1'..=b'9']));
        if reserved
            || name.ends_with([' ', '.'])
            || name
                .chars()
                .any(|c| c.is_control() || "<>:\"/\\|?*".contains(c))
        {
            return Err("Invalid Windows productName".into());
        }
    }
    if let Some(id) = string(spec, "identifier")?
        && id
            .split('.')
            .any(|p| p.is_empty() || !p.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-'))
    {
        return Err("Invalid Tauri identifier".into());
    }
    if let Some(v) = string(spec, "version")? {
        parse_r_version(v).map_err(|_| "Expected a numeric major.minor.patch version")?;
    }
    if let Some(v) = string(spec, "rVersion")? {
        parse_r_version(v).map_err(|_| "Expected an exact rVersion in major.minor.patch form")?;
    }
    Ok(())
}

#[derive(Default)]
struct Inventory {
    files: BTreeSet<String>,
    dirs: BTreeSet<String>,
    missing: BTreeSet<String>,
}

fn is_link(meta: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if meta.file_attributes() & 0x400 != 0 {
            return true; // Directory junctions are reparse points too.
        }
    }
    meta.is_symlink()
}

impl Inventory {
    fn entry(&mut self, app: &Path, relative: &str, directory_only: bool) -> Result<()> {
        // Check each ancestor before walking it, including for an explicitly named file.
        let mut path = app.to_owned();
        let mut parts = relative.split('/').peekable();
        while let Some(part) = parts.next() {
            path.push(part);
            let meta = match fs::symlink_metadata(&path) {
                Ok(meta) => meta,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    self.missing.insert(relative.to_owned());
                    return Ok(());
                }
                Err(e) => return Err(format!("{}: {e}", path.display()).into()),
            };
            if is_link(&meta) {
                return Err(format!("App resource contains a link: {}", path.display()).into());
            }
            if parts.peek().is_none() {
                return self.walk(&path, relative, meta, directory_only);
            }
        }
        unreachable!("app_paths rejects empty entries")
    }

    fn walk(
        &mut self,
        path: &Path,
        relative: &str,
        meta: fs::Metadata,
        directory_only: bool,
    ) -> Result<()> {
        if is_link(&meta) {
            return Err(format!("App resource contains a link: {}", path.display()).into());
        }
        if directory_only && !meta.is_dir() {
            return Err(format!("Expected an app directory: {relative}").into());
        }
        if meta.is_file() {
            self.files.insert(relative.to_owned());
        } else if meta.is_dir() {
            self.dirs.insert(relative.to_owned());
            if !directory_only {
                for child in fs::read_dir(path)? {
                    let child = child?;
                    let name = child
                        .file_name()
                        .into_string()
                        .map_err(|_| "App resources must have UTF-8 names")?;
                    self.walk(
                        &child.path(),
                        &format!("{relative}/{name}"),
                        child.metadata()?,
                        false,
                    )?;
                }
            }
        } else {
            return Err(format!("Expected an app file or directory: {relative}").into());
        }
        Ok(())
    }

    fn parents(&self) -> BTreeSet<&str> {
        self.files
            .iter()
            .chain(&self.dirs)
            .flat_map(|path| path.match_indices('/').map(|(i, _)| &path[..i]))
            .collect()
    }
}

fn report_line(prefix: &str, value: &str) {
    // A path or publisher can contain terminal control characters.
    if value.chars().any(char::is_control) {
        println!("{prefix}{}", value.escape_debug());
    } else {
        println!("{prefix}{value}");
    }
}

fn group<'a>(label: &str, entries: impl IntoIterator<Item = &'a str>) {
    println!("{label}:");
    for entry in entries {
        report_line("  ", entry);
    }
}

fn installer_inputs(spec: &Table, base: &Path) -> Result<()> {
    if let Some(publisher) = string(spec, "publisher")? {
        report_line("Publisher: ", publisher);
    }
    for (key, label) in [
        ("icon", "Installer icon: "),
        ("licenseFile", "Installer license: "),
    ] {
        if let Some(source) = string(spec, key)? {
            let path = base.join(source);
            let meta = fs::metadata(&path)
                .map_err(|err| format!("Installer {key} at {}: {err}", path.display()))?;
            if !meta.is_file() || meta.len() == 0 {
                return Err(
                    format!("Expected a nonempty file for {key}: {}", path.display()).into(),
                );
            }
            report_line(label, &path.canonicalize()?.to_string_lossy());
        }
    }
    if let Some(source) = string(spec, "sourceBundle")? {
        let path = base.join(source);
        let meta = fs::symlink_metadata(&path)
            .map_err(|err| format!("sourceBundle at {}: {err}", path.display()))?;
        if is_link(&meta) || !(meta.is_dir() || meta.is_file() && meta.len() > 0) {
            return Err(format!(
                "Expected a sourceBundle directory or nonempty archive: {}",
                path.display()
            )
            .into());
        }
        report_line("Source bundle: ", &path.canonicalize()?.to_string_lossy());
    }
    Ok(())
}

fn runtime(spec: &Table, base: &Path) -> Result<()> {
    let lock = string(spec, "lock")?.ok_or("Missing lock in config")?;
    let version = string(spec, "rVersion")?;
    let dependency = string(spec, "dependencyFile")?;
    let managed = dependency.is_some()
        || spec.contains_key("packages")
        || (version.is_some() && lock == ".shinytauri/windows.lock");
    if managed && lock != ".shinytauri/windows.lock" {
        return Err("For managed dependencies set lock = \".shinytauri/windows.lock\".".into());
    }
    if let Some(path) = dependency {
        let path = base.join(path);
        if !path.is_file()
            || !matches!(
                path.file_name().and_then(|x| x.to_str()),
                Some("renv.lock" | "DESCRIPTION")
            )
        {
            return Err(format!(
                "Expected an existing dependencyFile named renv.lock or DESCRIPTION: {}",
                path.display()
            )
            .into());
        }
    }
    if let Some(packages) = spec.get("packages") {
        for p in strings(packages, "packages")? {
            let (name, pin) = p.split_once('=').map_or((p, None), |(n, v)| (n, Some(v)));
            if !name.starts_with(|c: char| c.is_ascii_alphabetic())
                || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'.')
                || pin.is_some_and(|v| {
                    v.split(['.', '-'])
                        .any(|s| s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()))
                })
            {
                return Err("Expected NAME or NAME=VERSION in packages".into());
            }
        }
    }
    let path = base.join(lock);
    if !path.is_file() {
        return Err(format!(
            "Missing Windows lock: {}. Run {}.",
            path.display(),
            if managed {
                "shinytauri lock"
            } else {
                "rig proj lock"
            }
        )
        .into());
    }
    let lock = init::read_lock(&path)?;
    let target = init::windows_target(&lock, &path, if managed { None } else { version })?;
    let selected = target
        .get("r_version")
        .and_then(Value::as_str)
        .ok_or("Missing r_version in Windows lock")?;
    parse_r_version(selected)?;
    if version.is_some_and(|v| v != selected) {
        return Err(format!(
            "Windows lock targets R {selected} but rVersion differs; run shinytauri lock again."
        )
        .into());
    }
    let count = init::main_packages(target).count();
    println!("Windows R: {selected}\nMain packages: {count}");
    Ok(())
}

pub(crate) fn report(config: PathBuf) -> Result<()> {
    let config = config.canonicalize()?;
    let base = config.parent().unwrap();
    let spec: Table = fs::read_to_string(&config)?
        .parse()
        .map_err(|e: toml::de::Error| {
            format!(
                "Invalid bundle config {}: {}",
                config.display(),
                e.message()
            )
        })?;
    metadata(&spec)?;
    let app_path = base.join(string(&spec, "app")?.ok_or("Missing app directory in config")?);
    if !app_path.is_dir() {
        return Err(format!("Expected an app directory: {}", app_path.display()).into());
    }
    let app = app_path.canonicalize()?;
    let files = app_paths(&spec, "files")?;
    let empty_dirs = app_paths(&spec, "emptyDirs")?;
    init::check_entry_names(files.iter().chain(&empty_dirs).copied())?;
    let mut inventory = Inventory::default();
    for relative in files {
        inventory.entry(&app, relative, false)?;
    }
    for relative in empty_dirs {
        inventory.entry(&app, relative, true)?;
    }

    init::check_entry_names(
        inventory
            .files
            .iter()
            .chain(&inventory.dirs)
            .map(String::as_str),
    )?;
    let parents = inventory.parents();
    group("Included files", inventory.files.iter().map(String::as_str));
    group(
        "Empty directories",
        inventory
            .dirs
            .iter()
            .map(String::as_str)
            .filter(|d| !parents.contains(d)),
    );
    let candidates = init::resources(&app)?;
    let added: BTreeSet<_> = candidates
        .files
        .iter()
        .filter(|s| !inventory.files.contains(*s))
        .cloned()
        .chain(
            candidates
                .empty_dirs
                .iter()
                .filter(|s| !inventory.dirs.contains(*s) && !parents.contains(s.as_str()))
                .map(|s| format!("{s}/")),
        )
        .collect();
    group(
        "Not included (init candidates)",
        added.iter().map(String::as_str),
    );
    group(
        "Missing entries",
        inventory.missing.iter().map(String::as_str),
    );
    installer_inputs(&spec, base)?;
    runtime(&spec, base)?;
    if !inventory.missing.is_empty() {
        return Err("Some configured app resources are missing".into());
    }
    if !inventory
        .files
        .iter()
        .any(|s| s.eq_ignore_ascii_case("app.R") || s.eq_ignore_ascii_case("server.R"))
    {
        return Err("Missing Shiny entry point in files: include app.R or server.R".into());
    }
    Ok(())
}
