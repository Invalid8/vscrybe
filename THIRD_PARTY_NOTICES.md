# Third-party notices

vScribe is MIT licensed (see [LICENSE](LICENSE)). It builds on the work below. Each project keeps its own
license; the full texts ship inside the respective packages and are linked here.

## Bundled in the source tree

| Component | Version | License | Where |
| --- | --- | --- | --- |
| [htmx](https://htmx.org) | 2.0.4 | [0BSD](https://github.com/bigskysoftware/htmx/blob/master/LICENSE) | `src/static/htmx.min.js` |
| [Alpine.js](https://alpinejs.dev) | 3.14.8 | [MIT](https://github.com/alpinejs/alpine/blob/main/LICENSE.md) | `src/static/alpine.min.js` |
| [Lucide](https://lucide.dev) icons | — | [ISC](https://github.com/lucide-icons/lucide/blob/main/LICENSE) | `src/templates/_icons.svg` |
| [Bricolage Grotesque](https://github.com/ateliertriay/bricolage) | — | [SIL OFL 1.1](https://openfontlicense.org) | `src/static/fonts/` |
| [Onest](https://github.com/simpals/onest) | — | [SIL OFL 1.1](https://openfontlicense.org) | `src/static/fonts/` |
| [JetBrains Mono](https://github.com/JetBrains/JetBrainsMono) | — | [SIL OFL 1.1](https://openfontlicense.org) | `src/static/fonts/` |
| JFK inaugural address sample | — | Public domain (US government work) | `tests/fixtures/jfk.opus`, re-encoded from [whisper.cpp's sample](https://github.com/ggerganov/whisper.cpp/blob/master/samples/jfk.wav) |

## Downloaded at first run

| Model | License |
| --- | --- |
| [OpenAI Whisper](https://github.com/openai/whisper) weights and preprocessor configs | [MIT](https://github.com/openai/whisper/blob/main/LICENSE) |
| [Systran/faster-whisper-small](https://huggingface.co/Systran/faster-whisper-small) (CTranslate2 conversion) | MIT |
| [dropbox-dash/faster-whisper-large-v3-turbo](https://huggingface.co/dropbox-dash/faster-whisper-large-v3-turbo) (CTranslate2 conversion) | MIT |

## Compiled into the app

| Component | License |
| --- | --- |
| [CTranslate2](https://github.com/OpenNMT/CTranslate2) via [ct2rs](https://github.com/jkawamoto/ctranslate2-rs) | MIT |
| [Intel oneMKL](https://www.intel.com/content/www/us/en/developer/tools/oneapi/onemkl.html) (statically linked by CTranslate2) | [Intel Simplified Software License](https://www.intel.com/content/www/us/en/developer/articles/license/onemkl-license-faq.html) |
| [Tauri](https://tauri.app), [axum](https://github.com/tokio-rs/axum), [MiniJinja](https://github.com/mitsuhiko/minijinja) and the other Rust crates | MIT or Apache-2.0 (a few BSD/ISC/Zlib) |

The full list of Rust crates and exact versions is in `src-tauri/Cargo.lock`.

## System libraries (installed as package dependencies, not bundled)

| Component | License |
| --- | --- |
| [GStreamer](https://gstreamer.freedesktop.org) and its plugin sets, including gstreamer1.0-libav (FFmpeg) | LGPL-2.1-or-later (some plugins in -ugly/-bad carry their own terms) |
| [WebKitGTK](https://webkitgtk.org) | LGPL-2.1 / BSD |
