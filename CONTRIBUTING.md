# Contributing

Thanks for helping. Bug reports, fixes and small features are all welcome. For anything large, open an issue first so
we can agree on the approach before you spend time on it.

## Project layout

```
src/                  the pages: MiniJinja templates, CSS, htmx/Alpine scripts, fonts, splash screen
src-tauri/            the app (Tauri 2, Rust)
  src/engine/         sessions and queue, GStreamer decoding, CTranslate2 transcription, model download
  src/web/            axum routes that render the templates and serve audio
  src/cli.rs          `vscribe transcribe` and `vscribe serve`
  src/lib.rs          the window: starts the server in-process and loads it
  linux/              .desktop template for the .deb
tests/fixtures/       sample audio
tests/e2e/            Playwright tests that drive the real app in Chromium and Firefox
```

## Development setup

On Ubuntu/Debian:

```sh
sudo apt install libwebkit2gtk-4.1-dev libgtk-3-dev librsvg2-dev build-essential cmake libclang-dev pkg-config \
  gstreamer1.0-plugins-base gstreamer1.0-plugins-good gstreamer1.0-libav
```

plus [Rust](https://rustup.rs), Node.js 20+ and, for the browser tests, [uv](https://docs.astral.sh/uv/). The GStreamer
plugins are only for playing audio inside the Linux app; decoding uses FFmpeg built into vScribe.

Build that FFmpeg once (about a minute; `.cargo/config.toml` points cargo at it):

```sh
packaging/ffmpeg/build.sh
```

The first cargo build also compiles CTranslate2, which takes a while.

```sh
npm ci
npm run tauri dev                                        # the desktop app
cargo run --manifest-path src-tauri/Cargo.toml -- serve  # just the server; open the printed URL in a browser
```

The first transcription downloads the Fast model (about 480 MB) into your cache folder.

## Tests

```sh
cargo test --manifest-path src-tauri/Cargo.toml          # Rust tests
cargo build --manifest-path src-tauri/Cargo.toml         # the browser tests use target/debug/vscribe
uv run --project tests/e2e playwright install chromium firefox   # once
uv run --project tests/e2e pytest tests/e2e              # browser tests in both browsers
uv run --project tests/e2e pytest tests/e2e --browser firefox --headed
```

The browser tests start `vscribe serve` with a throwaway data folder, so they never touch your sessions. They do
use the real Fast model from your cache. Please add or update tests with every change, and check UI changes in Firefox
as well as Chromium.

## Building the .deb

```sh
npx tauri build
# -> src-tauri/target/release/bundle/deb/vscribe_<version>_amd64.deb
```

Release builds run on Ubuntu 22.04 so the package installs on older systems too.

## Style

- Rust: small functions, descriptive names, no comments explaining what the code already says. `cargo fmt`.
- UI: the app must work fully offline and never send audio or text anywhere.
- Commits: short imperative subject line ("Fix drop on Firefox"), details in the body if needed.

## Releasing

1. Update the version in `src-tauri/Cargo.toml` (Tauri reads it from there).
2. Move the "Unreleased" notes in `CHANGELOG.md` under the new version.
3. Tag `vX.Y.Z` and push the tag. The release workflow builds the `.deb` and attaches it to a draft release.

## Code of conduct

Be kind and assume good intent. Harassment of any kind isn't tolerated.
