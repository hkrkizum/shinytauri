"""Drive the standard NSIS upgrade wizard and preserve the user's app data."""

import argparse
import ctypes
import subprocess
import time
from pathlib import Path

import psutil
import win32con
import win32gui
import win32process


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
    length = win32gui.SendMessage(hwnd, win32con.WM_GETTEXTLENGTH, 0, 0)
    text = ctypes.create_unicode_buffer(length + 1)
    win32gui.SendMessage(hwnd, win32con.WM_GETTEXT, len(text), ctypes.addressof(text))
    return text.value.replace("&", "")


def upgrade(setup, timeout=240):
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
            pids = {installer.pid, *(p.pid for p in process.children(recursive=True))}
            for window in visible_windows():
                if win32process.GetWindowThreadProcessId(window)[1] not in pids:
                    continue
                if win32gui.GetClassName(window) != "#32770" or not win32gui.IsWindowEnabled(window):
                    continue
                buttons = {
                    control_text(h): h for h in visible_windows(window)
                    if win32gui.GetClassName(h) == "Button"
                }
                # Both defaults matter: uninstall v1 and keep application data.
                if "Uninstall before installing" in buttons:
                    choice = buttons["Uninstall before installing"]
                    assert win32gui.SendMessage(choice, win32con.BM_GETCHECK, 0, 0) == 1, buttons
                    saw_previous_version = True
                if "Delete the application data" in buttons:
                    choice = buttons["Delete the application data"]
                    assert win32gui.SendMessage(choice, win32con.BM_GETCHECK, 0, 0) == 0, buttons
                    saw_uninstaller = True
                for label, button in buttons.items():
                    if label.startswith("Run shinytauri-smoke-minimal"):
                        # Launch the app separately under the offline UI driver.
                        win32gui.SendMessage(button, win32con.BM_SETCHECK, 0, 0)
                primary = win32gui.GetDlgItem(window, win32con.IDOK)
                if primary not in buttons.values() or not win32gui.IsWindowEnabled(primary):
                    continue
                page = (window, sorted(buttons), control_text(primary))
                if page != last_page:
                    print(win32gui.GetWindowText(window), page[-1], flush=True)
                    last_page = page
                    # The setup waits for its child uninstaller during this
                    # click. Post asynchronously so we can drive that dialog.
                    win32gui.PostMessage(primary, win32con.BM_CLICK, 0, 0)
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
    upgrade(parser.parse_args().setup)
