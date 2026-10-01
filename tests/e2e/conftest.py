import os
import subprocess
from pathlib import Path

import pytest
from playwright.sync_api import Page

ROOT = Path(__file__).parent.parent.parent
FIXTURES = ROOT / "tests" / "fixtures"
VN = Path(os.environ.get("VSCRIBE_BIN", ROOT / "src-tauri" / "target" / "debug" / "vscribe"))
READY = "VSCRIBE_READY "

RETURNING_USER = """
localStorage.setItem("vscribe-onboarded", "yes");
localStorage.setItem("vscribe-consent-mic", "granted");
localStorage.setItem("vscribe-consent-files", "granted");
"""

CLIPBOARD_SPY = """
window.copied = [];
Object.defineProperty(navigator, "clipboard", {
  configurable: true,
  value: { writeText: async (text) => { window.copied.push(text); } },
});
"""


@pytest.fixture(scope="session")
def engine_url(tmp_path_factory):
    home = tmp_path_factory.mktemp("engine")
    env = {**os.environ, "XDG_DATA_HOME": str(home / "data"), "XDG_STATE_HOME": str(home / "state")}
    engine = subprocess.Popen([VN, "serve", "--exit-with-stdin"], env=env, text=True,
                              stdin=subprocess.PIPE, stdout=subprocess.PIPE)
    line = engine.stdout.readline()
    assert line.startswith(READY), f"engine did not start: {line!r}"
    yield line.removeprefix(READY).strip()
    engine.stdin.close()
    engine.wait(timeout=10)


@pytest.fixture(scope="session")
def base_url(engine_url):
    return engine_url


@pytest.fixture(scope="session")
def browser_type_launch_args(browser_type_launch_args, browser_name):
    if browser_name == "firefox":
        prefs = {"media.navigator.streams.fake": True, "media.navigator.permission.disabled": True}
        return {**browser_type_launch_args, "firefox_user_prefs": prefs}
    return {**browser_type_launch_args, "args": ["--use-fake-device-for-media-stream", "--use-fake-ui-for-media-stream"]}


@pytest.fixture(scope="session")
def browser_context_args(browser_context_args, browser_name):
    permissions = ["microphone"] if browser_name == "chromium" else []
    return {**browser_context_args, "permissions": permissions, "accept_downloads": True}


@pytest.fixture
def new_user(page: Page) -> Page:
    page.add_init_script(CLIPBOARD_SPY)
    page.goto("/")
    return page


@pytest.fixture
def returning_user(page: Page) -> Page:
    page.add_init_script(RETURNING_USER + CLIPBOARD_SPY)
    page.goto("/")
    return page


@pytest.fixture
def onboarded_user(page: Page) -> Page:
    page.add_init_script('localStorage.setItem("vscribe-onboarded", "yes");' + CLIPBOARD_SPY)
    page.goto("/")
    return page
