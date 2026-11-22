//! Read standard NSIS controls and capture the pixels drawn by the desktop.
use anyhow::{Context, Result, bail, ensure};
use std::{
    fs::File,
    io,
    path::Path,
    ptr, slice, thread,
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    Foundation::{ERROR_INVALID_WINDOW_HANDLE, GetLastError, HWND, LPARAM, RECT, SetLastError},
    Graphics::Gdi::*,
    UI::WindowsAndMessaging::*,
};

pub fn dpi_aware() {
    // GetWindowRect and the desktop bitmap must use the same physical pixels.
    unsafe {
        SetProcessDPIAware();
    }
}

pub fn enum_windows(parent: Option<HWND>) -> Vec<HWND> {
    unsafe extern "system" fn collect(hwnd: HWND, data: LPARAM) -> i32 {
        unsafe {
            (*(data as *mut Vec<HWND>)).push(hwnd);
        }
        1
    }
    let mut windows = Vec::new();
    let data = &mut windows as *mut Vec<HWND> as LPARAM;
    unsafe {
        match parent {
            Some(parent) => EnumChildWindows(parent, Some(collect), data),
            None => EnumWindows(Some(collect), data),
        };
    }
    windows
}

pub fn pid(hwnd: HWND) -> u32 {
    let mut id = 0;
    unsafe {
        GetWindowThreadProcessId(hwnd, &mut id);
    }
    id
}

pub fn class_name(hwnd: HWND) -> Result<String> {
    let mut text = [0; 256];
    unsafe {
        SetLastError(0);
    }
    let n = unsafe { GetClassNameW(hwnd, text.as_mut_ptr(), text.len() as i32) };
    if n == 0 {
        let mut error = io::Error::last_os_error();
        // Wine can return zero without setting last-error when the HWND has
        // already disappeared. Confirm its absence before retrying.
        if error.raw_os_error() == Some(0) && unsafe { IsWindow(hwnd) } == 0 {
            error = io::Error::from_raw_os_error(ERROR_INVALID_WINDOW_HANDLE as i32);
        }
        return Err(error).context("GetClassNameW");
    }
    Ok(String::from_utf16_lossy(&text[..n as usize]))
}

pub fn caption(hwnd: HWND) -> Result<String> {
    // A standard Button caption can be read without messaging a busy installer.
    let mut text = [0; 512];
    unsafe {
        SetLastError(0);
    }
    let n = unsafe { GetWindowTextW(hwnd, text.as_mut_ptr(), text.len() as i32) };
    if n == 0 && unsafe { GetLastError() } != 0 {
        return Err(io::Error::last_os_error()).context("GetWindowTextW");
    }
    Ok(String::from_utf16_lossy(&text[..n as usize]).replace('&', ""))
}

pub fn click(hwnd: HWND) -> Result<()> {
    // Setup waits for its child uninstaller here. Keep polling both windows.
    if unsafe { PostMessageW(hwnd, BM_CLICK, 0, 0) } == 0 {
        return Err(io::Error::last_os_error()).context("post button click");
    }
    Ok(())
}

pub fn is_transient(error: &anyhow::Error) -> bool {
    // The wizard destroys its controls during page changes and on exit.
    matches!(
        error
            .downcast_ref::<io::Error>()
            .and_then(io::Error::raw_os_error),
        Some(1400 | 1421)
    )
}

fn window_rect(hwnd: HWND) -> Result<RECT> {
    let mut bounds = RECT::default();
    if unsafe { GetWindowRect(hwnd, &mut bounds) } == 0 {
        return Err(io::Error::last_os_error()).context("GetWindowRect");
    }
    Ok(bounds)
}

struct DesktopBitmap {
    desktop: HDC,
    memory: HDC,
    bitmap: HBITMAP,
    previous: HGDIOBJ,
}

impl Drop for DesktopBitmap {
    fn drop(&mut self) {
        unsafe {
            if !self.previous.is_null() {
                SelectObject(self.memory, self.previous);
            }
            if !self.bitmap.is_null() {
                DeleteObject(self.bitmap);
            }
            if !self.memory.is_null() {
                DeleteDC(self.memory);
            }
            if !self.desktop.is_null() {
                ReleaseDC(ptr::null_mut(), self.desktop);
            }
        }
    }
}

struct Frame {
    width: usize,
    height: usize,
    rgb: Vec<u8>,
}

impl Frame {
    fn grab(rect: RECT) -> Result<Self> {
        let width = rect.right - rect.left;
        let height = rect.bottom - rect.top;
        ensure!(width > 0 && height > 0, "empty wizard window");
        let mut dc = DesktopBitmap {
            desktop: unsafe { GetDC(ptr::null_mut()) },
            memory: ptr::null_mut(),
            bitmap: ptr::null_mut(),
            previous: ptr::null_mut(),
        };
        ensure!(!dc.desktop.is_null(), "could not get the desktop DC");
        dc.memory = unsafe { CreateCompatibleDC(dc.desktop) };
        ensure!(!dc.memory.is_null(), "could not create the capture DC");
        // Top-down BGRX rows. Binding a DIB lets us read pixels directly.
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width,
                biHeight: -height,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut pixels = ptr::null_mut();
        dc.bitmap = unsafe {
            CreateDIBSection(
                dc.desktop,
                &info,
                DIB_RGB_COLORS,
                &mut pixels,
                ptr::null_mut(),
                0,
            )
        };
        ensure!(
            !dc.bitmap.is_null() && !pixels.is_null(),
            "could not create the desktop bitmap"
        );
        dc.previous = unsafe { SelectObject(dc.memory, dc.bitmap) };
        ensure!(
            !dc.previous.is_null(),
            "could not select the desktop bitmap"
        );
        if unsafe {
            BitBlt(
                dc.memory,
                0,
                0,
                width,
                height,
                dc.desktop,
                rect.left,
                rect.top,
                SRCCOPY | CAPTUREBLT,
            )
        } == 0
        {
            return Err(io::Error::last_os_error()).context("copy the desktop");
        }
        ensure!(
            unsafe { GdiFlush() } != 0,
            "could not flush the GDI capture"
        );
        let (width, height) = (width as usize, height as usize);
        let bgrx = unsafe { slice::from_raw_parts(pixels.cast::<u8>(), width * height * 4) };
        let mut rgb = Vec::with_capacity(width * height * 3);
        for &[blue, green, red, _] in bgrx.as_chunks::<4>().0 {
            rgb.extend_from_slice(&[red, green, blue]);
        }
        Ok(Self { width, height, rgb })
    }

    fn painted(&self, left: i32, top: i32, right: i32, bottom: i32) -> bool {
        let x0 = left.clamp(0, self.width as i32) as usize;
        let x1 = right.clamp(0, self.width as i32) as usize;
        let mut darkest = [u8::MAX; 3];
        let mut lightest = [0; 3];
        if x0 >= x1 {
            return false;
        }
        for y in top.max(0)..bottom.min(self.height as i32) {
            let row = y as usize * self.width * 3;
            for px in self.rgb[row + x0 * 3..row + x1 * 3].as_chunks::<3>().0 {
                for c in 0..3 {
                    darkest[c] = darkest[c].min(px[c]);
                    lightest[c] = lightest[c].max(px[c]);
                    if lightest[c] - darkest[c] > 32 {
                        return true;
                    }
                }
            }
        }
        false
    }

    fn save(&self, path: &Path) -> Result<()> {
        let mut encoder = png::Encoder::new(
            io::BufWriter::new(File::create(path)?),
            self.width as u32,
            self.height as u32,
        );
        encoder.set_color(png::ColorType::Rgb);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header()?;
        writer.write_image_data(&self.rgb)?;
        writer.finish()?;
        Ok(())
    }
}

pub fn capture(dialog: HWND, label: Option<HWND>, path: &Path) -> Result<()> {
    let deadline = Instant::now() + Duration::from_secs(8);
    unsafe {
        SetForegroundWindow(dialog);
    }
    loop {
        unsafe {
            RedrawWindow(
                dialog,
                ptr::null(),
                ptr::null_mut(),
                RDW_UPDATENOW | RDW_ALLCHILDREN | RDW_INVALIDATE,
            );
        }
        let rect = window_rect(dialog)?;
        let frame = Frame::grab(rect)?;
        let painted = match label {
            None => true,
            Some(label) => {
                // Skip the checkbox/radio glyph and the outer border. A control
                // can report its caption while the desktop is still blank.
                let field = window_rect(label)?;
                frame.painted(
                    field.left - rect.left + field.bottom - field.top,
                    field.top - rect.top + 2,
                    field.right - rect.left - 2,
                    field.bottom - rect.top - 2,
                )
            }
        };
        if painted {
            return frame
                .save(path)
                .with_context(|| format!("write {}", path.display()));
        }
        if Instant::now() >= deadline {
            bail!(
                "NSIS did not paint the selected control for {}",
                path.display()
            );
        }
        thread::sleep(Duration::from_millis(100));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn screenshot_checks_the_label_and_individual_color_channels() {
        let frame = Frame {
            width: 3,
            height: 1,
            rgb: vec![255, 255, 255, 255, 0, 0, 0, 130, 0],
        };
        assert!(!frame.painted(1, 0, 2, 1));
        assert!(frame.painted(1, 0, 3, 1)); // Same luminance, different colors.
        assert!(!frame.painted(3, 0, 20, 1));
    }

    #[test]
    fn observing_a_destroyed_button_is_retryable() -> Result<()> {
        let handle = unsafe {
            CreateWindowExW(
                0,
                windows_sys::w!("Button"),
                windows_sys::w!("&Next"),
                WS_POPUP,
                0,
                0,
                20,
                20,
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null(),
            )
        };
        ensure!(
            !handle.is_null(),
            "create test button: {}",
            io::Error::last_os_error()
        );
        let observed = caption(handle);
        ensure!(unsafe { DestroyWindow(handle) } != 0, "destroy test button");
        assert_eq!(observed?, "Next");
        let error = class_name(handle).unwrap_err();
        assert!(is_transient(&error), "{error:#}");
        Ok(())
    }

    #[test]
    fn only_disappearing_controls_are_retried() {
        for code in [1400, 1421] {
            assert!(is_transient(
                &anyhow::Error::new(io::Error::from_raw_os_error(code)).context("wizard")
            ));
        }
        assert!(!is_transient(&io::Error::from_raw_os_error(5).into()));
    }
}
