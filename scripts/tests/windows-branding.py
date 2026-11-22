"""Check the installed host and its interactive NSIS setup on Windows."""
from pathlib import Path
import struct
import subprocess
import sys

import pefile


FIXTURES = Path(__file__).resolve().parent / "fixtures" / "branding"


def check_icon(exe):
    # Our single-frame fixture is a 32-bit bitmap. Read the actual PE icon
    # resources, since the host can also embed an icon in its program data.
    contents = (FIXTURES / "製品.ico").read_bytes()
    length, offset = struct.unpack_from("<II", contents, 14)
    expected = contents[offset:offset + length]
    with pefile.PE(exe, fast_load=True) as image:
        image.parse_data_directories(
            directories=[pefile.DIRECTORY_ENTRY["IMAGE_DIRECTORY_ENTRY_RESOURCE"]]
        )
        for resource_type in image.DIRECTORY_ENTRY_RESOURCE.entries:
            if resource_type.id == pefile.RESOURCE_TYPE["RT_ICON"]:
                for name in resource_type.directory.entries:
                    for language in name.directory.entries:
                        data = language.data.struct
                        if image.get_data(data.OffsetToData, data.Size) == expected:
                            return
    raise AssertionError(f"Missing the fixture icon in {exe}")


def check_license_page(setup, screenshot):
    import ctypes as c
    from ctypes import wintypes as w
    import time

    from PIL import ImageGrab

    user = c.WinDLL("user32", use_last_error=True)
    user.SetProcessDPIAware()
    callback_type = c.WINFUNCTYPE(w.BOOL, w.HWND, w.LPARAM)
    user.GetDlgItem.argtypes = (w.HWND, c.c_int)
    user.GetDlgItem.restype = w.HWND
    user.SendMessageW.argtypes = (w.HWND, w.UINT, w.WPARAM, w.LPARAM)
    user.SendMessageW.restype = w.LPARAM
    user.PostMessageW.argtypes = user.SendMessageW.argtypes

    def windows(parent=None):
        handles = []

        @callback_type
        def collect(hwnd, _):
            handles.append(hwnd)
            return True

        if parent:
            user.EnumChildWindows(w.HWND(parent), collect, 0)
        else:
            user.EnumWindows(collect, 0)
        return handles

    def class_name(hwnd):
        name = c.create_unicode_buffer(128)
        user.GetClassNameW(w.HWND(hwnd), name, len(name))
        return name.value

    def owned_by(hwnd, child):
        pid = w.DWORD()
        user.GetWindowThreadProcessId(w.HWND(hwnd), c.byref(pid))
        return pid.value == child.pid

    expected = (FIXTURES / "利用規約.txt").read_text(encoding="utf-8").splitlines()
    with subprocess.Popen([setup]) as process:
        try:
            advanced = False
            deadline = time.monotonic() + 90
            last_text = ""
            while time.monotonic() < deadline and process.poll() is None:
                for dialog in windows():
                    if not owned_by(dialog, process) or class_name(dialog) != "#32770":
                        continue
                    for field in windows(dialog):
                        if user.IsWindowVisible(w.HWND(field)) and class_name(field).lower().startswith("richedit"):
                            length = user.SendMessageW(field, 0x000E, 0, 0)  # WM_GETTEXTLENGTH
                            text = c.create_unicode_buffer(length + 1)
                            user.SendMessageW(field, 0x000D, len(text), c.addressof(text))
                            last_text = text.value
                            if all(line in last_text for line in expected):
                                # Capture the composited desktop. A window DC can omit
                                # the installer's child controls, including the agreement.
                                hwnd = w.HWND(dialog)
                                user.SetForegroundWindow(hwnd)
                                user.RedrawWindow(hwnd, None, None, 0x0181)
                                rect = w.RECT()
                                if not user.GetWindowRect(hwnd, c.byref(rect)):
                                    raise c.WinError(c.get_last_error())
                                ImageGrab.grab(
                                    bbox=(rect.left, rect.top, rect.right, rect.bottom),
                                    all_screens=True,
                                ).save(screenshot)
                                return
                    # Advance from the welcome page to the license page once.
                    next_button = user.GetDlgItem(dialog, 1)
                    if next_button and not advanced and user.IsWindowVisible(w.HWND(dialog)):
                        user.PostMessageW(next_button, 0x00F5, 0, 0)  # BM_CLICK
                        advanced = True
                time.sleep(0.2)
            raise AssertionError(
                f"The installer did not display the configured license (exit={process.poll()}, "
                f"last license text={last_text[:300]!r})"
            )
        finally:
            process.kill()


if __name__ == "__main__":
    mode, exe = sys.argv[1:3]
    check_icon(exe)
    if mode == "setup":
        check_license_page(exe, sys.argv[3])
    elif mode != "host":
        raise ValueError(f"Unknown mode: {mode}")
