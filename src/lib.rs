//! # Shotori — a Wayland-first screenshot tool built with gpui-kit
//!
//! Module map (assembled in `main.rs`: scoped keybinding + window creation):
//! - [`capture`]: wlr-screencopy capture (multi-output; the pixels submodule
//!   is pure pixel processing + tests, wayland is the event state machine)
//! - [`display`]: capture ↔ gpui display matching (position-based; includes
//!   the async wait for upstream zed#46378)
//! - [`clipboard`]: copy to clipboard (zwlr_data_control + resident
//!   background daemon)
//! - [`notify`]: desktop notifications (detached child process)
//! - [`selection`]: selection state machine (pure logic + unit tests)
//! - [`export`]: crop → PNG encoding → clipboard/disk (pure functions +
//!   unit tests)
//! - [`image_util`]: RGBA → RenderImage (the BGRA contract lives here)
//! - [`overlay`]: overlay assembly (layer-shell Overlay layer, one per screen)
//! - [`hud`]: overlay visuals (dim strips / selection border / hint bar)
//! - [`toolbar`]: selection toolbar (buttons share the keyboard action
//!   pipeline via dispatch_action)
//! - [`theme`]: visual constants (the seed of a homegrown design system)
//! - [`ocr`]: selection OCR (rapidocr-core + PP-OCRv6 models, default feature)
//!
//! The pin (floating image) feature lives on the `pin` branch (it depends on
//! the vendored set_layer_margin patch; can't ship on crates.io until
//! upstream merges it).

pub mod capture;
pub mod clipboard;
pub mod display;
pub mod export;
pub mod hud;
pub mod image_util;
pub mod notify;
pub mod overlay;
pub mod selection;
pub mod theme;
pub mod toolbar;

#[cfg(feature = "ocr")]
pub mod ocr;
#[cfg(feature = "ocr")]
pub mod ocr_setup;
