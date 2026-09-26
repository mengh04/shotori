//! # Command-line interface: hand-rolled, clap-free
//!
//! shotori's surface is four flags; a parser dependency would outweigh
//! the surface. Rules:
//!
//! - `--help`/`-h`, `--version`/`-V` — the basics every binary owes
//! - `--theme <NAME|FILE>` / `--print-theme` / `--no-config` — see
//!   [`crate::ui::theme::load`]
//! - unknown flags and positional arguments are errors (exit 2)
//!
//! The internal child-process entry points (`--notify`,
//! `--clipboard-daemon`) are matched on `argv[1]` in `main.rs` BEFORE
//! this parser runs — they take trailing free-form arguments and must
//! not be validated as flags.

/// Parsed command line (all flags optional, defaults = plain run).
#[derive(Debug, Default, PartialEq)]
pub struct Args {
    pub theme: Option<String>,
    pub print_theme: bool,
    /// Skip the `~/.config/shotori/theme.json` auto-pickup. An explicit
    /// `--theme` still wins over everything.
    pub no_config: bool,
    pub help: bool,
    pub version: bool,
}

impl Args {
    pub fn parse<I, S>(argv: I) -> Result<Self, String>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let mut args = Args::default();
        let mut iter = argv.into_iter().map(Into::into).peekable();
        while let Some(a) = iter.next() {
            match a.as_str() {
                "--help" | "-h" => args.help = true,
                "--version" | "-V" => args.version = true,
                "--print-theme" => args.print_theme = true,
                "--no-config" => args.no_config = true,
                "--theme" => {
                    let value = iter.next().ok_or_else(|| {
                        "missing value for --theme (dark | light | high_contrast | <file>)"
                            .to_string()
                    })?;
                    args.theme = Some(value);
                }
                other if other.starts_with('-') => {
                    return Err(format!("unexpected argument '{other}'"));
                }
                other => return Err(format!("unexpected argument '{other}'")),
            }
        }
        Ok(args)
    }

    /// `--help` text (version line included, generated per build).
    pub fn help_text() -> String {
        format!(
            "shotori {ver} — Wayland-first screenshot tool

Usage: shotori [OPTIONS]

Options:
  -h, --help               Print help and exit
  -V, --version            Print version and exit
      --theme <NAME|FILE>  Built-in theme (dark | light | high_contrast)
                           or a JSON theme file
      --print-theme        Print the resolved theme and exit
      --no-config          Skip ~/.config/shotori/theme.json auto-pickup

Everything else (select, copy, save, OCR, annotate) happens in
the overlay; see the keybinding table in the README.
",
            ver = env!("CARGO_PKG_VERSION"),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(v: &[&str]) -> Result<Args, String> {
        Args::parse(v.iter().map(|s| s.to_string()))
    }

    #[test]
    fn empty_is_a_plain_run() {
        assert_eq!(parse(&[]).unwrap(), Args::default());
    }

    #[test]
    fn basics() {
        assert_eq!(
            parse(&["-h"]).unwrap(),
            Args {
                help: true,
                ..Default::default()
            }
        );
        assert_eq!(
            parse(&["--version", "-V"]).unwrap(),
            Args {
                version: true,
                ..Default::default()
            }
        );
        assert_eq!(
            parse(&["--print-theme", "--no-config"]).unwrap(),
            Args {
                print_theme: true,
                no_config: true,
                ..Default::default()
            }
        );
    }

    #[test]
    fn theme_takes_the_next_token() {
        let a = parse(&["--theme", "light"]).unwrap();
        assert_eq!(a.theme.as_deref(), Some("light"));
        assert!(parse(&["--theme"]).is_err());
    }

    #[test]
    fn unknown_flags_and_positionals_error() {
        assert!(parse(&["--them"]).is_err());
        assert!(parse(&["-x"]).is_err());
        assert!(parse(&["foo.png"]).is_err());
    }

    #[test]
    fn internal_entry_points_do_not_reach_the_parser_in_main() {
        // documented contract: main.rs matches them on argv[1] first;
        // if they ever leaked through, the error message should say so
        assert!(parse(&["--notify", "s", "b"]).is_err());
    }
}
