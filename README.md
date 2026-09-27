# Shotori

[![Crates.io](https://img.shields.io/crates/v/shotori.svg)](https://crates.io/crates/shotori)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
![Platform](https://img.shields.io/badge/platform-Linux%20%7C%20Windows-8892bf)

A Wayland-native screenshot tool with built-in, on-device OCR — the entire UI
hand-drawn with [gpui-kit](https://crates.io/crates/gpui-kit). Also runs on
Windows (GDI capture, Win32 clipboard, toast notifications).

Freeze the screen, drag a selection, then copy it, save it, or read the text
out of it — all without leaving the keyboard.

**[简体中文](README.zh-CN.md)**

## Features

- Region selection with a live size label; everything else dims
- Adjust a drawn selection in place: drag inside to move, grab an edge or
  corner to resize
- Multi-monitor aware, including mixed scales and rotated outputs
- Copy to clipboard (`Enter` / `Ctrl+C`)
- Save to disk (`Ctrl+S`) via the system "save as" dialog
- OCR (`Ctrl+O`) — on-device, works on mixed Chinese/English text
- Desktop notifications with a thumbnail of the result; copy/save notifications on Linux include an **Open image** action (requires `xdg-open` and a notification server with action support)

Clipboard images opened from notifications retain their original resolution in the Shotori cache (`$XDG_CACHE_HOME/shotori` or `~/.cache/shotori` on Linux). Cached images older than 24 hours are removed on subsequent copy/save operations. Pencil, highlighter, polyline and filter previews reuse completed layers. Geometric shapes and number annotations also use a composite cache once the history reaches 64 marks. Replaced canvas previews explicitly release their GPU image cache entries. Long freehand strokes update coverage incrementally without darkening intersections. Large filters and complex strokes render in the background with one active task per session and coalesced updates. The previous preview may remain visible during computation; copy/save always render the current annotations at full precision.

## Requirements

**Linux** (the first-class platform):

- A wlroots-adjacent Wayland compositor (niri, sway, Hyprland, …)
- `xdg-desktop-portal` for the save dialog (installed by default on most
  desktops)
- A notification daemon (dunst, mako, swaync, …) is optional

**Windows** (10/11):

- Nothing beyond the OS: capture uses GDI, the clipboard is Win32,
  notifications are WinRT toasts, and clicking a window snaps to it via
  `EnumWindows`

## Installation

```bash
cargo install shotori        # crates.io (Linux & Windows)
paru -S shotori              # AUR (prebuilt binary)
```

Or grab a binary from [GitHub Releases](https://github.com/mengh04/shotori/releases)
(Linux tarball, Windows zip). Bind it to a key, e.g. in niri:

```kdl
Mod+Shift+S { spawn "shotori"; }
```

On Windows bind it to a key and/or keep a tray icon:

- **Tray** (recommended): `shotori --tray` stays resident with a tray
  icon; click it (or its menu) to start a screenshot. Autostart it via a
  shortcut in `shell:startup`.
- **Hotkey only**: create a shortcut to `shotori.exe` on the Desktop (or
  Start Menu), open its properties and fill the *Shortcut key* field —
  Explorer registers that hotkey globally. Note the field requires
  modifier keys (a bare key or exotic combo needs PowerToys Keyboard
  Manager's "Run Program" action or a one-line AutoHotkey script), and
  the Explorer shortcut path adds a noticeable launch delay.

On Linux the same `shotori --tray` speaks StatusNotifierItem (waybar,
KDE Plasma, a GNOME appindicator extension) — or keep binding `spawn
"shotori"` to a compositor key.

## Usage

Run `shotori` (or `shotori gui`); every screen freezes and a selection
overlay appears. A toolbar with equivalent buttons shows up below the
selection after release.

Non-interactive capture, no overlay:

```sh
shotori full                 # capture every screen → clipboard
shotori full -p ~/Pictures   # → timestamped PNG in a directory
shotori full -p shot.png -c  # → file AND clipboard
shotori full -d 2            # wait 2 s first
shotori --tray               # stay resident as a tray icon; click to
                             # start a screenshot (quit via its menu)
```

The full capture spans every screen (highest density wins, gaps stay
transparent — same export path as an interactive cross-screen
selection). Run `shotori --help` for the whole surface (themes below).
All screens share one selection: dragging on another screen replaces it, and a
selection can span screens. Cross-screen exports use the highest participating
pixel density; gaps between screens are transparent. Single-screen exports keep
the screen's native resolution.

| Key              | Action                                        |
| ---------------- | --------------------------------------------- |
| drag             | select a region                               |
| `Ctrl+A`         | select this whole screen; again → every screen (wraps) |
| `Enter` / `Ctrl+C` | copy the selection (or the full screen) to the clipboard |
| `Ctrl+S`         | save the selection — system "save as" dialog  |
| `Ctrl+O`         | OCR the selection → text to the clipboard     |
| `Esc` (dragging) | abandon the current drag                      |
| `Esc`            | exit                                          |

### Themes

The default `auto` follows system light/dark appearance, including changes while
an overlay is open. Set `dark`, `light` or `high_contrast` to keep a fixed theme.

```sh
shotori --theme light
shotori --print-theme   # auto shows both palettes; fixed themes show one
```

Copy [`docs/theme.example.toml`](docs/theme.example.toml) to
`~/.config/shotori/theme.toml` (`%APPDATA%\shotori\theme.toml` on Windows),
or pass `--theme /path/to/theme.toml`. Linux honors `XDG_CONFIG_HOME`.

```toml
base = "auto"
# accent = "#FF6A00"
# dim_opacity = 0.55
```

Backgrounds, icon colors, hover and selection states belong to a complete palette
and cannot be overridden separately. Selection tint follows the accent; text on
accent backgrounds and selected controls uses a readable contrasting color.
Optional `annotation_colors` contains exactly seven opaque colors. Invalid values
are reported and safely fall back; unknown keys reject the file. `--no-config`
skips automatic configuration, while an explicit `--theme` takes precedence.

Old JSON files are no longer loaded. Create the TOML file using the minimal example;
do not copy old `toolbar_*` overrides. The old JSON can remain as a backup.

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

### Eraser

Click the eraser icon or press `D`. The second row switches between brush and rectangle erasing. Brush mode offers 16 / 32 / 48 logical-pixel diameters; click or drag to erase. Rectangle mode erases the dragged area (`Shift` constrains it to a square).

Erasing restores the original capture, including areas covered by text, mosaic or blur. It only affects earlier annotations; new marks can be drawn over the restored area. Each gesture supports undo/redo and cancellation with `Esc`. Preview, copy and save share the same pixels across monitors. Wheel sizing, editable erase regions and clear-all remain follow-ups.

### Text annotations

Click the text icon or press `T`, choose a font size (16 / 24 / 32) and color, then click inside the selection to edit directly in a transparent text box. Text previews live in its actual color and size, grows with the content without a preset width, and wraps only at the selection’s right edge. The box grows vertically within the selection. Both toolbar rows remain available while editing; color and size changes apply immediately. IME preedit text, its underline, the caret and candidate placement use the same layout as the annotation. Supports multilingual, multiline input: click outside the box or press `Enter` to confirm, `Shift+Enter` inserts a line break, and `Esc` cancels. Confirmed text supports undo/redo and is included in copied and saved images.

Text uses system fonts and fallback; install appropriate fonts for CJK characters. Text wraps at the selection’s right edge and clips to the selection, including across monitors with different scales. Editing confirmed text, font selection, bold/italic, and rotation are not included yet.

### OCR

The first `Ctrl+O` asks before downloading the models (~31 MB, one time);
after that everything runs fully offline. Models are cached in
`~/.local/share/shotori/ocr-models/` (`%LOCALAPPDATA%\shotori\ocr-models\`
on Windows). Recognition is prewarmed while you
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
