use crate::{branding, desktop, notices, upgrade};
use anyhow::{Context, Result, ensure};
use std::{
    env, fs,
    path::{Path, PathBuf},
    process::{self, Command},
};
use winreg::{RegKey, enums::HKEY_CURRENT_USER};

#[derive(Debug, PartialEq)]
struct Installed {
    directory: PathBuf,
    host: PathBuf,
    version: String,
    publisher: Option<String>,
}

fn registration(kind: &str) -> Result<Installed> {
    let name = format!("shinytauri-smoke-{kind}");
    let key = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey(format!(
            r"Software\Microsoft\Windows\CurrentVersion\Uninstall\{name}"
        ))
        .with_context(|| format!("Read registration for {name}"))?;
    ensure!(
        key.get_value::<String, _>("DisplayName")? == name,
        "Unexpected registration for {name}"
    );
    let location: String = key.get_value("InstallLocation")?;
    let directory = PathBuf::from(location.trim_matches('"'));
    let host = directory.join(key.get_value::<String, _>("MainBinaryName")?);
    ensure!(host.is_file(), "Missing installed host {}", host.display());
    Ok(Installed {
        directory,
        host,
        version: key.get_value("DisplayVersion")?,
        publisher: key.get_value("Publisher").ok(),
    })
}

fn runner_path(name: &str) -> Result<PathBuf> {
    env::var_os(name)
        .map(PathBuf::from)
        .with_context(|| format!("Missing {name}"))
}

// netsh expects an ordinary DOS/UNC path, even when Rust opened the file
// through a mixture of separators or returned an extended-length path.
fn firewall_program_arg(exe: &Path) -> Result<std::ffi::OsString> {
    use std::path::{Component, Prefix};

    let full = exe
        .canonicalize()
        .with_context(|| format!("Resolve firewall program {}", exe.display()))?;
    let mut parts = full.components();
    let mut arg = std::ffi::OsString::from("program=");
    match parts.next() {
        Some(Component::Prefix(prefix)) => match prefix.kind() {
            Prefix::Disk(drive) | Prefix::VerbatimDisk(drive) => {
                arg.push(format!("{}:", char::from(drive)))
            }
            Prefix::UNC(server, share) | Prefix::VerbatimUNC(server, share) => {
                arg.push(r"\\");
                arg.push(server);
                arg.push(r"\");
                arg.push(share);
            }
            _ => anyhow::bail!("Unsupported firewall program path: {}", full.display()),
        },
        _ => anyhow::bail!(
            "Firewall program must have an absolute path: {}",
            full.display()
        ),
    }
    for part in parts {
        if let Component::Normal(name) = part {
            arg.push(r"\");
            arg.push(name);
        }
    }
    Ok(arg)
}

/// Keep the installed programs offline and leave the loopback Shiny session reachable.
struct Firewall {
    name: String,
    active: bool,
}
impl Firewall {
    fn block(&mut self, exe: &Path) -> Result<()> {
        let program = firewall_program_arg(exe)?;
        let status = Command::new("netsh")
            .args(["advfirewall", "firewall", "add", "rule", &format!("name={}", self.name),
                   "dir=out", "action=block", "profile=any",
                   "remoteip=0.0.0.0-126.255.255.255,128.0.0.0-255.255.255.255,::2-ffff:ffff:ffff:ffff:ffff:ffff:ffff:ffff"])
            .arg(program).status()?;
        ensure!(
            status.success(),
            "Could not block external traffic for {}",
            exe.display()
        );
        self.active = true;
        Ok(())
    }

    fn remove(&mut self) -> Result<()> {
        if self.active {
            let status = Command::new("netsh")
                .args([
                    "advfirewall",
                    "firewall",
                    "delete",
                    "rule",
                    &format!("name={}", self.name),
                ])
                .status()?;
            ensure!(
                status.success(),
                "Could not remove the acceptance firewall rules"
            );
            self.active = false;
        }
        Ok(())
    }

    fn block_installed(&mut self, directory: &Path) -> Result<()> {
        for entry in fs::read_dir(directory)? {
            let entry = entry?;
            let path = entry.path();
            let kind = entry.file_type()?;
            if kind.is_dir() {
                self.block_installed(&path)?;
            } else if kind.is_file()
                && path
                    .extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case("exe"))
            {
                self.block(&path)?;
            }
        }
        Ok(())
    }
}

fn install(
    kind: &str,
    installers: &Path,
    evidence: &Path,
    firewall: &mut Firewall,
) -> Result<Installed> {
    let setup = installers.join(format!("{kind}-setup.exe"));
    firewall.block(&setup)?;
    if kind == "rhino" {
        branding::check_setup(&setup, &evidence.join("installer-license.png"))?;
    }
    let status = Command::new(&setup).arg("/S").status()?;
    ensure!(status.success(), "{} exited with {status}", setup.display());
    let app = registration(kind)?;
    let rhino = kind == "rhino";
    notices::check(
        &app.directory.join("offline"),
        &runner_path("GITHUB_WORKSPACE")?.join("notices"),
        installers,
        if rhino { "4.6.2" } else { "4.5.3" },
        rhino,
    )?;
    if rhino {
        ensure!(app.version == "1.2.3", "Wrong Rhino version: {app:?}");
        ensure!(
            app.directory.join("offline/app/app/static").is_dir(),
            "Missing Rhino static directory"
        );
        ensure!(
            app.publisher.as_deref() == Some("販売 株式会社")
                && branding::company_name(&app.host)? == "販売 株式会社",
            "Wrong publisher in Rhino registration or host"
        );
        branding::check_host(&app.host)?;
    }
    firewall.block_installed(&app.directory)?;
    Ok(app)
}

fn check_update(
    old: &Installed,
    setup: &Path,
    evidence: &Path,
    firewall: &mut Firewall,
) -> Result<()> {
    ensure!(
        old.version == "0.1.0",
        "Expected 0.1.0 before upgrade: {old:?}"
    );
    let obsolete = old.directory.join("offline/app/www/style.css");
    ensure!(obsolete.is_file(), "Missing v1 stylesheet before upgrade");
    let note = runner_path("LOCALAPPDATA")?.join("org.example.minimal/note.txt");
    let unmanaged = old.directory.join("user-note.txt");
    const TEXT: &str = "saved with version 0.1.0\r\n";
    for file in [&note, &unmanaged] {
        fs::write(file, TEXT)?;
    }

    firewall.block(setup)?;
    upgrade::run(setup, evidence)?;
    let updated = registration("minimal")?;
    ensure!(
        updated.version == "0.2.0"
            && updated.directory == old.directory
            && updated.host == old.host,
        "Unexpected registration after upgrade: {updated:?}"
    );
    ensure!(
        !obsolete.try_exists()?,
        "Obsolete v1 stylesheet survived upgrade"
    );
    for file in [&note, &unmanaged] {
        ensure!(
            fs::read_to_string(file)? == TEXT,
            "User data changed: {}",
            file.display()
        );
    }
    desktop::run("upgrade", &updated.host, evidence)
}

fn exercise(temp: &Path, evidence: &Path, firewall: &mut Firewall) -> Result<()> {
    let runtimes = runner_path("ProgramFiles(x86)")?.join("Microsoft/EdgeWebView/Application");
    let mut found = false;
    for entry in fs::read_dir(runtimes)? {
        let exe = entry?.path().join("msedgewebview2.exe");
        if exe.is_file() {
            firewall.block(&exe)?;
            found = true;
        }
    }
    ensure!(found, "WebView2 Runtime is required");
    let installers = temp.join("installers");
    let minimal = install("minimal", &installers, evidence, firewall)?;
    let rhino = install("rhino", &installers, evidence, firewall)?;
    ensure!(
        !same_file::is_same_file(&minimal.host, &rhino.host)?,
        "Both products share an executable"
    );
    desktop::run("minimal", &minimal.host, evidence)?;
    check_update(
        &minimal,
        &installers.join("minimal-upgrade-setup.exe"),
        evidence,
        firewall,
    )?;
    ensure!(
        registration("rhino")? == rhino,
        "Upgrading minimal changed the separate Rhino installation"
    );
    desktop::run("rhino", &rhino.host, evidence)
}

pub(crate) fn run() -> Result<()> {
    ensure!(
        env::var("RUNNER_ENVIRONMENT").as_deref() == Ok("github-hosted")
            && env::var("RUNNER_OS").as_deref() == Ok("Windows"),
        "This suite requires a disposable GitHub-hosted Windows runner"
    );
    let temp = runner_path("RUNNER_TEMP")?;
    let evidence = temp.join("windows-acceptance");
    fs::create_dir_all(&evidence)?;
    let mut firewall = Firewall {
        name: format!("shinytauri-acceptance-{}", process::id()),
        active: false,
    };
    let result = exercise(&temp, &evidence, &mut firewall);
    let cleanup = firewall.remove();
    match (result, cleanup) {
        (Err(test), Err(cleanup)) => {
            Err(test.context(format!("Firewall cleanup also failed: {cleanup:#}")))
        }
        (test, cleanup) => test.and(cleanup),
    }
}

#[cfg(test)]
mod firewall_tests {
    use super::*;

    #[test]
    fn netsh_receives_native_absolute_paths_with_spaces_and_unicode() -> Result<()> {
        use std::os::windows::ffi::OsStringExt;

        let temp = tempfile::tempdir()?;
        let directory = temp.path().join("Program Files (x86)").join("利用者");
        fs::create_dir_all(&directory)?;
        let exe = directory.join("edge with spaces.exe");
        fs::write(&exe, [])?;
        let mixed = temp
            .path()
            .join("Program Files (x86)/利用者/edge with spaces.exe");
        for path in [&mixed, &exe.canonicalize()?] {
            use std::os::windows::ffi::OsStrExt;
            let value: Vec<_> = firewall_program_arg(path)?.encode_wide().collect();
            let prefix: Vec<_> = "program=".encode_utf16().collect();
            let plain =
                std::ffi::OsString::from_wide(value.strip_prefix(prefix.as_slice()).unwrap());
            let text = plain.to_str().unwrap();
            assert!(!text.contains('/') && !text.starts_with(r"\\?\"), "{text}");
            assert!(
                text.contains(r"Program Files (x86)\利用者\edge with spaces.exe"),
                "{text}"
            );
            assert!(same_file::is_same_file(Path::new(&plain), &exe)?);
        }
        Ok(())
    }
}
