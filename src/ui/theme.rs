//! Shotori visual constants — the seed of a homegrown design system.
//! We take the gpui-base custom-drawing route (see ROADMAP); colors and
//! spacing live here so they can grow into a theme system.

/// Dim layer outside the selection: 55% black
pub const DIM: u32 = 0x0000008C;
/// Accent color: orange (selection border)
pub const ACCENT: u32 = 0xFF6A00FF;
/// Pin window border when unfocused (used by the `pin` branch)
pub const PIN_BORDER: u32 = 0xFF6A0099;
/// Hint bar / chip background: dark gray-blue at 90%
pub const CHIP_BG: u32 = 0x16161DE6;
/// Hint bar text: light gray
pub const HINT_TEXT: u32 = 0xAAAAAAFF;
/// Toolbar button text: near-white
pub const BTN_TEXT: u32 = 0xEEEEEEFF;
/// Toolbar button hover background: 15% white
pub const BTN_HOVER_BG: u32 = 0xFFFFFF26;

/// Opaque annotation colors shared by the toolbar, preview and PNG export.
pub(crate) const ANNOTATION_COLORS: [(u32, &str); 7] = [
    (0xFF4545FF, "Red"),
    (0xFF8A32FF, "Orange"),
    (0xFFD43BFF, "Yellow"),
    (0x40C878FF, "Green"),
    (0x409CFFFF, "Blue"),
    (0x222222FF, "Black"),
    (0xFFFFFFFF, "White"),
];

// Floating annotation toolbar: neutral light surfaces keep colored swatches legible.
pub(crate) const TOOLBAR_BG: u32 = 0xFAFAFCFF;
pub(crate) const TOOLBAR_TEXT: u32 = 0x30343BFF;
pub(crate) const TOOLBAR_BORDER: u32 = 0xD9DCE2FF;
pub(crate) const TOOLBAR_HOVER: u32 = 0xEAEDF2FF;
pub(crate) const TOOLBAR_SELECTED: u32 = 0xFFE5D3FF;

// Low-contrast gray cells keep the filled mosaic icon visually quiet.
pub(crate) const MOSAIC_DARK: u32 = 0x858A93FF;
pub(crate) const MOSAIC_LIGHT: u32 = 0xCDD0D6FF;
