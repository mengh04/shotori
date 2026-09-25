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