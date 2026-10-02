from pathlib import Path

import pytest
from playwright.sync_api import Page, expect

from .conftest import CLIPBOARD_SPY, RETURNING_USER
from .test_sessions import TRANSCRIBED, add, expect_toast

SMALL = Path.home() / ".cache" / "vscribe" / "models" / "small"


def open_models(page: Page):
    page.get_by_role("button", name="Model", exact=True).click()
    page.get_by_role("button", name="Manage models…").click()
    expect(page.get_by_role("heading", name="Settings")).to_be_visible()
    expect(page.get_by_role("tab", name="Models")).to_have_attribute("aria-selected", "true")


def desktop_user(page: Page, folder: Path) -> Page:
    picker = f"window.__TAURI__ = {{ dialog: {{ open: async () => {str(folder)!r} }} }};"
    page.add_init_script(RETURNING_USER + CLIPBOARD_SPY + picker)
    page.goto("/")
    return page


def add_folder(page: Page, name: str):
    page.get_by_role("tab", name="This computer").click()
    page.get_by_role("button", name="Choose folder…").click()
    page.get_by_role("tabpanel").get_by_label("Name in the picker").fill(name)
    page.get_by_role("button", name="Add model").click()


@pytest.fixture
def model_folder(tmp_path: Path) -> Path:
    if not (SMALL / "model.bin").is_file():
        pytest.skip("the Fast model has not been downloaded on this machine")
    for file in ["config.json", "model.bin", "tokenizer.json", "vocabulary.txt"]:
        (tmp_path / file).symlink_to(SMALL / file)
    return tmp_path


def test_built_in_models_are_listed_and_cannot_be_removed(returning_user: Page):
    page = returning_user
    open_models(page)

    rows = page.locator(".model-row")
    expect(rows.filter(has_text="Fast")).to_contain_text("Built in")
    expect(rows.filter(has_text="Accurate")).to_contain_text("Built in")
    expect(rows.filter(has_text="Fast").get_by_role("button", name="Remove")).to_have_count(0)


def test_a_folder_model_is_added_used_and_removed(page: Page, model_folder: Path):
    page = desktop_user(page, model_folder)
    open_models(page)
    add_folder(page, "Folder Small")

    expect_toast(page, "Added the Folder Small model")
    expect(page.get_by_role("tab", name="Models")).to_have_attribute("aria-selected", "true")
    expect(page.locator(".model-row", has_text="Folder Small")).to_contain_text(model_folder.name)

    page.keyboard.press("Escape")
    page.get_by_role("button", name="Model", exact=True).click()
    page.get_by_role("option", name="Folder Small").click()
    add(page, "jfk.opus")
    expect(page.locator(".seg-text").first).to_contain_text("ask not what your country", timeout=TRANSCRIBED)

    open_models(page)
    row = page.locator(".model-row", has_text="Folder Small")
    row.get_by_role("button", name="Remove", exact=True).click()
    row.get_by_role("button", name="Remove Folder Small").click()
    expect_toast(page, "Removed the Folder Small model")
    expect(page.locator(".model-row", has_text="Folder Small")).to_have_count(0)
    assert model_folder.joinpath("model.bin").exists()

    page.keyboard.press("Escape")
    page.get_by_role("button", name="Model", exact=True).click()
    expect(page.get_by_role("option", name="Folder Small")).to_have_count(0)


def test_something_that_is_not_a_model_is_refused_with_the_reason(page: Page, tmp_path: Path):
    (tmp_path / "config.json").write_text("{}")
    page = desktop_user(page, tmp_path)
    open_models(page)
    add_folder(page, "Not A Model")

    expect(page.get_by_role("alert")).to_contain_text("isn't a Whisper model in CTranslate2")
    expect(page.get_by_role("alert")).to_contain_text("model.bin")
    expect(page.get_by_role("tab", name="This computer")).to_have_attribute("aria-selected", "true")
    expect(page.get_by_role("tabpanel").get_by_label("Name in the picker")).to_have_value("Not A Model")
    expect(page.locator(".model-row", has_text="Not A Model")).to_have_count(0)


def test_folders_are_left_to_the_desktop_app_in_a_browser(returning_user: Page):
    page = returning_user
    open_models(page)
    page.get_by_role("tab", name="This computer").click()

    expect(page.get_by_text("Folders are added in the vScribe desktop app")).to_be_visible()
    expect(page.get_by_role("button", name="Choose folder…")).to_be_hidden()
    expect(page.get_by_role("button", name="Add model")).to_be_hidden()


def test_hugging_face_models_are_chosen_from_a_list(returning_user: Page):
    page = returning_user
    open_models(page)
    page.get_by_role("tab", name="Hugging Face").click()

    expect(page.get_by_role("button", name="Add model")).to_be_disabled()
    expect(page.get_by_role("tabpanel").get_by_label("Name in the picker")).to_be_disabled()
    tiny = page.locator(".model-offer", has_text="Systran/faster-whisper-tiny")
    expect(tiny).to_contain_text("MB")
    tiny.click()

    expect(tiny).to_have_attribute("aria-pressed", "true")
    expect(page.get_by_role("tabpanel").get_by_label("Name in the picker")).to_have_value("faster-whisper-tiny")
    expect(page.get_by_role("button", name="Add model")).to_be_enabled()


def test_hugging_face_can_be_searched(returning_user: Page):
    page = returning_user
    open_models(page)
    page.get_by_role("tab", name="Hugging Face").click()
    search = page.get_by_label("Search Hugging Face")

    search.fill("african")
    expect(page.locator(".model-offers-title")).to_have_text("Results")
    gated = page.locator(".model-offer", has_text="Sunbird/faster-whisper-51-african-languages")
    expect(gated).to_be_disabled()
    expect(gated).to_contain_text("Needs a Hugging Face account that accepted its terms")

    search.fill("no-such-whisper-model-anywhere")
    expect(page.locator(".model-offers")).to_contain_text("No Whisper models in CTranslate2 format match")

    search.fill("")
    expect(page.locator(".model-offers-title")).to_have_text("Suggested")


def test_settings_open_from_the_sidebar_and_change_the_theme(returning_user: Page):
    page = returning_user
    page.get_by_role("button", name="Settings").click()
    page.get_by_role("tab", name="Appearance").click()
    page.get_by_role("radio", name="Dark").click()

    expect(page.locator("html")).to_have_attribute("data-theme", "dark")
    page.get_by_role("radio", name="System").click()
    expect(page.locator("html")).not_to_have_attribute("data-theme", "dark")


def test_select_all_in_the_desktop_app_stays_out_of_the_interface(page: Page, tmp_path: Path):
    page = desktop_user(page, tmp_path)
    page.locator("body").click(position={"x": 700, "y": 300})
    page.keyboard.press("Control+a")

    assert page.evaluate("window.getSelection().toString()") == ""
