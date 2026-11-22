use anyhow::{Context, Result, ensure};
use serde_json::Value;
use std::{
    collections::HashSet,
    ffi::OsString,
    fs, io,
    path::{Path, PathBuf},
};

const FAILURE_MARKER: &str = "Acceptance fixture startup failure";

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Outcome {
    Ready,
    AppError,
    PortTimeout(u64),
}

fn check_timeline(contents: &str, pid: u32, failed: bool) -> Result<Value> {
    let expected: &[&str] = if failed {
        &["prepare", "r", "load", "failed"]
    } else {
        &["prepare", "r", "load", "http", "http_response", "open"]
    };
    let mut records = contents
        .lines()
        .map(serde_json::from_str::<Value>)
        .collect::<serde_json::Result<Vec<_>>>()
        .context("decoding host JSONL")?;
    ensure!(
        records.len() == expected.len(),
        "expected {expected:?}, got {records:?}"
    );
    let mut previous = 0;
    let r_pid = records[1]["r_pid"]
        .as_u64()
        .context("missing R process ID")?;
    ensure!(r_pid > 0, "invalid R process ID");
    for (record, &event) in records.iter().zip(expected) {
        ensure!(
            record["event"].as_str() == Some(event),
            "expected {event}, got {record}"
        );
        ensure!(
            record["host_pid"].as_u64() == Some(u64::from(pid)),
            "wrong host process in {record}"
        );
        let elapsed = record["elapsed_ms"]
            .as_u64()
            .context("missing elapsed_ms")?;
        ensure!(elapsed >= previous, "elapsed_ms decreased in {record}");
        previous = elapsed;
        if event != "prepare" {
            ensure!(
                record["r_pid"].as_u64() == Some(r_pid),
                "different R process in {record}"
            );
        }
        if event == "failed" {
            ensure!(
                record["level"].as_str() == Some("error")
                    && record["detail"]
                        .as_str()
                        .is_some_and(|detail| !detail.is_empty()),
                "missing failure diagnostic in {record}"
            );
        }
        if event == "http_response" {
            ensure!(
                record["http_status"]
                    .as_u64()
                    .is_some_and(|s| (200..400).contains(&s)),
                "unexpected fixture HTTP response in {record}"
            );
        }
    }
    Ok(records.pop().unwrap())
}

fn check_port_timeout(record: &Value, seconds: u64) -> Result<()> {
    ensure!(
        record["detail"]
            == format!("Startup timed out after {seconds} seconds waiting for the Shiny port"),
        "wrong timeout diagnostic in {record}"
    );
    let elapsed = record["elapsed_ms"]
        .as_u64()
        .context("missing elapsed time")?;
    // Allow host/UI startup and runner scheduling, but reject the old 90 s deadline.
    ensure!(
        (seconds * 1000..(seconds + 30) * 1000).contains(&elapsed),
        "expected a {seconds} s deadline, got {elapsed} ms"
    );
    Ok(())
}

fn filenames(dir: &Path) -> Result<HashSet<OsString>> {
    let entries = match fs::read_dir(dir) {
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(HashSet::new()),
        result => result.with_context(|| format!("listing {}", dir.display()))?,
    };
    Ok(entries
        .map(|entry| entry.map(|entry| entry.file_name()))
        .collect::<io::Result<_>>()?)
}

pub(crate) struct RunLogs {
    dir: PathBuf,
    previous: HashSet<OsString>,
}

impl RunLogs {
    pub(crate) fn before_launch(dir: PathBuf) -> Result<Self> {
        let previous = filenames(&dir)?;
        Ok(Self { dir, previous })
    }

    fn new_names(&self, pid: u32) -> Result<(Vec<String>, Vec<String>)> {
        let host_prefix = format!("host-{pid}-");
        let r_prefix = format!("shiny-{pid}-");
        let mut host = Vec::new();
        let mut raw = Vec::new();
        for name in filenames(&self.dir)?.difference(&self.previous) {
            let Some(name) = name.to_str() else { continue };
            if name.starts_with(&host_prefix) && name.ends_with(".jsonl") {
                host.push(name.to_owned());
            } else if name.starts_with(&r_prefix) && name.ends_with(".log") {
                raw.push(name.to_owned());
            }
        }
        Ok((host, raw))
    }

    pub(crate) fn check(&self, pid: u32, outcome: Outcome) -> Result<(String, String, u64)> {
        let (host, raw) = self.new_names(pid)?;
        let ([host], [raw]) = (host.as_slice(), raw.as_slice()) else {
            anyhow::bail!(
                "expected one new host and R log for PID {pid} in {}, got {host:?} and {raw:?}",
                self.dir.display()
            )
        };
        let contents =
            fs::read_to_string(self.dir.join(host)).with_context(|| format!("reading {host}"))?;
        let last = check_timeline(&contents, pid, outcome != Outcome::Ready)
            .with_context(|| format!("checking {host}"))?;
        if let Outcome::PortTimeout(seconds) = outcome {
            check_port_timeout(&last, seconds)?;
        }
        if outcome == Outcome::AppError {
            let bytes = fs::read(self.dir.join(raw)).with_context(|| format!("reading {raw}"))?;
            ensure!(
                String::from_utf8_lossy(&bytes).contains(FAILURE_MARKER),
                "fixture diagnostic missing from {raw}"
            );
            ensure!(
                !contents.contains(FAILURE_MARKER),
                "raw R diagnostic leaked into {host}"
            );
        }
        Ok((
            host.clone(),
            raw.clone(),
            last["elapsed_ms"]
                .as_u64()
                .context("missing final elapsed time")?,
        ))
    }

    pub(crate) fn copy_evidence(&self, pid: u32, run: &str, evidence: &Path) -> Result<()> {
        let (host, raw) = self.new_names(pid)?;
        let mut errors = Vec::new();
        for name in host.into_iter().chain(raw) {
            if let Err(error) =
                fs::copy(self.dir.join(&name), evidence.join(format!("{run}-{name}")))
            {
                errors.push(format!("{name}: {error}"));
            }
        }
        ensure!(
            errors.is_empty(),
            "could not preserve logs: {}",
            errors.join(", ")
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const PID: u32 = 145;
    const R_PID: u32 = 223;

    fn timeline(failed: bool) -> Vec<serde_json::Value> {
        let mut events = vec!["prepare", "r", "load"];
        if failed {
            events.push("failed");
        } else {
            events.extend(["http", "http_response", "open"]);
        }
        events
            .iter()
            .enumerate()
            .map(|(i, stage)| {
                let mut line =
                    json!({"event": stage, "host_pid": PID, "elapsed_ms": i * 50, "level": "info"});
                if i > 0 {
                    line["r_pid"] = json!(R_PID)
                }
                if *stage == "failed" {
                    line["level"] = json!("error");
                    line["detail"] = json!("R exited during startup");
                }
                if *stage == "http_response" {
                    line["http_status"] = json!(302);
                }
                line
            })
            .collect()
    }

    fn text(events: &[serde_json::Value]) -> String {
        events
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn distinguishes_a_configured_port_timeout_from_generic_or_late_failure() {
        let mut lines = timeline(true);
        lines[3]["detail"] = json!("Startup timed out after 20 seconds waiting for the Shiny port");
        lines[3]["elapsed_ms"] = json!(20_150);
        let check = |v: &[Value]| check_port_timeout(v.last().unwrap(), 20);
        check(&lines).unwrap();
        for elapsed in [19_000, 90_000] {
            lines[3]["elapsed_ms"] = json!(elapsed);
            assert!(check(&lines).is_err());
        }
        lines[3]["elapsed_ms"] = json!(20_150);
        for detail in [
            "R exited during startup",
            "Startup timed out after 20 seconds waiting for the Shiny HTTP response",
        ] {
            lines[3]["detail"] = json!(detail);
            assert!(check(&lines).is_err());
        }
    }

    #[test]
    fn accepts_a_redirect_and_an_intentional_child_exit() {
        check_timeline(&text(&timeline(false)), PID, false).unwrap();
        check_timeline(&text(&timeline(true)), PID, true).unwrap();
    }

    #[test]
    fn catches_wrong_process_order_timing_and_http_status() {
        for (index, key, value) in [
            (0, "host_pid", json!(PID + 1)),
            (1, "r_pid", json!(0)),
            (2, "r_pid", json!(R_PID + 1)),
            (3, "elapsed_ms", json!(1)),
            (3, "event", json!("open")),
            (4, "http_status", json!(404)),
            (4, "http_status", json!("200")),
        ] {
            let mut events = timeline(false);
            events[index][key] = value;
            assert!(
                check_timeline(&text(&events), PID, false).is_err(),
                "{index} {key}"
            );
        }
        assert!(check_timeline(&text(&timeline(false)[..5]), PID, false).is_err());
        assert!(check_timeline("not JSON", PID, false).is_err());
        let mut events = timeline(true);
        events[3]["detail"] = json!("");
        assert!(check_timeline(&text(&events), PID, true).is_err());
    }

    #[test]
    fn copies_only_this_run_and_keeps_evidence_even_for_invalid_logs() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("diagnostics");
        let evidence = root.path().join("evidence");
        fs::create_dir(&evidence).unwrap();
        let missing = RunLogs::before_launch(dir.clone()).unwrap();
        assert!(missing.check(PID, Outcome::AppError).is_err());
        fs::create_dir(&dir).unwrap();
        fs::write(dir.join("host-145-old.jsonl"), b"old").unwrap();
        let logs = RunLogs::before_launch(dir.clone()).unwrap();
        let raw = dir.join("shiny-145-new.log");
        let host = dir.join("host-145-new.jsonl");
        fs::write(dir.join("host-1451-other.jsonl"), b"other").unwrap();
        fs::write(
            &raw,
            [b"\xff".as_slice(), FAILURE_MARKER.as_bytes()].concat(),
        )
        .unwrap();
        let lines = text(&timeline(true));
        fs::write(&host, &lines).unwrap();
        let (host_name, r_name, elapsed_ms) = logs.check(PID, Outcome::AppError).unwrap();
        assert_eq!(elapsed_ms, 150);
        assert!(logs.check(PID, Outcome::PortTimeout(20)).is_err());
        assert_eq!(host_name, "host-145-new.jsonl");
        assert_eq!(r_name, "shiny-145-new.log");
        fs::write(&raw, b"missing the marker").unwrap();
        assert!(logs.check(PID, Outcome::AppError).is_err());
        fs::write(&raw, FAILURE_MARKER).unwrap();
        fs::write(
            &host,
            lines.replace("R exited during startup", FAILURE_MARKER),
        )
        .unwrap();
        assert!(logs.check(PID, Outcome::AppError).is_err());
        fs::write(&host, b"{malformed").unwrap();
        assert!(logs.check(PID, Outcome::AppError).is_err());
        logs.copy_evidence(PID, "intentional-failure", &evidence)
            .unwrap();
        assert_eq!(
            fs::read(evidence.join("intentional-failure-host-145-new.jsonl")).unwrap(),
            b"{malformed"
        );
        assert_eq!(fs::read_dir(&evidence).unwrap().count(), 2);
        fs::write(&host, &lines).unwrap();
        fs::write(dir.join("host-145-duplicate.jsonl"), &lines).unwrap();
        assert!(logs.check(PID, Outcome::AppError).is_err());
    }
}
