//! Drive Tauri's standard interactive NSIS upgrade with the defaults visible to users.
use crate::{processes, winui};
use anyhow::{Context, Result, ensure};
use std::{
    collections::{BTreeMap, HashSet},
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
    finish_posted: bool,
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

        let mut buttons = BTreeMap::new();
        for control in winui::enum_windows(Some(dialog)) {
            if unsafe { IsWindowVisible(control) } != 0 && winui::class_name(control)? == "Button" {
                buttons.insert(winui::caption(control)?, control);
            }
        }
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
        if self.finish_posted || !buttons.values().any(|&hwnd| hwnd == primary) {
            return Ok(());
        }
        let caption = winui::caption(primary)?;
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
        self.finish_posted = finish;
        Ok(())
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
            ensure!(
                Instant::now() < deadline,
                "NSIS timed out on {:?}",
                self.previous_page
            );
            // NSIS briefly keeps its final window while destroying its controls.
            // After clicking Finish, only the actual installer exit matters.
            if self.finish_posted {
                thread::sleep(Duration::from_millis(200));
                continue;
            }
            // The previous version's uninstall dialog belongs to a child process.
            let mut owners: HashSet<_> = processes::descendants(setup.id())?.into_iter().collect();
            owners.insert(setup.id());
            for window in winui::enum_windows(None) {
                if self.finish_posted {
                    break;
                }
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
            let hwnd = unsafe {
                CreateWindowExW(
                    0,
                    windows_sys::w!("Button"),
                    text,
                    WS_POPUP | style,
                    0,
                    0,
                    100,
                    20,
                    ptr::null_mut(),
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
    fn finish_is_clicked_once_and_its_closing_controls_are_left_alone() -> Result<()> {
        let finish = Button::new(windows_sys::w!("&Finish"), 0)?;
        let launch = Button::new(
            windows_sys::w!("Run shinytauri-smoke-minimal"),
            BS_AUTOCHECKBOX as u32,
        )?;
        let mut buttons = BTreeMap::from([
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
        buttons.insert("another closing button".into(), launch.0);
        unsafe {
            SendMessageW(launch.0, BM_SETCHECK, 1, 0);
        }
        wizard.advance(finish.0, finish.0, &buttons)?;
        assert_eq!(check_button(launch.0, BM_GETCHECK)?, 1);
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
}
