use crate::winui;
use anyhow::{Context, Result, bail, ensure};
use std::ffi::{OsStr, c_void};
use std::io;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use std::process::{Child, Command};
use std::ptr;
use std::slice;
use std::thread;
use std::time::{Duration, Instant};
use windows_sys::Win32::Foundation::{FreeLibrary, HMODULE, HWND};
use windows_sys::Win32::Storage::FileSystem::{
    GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW,
};
use windows_sys::Win32::System::LibraryLoader::{
    EnumResourceLanguagesW, EnumResourceNamesW, FindResourceExW, LOAD_LIBRARY_AS_DATAFILE,
    LOAD_LIBRARY_AS_IMAGE_RESOURCE, LoadLibraryExW, LoadResource, LockResource, SizeofResource,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetDlgItem, IDOK, IsWindowVisible, RT_ICON, SMTO_ABORTIFHUNG, SendMessageTimeoutW, WM_GETTEXT,
    WM_GETTEXTLENGTH,
};
use windows_sys::core::{BOOL, PCWSTR};

const ICON: &[u8] = include_bytes!("../../fixtures/branding/製品.ico");
const LICENSE: &str = include_str!("../../fixtures/branding/利用規約.txt");

fn wide(value: &OsStr) -> Vec<u16> {
    value.encode_wide().chain(Some(0)).collect()
}

struct IconComparison<'a> {
    bitmap: &'a [u8],
    matched: bool,
}

unsafe extern "system" fn compare_icon(
    module: HMODULE,
    kind: PCWSTR,
    name: PCWSTR,
    language: u16,
    context: isize,
) -> BOOL {
    // EnumResourceLanguagesW calls this synchronously with its module still loaded.
    let comparison = unsafe { &mut *(context as *mut IconComparison<'_>) };
    let info = unsafe { FindResourceExW(module, kind, name, language) };
    if !info.is_null()
        && unsafe { SizeofResource(module, info) } as usize == comparison.bitmap.len()
    {
        let data = unsafe { LockResource(LoadResource(module, info)) }.cast::<u8>();
        if !data.is_null()
            && unsafe { slice::from_raw_parts(data, comparison.bitmap.len()) } == comparison.bitmap
        {
            comparison.matched = true;
        }
    }
    i32::from(!comparison.matched)
}

unsafe extern "system" fn compare_languages(
    module: HMODULE,
    kind: PCWSTR,
    name: PCWSTR,
    context: isize,
) -> BOOL {
    unsafe { EnumResourceLanguagesW(module, kind, name, Some(compare_icon), context) }
}

pub fn check_host(exe: &Path) -> Result<()> {
    // The fixture has one image. Only its bitmap, without the ICO directory,
    // should appear in an RT_ICON resource in either the host or the installer.
    let size = u32::from_le_bytes(ICON[14..18].try_into()?) as usize;
    let offset = u32::from_le_bytes(ICON[18..22].try_into()?) as usize;
    let mut comparison = IconComparison {
        bitmap: ICON
            .get(offset..offset + size)
            .context("Invalid ICO fixture")?,
        matched: false,
    };
    let path = wide(exe.as_os_str());
    // Map the PE as data, without running its loader or any entry point.
    let module = unsafe {
        LoadLibraryExW(
            path.as_ptr(),
            ptr::null_mut(),
            LOAD_LIBRARY_AS_DATAFILE | LOAD_LIBRARY_AS_IMAGE_RESOURCE,
        )
    };
    if module.is_null() {
        return Err(io::Error::last_os_error())
            .with_context(|| format!("Open PE {}", exe.display()));
    }
    unsafe {
        EnumResourceNamesW(
            module,
            RT_ICON,
            Some(compare_languages),
            (&mut comparison as *mut IconComparison<'_>) as isize,
        );
        FreeLibrary(module);
    }
    ensure!(
        comparison.matched,
        "Missing the fixture RT_ICON in {}",
        exe.display()
    );
    Ok(())
}

fn version_value(block: &[u32], key: &str) -> Option<(*const c_void, usize)> {
    let key = wide(OsStr::new(key));
    let mut value = ptr::null_mut();
    let mut size = 0;
    let found =
        unsafe { VerQueryValueW(block.as_ptr().cast(), key.as_ptr(), &mut value, &mut size) };
    (found != 0 && !value.is_null()).then_some((value, size as usize))
}

pub fn company_name(exe: &Path) -> Result<String> {
    let path = wide(exe.as_os_str());
    let size = unsafe { GetFileVersionInfoSizeW(path.as_ptr(), ptr::null_mut()) };
    if size == 0 {
        return Err(io::Error::last_os_error())
            .with_context(|| format!("Read version info in {}", exe.display()));
    }
    // Version resource structures and the translated strings are DWORD-aligned.
    let mut block = vec![0_u32; (size as usize).div_ceil(4)];
    if unsafe { GetFileVersionInfoW(path.as_ptr(), 0, size, block.as_mut_ptr().cast()) } == 0 {
        return Err(io::Error::last_os_error())
            .with_context(|| format!("Read version info in {}", exe.display()));
    }
    let (pairs, bytes) = version_value(&block, r"\VarFileInfo\Translation")
        .context("Missing version resource translation table")?;
    // Translation lengths are in bytes, string value lengths are in UTF-16 units.
    let translations = unsafe { slice::from_raw_parts(pairs.cast::<u8>(), bytes) };
    for &[lang_lo, lang_hi, cp_lo, cp_hi] in translations.as_chunks::<4>().0 {
        let language = u16::from_le_bytes([lang_lo, lang_hi]);
        let codepage = u16::from_le_bytes([cp_lo, cp_hi]);
        let key = format!(r"\StringFileInfo\{language:04x}{codepage:04x}\CompanyName");
        if let Some((value, units)) = version_value(&block, &key) {
            let text = unsafe { slice::from_raw_parts(value.cast::<u16>(), units) };
            let text = text.strip_suffix(&[0]).unwrap_or(text);
            if !text.is_empty() {
                return String::from_utf16(text).context("Invalid UTF-16 CompanyName");
            }
        }
    }
    bail!(
        "No CompanyName in the version resources of {}",
        exe.display()
    )
}

fn rich_edit_text(hwnd: HWND) -> Result<String> {
    fn read(hwnd: HWND, message: u32, capacity: usize, buffer: isize) -> Result<usize> {
        let mut result = 0;
        if unsafe {
            SendMessageTimeoutW(
                hwnd,
                message,
                capacity,
                buffer,
                SMTO_ABORTIFHUNG,
                1000,
                &mut result,
            )
        } == 0
        {
            return Err(io::Error::last_os_error()).context("Read installer license control");
        }
        Ok(result)
    }
    // WM_GETTEXT retrieves the cross-process RichEdit document itself.
    let length = read(hwnd, WM_GETTEXTLENGTH, 0, 0)?;
    let mut text = vec![0_u16; length + 1];
    let written = read(hwnd, WM_GETTEXT, text.len(), text.as_mut_ptr() as isize)?;
    String::from_utf16(text.get(..written).context("Invalid license text length")?)
        .context("Invalid UTF-16 license text")
}

fn inspect_dialog(
    dialog: HWND,
    advanced: &mut bool,
    last_text: &mut String,
    screenshot: &Path,
) -> Result<bool> {
    if unsafe { IsWindowVisible(dialog) } == 0 || winui::class_name(dialog)? != "#32770" {
        return Ok(false);
    }
    for field in winui::enum_windows(Some(dialog)) {
        if unsafe { IsWindowVisible(field) } != 0
            && winui::class_name(field)?
                .to_ascii_lowercase()
                .starts_with("richedit")
        {
            *last_text = rich_edit_text(field)?;
            if LICENSE.lines().all(|line| last_text.contains(line)) {
                winui::capture(dialog, None, screenshot)?;
                return Ok(true);
            }
            // Stay on a license page even when its text is wrong or still loading.
            return Ok(false);
        }
    }
    let next = unsafe { GetDlgItem(dialog, IDOK) };
    if !*advanced && !next.is_null() && unsafe { IsWindowVisible(next) } != 0 {
        winui::click(next)?;
        *advanced = true;
    }
    Ok(false)
}

fn wait_for_license(child: &mut Child, screenshot: &Path) -> Result<()> {
    let deadline = Instant::now() + Duration::from_secs(90);
    let mut advanced = false;
    let mut last_text = String::new();
    while Instant::now() < deadline {
        if let Some(status) = child.try_wait()? {
            bail!("Installer exited before showing the license: {status}");
        }
        for dialog in winui::enum_windows(None)
            .into_iter()
            .filter(|hwnd| winui::pid(*hwnd) == child.id())
        {
            match inspect_dialog(dialog, &mut advanced, &mut last_text, screenshot) {
                Ok(true) => return Ok(()),
                Ok(false) => {}
                Err(error) if winui::is_transient(&error) => {}
                Err(error) => return Err(error),
            }
        }
        thread::sleep(Duration::from_millis(200));
    }
    bail!("Installer did not display the configured license (last text: {last_text:?})")
}

pub fn check_setup(exe: &Path, screenshot: &Path) -> Result<()> {
    check_host(exe)?;
    winui::dpi_aware();
    let mut child = Command::new(exe)
        .spawn()
        .with_context(|| format!("Start installer {}", exe.display()))?;
    let check = wait_for_license(&mut child, screenshot);
    // The interactive check stops at the agreement; the suite installs separately.
    let cleanup = child.kill().and_then(|()| child.wait().map(|_| ()));
    check.and(cleanup.context("Close interactive installer"))
}
