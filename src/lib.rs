//! # Shotori — a Wayland-first screenshot tool built with gpui-kit
//!
//! ## Layout
//!
//! ```text
//! main.rs / actions.rs        entry point, shared action contract
//! ├─ model/                   pure logic & state (unit-tested)
//! │  ├─ selection             Idle → Dragging → Selected lifecycle
//! │  ├─ session               state shared by all overlays (multi-monitor
//! │  │                        selections, hover, click-snap, cropping)
//! │  ├─ export                crop → PNG → disk/clipboard bytes
//! │  └─ placement             two-zone chrome geometry (label top,
//! │                           toolbar bottom — disjoint by construction)
//! ├─ platform/                compositor & desktop integration
//! │  ├─ capture/              screen freeze: wlr-screencopy (Linux) or
//! │  │                        GDI BitBlt (Windows); pixels is pure
//! │  │                        format/rotation processing
//! │  ├─ display               capture ↔ gpui display matching
//! │  └─ windowsnap/           window-rect backends: niri / sway /
//! │                           Hyprland IPC (Linux), EnumWindows (Windows)
//! ├─ ui/                      gpui windows & elements
//! │  ├─ overlay               overlay assembly (one per screen):
//! │  │                        layer-shell on Linux, borderless topmost
//! │  │                        popup on Windows
//! │  ├─ hud / toolbar         selection visuals & buttons
//! │  ├─ ocr_setup             first-run OCR model dialog
//! │  ├─ e2e                   SHOTORI_DEBUG_* backdoors for headless tests
//! │  └─ theme / image_util    constants; RGBA → RenderImage
//! ├─ annotation/             in-canvas annotations: arrow / number /
//! │                          pencil / highlighter / mosaic+blur
//! └─ clipboard / notify / ocr / save_dialog
//!      the four post-selection exits: clipboard (resident daemon on
//!      Linux, Win32 on Windows), notifications (D-Bus / toast), OCR
//!      engine, native save dialog
//! ```
//!
//! Dependency direction: `ui → model`, `model → platform` (session holds
//! captures and snap rects); `platform` and `ui` never reach back up.
//! `actions` is referenced by everyone but references no one — it is the
//! shared vocabulary between keybindings (main), buttons (toolbar) and
//! handlers (overlay).
//!
//! The pin (floating image) feature lives on the `pin` branch (it depends
//! on the vendored set_layer_margin patch; can't ship on crates.io until
//! upstream merges it).

pub mod actions;

pub mod args;
pub mod model;
pub mod platform;
pub mod ui;

// in-canvas annotation engine (private module tree: strokes, filters,
// numbering — driven by the overlay's keybindings)
mod annotation;

pub mod clipboard;
pub mod notify;
pub mod ocr;
pub mod save_dialog;
pub mod tray;
