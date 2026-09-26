//! # Shotori theme system
//!
//! Every visual constant of the overlay lives here, grouped into a
//! [`Theme`] struct instead of loose constants, so the look can be
//! swapped as a whole. Resolution order (see [`load`]):
//!
//! 1. a built-in base theme — `dark` (default) / `light` / `high_contrast`
//! 2. an optional JSON override file — `--theme <file>` or the XDG
//!    config fallback (`~/.config/shotori/theme.json`)
//!
//! The theme is installed once at startup ([`set`]) and read everywhere
//! ([`c`]); the overlay lives for seconds, so there is no hot-swap story.
//!
//! Colors are `u32` in gpui's `0xRRGGBBAA` layout (e.g. the orange
//! accent is `0xFF6A00FF`).

use std::sync::OnceLock;

pub mod load;

/// How many swatches the annotation palette carries.
pub const PALETTE: usize = 7;

/// Swatch display names (UI copy stays in code; themes carry only colors).
pub const PALETTE_NAMES: [&str; PALETTE] =
    ["Red", "Orange", "Yellow", "Green", "Blue", "Black", "White"];

/// The complete visual vocabulary of the overlay. One struct so a theme
/// is a value: build it, override it, ship it.
pub struct Theme {
    /// Dim layer OUTSIDE the selection: color at full opacity + strength.
    /// What gets painted is [`Theme::dim`] (color with alpha applied).
    pub dim_color: u32,
    /// Dim strength, 0.0 (clear) .. 1.0 (solid). Default dark: 0.55.
    pub dim_opacity: f32,
    /// Accent color: selection border, focused controls, OCR chip border.
    pub accent: u32,
    /// Pin window border when unfocused (used by the `pin` branch).
    pub pin_border: u32,

    /// Hint bar / chip background (OCR setup dialog).
    pub chip_bg: u32,
    /// Hint bar / chip text.
    pub hint_text: u32,
    /// Classic (dark chip) button text.
    pub btn_text: u32,
    /// Classic button hover background.
    pub btn_hover_bg: u32,

    /// Floating annotation toolbar. dark uses dark surfaces (the
    /// historical near-white toolbar clashed with the dark chrome).
    pub toolbar_bg: u32,
    pub toolbar_text: u32,
    pub toolbar_border: u32,
    pub toolbar_hover: u32,
    pub toolbar_selected: u32,
    /// Swatch outline: needs contrast against BOTH the toolbar and the
    /// swatch fill (a #222222 swatch on a dark toolbar is invisible
    /// without it), so it does not simply reuse `toolbar_border`.
    pub swatch_border: u32,

    /// Mosaic tool icon grays (keep the filled icon visually quiet).
    pub mosaic_dark: u32,
    pub mosaic_light: u32,

    /// Annotation swatch palette (opaque — exported PNGs must be solid).
    pub annotation_colors: [u32; PALETTE],
}

impl Theme {
    /// The default look since v0.1 — dark chips over a 55% dim.
    pub const fn dark() -> Self {
        Self {
            dim_color: 0x000000FF,
            dim_opacity: 0.55,
            accent: 0xFF6A00FF,
            pin_border: 0xFF6A0099,
            chip_bg: 0x16161DE6,
            hint_text: 0xAAAAAAFF,
            btn_text: 0xEEEEEEFF,
            btn_hover_bg: 0xFFFFFF26,
            toolbar_bg: 0x202028FF,
            toolbar_text: 0xE8E8EEFF,
            toolbar_border: 0x3E3E4CFF,
            toolbar_hover: 0x2C2C3AFF,
            toolbar_selected: 0x453528FF,
            swatch_border: 0xFFFFFF38,
            mosaic_dark: 0x858A93FF,
            mosaic_light: 0xCDD0D6FF,
            annotation_colors: [
                0xFF4545FF, 0xFF8A32FF, 0xFFD43BFF, 0x40C878FF, 0x409CFFFF, 0x222222FF, 0xFFFFFFFF,
            ],
        }
    }

    /// Light chips for bright wallpapers: same accent, softer surfaces.
    /// Toolbar fields are spelled out (not inherited): dark owns dark
    /// surfaces since the white-toolbar fix, inheritance would leak them.
    pub const fn light() -> Self {
        Self {
            chip_bg: 0xFAFAFCE6,
            hint_text: 0x30343BFF,
            btn_text: 0x30343BFF,
            btn_hover_bg: 0x00000014,
            pin_border: 0x30343B99,
            toolbar_bg: 0xFAFAFCFF,
            toolbar_text: 0x30343BFF,
            toolbar_border: 0xC3C7CEFF,
            toolbar_hover: 0xEAEDF2FF,
            toolbar_selected: 0xFFE5D3FF,
            swatch_border: 0x00000026,
            ..dark_const()
        }
    }

    /// Maximum-contrast UI copy: pure black chips, white text, hard borders.
    pub const fn high_contrast() -> Self {
        Self {
            chip_bg: 0x000000FA,
            hint_text: 0xFFFFFFFF,
            btn_text: 0xFFFFFFFF,
            toolbar_bg: 0xFFFFFFFF,
            toolbar_text: 0x000000FF,
            toolbar_border: 0x000000FF,
            toolbar_hover: 0x00000026,
            toolbar_selected: 0x30303AFF,
            swatch_border: 0x00000059,
            ..dark_const()
        }
    }

    /// The dim layer quad color: `dim_color` with `dim_opacity` baked in
    /// (this is what `rgba()` wants at the call sites).
    pub fn dim(&self) -> u32 {
        let a = (self.dim_opacity.clamp(0., 1.) * 255.).round() as u32;
        (self.dim_color & 0xFFFFFF00) | a.min(0xFF)
    }
}

/// `..dark()` spelled for const fn (struct-update syntax is not const).
const fn dark_const() -> Theme {
    Theme::dark()
}

static ACTIVE: OnceLock<Theme> = OnceLock::new();
const FALLBACK: Theme = Theme::dark();

/// The process-wide active theme. Falls back to `dark` when nobody
/// called [`set`] (tests, library embeds).
pub fn c() -> &'static Theme {
    ACTIVE.get().unwrap_or(&FALLBACK)
}

/// Install the theme; called once during startup. A second call is
/// ignored — resolution happened, the decision is final.
pub fn set(theme: Theme) {
    let _ = ACTIVE.set(theme);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// dark().dim() must equal the original hard-coded DIM constant.
    #[test]
    fn dark_dim_matches_legacy_constant() {
        assert_eq!(Theme::dark().dim(), 0x0000008C);
    }

    #[test]
    fn dim_opacity_clamps() {
        let mut t = Theme::dark();
        t.dim_opacity = -1.;
        assert_eq!(t.dim() & 0xFF, 0);
        t.dim_opacity = 2.;
        assert_eq!(t.dim() & 0xFF, 0xFF);
        t.dim_opacity = 0.;
        assert_eq!(t.dim(), 0x00000000);
    }

    /// Annotation colors must stay opaque in every built-in theme:
    /// they go into exported PNGs.
    #[test]
    fn builtin_palette_is_opaque_and_full() {
        for t in [Theme::dark(), Theme::light(), Theme::high_contrast()] {
            assert_eq!(t.annotation_colors.len(), PALETTE);
            assert!(t.annotation_colors.iter().all(|c| c & 0xFF == 0xFF));
        }
    }
}
