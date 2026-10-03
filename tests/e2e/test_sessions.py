import base64
import io
import re
import uuid
import zipfile

import pytest
from playwright.sync_api import Page, expect

from .conftest import FIXTURES

JFK = "and so my fellow americans ask not what your country can do for you"


def words(text: str) -> str:
    return " ".join(re.findall(r"[a-z']+", text.lower()))
TRANSCRIBED = 120_000


def expect_toast(page: Page, message: str, kind: str = "success"):
    expect(page.locator(f".toast.is-{kind} p", has_text=message)).to_be_visible()


def expect_tray_stored(page: Page, count: int):
    page.wait_for_function(
        """(count) => new Promise((resolve) => {
          const open = indexedDB.open("vscribe", 1);
          open.onsuccess = () => {
            const request = open.result.transaction("staged").objectStore("staged").count();
            request.onsuccess = () => {
              open.result.close();
              resolve(request.result === count);
            };
          };
        })""",
        arg=count,
    )


def add(page: Page, *names: str):
    page.set_input_files("#pick-files", [FIXTURES / n for n in names])
    page.get_by_role("button", name="Send", exact=True).click()


@pytest.fixture
def session(returning_user: Page) -> Page:
    add(returning_user, "jfk.opus")
    expect(returning_user.locator(".seg-text").first).to_contain_text("ask not what your country", timeout=TRANSCRIBED)
    return returning_user


def test_added_note_is_transcribed_and_listed(returning_user: Page):
    page = returning_user
    add(page, "jfk.opus")

    expect_toast(page, "Added 1 voice note")
    expect(page).to_have_url(re.compile(r"/\?s=\w+$"))
    expect(page.locator(".status-line")).to_contain_text(re.compile("Transcribing|Waiting"))
    expect(page.locator(".transcript")).to_contain_text("ask not what your country can do for you", timeout=TRANSCRIBED)
    expect(page.locator(".vn-meta")).to_contain_text("EN")
    expect(page.locator("#sessions .note.is-selected .note-title")).to_contain_text(re.compile(r"^And so,? my fellow Americans"))
    expect(page.locator(".doc-meta")).to_contain_text("0:11 total")


def test_player_plays_and_seeks_from_a_timestamp(session: Page):
    page = session
    audio = page.locator(".vn audio")
    page.get_by_role("button", name="Play").click()
    expect(page.get_by_role("button", name="Pause")).to_be_visible()
    page.get_by_role("button", name="Pause").click()
    expect(page.get_by_role("button", name="Play")).to_be_visible()

    stamps = page.locator(".stamp")
    last = stamps.nth(stamps.count() - 1)
    start = int(last.inner_text().split(":")[1])
    last.click()
    page.wait_for_function("(at) => document.querySelector('.vn audio').currentTime >= at", arg=start)
    assert audio.evaluate("a => a.paused") is False
    expect(page.locator(".seg.is-live")).to_have_count(1)


def test_copy_buttons(session: Page):
    page = session
    page.get_by_role("button", name="Copy", exact=True).click()
    expect(page.get_by_role("button", name="Copied")).to_be_visible()
    page.get_by_role("button", name="With times").click()
    page.locator(".session-actions").get_by_role("button", name="Copy all").click()

    page.wait_for_function("window.copied.length === 3")
    plain, stamped, everything = page.evaluate("window.copied")
    assert words(plain).startswith(JFK)
    assert stamped.startswith("[0:00] ")
    assert everything.startswith("jfk.opus (0:11)\n")
    assert words(everything.split("\n", 1)[1]).startswith(JFK)


def test_downloads_and_export(session: Page):
    page = session
    with page.expect_download() as note:
        page.get_by_role("link", name="Download .txt").click()
    assert note.value.suggested_filename == "jfk.txt"
    assert words(open(note.value.path()).read()).startswith(JFK)

    with page.expect_download() as whole:
        page.locator(".session-actions").get_by_role("link", name="Download session as .txt").click()
    assert re.fullmatch(r"session-\d{4}-\d\d-\d\d-\d{4}\.txt", whole.value.suggested_filename)

    with page.expect_download() as export:
        page.get_by_role("link", name="Export transcripts").click()
    assert re.fullmatch(r"voice-notes-\d{4}-\d\d-\d\d\.zip", export.value.suggested_filename)
    names = zipfile.ZipFile(io.BytesIO(open(export.value.path(), "rb").read())).namelist()
    assert any(name.endswith("/jfk.txt") for name in names)


def test_dropping_files_adds_them(returning_user: Page):
    page = returning_user
    data = base64.b64encode((FIXTURES / "jfk.opus").read_bytes()).decode()
    transfer = page.evaluate_handle("""(data) => {
        const bytes = Uint8Array.from(atob(data), (c) => c.charCodeAt(0));
        const transfer = new DataTransfer();
        transfer.items.add(new File([bytes], "dropped.opus", { type: "audio/ogg" }));
        return transfer;
    }""", data)

    page.dispatch_event("body", "dragenter", {"dataTransfer": transfer})
    expect(page.locator(".dropzone")).to_be_visible()
    expect(page.locator(".dropzone")).to_contain_text("Start a new session")
    page.dispatch_event("body", "drop", {"dataTransfer": transfer})

    expect(page.locator(".dropzone")).to_be_hidden()
    expect(page.locator(".chip-name")).to_have_text("dropped.opus")
    page.get_by_role("button", name="Send", exact=True).click()
    expect(page.locator(".vn-name")).to_have_text("dropped.opus")


def test_folder_upload_skips_non_audio(returning_user: Page, tmp_path):
    page = returning_user
    (tmp_path / "chat").mkdir()
    (tmp_path / "chat" / "voice.opus").write_bytes((FIXTURES / "jfk.opus").read_bytes())
    (tmp_path / "chat" / "notes.txt").write_text("not audio")

    page.set_input_files("#pick-folder", tmp_path / "chat")
    page.get_by_role("button", name="Send", exact=True).click()

    expect_toast(page, "Added 1 voice note · skipped 1 file that isn’t audio")
    expect(page.locator(".vn-name")).to_have_text("voice.opus")


def test_non_audio_files_are_refused(returning_user: Page, tmp_path):
    page = returning_user
    text = tmp_path / "notes.txt"
    text.write_text("not audio")
    page.set_input_files("#pick-files", text)
    page.get_by_role("button", name="Send", exact=True).click()
    expect_toast(page, "None of those files are audio", "error")


def test_rename_search_and_delete_session(session: Page):
    page = session
    wrap = page.locator(".title-wrap")
    wrap.get_by_role("button", name="Rename session").click()
    field = wrap.get_by_label("Session name")
    expect(field).to_be_focused()
    name = f"Kennedy speech {uuid.uuid4().hex[:6]}"
    page.keyboard.type(name)
    expect(wrap.locator(".title-count")).to_have_text(f"{len(name)} / 80")
    page.keyboard.press("Enter")
    expect_toast(page, "Session renamed")
    expect(wrap.locator(".title-text")).to_have_text(name)
    page.reload()
    expect(wrap.locator(".title-text")).to_have_text(name)

    search = page.get_by_placeholder("Search sessions")
    search.fill(name.upper())
    expect(page.locator("#sessions .note")).to_have_count(1)
    search.fill("no such session anywhere")
    expect(page.locator("#sessions")).to_contain_text("Nothing matches")
    search.fill("")

    page.locator(".session-actions .danger").click()
    page.locator(".session-actions .danger-solid").click()
    expect_toast(page, "Session deleted")
    expect(page).to_have_url(re.compile(r"/$"))
    expect(page.get_by_role("heading", name="Drop voice notes to start a session.")).to_be_visible()


def test_rename_from_the_sidebar_while_transcribing(returning_user: Page):
    page = returning_user
    add(page, "jfk.opus")
    row = page.locator("#sessions .note.is-selected")
    expect(row).to_have_class(re.compile("is-running"))
    row.get_by_role("button", name="Rename session").click()
    expect(row.get_by_label("Session name")).to_be_focused()
    page.keyboard.type("Typed while it was still transcribing", delay=60)
    page.keyboard.press("Enter")
    expect_toast(page, "Session renamed")
    expect(row.locator(".note-title")).to_have_text("Typed while it was still transcribing", timeout=TRANSCRIBED)


def test_redo_and_remove_note(session: Page):
    page = session
    note = page.locator(".vn")
    note.get_by_role("button", name="More options").click()
    note.get_by_label("Model for redo", exact=True).click()
    note.get_by_role("option", name="Fast").click()
    note.get_by_role("button", name="Redo", exact=True).click()
    expect_toast(page, "Transcribing again with the Fast model")
    expect(note.locator(".transcript")).to_contain_text("ask not", timeout=TRANSCRIBED)

    note.get_by_role("button", name="More options").click()
    note.get_by_role("button", name="Remove from session").click()
    note.get_by_role("button", name="Remove", exact=True).click()
    expect_toast(page, "Voice note removed · the session was empty, so it's gone too")
    expect(page).to_have_url(re.compile(r"/$"))


def test_theme_cycles_and_persists(returning_user: Page):
    page = returning_user
    html = page.locator("html")
    button = page.get_by_role("button", name=re.compile("theme, click to change"))

    expect(button).to_have_text("System theme")
    button.click()
    expect(html).to_have_attribute("data-theme", "light")
    button.click()
    expect(html).to_have_attribute("data-theme", "dark")
    page.reload()
    expect(html).to_have_attribute("data-theme", "dark")
    button.click()
    expect(html).not_to_have_attribute("data-theme", re.compile(".*"))


def test_staged_notes_wait_for_send_and_can_be_removed(returning_user: Page, tmp_path):
    page = returning_user
    second = tmp_path / "second.opus"
    second.write_bytes((FIXTURES / "jfk.opus").read_bytes() + b"")
    page.set_input_files("#pick-files", [FIXTURES / "jfk.opus"])
    page.set_input_files("#pick-files", [second])
    expect(page.locator(".chip")).to_have_count(2)
    expect(page.locator(".vn")).to_have_count(0)

    page.set_input_files("#pick-files", [FIXTURES / "jfk.opus"])
    expect_toast(page, "jfk.opus is already in the list.", "error")
    expect(page.locator(".chip")).to_have_count(2)

    page.get_by_role("button", name="Remove second.opus").click()
    expect(page.locator(".chip")).to_have_count(1)
    page.get_by_role("button", name="Send", exact=True).click()
    expect_toast(page, "Added 1 voice note")
    expect(page.locator(".vn-name")).to_have_text("jfk.opus")
    expect(page.locator(".chip")).to_have_count(0)


def test_staged_notes_survive_a_reload_and_can_be_previewed(returning_user: Page):
    page = returning_user
    page.set_input_files("#pick-files", [FIXTURES / "jfk.opus"])
    expect(page.locator(".chip")).to_have_count(1)
    expect_tray_stored(page, 1)
    page.reload()
    expect(page.locator(".chip-name")).to_have_text("jfk.opus")

    page.get_by_role("button", name="Listen to jfk.opus").click()
    expect(page.get_by_role("button", name="Stop jfk.opus")).to_be_visible()
    page.get_by_role("button", name="Stop jfk.opus").click()
    expect(page.get_by_role("button", name="Listen to jfk.opus")).to_be_visible()

    page.get_by_role("button", name="Send", exact=True).click()
    expect(page.locator(".vn-name")).to_have_text("jfk.opus")
    expect_tray_stored(page, 0)
    page.reload()
    expect(page.locator(".chip")).to_have_count(0)
