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

- Region selection with a live size label; everything else dims
- Multi-monitor aware, including mixed scales and rotated outputs
- Copy to clipboard (`Enter` / `Ctrl+C`)
- Save to disk (`Ctrl+S`) via the system "save as" dialog
- OCR (`Ctrl+O`) — on-device, works on mixed Chinese/English text
- Desktop notifications with a thumbnail of the result

## Requirements

- Linux with a wlroots-adjacent Wayland compositor (niri, sway, Hyprland, …)
- `xdg-desktop-portal` for the save dialog (installed by default on most
  desktops)
- A notification daemon (dunst, mako, swaync, …) is optional

## Installation

```bash
cargo install shotori        # crates.io
paru -S shotori              # AUR (prebuilt binary)
```

Or grab a binary from [GitHub Releases](https://github.com/mengh04/shotori/releases).
Bind it to a key, e.g. in niri:

```kdl
Mod+Shift+S { spawn "shotori"; }
```

## Usage

Run `shotori`; every screen freezes and a selection overlay appears. A
toolbar with equivalent buttons shows up below the selection after release.
All screens share one selection: dragging on another screen replaces it, and a
selection can span screens. Cross-screen exports use the highest participating
pixel density; gaps between screens are transparent. Single-screen exports keep
the screen's native resolution.

| Key              | Action                                        |
| ---------------- | --------------------------------------------- |
| drag             | select a region                               |
| `Enter` / `Ctrl+C` | copy the selection (or the full screen) to the clipboard |
| `Ctrl+S`         | save the selection — system "save as" dialog  |
| `Ctrl+O`         | OCR the selection → text to the clipboard     |
| `Esc` (dragging) | abandon the current drag                      |
| `Esc`            | exit                                          |

### OCR

The first `Ctrl+O` asks before downloading the models (~31 MB, one time);
after that everything runs fully offline. Models are cached in
`~/.local/share/shotori/ocr-models/`. Recognition is prewarmed while you
draw, so it usually completes within a few hundred milliseconds. Very small
text strains the model; HiDPI screens fare better.

## Building from source

```bash
git clone https://github.com/mengh04/shotori
cd shotori
cargo build --release
cargo test    # unit tests, no compositor needed
```

## Design notes

Implementation write-ups (the resident-offer clipboard model, display
matching under mixed scales, a 1 px seam forensics story) live in
[ROADMAP.md](ROADMAP.md).

## License

[MIT](LICENSE)
