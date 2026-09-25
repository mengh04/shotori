# Shotori

**A Wayland-first screenshot tool with built-in OCR, drawn entirely by hand with [gpui-kit](https://crates.io/crates/gpui-kit).**

[English](README.md) · [简体中文](README.zh-CN.md)

Freeze the screen, drag a selection, then copy it, save it, or read the text
out of it — all without leaving the keyboard.

![workflow](https://img.shields.io/badge/platform-Linux%20%2F%20Wayland-8892bf) ![license](https://img.shields.io/badge/license-MIT-blue)

## Features

- **Region screenshot** — drag to select; the rest of the screen dims and the
  selection "sees through". A size label follows the selection live.
- **Multi-monitor done right** — one overlay per output, pinned to the screen
  it captured. Mixed scales (1.0 / 1.5 / 2.0) and rotated outputs (portrait
  panels) are handled; tested on a three-monitor niri setup.
- **Copy** (`Enter` / `Ctrl+C`) — PNG to the Wayland clipboard via a resident
  background daemon (the wl-copy model), so the clipboard outlives the tool.
- **Save** (`Ctrl+S`) — timestamped PNG into `~/Pictures/Shotori/`, automatic
  suffixes on name collisions.
- **OCR** (`Ctrl+O`) — on-device text recognition (PP-OCRv6 small via ONNX
  Runtime), text straight to the clipboard. Chinese/English mixed content
  works. The engine prewarms while you draw the selection; a spinner badge
  shows while inferring.
  - First use shows a confirm card, a byte-accurate progress bar, and a
    cancel button. Models (~31 MB, from ModelScope) are verified by sha256
    and written atomically — a cancelled download never leaves garbage.
    Cached in `~/.local/share/shotori/ocr-models/`.
- **Desktop notifications** — every exit (copy / save / OCR) reports back
  with a thumbnail of the screenshot, via `org.freedesktop.Notifications`.
- No selection + `Enter` = full-screen capture. Everything works from a
  keybinding; notifications mean you never needed a terminal.

## Requirements

- Linux + a wlroots-adjacent Wayland compositor (niri, sway, Hyprland, …)
  exposing `zwlr_screencopy-unstable-v1` and `zwlr-data-control-v1`.
- A notification daemon is optional (copy/save/OCR work fine without one).
- OCR adds ONNX Runtime at build time (a prebuilt library is downloaded
  automatically by the build script).

## Install

```bash
cargo install shotori
```

Then bind it, e.g. in niri's `binds.kdl`:

```kdl
Mod+Shift+S { spawn "shotori"; }
```

## Usage

```
shotori            # freeze all screens, select, act
```

| Key | Action |
|-----|--------|
| drag | select a region (press again to re-select) |
| `Enter` / `Ctrl+C` | copy selection (or full screen) to clipboard |
| `Ctrl+S` | save selection as PNG |
| `Ctrl+O` | OCR selection → text to clipboard |
| `Esc` (while dragging) | abandon this drag |
| `Esc` (otherwise) | exit |

A toolbar with the same actions appears under the selection after release.

### OCR notes

- The engine is PP-OCRv6 small (detection + orientation + recognition),
  running fully offline once the one-time model download is done.
- Small text (< ~16 px on a 1080p screen) struggles; HiDPI screens do
  better (more physical pixels).
- OCR is a default feature. For a slimmer binary without it:
  `cargo install shotori --no-default-features`.

## Building from source

```bash
cargo build --release
cargo test                        # 21 unit tests, no compositor needed
cargo build --no-default-features # slim build without OCR
```

Also ships `screencap`, a small debugging front end for the capture code:

```bash
cargo run --bin screencap -- --all
```

## Design notes

The interesting bits — the resident-offer clipboard model, display matching
under mixed scales, transform semantics that contradict the protocol
wording, pixel-forensics on a 1 px seam bug — are written up in
[ROADMAP.md](ROADMAP.md).

## License

MIT — see [LICENSE](LICENSE).
