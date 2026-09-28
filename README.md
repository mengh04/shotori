# Shotori

[![Crates.io](https://img.shields.io/crates/v/shotori.svg)](https://crates.io/crates/shotori)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
![Platform](https://img.shields.io/badge/platform-Linux-8892bf)

A Wayland-native screenshot tool with built-in, on-device OCR — the entire UI
hand-drawn with [gpui-kit](https://crates.io/crates/gpui-kit).

Freeze the screen, drag a selection, then copy, save, annotate, or OCR it —
without leaving the keyboard.

**[简体中文](README.zh-CN.md)**

## Features

- Region selection, adjustable in place; multi-monitor aware, including mixed
  scales and rotated outputs, with selections spanning screens
- Annotations: rectangle, ellipse, line, polyline, arrow, numbered steps,
  pencil, highlighter, mosaic/blur, eraser, and text
- Pin: crop a selection into a floating always-on-top image that
  survives the overlay — drag across outputs, scroll to zoom
- Copy to clipboard, save via the system "save as" dialog, or OCR to text
  (fully offline after a one-time ~31 MB model download)
- Non-interactive full-screen capture from the CLI
- Optional tray icon; themes following the system light/dark appearance

## Requirements

- A wlroots-adjacent Wayland compositor (niri, sway, Hyprland, …)
- `xdg-desktop-portal` for the save dialog (installed by default on most
  desktops)
- A notification daemon (dunst, mako, swaync, …) is optional

## Installation

```bash
cargo install shotori        # crates.io
paru -S shotori              # AUR (prebuilt binary)
```

Or grab a binary from
[GitHub Releases](https://github.com/mengh04/shotori/releases).

Bind it to a key, e.g. in niri:

```kdl
Mod+Shift+S { spawn "shotori"; }
```

`shotori tray` stays resident with a tray icon (StatusNotifierItem; works
with waybar, KDE Plasma, and the GNOME appindicator extension).

## Usage

Run `shotori` (or `shotori gui`): every screen freezes and a selection overlay
appears. A toolbar with equivalent buttons shows up below the selection after
release.

| Key                | Action                                                   |
| ------------------ | -------------------------------------------------------- |
| drag               | select a region                                          |
| `Ctrl+A`           | select this whole screen; again → every screen           |
| `Enter` / `Ctrl+C` | copy the selection to the clipboard                      |
| `Ctrl+S`           | save the selection — system "save as" dialog             |
| `Ctrl+O`           | OCR the selection → text to the clipboard                |
| `Ctrl+P`           | pin the selection to the screen                          |
| `Esc`              | abandon the current drag / exit                          |

Non-interactive capture, no overlay:

```sh
shotori full                 # capture every screen → clipboard
shotori full -p ~/Pictures   # → timestamped PNG in a directory
shotori full -d 2            # wait 2 s first
```

Themes: `shotori --theme light` (also `dark`, `high_contrast`; `auto` follows
the system). For a custom palette, copy
[`docs/theme.example.toml`](docs/theme.example.toml) to
`~/.config/shotori/theme.toml`.

Run `shotori --help` for the full CLI surface.

## Development

```bash
git clone https://github.com/mengh04/shotori
cd shotori
cargo build --release
cargo test    # unit tests, no compositor needed
```

CI enforces `cargo fmt --all --check` and
`cargo clippy --all-targets -- -D warnings` — run both before pushing.

- Module map: the header of [`src/lib.rs`](src/lib.rs)
- Decisions & pitfall archive: [ROADMAP.md](ROADMAP.md)

## License

[MIT](LICENSE)
