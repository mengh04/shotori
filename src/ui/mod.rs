//! # UI: gpui windows and elements
//!
//! - [`overlay`]: the layer-shell overlay assembly (events → model,
//!   model → rendering); deliberately thin — logic lives in
//!   `crate::model`, actions in `crate::actions`
//! - [`hud`]: selection backdrop / label / hint bar / hover outline
//! - [`toolbar`]: the post-selection button row
//! - [`placement`] lives in `crate::model` — the geometry both of these
//!   consume
//! - [`ocr_setup`]: first-run model-download dialog
//! - [`e2e`]: runtime debug backdoors driving headless tests
//!   (`SHOTORI_DEBUG_*`)
//! - [`theme`]: visual constants
//! - [`image_util`]: RGBA → RenderImage (the BGRA contract)

pub mod e2e;
pub mod hud;
pub mod image_util;
pub mod ocr_setup;
pub mod overlay;
pub mod theme;
pub mod toolbar;
