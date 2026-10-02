# Changelog

All notable changes are listed here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/)
and the project uses [Semantic Versioning](https://semver.org).

## [Unreleased]

## [0.2.0] - 2026-10-01

Renamed to **vScribe** and rewritten as a native Rust app. Your sessions and downloaded models move over automatically.

### Changed
- New name: vScribe, "Convert audio and voice notes to text". The command is now `vscribe`, and data lives in
  `~/.local/share/vscribe` (moved from `~/.local/share/vn-transcribe` on first launch).
- The whole app is now one Rust program (Tauri, axum, CTranslate2). No Python, no separate engine process.
- Audio is decoded with the system's GStreamer instead of a bundled FFmpeg.
- The command line is part of the app binary (`vscribe transcribe`, `vscribe serve`); `vn ui` is gone.

### Fixed
- Accented English was sometimes detected as another language (often Yoruba), giving garbage text and very slow
  transcription. The language is now a setting, English by default, next to the model picker and on Redo.

### Added
- Language setting with English as the default, plus Auto-detect; `vscribe transcribe --language <code>`.
- Files you add wait in a tray until you press Send. You can preview or remove them, and they survive a restart.
- Recording works in the desktop app (recorded as WAV), with preview before sending.
- Custom title bar, custom dropdowns, a ⋮ menu for Redo and Remove, and a renaming panel with a character limit.
- A notification when transcription finishes while the app is in the background.
- "Saved to Downloads" toasts with Show in folder, and progress panels for adding files and exporting.

## [0.1.0] - 2026-10-01

First release (Python).

[0.2.0]: https://github.com/Invalid8/vscribe/releases/tag/v0.2.0
[0.1.0]: https://github.com/Invalid8/vscribe/releases/tag/v0.1.0
