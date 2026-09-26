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

### Rectangle and ellipse annotations

After selecting a region, click the rectangle icon (`R`) or ellipse icon (`E`),
then drag inside the selection. Hold `Shift` for a square or circle. Annotations
can span outputs within a cross-screen selection. Both tools share colors, stroke
widths and undo/redo history.

- Pick a color swatch directly (red, orange, yellow, green, blue, black or white)
  and choose a stroke-width dot (1, 3 or 5 logical pixels) in the second row.
- `Ctrl+Z` undoes; `Ctrl+Y` or `Ctrl+Shift+Z` redoes. History actions are
  keyboard-only to keep the toolbar compact.
- `Esc` cancels the current stroke; another `Esc`, or the active tool’s key, leaves the tool
  while keeping completed annotations.
- Drawing a new selection after leaving the tool clears the old annotations.
- Copy and save include annotations; OCR reads the unmarked capture.

Rectangle and ellipse outlines are available. Moving, resizing, rotation,
fill and rounded corners are not implemented yet.

### Line and polyline annotations

- Click the line icon or press `L`, then drag and release to draw a straight line.
- Click the polyline icon or press `P`, then click to add vertices. Double-click,
  right-click or press `Enter` to finish the entire polyline.
- Hold `Shift` to constrain the current segment to 45° increments. Finishing keeps
  confirmed vertices only, excluding the floating preview segment.
- Both tools share colors, widths and history, support cross-screen drawing and
  antialiased export. A complete polyline is one undo step.
- `Esc` cancels the current drawing, then leaves the tool on the next press.
  Copy/save finish an active polyline before exporting.

Currently supports solid strokes with round caps and joins. Vertex editing,
dashes and configurable endpoint styles are planned follow-ups.

### Arrow annotations

Click the arrow icon or press `A`, then drag from the start toward the target and
release to finish. Hold `Shift` for 45° increments. Arrows share colors, widths
and undo history, with cross-screen preview and antialiased export. The head
scales with stroke width and shrinks for short arrows.

Currently supports a filled triangular head and a solid shaft. Endpoint editing,
alternative arrow styles and comment text are planned follow-ups.

### Sequence number annotations

Click the sequence icon or press `N`, then click inside the selection to place
numbered circular badges (1, 2, 3…). Drag before releasing to adjust placement.
The second row offers `S`, `M`, `L` sizes (24, 32, 40 logical pixels) and shared
colors. Digits automatically use black or white for contrast.

Numbering is shared across outputs. Undoing the latest number lets a new badge
reuse it; redo restores the original number. Switching tools preserves numbering;
a new selection restarts at 1. Supports multiple digits, cross-screen preview and
antialiased export. Selections smaller than 16 logical pixels cannot hold a badge.
Custom starting values, letters/Roman numerals, leader arrows and comments are
not implemented yet.

Digits are rendered by `ab_glyph` using the bundled DejaVu Sans Bold font, without
custom digit-outline code. Preview and export share badge rasterization at their
target scale; preview images are cached by position, style and DPI. See the
[font license](assets/DejaVuSans-LICENSE.txt).

### Pencil annotations

Click the pencil icon or press `B`, then drag inside the selection to draw freely.
A click places a round dot. Colors and the three stroke widths are shared with
other drawing tools. Each drag is one undo/redo step; `Esc` cancels an unfinished
stroke. Supports cross-screen drawing and antialiased export.

Straight-segment mode, wheel width adjustment and configurable smoothing remain
follow-ups.

### Highlighter annotations

Click the highlighter icon or press `H` to draw translucent strokes over text.
The default color is yellow, with 12, 20 and 32 logical-pixel widths; highlighter
color and width are remembered separately from pencil/shape styles. Opacity is
fixed at approximately 38%. Retracing or crossing within one stroke keeps an even
coat, while separate strokes stack. Supports whole-stroke undo/redo, cross-screen
preview and antialiased export; OCR still uses the original image.

Rectangle highlighting, multiply blending, adjustable opacity and wheel width
adjustment remain follow-ups.

### Mosaic and blur annotations

Click the mosaic icon or press `M`, then drag a rectangle over the area to process.
The second row switches between mosaic and blur and offers Low/Medium/High strength.
Mosaic averages pixel blocks; blur uses an alpha-weighted box filter. Effects apply
in drawing order, including to earlier annotations. Each rectangle is one undo/redo
step; `Esc` cancels a draft. Preview and export share the composed pixels across
screens, while OCR continues to use the original image.

Brush mode, editing existing filter regions and smart erasing remain follow-ups.

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
