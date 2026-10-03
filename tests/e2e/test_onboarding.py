from playwright.sync_api import Page, expect

from .conftest import VERSION


def stored(page: Page, key: str):
    return page.evaluate("(key) => localStorage.getItem(key)", key)


def test_first_visit_walks_through_the_intro(new_user: Page):
    dialog = new_user.locator("dialog.onboard")
    step = dialog.locator(".onboard-step")
    expect(dialog).to_be_visible()
    expect(step).to_have_text("1 of 4")
    expect(dialog.get_by_role("button", name="Back")).to_be_hidden()

    for expected in ["2 of 4", "3 of 4", "4 of 4"]:
        dialog.get_by_role("button", name="Next").click()
        expect(step).to_have_text(expected)

    dialog.get_by_role("button", name="Back").click()
    expect(step).to_have_text("3 of 4")
    dialog.get_by_role("button", name="Go to card 4").click()
    expect(dialog.get_by_role("heading", name="Nothing leaves this computer.")).to_be_visible()
    expect(dialog.locator(".onboard-model")).to_be_visible()

    dialog.get_by_role("button", name="Get started").click()
    expect(dialog).to_be_hidden()
    assert stored(new_user, "vscribe-onboarded") == "yes"

    new_user.reload()
    expect(new_user.get_by_role("heading", name="Drop voice notes to start a session.")).to_be_visible()
    expect(dialog).to_be_hidden()


def test_skip_ends_the_intro(new_user: Page):
    new_user.locator("dialog.onboard").get_by_role("button", name="Skip").click()
    expect(new_user.locator("dialog.onboard")).to_be_hidden()
    assert stored(new_user, "vscribe-onboarded") == "yes"


def test_escape_ends_the_intro(new_user: Page):
    expect(new_user.locator("dialog.onboard")).to_be_visible()
    new_user.keyboard.press("Escape")
    expect(new_user.locator("dialog.onboard")).to_be_hidden()
    assert stored(new_user, "vscribe-onboarded") == "yes"


def test_arrow_keys_move_between_cards(new_user: Page):
    step = new_user.locator("dialog.onboard .onboard-step")
    new_user.keyboard.press("ArrowRight")
    new_user.keyboard.press("ArrowRight")
    expect(step).to_have_text("3 of 4")
    new_user.keyboard.press("ArrowLeft")
    expect(step).to_have_text("2 of 4")


def test_about_replays_the_intro(returning_user: Page):
    expect(returning_user.locator("dialog.onboard")).to_be_hidden()
    returning_user.get_by_role("button", name="About vScribe").click()
    returning_user.get_by_role("button", name="Show the intro again").click()
    expect(returning_user.locator("dialog.onboard")).to_be_visible()
    expect(returning_user.locator("dialog.onboard .onboard-step")).to_have_text("1 of 4")


def test_about_report_link_fills_in_the_version(returning_user: Page):
    returning_user.get_by_role("button", name="About vScribe").click()
    about = returning_user.locator("dialog.about").filter(has_text="Found a problem?")
    expect(about.locator(".about-version")).to_have_text(f"Version {VERSION}")
    link = about.get_by_role("link", name="Report an issue")
    href = link.get_attribute("href")
    assert href.startswith(f"mailto:b.fadamitan2019@gmail.com?subject=vScribe%20issue%20({VERSION})&body=")
    assert "What%20happened%3A" in href
