//! # Platform: compositor and desktop integration
//!
//! Everything that talks past gpui to the outside world, split per-OS
//! behind platform-neutral signatures:
//! - [`capture`]: screen freeze of all outputs — wlr-screencopy on
//!   Linux (the `wayland` submodule is the event state machine) or GDI
//!   `BitBlt` per monitor on Windows; `pixels` is pure processing
//! - [`display`]: capture ↔ gpui display matching (position-based,
//!   same formula on both platforms)
//! - [`windowsnap`]: window-rect backends for click-to-capture —
//!   niri / sway / Hyprland IPC on Linux, `EnumWindows` on Windows

pub mod capture;
pub mod display;
pub mod windowsnap;

// Windows-only post-creation overlay window corrections (client-origin
// alignment; see the module docs for the upstream asymmetry it fixes)
#[cfg(target_os = "windows")]
pub(crate) mod overlay_fixup;
