//! # Platform: compositor and desktop integration
//!
//! Everything that talks past gpui to the outside world:
//! - [`capture`]: wlr-screencopy freeze of all outputs (the `wayland`
//!   submodule is the event state machine, `pixels` is pure processing)
//! - [`display`]: capture ↔ gpui display matching (position-based)
//! - [`windowsnap`]: per-compositor window-rect backends for snapping

pub mod capture;
pub mod display;
pub mod windowsnap;
