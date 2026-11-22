//! Drive Tauri's standard interactive NSIS upgrade with the defaults visible to users.
use crate::{processes, winui};
use anyhow::{Context, Result, ensure};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    io,
    path::Path,
    process::{Child, Command},
    thread,
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    Foundation::HWND,
    UI::{Input::KeyboardAndMouse::IsWindowEnabled, WindowsAndMessaging::*},
};

#[derive(Default)]
struct Wizard {
    saw_previous_version: bool,
    saw_uninstaller: bool,
    previous_page: Option<(HWND, Vec<String>, String)>,
    finishing: HashMap<HWND, FinishPage>,
}

struct FinishPage {
    pid: u32,
    primary: HWND,
    buttons: BTreeMap<String, HWND>,
    posted: Instant,
    retries: u8,
}

fn check_button(button: HWND, message: u32) -> Result<usize> {
    let mut state = 0;
    if unsafe { SendMessageTimeoutW(button, message, 0, 0, SMTO_ABORTIFHUNG, 2000, &mut state) }
        == 0
    {
        return Err(io::Error::last_os_error()).context("read or clear NSIS checkbox");
    }
    Ok(state)
}

fn visible_buttons(dialog: HWND) -> Result<BTreeMap<String, HWND>> {
    let mut buttons = BTreeMap::new();
    for control in winui::enum_windows(Some(dialog)) {
        if unsafe { IsWindowVisible(control) } != 0 && winui::class_name(control)? == "Button" {
            buttons.insert(winui::caption(control)?, control);
        }
    }
    Ok(buttons)
}

impl Wizard {
    fn visit(&mut self, dialog: HWND, evidence: &Path) -> Result<()> {
        if unsafe { IsWindowVisible(dialog) == 0 || IsWindowEnabled(dialog) == 0 }
            || winui::class_name(dialog)? != "#32770"
        {
            return Ok(());
        }
        let primary = unsafe { GetDlgItem(dialog, IDOK) };
        if primary.is_null() || unsafe { IsWindowEnabled(primary) } == 0 {
            return Ok(());
        }

        if self.wait_for_finish(dialog, primary, Instant::now())? {
            return Ok(());
        }

        let buttons = visible_buttons(dialog)?;
        if !self.saw_previous_version
            && let Some(&choice) = buttons.get("Uninstall before installing")
        {
            ensure!(
                check_button(choice, BM_GETCHECK)? == 1,
                "old-version uninstall must be selected"
            );
            winui::capture(dialog, Some(choice), &evidence.join("upgrade-choice.png"))?;
            self.saw_previous_version = true;
        }
        if !self.saw_uninstaller
            && let Some(&choice) = buttons.get("Delete the application data")
        {
            ensure!(
                check_button(choice, BM_GETCHECK)? == 0,
                "application data deletion must be unchecked"
            );
            winui::capture(
                dialog,
                Some(choice),
                &evidence.join("upgrade-keep-data.png"),
            )?;
            self.saw_uninstaller = true;
        }
        self.advance(dialog, primary, &buttons)
    }

    fn advance(
        &mut self,
        dialog: HWND,
        primary: HWND,
        buttons: &BTreeMap<String, HWND>,
    ) -> Result<()> {
        let caption = winui::caption(primary)?;
        if buttons.get(&caption) != Some(&primary) {
            return Ok(());
        }
        let finish = caption.trim() == "Finish";
        let page = (dialog, buttons.keys().cloned().collect(), caption);
        if self.previous_page.as_ref() == Some(&page) {
            return Ok(());
        }
        if finish {
            // Configure the final page once. The offline WebView test will
            // launch the application after the installer has exited.
            for (text, &control) in buttons {
                if text.starts_with("Run shinytauri-smoke-minimal") {
                    check_button(control, BM_SETCHECK)?;
                }
            }
        }
        println!("{} {}", winui::caption(dialog)?, page.2);
        winui::click(primary)?;
        self.previous_page = Some(page);
        if finish {
            self.finishing.insert(
                dialog,
                FinishPage {
                    pid: winui::pid(dialog),
                    primary,
                    buttons: buttons.clone(),
                    posted: Instant::now(),
                    retries: 0,
                },
            );
        }
        Ok(())
    }

    // NSIS switches its Finish page to an nsDialogs message loop after
    // creating the controls. A queued BM_CLICK can arrive before that loop
    // handles it. Retry only the very same live, actionable primary button.
    fn wait_for_finish(&mut self, dialog: HWND, primary: HWND, now: Instant) -> Result<bool> {
        let Some(page) = self.finishing.get_mut(&dialog) else {
            return Ok(false);
        };
        if page.pid != winui::pid(dialog) || page.primary != primary {
            self.finishing.remove(&dialog);
            self.previous_page = None;
            return Ok(false);
        }
        if page.retries < 3
            && now.duration_since(page.posted) >= Duration::from_secs(2)
            && unsafe { IsWindowVisible(primary) != 0 && IsWindowEnabled(primary) != 0 }
            && visible_buttons(dialog)? == page.buttons
        {
            // BN_CLICKED is zero. Notify the parent directly, exactly as
            // this button does when clicked, even if its dialog lacks focus.
            if unsafe { PostMessageW(dialog, WM_COMMAND, IDOK as usize, primary as isize) } == 0 {
                return Err(io::Error::last_os_error()).context("notify NSIS Finish");
            }
            page.posted = now;
            page.retries += 1;
            println!(
                "retry NSIS Finish (pid {}, HWND {dialog:?}, attempt {})",
                page.pid, page.retries
            );
        }
        // Closing controls can disappear at any time. Do not touch the Run
        // checkbox again. Other dialogs in this process tree still need service.
        Ok(true)
    }

    fn drive(&mut self, setup: &mut Child, evidence: &Path, timeout: Duration) -> Result<()> {
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(status) = setup.try_wait()? {
                ensure!(status.success(), "NSIS exited with {status}");
                ensure!(
                    self.saw_previous_version && self.saw_uninstaller,
                    "setup did not upgrade the installed product"
                );
                return Ok(());
            }
            let mut owners: HashSet<_> = processes::descendants(setup.id())?.into_iter().collect();
            owners.insert(setup.id());
            let windows = winui::enum_windows(None);
            if Instant::now() >= deadline {
                // Capture current windows; the last button we posted need not
                // belong to the dialog that is blocking the installer now.
                for window in &windows {
                    let window = *window;
                    let pid = winui::pid(window);
                    if owners.contains(&pid) && unsafe { IsWindowVisible(window) } != 0 {
                        let primary = unsafe { GetDlgItem(window, IDOK) };
                        eprintln!(
                            "remaining HWND {window:?} pid={pid} title={:?} enabled={} primary={primary:?} caption={:?} enabled={}",
                            winui::caption(window),
                            unsafe { IsWindowEnabled(window) },
                            winui::caption(primary),
                            unsafe { IsWindowEnabled(primary) },
                        );
                        let path = evidence
                            .join(format!("upgrade-timeout-{pid}-{:x}.png", window as usize));
                        if let Err(error) = winui::capture(window, None, &path) {
                            eprintln!("cannot capture {}: {error:#}", path.display());
                        }
                    }
                }
                anyhow::bail!(
                    "NSIS timed out on {:?}; remaining PIDs {owners:?}",
                    self.previous_page
                );
            }
            for window in windows {
                if owners.contains(&winui::pid(window)) {
                    match self.visit(window, evidence) {
                        Err(error) if winui::is_transient(&error) => continue,
                        result => result?,
                    }
                }
            }
            thread::sleep(Duration::from_millis(200));
        }
    }
}

pub fn run(setup: &Path, evidence: &Path) -> Result<()> {
    winui::dpi_aware();
    let mut child = Command::new(setup)
        .spawn()
        .with_context(|| format!("launch {}", setup.display()))?;
    let result = Wizard::default().drive(&mut child, evidence, Duration::from_secs(240));
    if result.is_err() && child.try_wait()?.is_none() {
        processes::terminate_tree(&mut child)?;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ptr;

    struct Button(HWND);
    impl Button {
        fn new(text: windows_sys::core::PCWSTR, style: u32) -> Result<Self> {
            Self::create(text, WS_POPUP | style, ptr::null_mut())
        }
        fn create(text: windows_sys::core::PCWSTR, style: u32, parent: HWND) -> Result<Self> {
            let hwnd = unsafe {
                CreateWindowExW(
                    0,
                    windows_sys::w!("Button"),
                    text,
                    style,
                    0,
                    0,
                    100,
                    20,
                    parent,
                    ptr::null_mut(),
                    ptr::null_mut(),
                    ptr::null(),
                )
            };
            ensure!(
                !hwnd.is_null(),
                "create test button: {}",
                io::Error::last_os_error()
            );
            Ok(Self(hwnd))
        }
    }
    impl Drop for Button {
        fn drop(&mut self) {
            unsafe {
                DestroyWindow(self.0);
            }
        }
    }

    #[test]
    fn finish_preserves_closing_controls_and_handles_a_new_dialog() -> Result<()> {
        let finish = Button::new(windows_sys::w!("&Finish"), 0)?;
        let launch = Button::new(
            windows_sys::w!("Run shinytauri-smoke-minimal"),
            BS_AUTOCHECKBOX as u32,
        )?;
        let buttons = BTreeMap::from([
            ("Finish".into(), finish.0),
            ("Run shinytauri-smoke-minimal".into(), launch.0),
        ]);
        unsafe {
            SendMessageW(launch.0, BM_SETCHECK, 1, 0);
        }
        let mut wizard = Wizard::default();
        wizard.advance(finish.0, finish.0, &buttons)?;
        assert_eq!(check_button(launch.0, BM_GETCHECK)?, 0);
        // NSIS can change the apparent page as its controls get dismantled.
        unsafe {
            SendMessageW(launch.0, BM_SETCHECK, 1, 0);
        }
        assert!(wizard.wait_for_finish(finish.0, finish.0, Instant::now())?);
        assert_eq!(check_button(launch.0, BM_GETCHECK)?, 1);
        let other = Button::new(windows_sys::w!("&Close"), 0)?;
        assert!(!wizard.wait_for_finish(other.0, other.0, Instant::now())?);
        wizard.advance(
            other.0,
            other.0,
            &BTreeMap::from([("Close".into(), other.0)]),
        )?;
        let mut other_click = MSG::default();
        assert_ne!(
            unsafe { PeekMessageW(&mut other_click, other.0, BM_CLICK, BM_CLICK, PM_REMOVE) },
            0
        );
        let mut message = MSG::default();
        assert_ne!(
            unsafe { PeekMessageW(&mut message, finish.0, BM_CLICK, BM_CLICK, PM_REMOVE) },
            0
        );
        assert_eq!(
            unsafe { PeekMessageW(&mut message, finish.0, BM_CLICK, BM_CLICK, PM_REMOVE) },
            0
        );
        Ok(())
    }
    #[test]
    fn unhandled_finish_retries_the_original_visible_button_using_parent_command() -> Result<()> {
        let dialog = Button::new(windows_sys::w!("Setup"), WS_VISIBLE)?;
        let finish = Button::create(windows_sys::w!("&Finish"), WS_CHILD | WS_VISIBLE, dialog.0)?;
        let run = Button::create(
            windows_sys::w!("Run shinytauri-smoke-minimal"),
            WS_CHILD | WS_VISIBLE,
            dialog.0,
        )?;
        let buttons = visible_buttons(dialog.0)?;
        let mut wizard = Wizard::default();
        wizard.advance(dialog.0, finish.0, &buttons)?;
        let mut msg = MSG::default();
        // Consume the first click without handling it, as a switching page can.
        assert_ne!(
            unsafe { PeekMessageW(&mut msg, finish.0, BM_CLICK, BM_CLICK, PM_REMOVE) },
            0
        );
        let posted = wizard.finishing[&dialog.0].posted;
        assert!(wizard.wait_for_finish(dialog.0, finish.0, posted)?);
        assert_eq!(
            unsafe { PeekMessageW(&mut msg, dialog.0, WM_COMMAND, WM_COMMAND, PM_REMOVE) },
            0
        );
        let retry = posted + Duration::from_secs(3);
        assert!(wizard.wait_for_finish(dialog.0, finish.0, retry)?);
        assert_ne!(
            unsafe { PeekMessageW(&mut msg, dialog.0, WM_COMMAND, WM_COMMAND, PM_REMOVE) },
            0
        );
        assert_eq!((msg.wParam, msg.lParam), (IDOK as usize, finish.0 as isize));
        // Teardown may change a child while Finish itself still looks actionable.
        unsafe {
            SetWindowTextW(run.0, windows_sys::w!("Closing"));
        }
        assert!(wizard.wait_for_finish(dialog.0, finish.0, retry + Duration::from_secs(3))?);
        assert_eq!(
            unsafe { PeekMessageW(&mut msg, dialog.0, WM_COMMAND, WM_COMMAND, PM_REMOVE) },
            0
        );
        // A genuine replacement for the primary button can start a new page.
        assert!(!wizard.wait_for_finish(dialog.0, run.0, retry + Duration::from_secs(4))?);
        Ok(())
    }
}
