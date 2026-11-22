"""Drive the standard NSIS upgrade wizard and preserve the user's app data."""

import argparse
import ctypes
import subprocess
import time
from pathlib import Path

import psutil
import pywintypes
import win32con
import win32gui
import win32process
from PIL import ImageGrab


def visible_windows(parent=None):
    handles = []

    def collect(hwnd, unused):
        if win32gui.IsWindowVisible(hwnd):
            handles.append(hwnd)
        return True

    if parent is None:
        win32gui.EnumWindows(collect, None)
    else:
        win32gui.EnumChildWindows(parent, collect, None)
    return handles


def control_text(hwnd):
    # Standard NSIS buttons use the Win32 caption. Reading it does not
    # send a synchronous message to a busy (or closing) installer thread.
    return win32gui.GetWindowText(hwnd).replace("&", "")


def capture(window, choice, path):
    # A button can already have its correct caption and state before Windows
    # paints its text. Save the composited screen once the label is visible.
    deadline = time.monotonic() + 8
    while time.monotonic() < deadline:
        win32gui.RedrawWindow(
            window, None, 0, win32con.RDW_UPDATENOW | win32con.RDW_ALLCHILDREN | win32con.RDW_INVALIDATE
        )
        rect = win32gui.GetWindowRect(window)
        left, top, right, bottom = win32gui.GetWindowRect(choice)
        image = ImageGrab.grab(bbox=rect, all_screens=True)
        # Exclude the radio/checkbox glyph and the outer border.
        label = image.crop((
            left - rect[0] + bottom - top, top - rect[1] + 2,
            right - rect[0] - 2, bottom - rect[1] - 2,
        )).convert("RGB")
        if any(light - dark > 32 for dark, light in label.getextrema()):
            image.save(path)
            return
        time.sleep(0.1)
    raise TimeoutError(f"NSIS did not paint {control_text(choice)!r}")


def upgrade(setup, evidence, timeout=240):
    # GetWindowRect and ImageGrab must use the same physical pixels.
    ctypes.windll.user32.SetProcessDPIAware()
    installer = subprocess.Popen([setup])
    process = psutil.Process(installer.pid)
    saw_previous_version = False
    saw_uninstaller = False
    last_page = None
    deadline = time.monotonic() + timeout
    try:
        while installer.poll() is None:
            if time.monotonic() >= deadline:
                raise TimeoutError(f"NSIS did not finish: {last_page}")
            try:
                pids = {installer.pid, *(p.pid for p in process.children(recursive=True))}
            except psutil.NoSuchProcess:
                installer.wait(timeout=5)
                break
            for window in visible_windows():
                try:
                    if win32process.GetWindowThreadProcessId(window)[1] not in pids:
                        continue
                    if win32gui.GetClassName(window) != "#32770" or not win32gui.IsWindowEnabled(window):
                        continue
                    primary = win32gui.GetDlgItem(window, win32con.IDOK)
                    if not primary or not win32gui.IsWindowEnabled(primary):
                        continue
                    buttons = {
                        control_text(h): h for h in visible_windows(window)
                        if win32gui.GetClassName(h) == "Button"
                    }
                    # Both defaults matter: uninstall v1 and keep application data.
                    if "Uninstall before installing" in buttons and not saw_previous_version:
                        choice = buttons["Uninstall before installing"]
                        assert win32gui.SendMessage(choice, win32con.BM_GETCHECK, 0, 0) == 1, buttons
                        capture(window, choice, evidence / "upgrade-choice.png")
                        saw_previous_version = True
                    if "Delete the application data" in buttons and not saw_uninstaller:
                        choice = buttons["Delete the application data"]
                        assert win32gui.SendMessage(choice, win32con.BM_GETCHECK, 0, 0) == 0, buttons
                        capture(window, choice, evidence / "upgrade-keep-data.png")
                        saw_uninstaller = True
                    for label, button in buttons.items():
                        if label.startswith("Run shinytauri-smoke-minimal"):
                            # Launch the app separately under the offline UI driver.
                            win32gui.SendMessage(button, win32con.BM_SETCHECK, 0, 0)
                    if primary not in buttons.values():
                        continue
                    page = (window, sorted(buttons), control_text(primary))
                    if page != last_page:
                        print(win32gui.GetWindowText(window), page[-1], flush=True)
                        last_page = page
                        # The setup waits for its child uninstaller during this
                        # click. Post asynchronously so we can drive that dialog.
                        win32gui.PostMessage(primary, win32con.BM_CLICK, 0, 0)
                except pywintypes.error as error:
                    # NSIS destroys its controls as soon as a page or the wizard
                    # closes. Take a fresh snapshot on the next poll.
                    if error.winerror not in (1400, 1421):
                        raise
            time.sleep(0.2)
        assert installer.returncode == 0, installer.returncode
        assert saw_previous_version and saw_uninstaller, "NSIS did not upgrade the installed product"
    finally:
        if installer.poll() is None:
            installer.kill()
            installer.wait()


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("setup", type=Path)
    parser.add_argument("evidence", type=Path)
    args = parser.parse_args()
    upgrade(args.setup, args.evidence)
