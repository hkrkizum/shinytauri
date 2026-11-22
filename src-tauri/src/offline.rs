use crate::{
    readiness,
    startup::{Event, Snapshot, Startup},
};
use std::{
    fs::File,
    io::{self, Write},
    os::windows::{
        io::{AsRawHandle, FromRawHandle, OwnedHandle},
        process::CommandExt,
    },
    path::Path,
    process::{Child, Command, ExitStatus, Stdio},
    ptr,
    sync::Arc,
    thread,
    time::{Duration, Instant},
};
use tauri::{
    Manager, Url, Webview, WebviewWindow,
    webview::{PageLoadEvent, PageLoadPayload},
};
use windows_sys::Win32::System::{
    JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
        SetInformationJobObject,
    },
    Threading::CREATE_NO_WINDOW,
};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

// Windows closes this non-inheritable handle even if the host crashes. The job
// then terminates the R server and every mirai worker that inherited membership.
fn process_job() -> io::Result<OwnedHandle> {
    let raw = unsafe { CreateJobObjectW(ptr::null(), ptr::null()) };
    if raw.is_null() {
        return Err(io::Error::last_os_error());
    }
    let job = unsafe { OwnedHandle::from_raw_handle(raw) };
    let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    let ok = unsafe {
        SetInformationJobObject(
            raw,
            JobObjectExtendedLimitInformation,
            (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
            std::mem::size_of_val(&limits) as u32,
        )
    };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(job)
}

struct Host {
    startup: Startup,
    page: Url,
}

impl Host {
    fn is_startup_page(&self, url: &Url) -> bool {
        self.page.origin() == url.origin() && matches!(url.path(), "/" | "/index.html")
    }

    fn render(&self, view: &Webview, snapshot: &Snapshot) {
        // Recheck the document inside the WebView: a navigation may have won
        // the race after the host checked the current page.
        if let Ok(script) = snapshot.guarded_script(&self.page.origin().ascii_serialization()) {
            let _ = view.eval(script);
        }
    }

    fn publish(&self, window: &WebviewWindow, snapshot: Snapshot) {
        if window.url().is_ok_and(|url| self.is_startup_page(&url)) {
            self.render(window.as_ref(), &snapshot);
        }
    }
}

pub fn page_loaded(view: &Webview, payload: &PageLoadPayload<'_>) {
    if payload.event() == PageLoadEvent::Finished
        && view.label() == "main"
        && let Some(host) = view.try_state::<Arc<Host>>()
        && host.is_startup_page(payload.url())
    {
        host.render(view, &host.startup.snapshot());
    }
}

fn assign_child(job: &OwnedHandle, child: &mut Child) -> io::Result<()> {
    // The entry script waits on stdin before it can start any workers.
    if unsafe { AssignProcessToJobObject(job.as_raw_handle(), child.as_raw_handle()) } == 0 {
        let error = io::Error::last_os_error();
        let _ = child.kill();
        let _ = child.wait();
        return Err(error);
    }
    Ok(())
}

fn serve(
    host: &Host,
    window: &WebviewWindow,
    payload: &Path,
    log: Option<File>,
) -> Result<ExitStatus> {
    let deadline = Instant::now() + Duration::from_secs(90);
    let scratch = tempfile::tempdir()?;
    let ready = scratch.path().join("ready");
    let home = payload.join("R");
    let library = payload.join("library");
    let bin = home.join("bin/x64");
    let job = process_job()?;
    let mut command = Command::new(bin.join("Rterm.exe"));
    let quarto = crate::quarto::executable(payload);
    if quarto.is_file() {
        command.env("SHINYTAURI_QUARTO", quarto);
    } else {
        command.env_remove("SHINYTAURI_QUARTO");
    }
    // Log storage can be unavailable. Keep running and retain the in-memory
    // diagnostics; a child must never block on an unread output pipe.
    let stderr = log.as_ref().and_then(|f| f.try_clone().ok());
    let mut child = command
        .args(["--vanilla", "--slave", "-f"])
        .arg(payload.join("serve-bundled.R"))
        .current_dir(payload.join("app"))
        .env("R_HOME", &home)
        .env("R_LIBS", &library)
        .env("R_LIBS_USER", &library)
        .env("R_LIBS_SITE", &library)
        .env(
            "R_DEFAULT_PACKAGES",
            "datasets,utils,grDevices,graphics,stats,methods",
        )
        .env("R_PROFILE_USER", "NUL")
        .env("R_ENVIRON_USER", "NUL")
        .env("SHINYTAURI_READY_FILE", &ready)
        .creation_flags(CREATE_NO_WINDOW)
        .stdin(Stdio::piped())
        .stdout(log.map_or_else(Stdio::null, Stdio::from))
        .stderr(stderr.map_or_else(Stdio::null, Stdio::from))
        .spawn()?;
    assign_child(&job, &mut child)?;
    host.publish(window, host.startup.child_spawned(child.id()));
    child
        .stdin
        .take()
        .ok_or("missing R stdin pipe")?
        .write_all(b"start\n")?;
    host.publish(window, host.startup.advance(Event::Load));

    let port = readiness::wait_for_port(&ready, &mut child, deadline)?;
    host.publish(window, host.startup.advance(Event::Http));
    let status = readiness::wait_for_http(port, &mut child, deadline)?;
    host.startup.http_response(status);

    host.publish(window, host.startup.advance(Event::Open));
    window.navigate(Url::parse(&format!("http://127.0.0.1:{port}/"))?)?;
    // Keep the job handle until R or the desktop host exits.
    drop(scratch);
    Ok(child.wait()?)
}

pub fn start(app: &mut tauri::App) -> Result<()> {
    let window = app
        .get_webview_window("main")
        .ok_or("missing main window")?;
    let config = app
        .config()
        .app
        .windows
        .iter()
        .find(|config| config.label == window.label())
        .ok_or("missing main window configuration")?;
    // Tauri serves its built-in protocol through this origin in WebView2.
    // Respect useHttpsScheme when returning from a failed Shiny session.
    let page = Url::parse(if config.use_https_scheme {
        "https://tauri.localhost/"
    } else {
        "http://tauri.localhost/"
    })?;
    let logs = app.path().app_log_dir().ok();
    let (startup, log) = Startup::new(&config.title, logs.as_deref());
    let host = Arc::new(Host { startup, page });
    app.manage(Arc::clone(&host));
    let payload = app.path().resource_dir();
    thread::spawn(move || {
        // If the splash finished loading before setup, it may have missed the
        // page-load event. The same guard also tolerates an uninitialized page.
        host.publish(&window, host.startup.snapshot());
        let result = payload
            .map_err(Box::<dyn std::error::Error>::from)
            .and_then(|dir| serve(&host, &window, &dir.join("offline"), log));
        let snapshot = match result {
            Ok(status) => host.startup.stop(&format!("R server stopped ({status})")),
            Err(error) => host.startup.fail(&error.to_string()),
        };
        // navigate() may still be in flight when R exits, even if url() still
        // points to the splash. A fresh local navigation wins that race and
        // page_loaded replays the full terminal snapshot once the DOM exists.
        host.publish(&window, snapshot);
        if let Err(error) = window.navigate(host.page.clone()) {
            host.startup
                .info("diagnostic_navigation_failed", Some(&error.to_string()));
            let _ = window.set_title("Shiny アプリが停止しました");
        }
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closing_the_job_terminates_the_blocked_child() {
        let job = process_job().unwrap();
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args(["--ignored", "--exact", "readiness::tests::hold_child"])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .spawn()
            .unwrap();
        assign_child(&job, &mut child).unwrap();
        assert!(child.try_wait().unwrap().is_none());
        drop(job);
        let deadline = Instant::now() + Duration::from_secs(5);
        while child.try_wait().unwrap().is_none() {
            if Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                panic!("R-like child survived closed job handle");
            }
            thread::sleep(Duration::from_millis(30));
        }
    }
}
