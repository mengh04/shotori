# Shotori

[![Crates.io](https://img.shields.io/crates/v/shotori.svg)](https://crates.io/crates/shotori)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
![Platform](https://img.shields.io/badge/platform-Linux%20%7C%20Wayland-8892bf)

A Wayland-native screenshot tool with built-in, on-device OCR — the entire UI
hand-drawn with [gpui-kit](https://crates.io/crates/gpui-kit).

Freeze the screen, drag a selection, then copy it, save it, or read the text
out of it — all without leaving the keyboard.

**[简体中文](README.zh-CN.md)**

## Features

- **Region selection** — the screen freezes, everything outside your
  selection dims, and a live size label follows the drag.
- **Multi-monitor aware** — one overlay per output, pinned to the screen it
  captured. Mixed scales (1× / 1.5× / 2×) and rotated (portrait) outputs are
  handled.
- **Copy to clipboard** (`Enter` / `Ctrl+C`) — PNG served by a resident
  background daemon (the `wl-copy` model), so the clipboard outlives the
  process that filled it.
- **Save to disk** (`Ctrl+S`) — PNGs named like
  `Shotori_2026-09-26_12-34-56_789.png` (local time, milliseconds) in
  `~/Pictures/Shotori/`. Concurrent saves use automatic suffixes on name
  collisions and never overwrite existing files.
- **OCR** (`Ctrl+O`) — on-device text recognition (PP-OCRv6 via ONNX
  Runtime) straight to the clipboard. Works on mixed Chinese/English
  content, runs fully offline after a one-time model download.
- **Desktop notifications** — every exit (copy / save / OCR) reports back
  with a thumbnail of the screenshot.
- **No-terminal friendly** — everything works from a keybinding;
  notifications are the feedback channel.

## Requirements

- Linux with a wlroots-adjacent Wayland compositor (niri, sway, Hyprland, …)
  exposing `zwlr_screencopy-unstable-v1` and `zwlr-data-control-v1`.
- A notification daemon (dunst, mako, swaync, …) is optional — copy, save
  and OCR work fine without one.
- Building with the default features downloads a prebuilt ONNX Runtime
  during compilation.

## Installation

```bash
cargo install shotori
```

Bind it to a key, e.g. in niri:

```kdl
Mod+Shift+S { spawn "shotori"; }
```

## Usage

Run `shotori`; every screen freezes and a selection overlay appears.

| Key              | Action                                        |
| ---------------- | --------------------------------------------- |
| drag             | select a region                               |
| `Enter` / `Ctrl+C` | copy the selection (or the full screen) to the clipboard |
| `Ctrl+S`         | save the selection as PNG                     |
| `Ctrl+O`         | OCR the selection → text to the clipboard     |
| `Esc` (dragging) | abandon the current drag                      |
| `Esc`            | exit                                          |

A toolbar with equivalent buttons appears below the selection after release.

### OCR

- The engine is PP-OCRv6 small (detection + orientation + recognition),
  running locally through ONNX Runtime.
- First use opens a confirmation card, then a progress bar with a cancel
  button. Models (~31 MB, fetched from ModelScope) are verified with sha256
  and installed atomically. Cancel closes the dialog immediately; the worker
  removes its temporary file when the current network operation returns or
  reaches its 10-second timeout. Already verified models are kept for retry.
  They live in `~/.local/share/shotori/ocr-models/` and are reused from
  then on.
- The engine prewarms while you draw the selection, so recognition
  typically finishes within a few hundred milliseconds of pressing
  `Ctrl+O`; a spinner badge marks the wait.
- Small text (< ~16 px on a 1080p-class screen) strains the model; HiDPI
  screens fare better.
- OCR is a default feature. For a lighter binary without it:

  ```bash
  cargo install shotori --no-default-features
  ```

## Building from source

```bash
git clone https://github.com/mengh04/shotori
cd shotori
cargo build --release
cargo test                          # unit tests, no compositor needed
cargo build --no-default-features   # slim build without OCR
```

## Design notes

The resident-offer clipboard model, display matching under mixed scales,
output transforms that contradict their protocol names, and a pixel-level
forensics story about a 1 px seam bug are written up in
[ROADMAP.md](ROADMAP.md).

## License

[MIT](LICENSE)
