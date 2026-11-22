use serde::Serialize;
use std::{
    fs::{self, File},
    io::{self, Write},
    path::{Path, PathBuf},
    process,
    sync::{Mutex, MutexGuard},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Event {
    Prepare,
    R,
    Load,
    Http,
    Open,
    Failed,
    Stopped,
}

#[derive(Clone, Serialize)]
pub struct Step {
    pub event: Event,
    pub elapsed_ms: u64,
}

#[derive(Clone, Serialize)]
pub struct Snapshot {
    pub title: String,
    pub events: Vec<Step>,
    pub elapsed_ms: u64,
    pub host_log: Option<String>,
    pub r_log: Option<String>,
    pub error: Option<String>,
}

impl Snapshot {
    /// Evaluate only on the packaged page, including when a pending callback
    /// runs after navigation. Serialize all variable values as JSON literals.
    pub fn guarded_script(&self, origin: &str) -> serde_json::Result<String> {
        let origin = serde_json::to_string(origin)?;
        let snapshot = serde_json::to_string(self)?;
        Ok(format!(
            "if (location.origin === {origin} && (location.pathname === '/' || location.pathname === '/index.html') && document.documentElement?.hasAttribute('data-shinytauri-startup') && typeof window.shinytauriProgress === 'function') {{ window.shinytauriProgress({snapshot}); }}"
        ))
    }
}

fn milliseconds(duration: Duration) -> u64 {
    duration.as_millis().try_into().unwrap_or(u64::MAX)
}

fn timestamp_ms() -> u64 {
    milliseconds(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default(),
    )
}

fn open_log(directory: &Path, stem: &str, extension: &str) -> Option<(File, PathBuf)> {
    for n in 0u32.. {
        let suffix = if n == 0 {
            String::new()
        } else {
            format!("-{n}")
        };
        let path = directory.join(format!("{stem}{suffix}.{extension}"));
        match File::create_new(&path) {
            Ok(file) => return Some((file, path)),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(_) => return None,
        }
    }
    None
}

#[derive(Serialize)]
struct Record<'a, E> {
    timestamp_ms: u64,
    elapsed_ms: u64,
    level: &'static str,
    event: E,
    host_pid: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    r_pid: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    http_status: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<&'a str>,
}

struct State {
    events: Vec<Step>,
    host: Option<(File, PathBuf)>,
    r_log: Option<PathBuf>,
    r_pid: Option<u32>,
    error: Option<String>,
}

impl State {
    fn record(
        &mut self,
        elapsed_ms: u64,
        level: &'static str,
        event: impl Serialize,
        detail: Option<&str>,
        http_status: Option<u16>,
    ) {
        if let Some((file, _)) = &mut self.host {
            let record = Record {
                timestamp_ms: timestamp_ms(),
                elapsed_ms,
                level,
                event,
                host_pid: process::id(),
                r_pid: self.r_pid,
                http_status,
                detail,
            };
            let written = serde_json::to_vec(&record)
                .map_err(io::Error::from)
                .and_then(|mut bytes| {
                    bytes.push(b'\n');
                    file.write_all(&bytes)
                });
            if written.is_err() {
                self.host = None;
            }
        }
    }
}

pub struct Startup {
    started: Instant,
    title: String,
    state: Mutex<State>,
}

impl Startup {
    /// The returned file is solely for R stdout and stderr. The caller can
    /// still launch the child with null streams if logging is unavailable.
    pub fn new(title: &str, log_dir: Option<&Path>) -> (Self, Option<File>) {
        let started = Instant::now();
        let dir = log_dir.filter(|dir| fs::create_dir_all(dir).is_ok());
        let launch = format!("{}-{}", process::id(), timestamp_ms());
        let host = dir.and_then(|d| open_log(d, &format!("host-{launch}"), "jsonl"));
        let raw = dir.and_then(|d| open_log(d, &format!("shiny-{launch}"), "log"));
        let (r_file, r_log) = raw.unzip();
        let mut state = State {
            events: vec![Step {
                event: Event::Prepare,
                elapsed_ms: 0,
            }],
            host,
            r_log,
            r_pid: None,
            error: None,
        };
        state.record(0, "info", Event::Prepare, None, None);
        (
            Self {
                started,
                title: title.to_owned(),
                state: Mutex::new(state),
            },
            r_file,
        )
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn snapshot(&self) -> Snapshot {
        self.view(&self.state())
    }

    fn view(&self, state: &State) -> Snapshot {
        let last = state.events.last().expect("prepare is always recorded");
        let elapsed_ms = if matches!(last.event, Event::Open | Event::Failed | Event::Stopped) {
            last.elapsed_ms
        } else {
            milliseconds(self.started.elapsed())
        };
        Snapshot {
            title: self.title.clone(),
            events: state.events.clone(),
            elapsed_ms,
            host_log: state
                .host
                .as_ref()
                .map(|(_, path)| path.display().to_string()),
            r_log: state.r_log.as_ref().map(|path| path.display().to_string()),
            error: state.error.clone(),
        }
    }

    pub fn child_spawned(&self, pid: u32) -> Snapshot {
        let mut state = self.state();
        state.r_pid = Some(pid);
        self.transition(&mut state, Event::R, None)
    }

    pub fn advance(&self, event: Event) -> Snapshot {
        self.transition(&mut self.state(), event, None)
    }

    pub fn fail(&self, detail: &str) -> Snapshot {
        self.transition(&mut self.state(), Event::Failed, Some(detail))
    }

    pub fn stop(&self, detail: &str) -> Snapshot {
        self.transition(&mut self.state(), Event::Stopped, Some(detail))
    }

    fn transition(&self, state: &mut State, event: Event, detail: Option<&str>) -> Snapshot {
        let previous = state
            .events
            .last()
            .expect("prepare is always recorded")
            .event;
        if event > previous && !matches!(previous, Event::Failed | Event::Stopped) {
            let elapsed_ms = milliseconds(self.started.elapsed());
            let failed = matches!(event, Event::Failed | Event::Stopped);
            state.events.push(Step { event, elapsed_ms });
            state.record(
                elapsed_ms,
                if failed { "error" } else { "info" },
                event,
                detail,
                None,
            );
            if failed {
                let reason = match (event, previous) {
                    (Event::Stopped, _) => {
                        "実行中の R プロセスが終了し、アプリ画面に接続できなくなりました。"
                    }
                    (_, Event::Prepare | Event::R) => "同梱の R を起動できませんでした。",
                    (_, Event::Load) => "Shiny アプリを読み込めませんでした。",
                    (_, Event::Http) => "Shiny サーバーの応答を確認できませんでした。",
                    _ => "アプリ画面を開けませんでした。",
                };
                state.error = Some(reason.to_owned());
            }
        }
        self.view(state)
    }

    pub fn info(&self, event: &'static str, detail: Option<&str>) {
        let mut state = self.state();
        state.record(
            milliseconds(self.started.elapsed()),
            "info",
            event,
            detail,
            None,
        );
    }

    pub fn http_response(&self, status: u16) {
        let mut state = self.state();
        state.record(
            milliseconds(self.started.elapsed()),
            "info",
            "http_response",
            None,
            Some(status),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use std::fs;
    use std::io::{BufRead, BufReader, Write};
    use tempfile::tempdir;

    fn rows(snapshot: &Snapshot) -> Vec<Value> {
        let log = fs::File::open(snapshot.host_log.as_ref().unwrap()).unwrap();
        BufReader::new(log)
            .lines()
            .map(|line| serde_json::from_str(&line.unwrap()).unwrap())
            .collect()
    }

    #[test]
    fn late_snapshot_replays_actual_transitions_without_duplicating_file_events() {
        let temp = tempdir().unwrap();
        let (startup, mut raw) = Startup::new("日本語の集計", Some(temp.path()));
        startup.child_spawned(354);
        startup.advance(Event::Load);
        startup.advance(Event::Http);
        startup.http_response(200);
        startup.advance(Event::Open);
        for _ in 0..2 {
            let snapshot = startup.snapshot();
            let snapshot_json = serde_json::to_value(&snapshot).unwrap();
            assert_eq!(snapshot_json["title"], "日本語の集計");
            assert_eq!(
                snapshot_json["events"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|line| line["event"].as_str().unwrap())
                    .collect::<Vec<_>>(),
                ["prepare", "r", "load", "http", "open"]
            );
            let logs = rows(&snapshot);
            assert_eq!(
                logs.iter()
                    .map(|line| line["event"].as_str().unwrap())
                    .collect::<Vec<_>>(),
                ["prepare", "r", "load", "http", "http_response", "open"]
            );
            assert!(logs.iter().all(|row| row["host_pid"] == std::process::id()
                && row["level"] == "info"
                && row["timestamp_ms"].as_u64().unwrap() > 1_600_000_000_000));
            assert!(
                logs.windows(2)
                    .all(|pair| pair[1]["elapsed_ms"].as_u64() >= pair[0]["elapsed_ms"].as_u64())
            );
            assert!(logs[0].get("r_pid").is_none());
            assert_eq!(logs[1]["r_pid"], 354);
            assert_eq!(logs[4]["http_status"], 200);
            assert_eq!(logs[5]["r_pid"], 354);
        }
        let snapshot = startup.snapshot();
        raw.as_mut()
            .unwrap()
            .write_all(b"R stdout fixture\n")
            .unwrap();
        assert_eq!(
            fs::read_to_string(snapshot.r_log.unwrap()).unwrap(),
            "R stdout fixture\n"
        );
    }

    #[test]
    fn failure_detail_is_one_json_record_and_only_in_the_host_log() {
        let temp = tempdir().unwrap();
        let (startup, _) = Startup::new("app", Some(temp.path()));
        let detail = "R \"quoted\"\nC:\\Sales\\顧客";
        startup.info("fixture_checkpoint", None);
        let snapshot = startup.fail(detail);
        assert_eq!(snapshot.events.last().unwrap().event, Event::Failed);
        assert_eq!(
            snapshot.elapsed_ms,
            snapshot.events.last().unwrap().elapsed_ms
        );
        assert!(
            snapshot
                .error
                .as_ref()
                .is_some_and(|explanation| explanation.contains("R"))
        );
        let callback = snapshot.guarded_script("http://tauri.localhost").unwrap();
        assert!(!callback.contains("顧客") && !callback.contains("fixture_checkpoint"));
        let logs = rows(&snapshot);
        assert_eq!(logs.len(), 3);
        assert_eq!(logs[1]["event"], "fixture_checkpoint");
        assert_eq!(logs[2]["detail"], detail);
        assert_eq!(logs[2]["level"], "error");
        assert_eq!(startup.advance(Event::Load).events.len(), 2);
    }

    #[test]
    fn unavailable_log_directory_still_reports_failure_safely() {
        let temp = tempdir().unwrap();
        let file = temp.path().join("occupied");
        fs::write(&file, "untouched").unwrap();
        let (startup, raw) = Startup::new("app", Some(&file));
        assert!(raw.is_none());
        startup.advance(Event::Load);
        let snapshot = startup.fail("private details");
        assert!(snapshot.host_log.is_none() && snapshot.r_log.is_none());
        assert!(
            snapshot
                .error
                .as_ref()
                .is_some_and(|text| !text.is_empty() && !text.contains("private"))
        );
        assert_eq!(fs::read_to_string(&file).unwrap(), "untouched");
    }

    #[test]
    fn shutdown_after_navigation_is_distinct_from_startup_failure() {
        let temp = tempdir().unwrap();
        let (startup, _) = Startup::new("app", Some(temp.path()));
        startup.advance(Event::Open);
        let snapshot = startup.stop("R exited");
        assert_eq!(snapshot.events.last().unwrap().event, Event::Stopped);
        assert!(
            snapshot
                .error
                .as_ref()
                .is_some_and(|text| !text.is_empty() && !text.contains("R exited"))
        );
        assert_eq!(rows(&snapshot).last().unwrap()["event"], "stopped");
    }

    #[test]
    fn log_name_collisions_preserve_prior_bytes() {
        let temp = tempdir().unwrap();
        let (mut file, first) = open_log(temp.path(), "host-1-123", "jsonl").unwrap();
        file.write_all(b"keep").unwrap();
        let (_, second) = open_log(temp.path(), "host-1-123", "jsonl").unwrap();
        assert_ne!(first, second);
        assert_eq!(fs::read_to_string(first).unwrap(), "keep");
    }
}
