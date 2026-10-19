"""The Linux build's WebDriver acceptance: drives the real window (WebKitGTK,
through tauri-driver and WebKitWebDriver) on this machine's real package
managers, read-only, and uninstalls a throwaway AppImage end to end.

The app runs with HOME and WATTCLEAN_DATA_DIR in a temporary folder, so it
never sees or changes a real home folder or Trash; the package managers are
only asked what they installed. Python's standard library only.

    python3 e2e/linux/smoke.py <path to the debug wattclean binary>

Needs tauri-driver (scripts/setup-tauri-driver.sh) and WebKitWebDriver
(Debian/Ubuntu: webkit2gtk-driver); with no display, run it under xvfb-run.
"""
import json
import os
import shutil
import subprocess
import sys
import tempfile
import time
import urllib.request

BASE = "http://127.0.0.1:4444"


def call(method, path, body=None):
    data = None if body is None else json.dumps(body).encode()
    request = urllib.request.Request(
        BASE + path, data=data, method=method, headers={"Content-Type": "application/json"}
    )
    with urllib.request.urlopen(request, timeout=120) as response:
        return json.loads(response.read() or b"{}")


class Window:
    def __init__(self, session):
        self.session = session

    def js(self, script):
        return call("POST", f"/session/{self.session}/execute/sync", {"script": script, "args": []})["value"]

    def invoke(self, command):
        script = (
            "const done = arguments[arguments.length - 1];"
            f"window.__TAURI_INTERNALS__.invoke({json.dumps(command)}).then(done, e => done({{error: e}}));"
        )
        return call("POST", f"/session/{self.session}/execute/async", {"script": script, "args": []})["value"]

    def wait(self, script, what, limit=90):
        end = time.time() + limit
        while time.time() < end:
            value = self.js(script)
            if value:
                return value
            time.sleep(0.3)
        raise AssertionError(f"timed out waiting for {what}")


def check_shell(window):
    nav = window.wait(
        "const b = document.body.textContent; if (b.includes('could not start')) return [b];"
        "const n = [...document.querySelectorAll('.nav-item')].map(b => b.id); return n.includes('nav-cleaner') && n",
        "the navigation",
    )
    assert "nav-registry" not in nav and "nav-programs" in nav, nav
    info = window.invoke("app_info")
    assert info["platform"] == "linux", info
    groups = window.wait("return [...document.querySelectorAll('.group-title')].map(g => g.textContent)", "the cleaners")
    assert groups[0] == "Linux", groups
    print("shell OK:", nav, groups)


def check_preview(window):
    window.js("document.getElementById('nav-cleaner').click()")
    window.wait("const b = document.getElementById('cleaner-preview'); return b && !b.disabled", "Preview")
    window.js("document.getElementById('cleaner-preview').click()")
    summary = window.wait("const s = document.getElementById('report-summary'); return s && s.textContent", "the preview", 180)
    window.js("document.querySelectorAll('.result-head[aria-expanded=\"false\"]').forEach(h => h.click())")
    kinds = window.js("return [...new Set([...document.querySelectorAll('.item-kind')].map(k => k.textContent))]")
    print("preview OK:", summary, kinds)


def check_settings(window):
    shown = window.js(
        "document.getElementById('nav-settings').click();"
        "return [...document.querySelectorAll('fieldset.settings-group')].filter(f => !f.hidden).map(f => f.querySelector('legend').textContent)"
    )
    assert "Community cleaners" not in shown and "Registry cleaner" not in shown, shown
    print("settings OK:", shown)


def check_uninstall(window, home):
    """A throwaway AppImage and its settings folder go to the Trash."""
    appimage = os.path.join(home, "Applications", "WattCleanSmoke-1.0-x86_64.AppImage")
    leftover = os.path.join(home, ".config", "wattcleansmoke")
    os.makedirs(os.path.dirname(appimage))
    os.makedirs(leftover)
    with open(appimage, "wb") as file:
        file.write(b"not really an AppImage")
    window.js("document.getElementById('nav-programs').click(); document.getElementById('un-pkg-refresh').click()")
    row = "[...document.querySelectorAll('#un-pkg-table tbody tr')].find(r => r.cells[0].firstChild.textContent === 'WattCleanSmoke')"
    window.wait(f"return {row} && true", "the AppImage in the list")
    window.js(f"{row}.click(); document.getElementById('un-pkg-uninstall').click()")
    window.wait("return document.querySelector('dialog[open] button[value=ok]') && true", "the confirmation")
    window.js("document.querySelector('dialog[open] button[value=ok]').click()")
    offered = window.wait(
        "const l = document.getElementById('un-pkg-leftovers'); return l && [...l.querySelectorAll('.path')].map(p => p.textContent)",
        "the leftovers",
    )
    assert offered == [leftover], offered
    window.js("document.querySelectorAll('#un-pkg-leftovers input').forEach(b => b.click());"
              "document.querySelector('dialog[open] button[value=ok]').click()")
    end = time.time() + 30
    while time.time() < end and os.path.exists(leftover):
        time.sleep(0.3)
    trash = os.path.join(home, ".local", "share", "Trash")
    assert not os.path.exists(appimage) and not os.path.exists(leftover), "not moved"
    assert sorted(os.listdir(os.path.join(trash, "files"))) == [os.path.basename(appimage), "wattcleansmoke"]
    with open(os.path.join(trash, "info", "wattcleansmoke.trashinfo")) as record:
        assert ("Path=" + leftover) in record.read()
    print("uninstall OK: both in the Trash with their records")


def main():
    app = os.path.abspath(sys.argv[1])
    sandbox = tempfile.mkdtemp(prefix="wattclean-smoke-")
    home = os.path.join(sandbox, "home")
    os.makedirs(home)
    env = dict(os.environ, HOME=home, WATTCLEAN_DATA_DIR=os.path.join(sandbox, "data"))
    for name in ("XDG_CONFIG_HOME", "XDG_CACHE_HOME", "XDG_DATA_HOME", "XDG_STATE_HOME"):
        env.pop(name, None)
    driver_path = shutil.which("tauri-driver", path=os.pathsep.join([".webdriver", env.get("PATH", "")]))
    if not driver_path:
        raise SystemExit("tauri-driver was not found: run scripts/setup-tauri-driver.sh")
    driver = subprocess.Popen([driver_path], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    try:
        for _ in range(100):
            try:
                call("GET", "/status")
                break
            except OSError:
                time.sleep(0.2)
        caps = {"capabilities": {"alwaysMatch": {"tauri:options": {"application": app}}}}
        session = call("POST", "/session", caps)["value"]["sessionId"]
        try:
            window = Window(session)
            check_shell(window)
            check_preview(window)
            check_uninstall(window, home)
            check_settings(window)
        finally:
            call("DELETE", f"/session/{session}")
    finally:
        driver.terminate()
        driver.wait(timeout=30)
        shutil.rmtree(sandbox, ignore_errors=True)
    print("LINUX ACCEPTANCE PASSED")


if __name__ == "__main__":
    main()
