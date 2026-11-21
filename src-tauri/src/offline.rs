use std::{
    fs::{self, File, OpenOptions},
    io::{self, Write},
    os::windows::{
        io::{AsRawHandle, FromRawHandle, OwnedHandle},
        process::CommandExt,
    },
    path::Path,
    process::{Command, Stdio},
    ptr, thread,
    time::{Duration, Instant},
};
use tauri::{Manager, Url, WebviewWindow};
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

fn serve(window: &WebviewWindow, payload: &Path, log: &File) -> Result<()> {
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
        .stdout(log.try_clone()?)
        .stderr(log.try_clone()?)
        .spawn()?;
    // The entry script waits on stdin before it can start any workers.
    if unsafe { AssignProcessToJobObject(job.as_raw_handle(), child.as_raw_handle()) } == 0 {
        let error = io::Error::last_os_error();
        let _ = child.kill();
        let _ = child.wait();
        return Err(error.into());
    }
    child
        .stdin
        .take()
        .expect("piped stdin")
        .write_all(b"start\n")?;

    let deadline = Instant::now() + Duration::from_secs(90);
    loop {
        if let Some(status) = child.try_wait()? {
            return Err(format!("R exited during startup ({status})").into());
        }
        match fs::read_to_string(&ready) {
            Ok(address) => {
                // Shiny publishes this address after successfully binding the socket.
                let port: u16 = address
                    .trim()
                    .strip_prefix("http://127.0.0.1:")
                    .ok_or("unexpected Shiny address")?
                    .parse()?;
                window.navigate(Url::parse(&format!("http://127.0.0.1:{port}/"))?)?;
                break;
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
        if Instant::now() >= deadline {
            return Err("Shiny startup timed out".into());
        }
        thread::sleep(Duration::from_millis(100));
    }
    // The ready file is no longer needed. Keep the job handle until R or the host exits.
    drop(scratch);
    Err(format!("R server stopped ({})", child.wait()?).into())
}

pub fn start(app: &mut tauri::App) -> Result<()> {
    let window = app
        .get_webview_window("main")
        .ok_or("missing main window")?;
    let payload = app.path().resource_dir()?.join("offline");
    let logs = app.path().app_log_dir()?;
    fs::create_dir_all(&logs)?;
    let path = logs.join(format!("shiny-{}.log", std::process::id()));
    let mut log = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(&path)?;
    thread::spawn(move || {
        if let Err(error) = serve(&window, &payload, &log) {
            let _ = writeln!(log, "{error}");
            let _ = window.set_title(&format!(
                "Shiny アプリを起動・実行できませんでした — {}",
                path.display()
            ));
            let _ = window.eval("document.body.textContent = 'Shiny アプリが停止しました。ウィンドウタイトルのログを確認してください。'");
        }
    });
    Ok(())
}
