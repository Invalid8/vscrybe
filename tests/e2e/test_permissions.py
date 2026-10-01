import re

from playwright.sync_api import Page, expect

from .conftest import FIXTURES


def consent(page: Page):
    return page.locator("dialog.consent")


def test_file_access_asks_once(onboarded_user: Page):
    page = onboarded_user
    choosers = []
    page.on("filechooser", lambda chooser: choosers.append(chooser))

    page.get_by_role("button", name="Add voice notes", exact=True).click()
    expect(consent(page).get_by_role("heading")).to_have_text("Add files from your computer?")
    expect(consent(page).locator(".consent-points li")).to_have_count(3)
    consent(page).get_by_role("button", name="Not now").click()
    expect(consent(page)).to_be_hidden()
    assert choosers == []
    assert page.evaluate("localStorage.getItem('vscribe-consent-files')") is None

    page.get_by_role("button", name="Add voice notes", exact=True).click()
    with page.expect_file_chooser() as chooser:
        consent(page).get_by_role("button", name="Allow file access").click()
    assert chooser.value.is_multiple()
    assert page.evaluate("localStorage.getItem('vscribe-consent-files')") == "granted"

    page.reload()
    with page.expect_file_chooser():
        page.get_by_role("button", name="Convert a whole folder").click()
    expect(consent(page)).to_be_hidden()


def test_choosing_files_after_consent_starts_a_session(onboarded_user: Page):
    page = onboarded_user
    page.get_by_role("button", name="Choose files").click()
    with page.expect_file_chooser() as chooser:
        consent(page).get_by_role("button", name="Allow file access").click()
    chooser.value.set_files(FIXTURES / "jfk.opus")
    page.get_by_role("button", name="Send", exact=True).click()
    expect(page.locator(".vn-name")).to_have_text("jfk.opus")
    expect(page).to_have_url(re.compile(r"\?s=\w+"))


def test_microphone_denied_does_not_record(onboarded_user: Page):
    page = onboarded_user
    page.get_by_role("button", name="Record a voice note").click()
    expect(consent(page).get_by_role("heading")).to_have_text("Use your microphone?")
    consent(page).get_by_role("button", name="Not now").click()
    expect(page.locator(".composer-rec")).to_be_hidden()
    assert page.evaluate("localStorage.getItem('vscribe-consent-mic')") is None


def test_recording_after_consent_is_transcribed(onboarded_user: Page):
    page = onboarded_user
    page.get_by_role("button", name="Record a voice note").click()
    consent(page).get_by_role("button", name="Allow microphone").click()

    expect(page.locator(".composer-rec")).to_be_visible()
    expect(page.locator(".rec-time")).to_have_text("0:02", timeout=5000)
    page.get_by_role("button", name="Pause recording").click()
    expect(page.locator(".rec-dot")).to_have_class(re.compile("is-paused"))
    page.get_by_role("button", name="Resume recording").click()
    page.get_by_role("button", name="Stop recording").click()
    expect(page.locator(".chip-name")).to_have_text(re.compile(r"^Recording .*\.wav$"))
    page.get_by_role("button", name="Send", exact=True).click()

    note = page.locator(".vn")
    expect(note.locator(".vn-name")).to_have_text(re.compile(r"^Recording \d{4}-\d\d-\d\d at [\d.]+\.wav$"))
    expect(note.locator(".transcript")).to_be_visible(timeout=120_000)
    expect(note).not_to_have_class(re.compile("is-failed"))
    assert page.evaluate("localStorage.getItem('vscribe-consent-mic')") == "granted"


def test_escape_discards_a_recording(returning_user: Page):
    page = returning_user
    page.get_by_role("button", name="Record a voice note").click()
    expect(page.locator(".composer-rec")).to_be_visible()
    page.keyboard.press("Escape")
    expect(page.locator(".composer-rec")).to_be_hidden()
    expect(page.locator(".vn")).to_have_count(0)
