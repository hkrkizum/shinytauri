//! Process ownership and normal shutdown checks for the installed Windows apps.
use anyhow::{Context, Result, bail, ensure};
use std::{
    collections::{HashMap, HashSet},
    ffi::{OsStr, OsString},
    io,
    mem::size_of,
    os::windows::{
        ffi::OsStringExt,
        io::{AsRawHandle, FromRawHandle, OwnedHandle},
    },
    path::{Path, PathBuf},
    process::Child,
    ptr::null_mut,
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    Foundation::{
        ERROR_INVALID_PARAMETER, ERROR_NO_MORE_FILES, HANDLE, HWND, INVALID_HANDLE_VALUE, LPARAM,
        WAIT_FAILED, WAIT_OBJECT_0, WAIT_TIMEOUT,
    },
    System::{
        Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
            TH32CS_SNAPPROCESS,
        },
        Threading::{
            OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE, PROCESS_TERMINATE,
            QueryFullProcessImageNameW, TerminateProcess, WaitForSingleObject,
        },
    },
    UI::WindowsAndMessaging::{
        EnumWindows, GetWindowThreadProcessId, IsWindowVisible, PostMessageW, WM_CLOSE,
    },
};

struct Entry {
    pid: u32,
    name: OsString,
}

// Toolhelp snapshots can include unrelated orphan processes as well as browsers,
// R workers and the NSIS child uninstaller. Walk only the requested subtree.
fn child_entries(root: u32) -> Result<Vec<Entry>> {
    let raw = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if raw == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error()).context("snapshot Windows processes");
    }
    let snapshot = unsafe { OwnedHandle::from_raw_handle(raw) };
    let mut process = PROCESSENTRY32W {
        dwSize: size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };
    let mut children = HashMap::<u32, Vec<Entry>>::new();
    let mut found = unsafe { Process32FirstW(snapshot.as_raw_handle(), &mut process) };
    while found != 0 {
        let len = process
            .szExeFile
            .iter()
            .position(|&c| c == 0)
            .unwrap_or(process.szExeFile.len());
        children
            .entry(process.th32ParentProcessID)
            .or_default()
            .push(Entry {
                pid: process.th32ProcessID,
                name: OsString::from_wide(&process.szExeFile[..len]),
            });
        found = unsafe { Process32NextW(snapshot.as_raw_handle(), &mut process) };
    }
    let end = io::Error::last_os_error();
    if end.raw_os_error() != Some(ERROR_NO_MORE_FILES as i32) {
        return Err(end).context("enumerate Windows processes");
    }
    let mut result = Vec::new();
    let mut pending = vec![root];
    let mut seen = HashSet::from([root]);
    while let Some(parent) = pending.pop() {
        for child in children.remove(&parent).unwrap_or_default() {
            if seen.insert(child.pid) {
                pending.push(child.pid);
                result.push(child);
            }
        }
    }
    Ok(result)
}

pub(crate) fn descendants(root: u32) -> Result<Vec<u32>> {
    Ok(child_entries(root)?.into_iter().map(|e| e.pid).collect())
}

fn open(pid: u32, rights: u32) -> Result<Option<OwnedHandle>> {
    let handle = unsafe { OpenProcess(rights, 0, pid) };
    if handle.is_null() {
        let err = io::Error::last_os_error();
        if err.raw_os_error() == Some(ERROR_INVALID_PARAMETER as i32) {
            return Ok(None); // Exited since the snapshot.
        }
        return Err(err).with_context(|| format!("open process {pid}"));
    }
    Ok(Some(unsafe { OwnedHandle::from_raw_handle(handle) }))
}

fn exited(handle: HANDLE, timeout: Duration) -> Result<bool> {
    let ms = timeout.as_millis().min((u32::MAX - 1) as u128) as u32;
    match unsafe { WaitForSingleObject(handle, ms) } {
        WAIT_OBJECT_0 => Ok(true),
        WAIT_TIMEOUT => Ok(false),
        WAIT_FAILED => Err(io::Error::last_os_error()).context("wait for process"),
        status => bail!("unexpected process wait status {status}"),
    }
}

fn image_path(handle: &OwnedHandle) -> Result<PathBuf> {
    let mut text = vec![0; 32768]; // Maximum extended-length Win32 path, in UTF-16 units.
    let mut len = text.len() as u32;
    if unsafe { QueryFullProcessImageNameW(handle.as_raw_handle(), 0, text.as_mut_ptr(), &mut len) }
        == 0
    {
        return Err(io::Error::last_os_error()).context("read process executable path");
    }
    Ok(OsString::from_wide(&text[..len as usize]).into())
}

fn is_r(name: &OsStr) -> bool {
    ["R.exe", "Rterm.exe", "Rscript.exe"]
        .iter()
        .any(|r| name.eq_ignore_ascii_case(r))
}

pub(crate) struct RProcesses(Vec<(u32, OwnedHandle)>);

impl RProcesses {
    pub(crate) fn capture(root: u32, bundled_r: &Path, minimum: usize) -> Result<Self> {
        let bundle = same_file::Handle::from_path(bundled_r)
            .with_context(|| format!("open bundled R directory {}", bundled_r.display()))?;
        let mut processes = Vec::new();
        for entry in child_entries(root)?.into_iter().filter(|e| is_r(&e.name)) {
            let Some(handle) = open(
                entry.pid,
                PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE | PROCESS_TERMINATE,
            )?
            else {
                continue;
            };
            let image = image_path(&handle)?;
            ensure!(
                image.file_name().is_some_and(is_r),
                "R process {} changed before inspection",
                entry.pid
            );
            let resolved = image
                .canonicalize()
                .with_context(|| format!("resolve {}", image.display()))?;
            let mut bundled = false;
            // Compare directory file IDs: Windows can report an 8.3 alias or a
            // different spelling even when both paths refer to the same directory.
            for ancestor in resolved.ancestors().skip(1) {
                if same_file::Handle::from_path(ancestor)? == bundle {
                    bundled = true;
                    break;
                }
            }
            ensure!(
                bundled,
                "R process {} uses {} outside {}",
                entry.pid,
                image.display(),
                bundled_r.display()
            );
            processes.push((entry.pid, handle));
        }
        ensure!(
            processes.len() >= minimum,
            "expected at least {minimum} bundled R processes under host {root}, found {}",
            processes.len()
        );
        Ok(Self(processes))
    }

    pub(crate) fn contains(&self, pid: u32) -> bool {
        self.0.iter().any(|(child, _)| *child == pid)
    }

    pub(crate) fn wait_exited(&self, timeout: Duration) -> Result<()> {
        let start = Instant::now();
        for (pid, handle) in &self.0 {
            ensure!(
                exited(
                    handle.as_raw_handle(),
                    timeout.saturating_sub(start.elapsed())
                )?,
                "R process {pid} did not exit within {timeout:?}"
            );
        }
        Ok(())
    }

    pub(crate) fn terminate_live(&self) -> Result<()> {
        terminate_handles(&self.0)
    }
}

pub(crate) fn close_main_window(host: &mut Child) -> Result<()> {
    struct Search {
        pid: u32,
        window: HWND,
    }
    unsafe extern "system" fn visit(window: HWND, arg: LPARAM) -> i32 {
        let search = unsafe { &mut *(arg as *mut Search) };
        let mut pid = 0;
        unsafe { GetWindowThreadProcessId(window, &mut pid) };
        if pid == search.pid && unsafe { IsWindowVisible(window) } != 0 {
            search.window = window;
            return 0;
        }
        1
    }
    let mut search = Search {
        pid: host.id(),
        window: null_mut(),
    };
    // EnumWindows calls visit synchronously, so the reference remains live.
    unsafe { EnumWindows(Some(visit), &mut search as *mut Search as LPARAM) };
    ensure!(
        !search.window.is_null(),
        "no visible installed app window for host {}",
        host.id()
    );
    if unsafe { PostMessageW(search.window, WM_CLOSE, 0, 0) } == 0 {
        return Err(io::Error::last_os_error()).context("send WM_CLOSE to installed app");
    }
    ensure!(
        exited(host.as_raw_handle(), Duration::from_secs(20))?,
        "host {} did not exit on WM_CLOSE",
        host.id()
    );
    let status = host.wait()?;
    ensure!(
        status.success(),
        "installed app exited with {status} on WM_CLOSE"
    );
    Ok(())
}

/// Error-path cleanup for a hung host or installer. Open handles before
/// terminating the root, so that subsequent waits/termination never act on
/// a different process that reused one of these PIDs.
pub(crate) fn terminate_tree(root: &mut Child) -> Result<()> {
    let mut handles = Vec::new();
    let mut error = None;
    match descendants(root.id()) {
        Ok(ids) => {
            for pid in ids {
                match open(pid, PROCESS_SYNCHRONIZE | PROCESS_TERMINATE) {
                    Ok(Some(handle)) => handles.push((pid, handle)),
                    Ok(None) => {}
                    Err(err) => {
                        error.get_or_insert(err);
                    }
                }
            }
        }
        Err(err) => {
            error = Some(err);
        }
    }
    // A PID could have changed owners between the initial snapshot and
    // OpenProcess. Recheck the tree now that all candidate handles are pinned.
    if !handles.is_empty() {
        match descendants(root.id()) {
            Ok(current) => {
                let current: HashSet<_> = current.into_iter().collect();
                handles.retain(|(pid, _)| current.contains(pid));
            }
            Err(err) => {
                handles.clear();
                error.get_or_insert(err);
            }
        }
    }
    if root.try_wait()?.is_none()
        && let Err(err) = root.kill()
    {
        error.get_or_insert(err.into());
    }
    if let Err(err) = terminate_handles(&handles) {
        error.get_or_insert(err);
    }
    ensure!(
        exited(root.as_raw_handle(), Duration::from_secs(10))?,
        "host {} survived cleanup",
        root.id()
    );
    root.wait()?;
    error.map_or(Ok(()), Err)
}

fn terminate_handles(handles: &[(u32, OwnedHandle)]) -> Result<()> {
    let mut error = None;
    for (pid, handle) in handles.iter().rev() {
        if !exited(handle.as_raw_handle(), Duration::ZERO)?
            && unsafe { TerminateProcess(handle.as_raw_handle(), 1) } == 0
        {
            let failure = io::Error::last_os_error();
            if !exited(handle.as_raw_handle(), Duration::ZERO)? {
                error.get_or_insert(anyhow::anyhow!("terminate child {pid}: {failure}"));
            }
        }
    }
    let start = Instant::now();
    for (pid, handle) in handles {
        let remaining = Duration::from_secs(10).saturating_sub(start.elapsed());
        ensure!(
            exited(handle.as_raw_handle(), remaining)?,
            "process {pid} survived cleanup"
        );
    }
    error.map_or(Ok(()), Err)
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        env, fs, io,
        process::{Command, Stdio},
    };

    #[test]
    fn validates_image_and_waits_for_the_same_r_process() -> anyhow::Result<()> {
        // Use a private, renamed copy of the test executable as the installed R.
        if env::var_os("SHINYTAURI_TEST_PROCESS_CHILD").is_some() {
            io::stdin().read_line(&mut String::new())?;
            return Ok(());
        }
        let root = tempfile::tempdir()?;
        let bundle = root.path().join("bundled R");
        let elsewhere = root.path().join("different R");
        fs::create_dir_all(&bundle)?;
        fs::create_dir_all(&elsewhere)?;
        let executable = bundle.join("Rterm.exe");
        fs::copy(env::current_exe()?, &executable)?;
        // NTFS can disable 8.3 names; in that case Windows returns the original path.
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::Storage::FileSystem::GetShortPathNameW;
        let long: Vec<u16> = executable.as_os_str().encode_wide().chain([0]).collect();
        let mut short = vec![0; 32768];
        let len =
            unsafe { GetShortPathNameW(long.as_ptr(), short.as_mut_ptr(), short.len() as u32) };
        ensure!(
            len > 0 && (len as usize) < short.len(),
            "read executable's 8.3 alias"
        );
        let mut command = Command::new(OsString::from_wide(&short[..len as usize]));
        command
            .args([
                "--exact",
                "processes::tests::validates_image_and_waits_for_the_same_r_process",
            ])
            .env("SHINYTAURI_TEST_PROCESS_CHILD", "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::null());
        let mut child = command.spawn()?;
        let host = std::process::id();
        let result = (|| -> anyhow::Result<()> {
            assert!(descendants(host)?.contains(&child.id()));
            assert!(RProcesses::capture(host, &elsewhere, 1).is_err());
            let runtimes = RProcesses::capture(host, &bundle, 1)?;
            assert!(runtimes.wait_exited(Duration::ZERO).is_err());
            drop(child.stdin.take());
            assert!(child.wait()?.success());
            runtimes.wait_exited(Duration::from_secs(2))?;

            let mut survivor = command.spawn()?;
            let captured = RProcesses::capture(host, &bundle, 1)?;
            let mut other = command.spawn()?;
            ensure!(
                captured.wait_exited(Duration::ZERO).is_err(),
                "a live R worker passed the exit check"
            );
            captured.terminate_live()?;
            ensure!(
                !survivor.wait()?.success(),
                "the R survivor did not require termination"
            );
            ensure!(
                other.try_wait()?.is_none(),
                "cleanup killed an R process outside its captured handles"
            );
            drop(other.stdin.take());
            ensure!(other.wait()?.success());
            Ok(())
        })();
        let _ = child.kill();
        let _ = child.wait();
        result
    }
}
