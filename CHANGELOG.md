# Changelog

All notable changes are listed here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/)
and the project uses [Semantic Versioning](https://semver.org).

## [Unreleased]

## [0.3.0] - 2026-10-03

First public release, now also for Windows and macOS.

### Added
- Settings window (gear beside About) with Models, Hugging Face, This computer, Appearance and About tabs. Add
  Whisper models from a Hugging Face list or a folder on this computer; a downloading model shows its progress.
- Windows and macOS builds, for both Apple silicon and Intel Macs, and an AppImage for any Linux distribution.

### Changed
- Audio is decoded by FFmpeg built into vScribe, so nothing has to be installed on any system. It is also more
  accurate than the GStreamer pipeline it replaces (stereo was mixed down about 3 dB too quiet).
- Only one model is kept in memory at a time, and uploads are written to disk instead of held in memory.

### Fixed
- On Windows, recording showed a second, browser-style microphone prompt naming the app's local address and
  port. vScribe's own consent dialog is now the only one.
- Long stretches of speech could be left out when Whisper stopped early in a 30-second window. Transcription now
  resumes where it stopped, retries unreliable output and discards repetition loops.
- Files with a few undecodable packets (some `.amr` recordings) failed entirely; those packets are now skipped.
- Download progress stayed at 0% until a model finished, and a second download waited behind the first.
- Gated Hugging Face models failed with "check your internet connection"; they are now marked and refused.
- Dialogs covered the window controls, and clicking a "Transcription finished" notification didn't open the app.

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

## 0.1.0 - 2026-10-01

First release (Python).

[Unreleased]: https://github.com/Invalid8/vscribe/compare/v0.3.0...HEAD
[0.3.0]: https://github.com/Invalid8/vscribe/releases/tag/v0.3.0
[0.2.0]: https://github.com/Invalid8/vscribe/tree/177b7d3
