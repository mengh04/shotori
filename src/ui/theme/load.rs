//! # Theme resolution: CLI flag + JSON override file
//!
//! The rules, one line each:
//! - `--theme <name>` — a built-in: `dark` (default) / `light` / `high_contrast`
//! - `--theme <path>` — a JSON file; may name a `base`, defaults to `dark`
//! - no flag, but `$XDG_CONFIG_HOME/shotori/theme.json` (default
//!   `~/.config/shotori/theme.json`) exists — that file loads
//! - otherwise — `dark`
//!
//! Unknown fields, bad hex or out-of-range values are reported to stderr
//! and **the offending value falls back** — a typo in a color file must
//! never cost anyone a screenshot. `--print-theme` prints the resolved
//! theme (and the errors, non-zero exit) instead of running.

use std::path::{Path, PathBuf};

use serde::Deserialize;

use super::{PALETTE, Theme};

/// The JSON override file: every field optional, misspellings rejected.
#[derive(Deserialize, Default, Debug, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct ThemeFile {
    /// JSON has no comments; a single `"//"` key is accepted as one.
    #[serde(rename = "//")]
    pub comment: Option<String>,
    /// Built-in base to start from before applying overrides.
    pub base: Option<String>,
    pub dim_color: Option<String>,
    /// 0.0 .. 1.0
    pub dim_opacity: Option<f32>,
    pub accent: Option<String>,
    pub pin_border: Option<String>,
    pub chip_bg: Option<String>,
    pub hint_text: Option<String>,
    pub btn_text: Option<String>,
    pub btn_hover_bg: Option<String>,
    pub toolbar_bg: Option<String>,
    pub toolbar_text: Option<String>,
    pub toolbar_border: Option<String>,
    pub toolbar_hover: Option<String>,
    pub toolbar_selected: Option<String>,
    pub swatch_border: Option<String>,
    pub mosaic_dark: Option<String>,
    pub mosaic_light: Option<String>,
    /// Up to [`PALETTE`] colors; missing tail keeps the base values.
    pub annotation_colors: Option<Vec<String>>,
}

/// `#RRGGBB` or `#RRGGBBAA` (the `#` is optional) → `0xRRGGBBAA`.
fn hex(s: &str) -> Result<u32, String> {
    let s = s.strip_prefix('#').unwrap_or(s);
    let expect_alpha = match s.len() {
        6 => false,
        8 => true,
        _ => return Err(format!("\"{s}\": expected #RRGGBB or #RRGGBBAA")),
    };
    let v = u32::from_str_radix(s, 16).map_err(|_| format!("\"{s}\": not a hex color"))?;
    Ok(if expect_alpha { v } else { v << 8 | 0xFF })
}

/// `0xRRGGBBAA` → `#RRGGBBAA` (for `--print-theme`).
fn to_hex(v: u32) -> String {
    format!("#{:08X}", v)
}

/// Where the active theme came from (for logs and `--print-theme`).
#[derive(Debug, PartialEq)]
pub enum Source {
    Default,
    BuiltIn(String),
    File(PathBuf),
}

/// Built-in name → theme.
pub fn built_in(name: &str) -> Result<Theme, String> {
    match name {
        "dark" => Ok(Theme::dark()),
        "light" => Ok(Theme::light()),
        "high_contrast" => Ok(Theme::high_contrast()),
        _ => Err(format!(
            "unknown theme \"{name}\" (want dark | light | high_contrast)"
        )),
    }
}

/// Apply every present override onto `theme`. First error wins; the
/// caller decides whether to keep the partially applied theme (CLI does:
/// a later bad field should not undo earlier good ones).
pub fn apply(theme: &mut Theme, file: &ThemeFile) -> Result<(), Vec<String>> {
    let mut errs = Vec::new();

    macro_rules! color {
        ($field:ident) => {
            if let Some(v) = &file.$field {
                match hex(v) {
                    Ok(c) => theme.$field = c,
                    Err(e) => errs.push(format!("{}: {e}", stringify!($field))),
                }
            }
        };
    }

    if let Some(v) = &file.dim_color {
        match hex(v) {
            Ok(c) => theme.dim_color = c,
            Err(e) => errs.push(format!("dim_color: {e}")),
        }
    }
    if let Some(v) = file.dim_opacity {
        if (0.0..=1.0).contains(&v) {
            theme.dim_opacity = v;
        } else {
            errs.push(format!("dim_opacity: {v} out of range 0.0..1.0"));
        }
    }
    color!(accent);
    color!(pin_border);
    color!(chip_bg);
    color!(hint_text);
    color!(btn_text);
    color!(btn_hover_bg);
    color!(toolbar_bg);
    color!(toolbar_text);
    color!(toolbar_border);
    color!(toolbar_hover);
    color!(toolbar_selected);
    color!(swatch_border);
    color!(mosaic_dark);
    color!(mosaic_light);

    if let Some(list) = &file.annotation_colors {
        if list.len() > PALETTE {
            errs.push(format!(
                "annotation_colors: {} entries, max {PALETTE}",
                list.len()
            ));
        } else {
            for (slot, v) in theme.annotation_colors.iter_mut().zip(list) {
                match hex(v) {
                    Ok(c) => *slot = c,
                    Err(e) => errs.push(format!("annotation_colors: {e}")),
                }
            }
        }
    }

    if errs.is_empty() { Ok(()) } else { Err(errs) }
}

/// Load + parse a theme file (base + overrides in one document).
pub fn load_file(path: &Path) -> Result<(Theme, Vec<String>), String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let file: ThemeFile =
        serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut theme = match file.base.as_deref() {
        None | Some("dark") => Theme::dark(),
        Some(name) => built_in(name).map_err(|e| format!("{}: {e}", path.display()))?,
    };
    let errs = match apply(&mut theme, &file) {
        Ok(()) => Vec::new(),
        Err(es) => es,
    };
    Ok((theme, errs))
}

/// The auto-pickup config file: `$XDG_CONFIG_HOME/shotori/theme.json`
/// (falling back to `~/.config/shotori/theme.json`) on Linux,
/// `%APPDATA%\shotori\theme.json` on Windows.
pub fn config_path() -> Option<PathBuf> {
    #[cfg(target_os = "linux")]
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    #[cfg(target_os = "windows")]
    let base = std::env::var_os("APPDATA").map(PathBuf::from)?;
    Some(base.join("shotori").join("theme.json"))
}

/// Resolve + install from the parsed command line ([`crate::args::Cli`]).
/// `--print-theme` prints and exits 0 (or 1 on errors).
pub fn init(args: &crate::args::Cli) {
    let (theme, source, errs) = resolve(&args.theme, args.no_config);
    for e in &errs {
        eprintln!("[shotori] theme: {e}");
    }
    if args.print_theme {
        print_theme(&theme, &source);
        std::process::exit(if errs.is_empty() { 0 } else { 1 });
    }
    super::set(theme);
}

/// The resolution pipeline shared by [`init`] and tests.
fn resolve(value: &Option<String>, no_config: bool) -> (Theme, Source, Vec<String>) {
    match value {
        // A name → built-in. A path separator → file. Best effort.
        Some(v) => {
            if v.contains('/') || v.contains('.') {
                let (theme, errs) =
                    load_file(Path::new(v)).unwrap_or_else(|e| (Theme::dark(), vec![e]));
                (theme, Source::File(PathBuf::from(v)), errs)
            } else {
                match built_in(v) {
                    Ok(t) => (t, Source::BuiltIn(v.clone()), Vec::new()),
                    Err(e) => (Theme::dark(), Source::Default, vec![e]),
                }
            }
        }
        None if no_config => (Theme::dark(), Source::Default, Vec::new()),
        None => match config_path().filter(|p| p.exists()) {
            Some(p) => {
                let (theme, errs) = load_file(&p).unwrap_or_else(|e| (Theme::dark(), vec![e]));
                (theme, Source::File(p), errs)
            }
            None => (Theme::dark(), Source::Default, Vec::new()),
        },
    }
}

/// `--print-theme` output: source line, one line per field, palette tail.
fn print_theme(theme: &Theme, source: &Source) {
    use std::io::Write;
    // writeln + ignore errors: piping into `head` must not panic on the
    // broken pipe, a truncated dump is the expected outcome there
    let mut out = std::io::stdout().lock();
    let src = match source {
        Source::Default => "default (dark)".to_string(),
        Source::BuiltIn(n) => format!("built-in {n}"),
        Source::File(p) => p.display().to_string(),
    };
    let _ = writeln!(out, "shotori theme — source: {src}");
    let _ = writeln!(
        out,
        "dim            {} @ opacity {:.2}",
        to_hex(theme.dim_color),
        theme.dim_opacity
    );
    let _ = writeln!(out, "accent         {}", to_hex(theme.accent));
    let _ = writeln!(out, "pin_border     {}", to_hex(theme.pin_border));
    let _ = writeln!(out, "chip_bg        {}", to_hex(theme.chip_bg));
    let _ = writeln!(out, "hint_text      {}", to_hex(theme.hint_text));
    let _ = writeln!(out, "btn_text       {}", to_hex(theme.btn_text));
    let _ = writeln!(out, "btn_hover_bg   {}", to_hex(theme.btn_hover_bg));
    let _ = writeln!(out, "toolbar_bg     {}", to_hex(theme.toolbar_bg));
    let _ = writeln!(out, "toolbar_text   {}", to_hex(theme.toolbar_text));
    let _ = writeln!(out, "toolbar_border {}", to_hex(theme.toolbar_border));
    let _ = writeln!(out, "toolbar_hover  {}", to_hex(theme.toolbar_hover));
    let _ = writeln!(out, "toolbar_selected {}", to_hex(theme.toolbar_selected));
    let _ = writeln!(out, "swatch_border   {}", to_hex(theme.swatch_border));
    let _ = writeln!(out, "mosaic_dark    {}", to_hex(theme.mosaic_dark));
    let _ = writeln!(out, "mosaic_light   {}", to_hex(theme.mosaic_light));
    let palette = theme
        .annotation_colors
        .iter()
        .map(|c| to_hex(*c))
        .collect::<Vec<_>>()
        .join(" ");
    let _ = writeln!(out, "annotation_colors {palette}");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_accepts_six_eight_and_optional_hash() {
        assert_eq!(hex("#FF6A00").unwrap(), 0xFF6A00FF);
        assert_eq!(hex("FF6A0099").unwrap(), 0xFF6A0099);
        assert_eq!(hex("#00000000").unwrap(), 0x00000000);
        assert!(hex("#12345").is_err());
        assert!(hex("GGHHII").is_err());
    }

    #[test]
    fn partial_override_touches_only_that_field() {
        let mut t = Theme::dark();
        let f: ThemeFile = serde_json::from_str(r##"{"accent": "#00FF00"}"##).unwrap();
        apply(&mut t, &f).unwrap();
        assert_eq!(t.accent, 0x00FF00FF);
        assert_eq!(t.chip_bg, Theme::dark().chip_bg);
        assert_eq!(t.dim_opacity, Theme::dark().dim_opacity);
    }

    #[test]
    fn unknown_fields_are_rejected() {
        assert!(serde_json::from_str::<ThemeFile>(r##"{"acccent": "#fff"}"##).is_err());
        // one "//" comment key is legal, two are a duplicate-field error
        assert!(serde_json::from_str::<ThemeFile>(r##"{"//": "note"}"##).is_ok());
        assert!(serde_json::from_str::<ThemeFile>(r##"{"//": "a", "//": "b"}"##).is_err());
    }

    #[test]
    fn short_palette_overrides_prefix_long_palette_errors() {
        let mut t = Theme::dark();
        let f: ThemeFile = serde_json::from_str(r##"{"annotation_colors": ["#111111"]}"##).unwrap();
        apply(&mut t, &f).unwrap();
        assert_eq!(t.annotation_colors[0], 0x111111FF);
        assert_eq!(t.annotation_colors[1], Theme::dark().annotation_colors[1]);

        let f: ThemeFile = serde_json::from_str(
            r##"{"annotation_colors": ["#111","#222","#333","#444","#555","#666","#777","#888"]}"##,
        )
        .unwrap();
        assert!(apply(&mut t, &f).is_err());
    }

    #[test]
    fn bad_values_collect_errors_but_good_ones_apply() {
        let mut t = Theme::dark();
        let f: ThemeFile = serde_json::from_str(
            r##"{"accent": "#00FF00", "dim_opacity": 9.5, "chip_bg": "zzz"}"##,
        )
        .unwrap();
        let errs = apply(&mut t, &f).unwrap_err();
        assert_eq!(errs.len(), 2);
        assert_eq!(t.accent, 0x00FF00FF); // the good one stuck
    }

    #[test]
    fn file_load_with_base_and_overrides() {
        let dir = std::env::temp_dir().join("shotori-theme-test");
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("t.json");
        std::fs::write(
            &p,
            r##"{"base": "light", "accent": "#123456", "dim_opacity": 0.2}"##,
        )
        .unwrap();
        let (t, errs) = load_file(&p).unwrap();
        assert!(errs.is_empty());
        assert_eq!(t.accent, 0x123456FF);
        assert_eq!(t.dim_opacity, 0.2);
        assert_eq!(t.chip_bg, Theme::light().chip_bg); // base came through
    }
}
