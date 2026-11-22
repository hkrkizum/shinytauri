use crate::{
    browser::BrowserPage,
    logs::{Outcome, RunLogs},
    package::MINIMAL_TIMEOUT,
    processes,
};
use anyhow::{Context, Result, ensure};
use std::{
    env, fs,
    net::{Ipv4Addr, TcpListener},
    path::{Path, PathBuf},
    process::{Child, Command},
    thread,
    time::{Duration, Instant},
};

pub(crate) fn run(kind: &str, exe: &Path, evidence: &Path) -> Result<()> {
    ensure!(
        matches!(kind, "minimal" | "rhino" | "upgrade"),
        "unknown desktop fixture {kind}"
    );
    let exe = std::path::absolute(exe)?;
    let evidence = std::path::absolute(evidence)?;
    fs::create_dir_all(&evidence).with_context(|| format!("creating {}", evidence.display()))?;
    let settings = exe
        .parent()
        .context("missing installation directory")?
        .join("offline/startup.json");
    let settings: serde_json::Value = serde_json::from_slice(
        &fs::read(&settings).with_context(|| format!("read {}", settings.display()))?,
    )?;
    let expected = if kind == "minimal" {
        MINIMAL_TIMEOUT
    } else {
        90
    };
    ensure!(
        settings["startupTimeoutSeconds"] == expected,
        "wrong installed startup timeout for {kind}: {settings}"
    );
    if kind == "minimal" {
        let gates = tempfile::tempdir().context("creating the fixture gate directory")?;
        for (name, outcome) in [
            ("success", Outcome::Ready),
            ("failure", Outcome::AppError),
            ("timeout", Outcome::PortTimeout(MINIMAL_TIMEOUT)),
        ] {
            check(
                kind,
                &exe,
                &evidence,
                Some(&gates.path().join(name)),
                outcome,
            )?;
        }
        Ok(())
    } else {
        check(kind, &exe, &evidence, None, Outcome::Ready)
    }
}

fn launch(exe: &Path, evidence: &Path, gate: Option<&Path>, delay: u64) -> Result<(Child, u16)> {
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
    command.env("SHINYTAURI_SMOKE_STARTUP_DELAY", delay.to_string());
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

fn check(
    kind: &str,
    exe: &Path,
    evidence: &Path,
    gate: Option<&Path>,
    outcome: Outcome,
) -> Result<()> {
    let run = match outcome {
        Outcome::Ready => kind,
        Outcome::AppError => "intentional-failure",
        Outcome::PortTimeout(_) => "startup-timeout",
    };
    let identifier = if kind == "rhino" {
        "org.example.rhino"
    } else {
        "org.example.minimal"
    };
    let local = PathBuf::from(env::var_os("LOCALAPPDATA").context("missing LOCALAPPDATA")?);
    let logs = RunLogs::before_launch(local.join(identifier).join("logs"))?;
    // The update omits the setting. Its app deliberately takes longer than
    // the old product's custom limit while remaining within the 90 s default.
    let delay = if kind == "upgrade" {
        MINIMAL_TIMEOUT + 2
    } else {
        0
    };
    let (mut host, port) = launch(exe, evidence, gate, delay)?;
    let pid = host.id();
    let result = BrowserPage::attach(port, &mut host).and_then(|page| {
        let mut runtimes = None;
        let checks = (|| -> Result<()> {
            let bundled_r = exe
                .parent()
                .context("missing installation directory")?
                .join("offline/R");
            if let Some(gate) = gate {
                page.startup(outcome != Outcome::Ready, evidence)?;
                let action = match outcome {
                    Outcome::Ready => "start",
                    Outcome::AppError => "fail",
                    Outcome::PortTimeout(_) => "timeout",
                };
                // R must never observe an empty or partially written gate.
                let pending = gate.with_extension("tmp");
                fs::write(&pending, action)?;
                fs::rename(pending, gate).context("releasing the R startup gate")?;
                if let Outcome::PortTimeout(seconds) = outcome {
                    let worker = gate.with_extension("worker-ready");
                    let limit = Instant::now() + Duration::from_secs(seconds);
                    while !worker.try_exists()? {
                        ensure!(
                            Instant::now() < limit,
                            "timeout fixture's R worker did not start"
                        );
                        thread::sleep(Duration::from_millis(100));
                    }
                    // Capture both Rterm and its sleeping Rscript child before expiry.
                    let children = processes::RProcesses::capture(pid, &bundled_r, 2)?;
                    let worker_pid = fs::read_to_string(worker)?.trim().parse()?;
                    ensure!(
                        children.contains(worker_pid),
                        "bundled worker {worker_pid} not captured"
                    );
                    runtimes = Some(children);
                }
            }
            if outcome == Outcome::Ready {
                page.exercise(kind, evidence)?;
                let minimum = if kind == "rhino" { 2 } else { 1 };
                runtimes = Some(processes::RProcesses::capture(pid, &bundled_r, minimum)?);
            } else {
                let timeout = match outcome {
                    Outcome::PortTimeout(seconds) => Some(seconds),
                    _ => None,
                };
                page.failed(&evidence.join(format!("{run}.png")), timeout)?;
            }
            let (host_name, r_name, elapsed_ms) = logs.check(pid, outcome)?;
            ensure!(
                elapsed_ms >= delay * 1000,
                "configured fixture delay of {delay} s was skipped: {elapsed_ms} ms"
            );
            if outcome != Outcome::Ready {
                page.check_log_links(&host_name, &r_name)?;
            }
            // The failed window stays usable; its R job must already have exited.
            if matches!(outcome, Outcome::PortTimeout(_)) {
                runtimes
                    .as_ref()
                    .context("missing captured R job")?
                    .wait_exited(Duration::from_secs(5))?;
            }
            processes::close_main_window(&mut host)?;
            if outcome == Outcome::Ready
                && let Some(children) = &runtimes
            {
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
