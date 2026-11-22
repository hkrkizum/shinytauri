use crate::{branding, desktop, upgrade};
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

/// Keep the installed programs offline and leave the loopback Shiny session reachable.
struct Firewall {
    name: String,
    active: bool,
}
impl Firewall {
    fn block(&mut self, exe: &Path) -> Result<()> {
        let mut program = std::ffi::OsString::from("program=");
        program.push(exe);
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
    if kind == "rhino" {
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
    const TEXT: &str = "saved with version 0.1.0";
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
