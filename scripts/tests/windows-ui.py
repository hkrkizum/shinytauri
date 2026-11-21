"""Exercise the WebView2 window of an installed offline Shiny application."""

import argparse
import ctypes
import os
import re
import socket
import subprocess
import time
from pathlib import Path
from urllib.error import URLError
from urllib.request import urlopen

import psutil
from playwright.sync_api import expect, sync_playwright


def exercise(kind, page, evidence):
    if kind == "minimal":
        expect(page.locator("#total")).to_have_text("0000 (stringr 1.6.0)", timeout=30000)
        page.locator("#count").click()
        expect(page.locator("#total")).to_have_text("0001 (stringr 1.6.0)")
        assert page.locator("#total").evaluate(
            "el => getComputedStyle(el).fontVariantNumeric"
        ) == "tabular-nums"
    else:
        expect(page.locator("#app-counter-count")).to_have_text("カウント：0", timeout=30000)
        page.locator("#app-counter-increment").click()
        expect(page.locator("#app-counter-count")).to_have_text("カウント：1")
        page.locator("#app-dice-roll").click()
        page.locator("nav a[href$='#!/report']").click()
        page.locator("#app-report-render").click(timeout=60000)
        download = page.locator("#app-report-download")
        expect(download).to_be_visible(timeout=240000)
        expect(download).to_have_attribute("href", re.compile(r"/download/"), timeout=240000)
        # Request the same session download as the UI link, without requiring
        # the native Save As picker on the hosted runner.
        snapshot = download.evaluate("""async link => {
            const response = await fetch(link.href);
            if (!response.ok || !response.headers.get('content-disposition')?.includes('dice-dashboard.html')) {
                throw Error('Expected dice-dashboard.html from ' + response.url + ' (' + response.status + ')');
            }
            const doc = new DOMParser().parseFromString(await response.text(), 'text/html');
            return {
                values: Array.from(doc.querySelectorAll('strong'), e => e.textContent.trim()),
                headers: Array.from(doc.querySelectorAll('.card-header'), e => e.textContent.trim()),
                external: doc.querySelectorAll("script[src], link[rel=stylesheet]").length,
            };
        }""")
        assert {"1", "1,000 回"} <= set(snapshot["values"]), snapshot
        assert {"カウンター", "試行回数"} <= set(snapshot["headers"]), snapshot
        assert snapshot["external"] == 0, snapshot
    page.screenshot(path=evidence / f"{kind}.png")


def wait_for_webview(app, port):
    deadline = time.monotonic() + 90
    while time.monotonic() < deadline:
        if app.poll() is not None:
            raise RuntimeError(f"Desktop host exited early ({app.returncode})")
        try:
            with urlopen(f"http://127.0.0.1:{port}/json/version", timeout=1):
                return
        except (OSError, URLError):
            time.sleep(0.25)
    raise TimeoutError("WebView2 did not expose the test debugging endpoint")


def close_main_window(app):
    # WM_CLOSE exercises the normal Tauri window shutdown sequence.
    from ctypes import wintypes

    user32 = ctypes.windll.user32
    callback_type = ctypes.WINFUNCTYPE(wintypes.BOOL, wintypes.HWND, wintypes.LPARAM)
    handles = []

    @callback_type
    def visit(window, unused):
        pid = wintypes.DWORD()
        user32.GetWindowThreadProcessId(wintypes.HWND(window), ctypes.byref(pid))
        if pid.value == app.pid and user32.IsWindowVisible(wintypes.HWND(window)):
            handles.append(window)
        return True

    user32.EnumWindows(visit, 0)
    if not handles:
        raise RuntimeError("No visible installed app window to close")
    user32.PostMessageW(wintypes.HWND(handles[0]), 0x0010, 0, 0)
    app.wait(timeout=20)


def check(kind, exe, evidence):
    with socket.socket() as listener:
        listener.bind(("127.0.0.1", 0))
        port = listener.getsockname()[1]
    # An installed app should run with only Windows tools on PATH. Windows'
    # normal user directories (including WebView2 data) remain available.
    env = {
        k: v for k, v in os.environ.items()
        if not k.startswith(("R_", "RIG_", "QUARTO_", "SHINYTAURI_"))
    }
    windows = Path(env["SYSTEMROOT"])
    env.update(
        PATH=os.pathsep.join(map(str, (windows / "System32", windows))),
        WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=f"--remote-debugging-port={port}",
    )
    app = subprocess.Popen([exe], cwd=evidence, env=env)
    try:
        wait_for_webview(app, port)
        with sync_playwright() as driver:
            browser = driver.chromium.connect_over_cdp(f"http://127.0.0.1:{port}")
            context = browser.contexts[0]
            page = context.pages[0] if context.pages else context.wait_for_event("page")
            try:
                page.wait_for_url(re.compile(r"^http://127\.0\.0\.1:\d+/"), timeout=100000)
                exercise(kind, page, evidence)
            except Exception:
                # Preserve the UI state when a render or assertion fails.
                if not page.is_closed():
                    page.screenshot(path=evidence / f"{kind}-failure.png", timeout=5000)
                raise
            children = psutil.Process(app.pid).children(recursive=True)
            runtimes = [p for p in children if p.name().lower() in {"r.exe", "rterm.exe", "rscript.exe"}]
            assert len(runtimes) >= (2 if kind == "rhino" else 1), runtimes
            for proc in runtimes:
                assert Path(proc.exe()).is_relative_to(exe.parent / "offline" / "R"), proc.exe()
            close_main_window(app)
            _, alive = psutil.wait_procs(runtimes, timeout=10)
            assert not alive, f"R processes survived closing {kind}: {alive}"
    finally:
        if app.poll() is None:
            app.kill()
            app.wait(timeout=20)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("kind", choices=("minimal", "rhino"))
    parser.add_argument("exe", type=Path)
    parser.add_argument("evidence", type=Path)
    args = parser.parse_args()
    check(args.kind, args.exe, args.evidence)
