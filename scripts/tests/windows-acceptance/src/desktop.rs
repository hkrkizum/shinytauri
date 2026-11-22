use crate::{browser::BrowserPage, logs::RunLogs, processes};
use anyhow::{Context, Result, ensure};
use std::{
    env, fs,
    net::{Ipv4Addr, TcpListener},
    path::{Path, PathBuf},
    process::{Child, Command},
    time::Duration,
};

pub(crate) fn run(kind: &str, exe: &Path, evidence: &Path) -> Result<()> {
    ensure!(
        matches!(kind, "minimal" | "rhino" | "upgrade"),
        "unknown desktop fixture {kind}"
    );
    let exe = std::path::absolute(exe)?;
    let evidence = std::path::absolute(evidence)?;
    fs::create_dir_all(&evidence).with_context(|| format!("creating {}", evidence.display()))?;
    if kind == "minimal" {
        let gates = tempfile::tempdir().context("creating the fixture gate directory")?;
        check(
            kind,
            &exe,
            &evidence,
            Some(&gates.path().join("success")),
            false,
        )?;
        check(
            kind,
            &exe,
            &evidence,
            Some(&gates.path().join("failure")),
            true,
        )
    } else {
        check(kind, &exe, &evidence, None, false)
    }
}

fn launch(exe: &Path, evidence: &Path, gate: Option<&Path>) -> Result<(Child, u16)> {
    let mut command = Command::new(exe);
    command.current_dir(evidence);
    for (key, _) in env::vars_os() {
        let name = key.to_string_lossy().to_ascii_uppercase();
        if ["R_", "RIG_", "QUARTO_", "SHINYTAURI_"]
            .iter()
            .any(|prefix| name.starts_with(prefix))
        {
            command.env_remove(key);
        }
    }
    let windows = PathBuf::from(env::var_os("SYSTEMROOT").context("missing SYSTEMROOT")?);
    let path = env::join_paths([windows.join("System32"), windows])?;
    command.env("PATH", path);
    if let Some(gate) = gate {
        command.env("SHINYTAURI_SMOKE_GATE", gate);
    }
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
    let port = listener.local_addr()?.port();
    command.env(
        "WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS",
        format!("--remote-debugging-port={port}"),
    );
    drop(listener);
    Ok((
        command
            .spawn()
            .with_context(|| format!("launching installed app {}", exe.display()))?,
        port,
    ))
}

fn check(kind: &str, exe: &Path, evidence: &Path, gate: Option<&Path>, failed: bool) -> Result<()> {
    let run = if failed { "intentional-failure" } else { kind };
    let identifier = if kind == "rhino" {
        "org.example.rhino"
    } else {
        "org.example.minimal"
    };
    let local = PathBuf::from(env::var_os("LOCALAPPDATA").context("missing LOCALAPPDATA")?);
    let logs = RunLogs::before_launch(local.join(identifier).join("logs"))?;
    let (mut host, port) = launch(exe, evidence, gate)?;
    let pid = host.id();
    let result = BrowserPage::attach(port, &mut host).and_then(|page| {
        let mut runtimes = None;
        let checks = (|| -> Result<()> {
            if let Some(gate) = gate {
                page.startup(failed, evidence)?;
                // R must never observe an empty or partially written gate.
                let pending = gate.with_extension("tmp");
                fs::write(&pending, if failed { "fail" } else { "start" })?;
                fs::rename(pending, gate).context("releasing the R startup gate")?;
            }
            if failed {
                page.failed(evidence)?;
            } else {
                page.exercise(kind, evidence)?;
            }
            let (host_name, r_name) = logs.check(pid, failed)?;
            if failed {
                page.check_log_links(&host_name, &r_name)?;
            } else {
                let minimum = if kind == "rhino" { 2 } else { 1 };
                let bundled_r = exe
                    .parent()
                    .context("missing installation directory")?
                    .join("offline/R");
                runtimes = Some(processes::RProcesses::capture(pid, &bundled_r, minimum)?);
            }
            processes::close_main_window(&mut host)?;
            if let Some(children) = &runtimes {
                children.wait_exited(Duration::from_secs(10))?;
            }
            Ok(())
        })();
        if checks.is_err() {
            let screenshot = evidence.join(format!("{run}-unexpected.png"));
            if let Err(err) = page.screenshot(&screenshot) {
                eprintln!("Could not capture {}: {err:#}", screenshot.display());
            }
            // A worker can outlive both the host and its intermediate R parent.
            // Use the handles captured before WM_CLOSE to clean up such orphans.
            if let Some(children) = &runtimes
                && let Err(err) = children.terminate_live()
            {
                eprintln!("Could not clean up captured R processes: {err:#}");
            }
        }
        checks
    });
    // Keep the original assertion as the cause even when subsequent cleanup
    // fails. Always collect both host and R logs, including for a failed attach.
    let cleanup = if result.is_err() {
        processes::terminate_tree(&mut host).context("cleaning up installed app processes")
    } else {
        Ok(())
    };
    let evidence_result = logs.copy_evidence(pid, run, evidence);
    if let Err(err) = result {
        for secondary in [cleanup, evidence_result]
            .into_iter()
            .filter_map(Result::err)
        {
            eprintln!("Additional {run} failure: {secondary:#}");
        }
        return Err(err).with_context(|| format!("checking {run}"));
    }
    cleanup.and(evidence_result)
}
