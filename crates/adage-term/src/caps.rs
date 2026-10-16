//! What the terminal can do, decided once from the environment.
//!
//! Precedence, highest first:
//!
//! 1. `ADAGE_COLOR=never|16|256|truecolor` (an explicit override for people and tests);
//! 2. `NO_COLOR` (no-color.org): no colour at all;
//! 3. `CLICOLOR_FORCE` / `FORCE_COLOR`: colour even when not a terminal (at least 16 colours);
//! 4. not a terminal, or `TERM=dumb`: no colour;
//! 5. `COLORTERM=truecolor|24bit`, or a terminal known to render truecolor: truecolor;
//!    `TERM=*256color*`: 256 colours; anything else: the 16-colour theme.
//!
//! The ground is dark unless `ADAGE_THEME=day|light`, or `COLORFGBG` says the background is
//! light. Glyphs fall back to ASCII with `ADAGE_ASCII=1`, `TERM=dumb`, or a locale that is set
//! and not UTF-8 (`LANG=C`).

/// Colour depth.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    /// No colour: only bold, dim, italic, underline and reverse.
    None,
    /// The user's own 16-colour theme.
    Ansi16,
    /// The xterm-256 palette.
    Ansi256,
    /// 24-bit colour.
    TrueColor,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Caps {
    pub level: Level,
    /// UTF-8 glyphs (`●`, `◉`, `→`); otherwise their ASCII fallbacks.
    pub unicode: bool,
    /// The terminal's background is light: use the day values of each colour.
    pub day: bool,
    /// OSC 8 hyperlinks render as links.
    pub hyperlinks: bool,
    /// Bold and dim may be used. True at a terminal even under `NO_COLOR` (which is about
    /// colour only); false for pipes and files, which get no escape codes at all.
    pub effects: bool,
}

impl Caps {
    /// Plain text: no colour, UTF-8 glyphs, no links. What pipes and snapshots get.
    pub const fn plain() -> Caps {
        Caps { level: Level::None, unicode: true, day: false, hyperlinks: false, effects: false }
    }

    /// The capabilities of standard output.
    pub fn stdout() -> Caps {
        use std::io::IsTerminal;
        Caps::from_env(|k| std::env::var(k).ok(), std::io::stdout().is_terminal())
    }

    /// The capabilities of standard error.
    pub fn stderr() -> Caps {
        use std::io::IsTerminal;
        Caps::from_env(|k| std::env::var(k).ok(), std::io::stderr().is_terminal())
    }

    /// Decide from an environment lookup and whether the stream is a terminal (testable).
    pub fn from_env(env: impl Fn(&str) -> Option<String>, is_tty: bool) -> Caps {
        let set = |k: &str| env(k).filter(|v| !v.is_empty());
        let term = set("TERM").unwrap_or_default();
        let dumb = term == "dumb";
        let level = match set("ADAGE_COLOR").as_deref() {
            Some("never" | "none" | "0") => Level::None,
            Some("16") => Level::Ansi16,
            Some("256") => Level::Ansi256,
            Some("truecolor" | "24bit") => Level::TrueColor,
            _ => {
                let forced = ["CLICOLOR_FORCE", "FORCE_COLOR"].iter().any(|k| set(k).is_some_and(|v| v != "0"));
                if set("NO_COLOR").is_some() || ((!is_tty || dumb) && !forced) {
                    Level::None
                } else {
                    let rich = |k: &str| set(k).map(|v| v.to_ascii_lowercase());
                    let truecolor = matches!(rich("COLORTERM").as_deref(), Some("truecolor" | "24bit"))
                        || matches!(
                            set("TERM_PROGRAM").as_deref(),
                            Some("iTerm.app" | "WezTerm" | "vscode" | "ghostty" | "Hyper" | "rio")
                        )
                        || set("WT_SESSION").is_some()
                        || term.contains("kitty")
                        || term.contains("alacritty")
                        || term.contains("direct");
                    if truecolor {
                        Level::TrueColor
                    } else if term.contains("256") {
                        Level::Ansi256
                    } else {
                        Level::Ansi16
                    }
                }
            }
        };
        let effects = level > Level::None || (is_tty && !dumb);
        let unicode = if set("ADAGE_ASCII").is_some_and(|v| v != "0") || dumb {
            false
        } else {
            // The first locale variable that is set decides, as in setlocale(3).
            match ["LC_ALL", "LC_CTYPE", "LANG"].iter().find_map(|k| set(k)) {
                Some(l) => {
                    let l = l.to_ascii_lowercase();
                    l.contains("utf-8") || l.contains("utf8")
                }
                None => true,
            }
        };
        let day = match set("ADAGE_THEME").as_deref() {
            Some("day" | "light") => true,
            Some("night" | "dark") => false,
            _ => set("COLORFGBG")
                .and_then(|v| v.rsplit(';').next().and_then(|bg| bg.parse::<u8>().ok()))
                .is_some_and(|bg| bg == 7 || bg == 15),
        };
        let hyperlinks = is_tty
            && !dumb
            && (set("FORCE_HYPERLINK").is_some_and(|v| v != "0")
                || set("WT_SESSION").is_some()
                || set("KITTY_WINDOW_ID").is_some()
                || matches!(set("TERM_PROGRAM").as_deref(), Some("iTerm.app" | "WezTerm" | "vscode" | "ghostty"))
                || set("VTE_VERSION").and_then(|v| v.parse::<u32>().ok()).is_some_and(|v| v >= 5000));
        Caps { level, unicode, day, hyperlinks, effects }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn caps(vars: &[(&str, &str)], tty: bool) -> Caps {
        let vars: Vec<(String, String)> = vars.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        Caps::from_env(|k| vars.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone()), tty)
    }

    #[test]
    fn no_color_wins_over_everything_but_the_explicit_override() {
        assert_eq!(caps(&[("NO_COLOR", "1"), ("COLORTERM", "truecolor")], true).level, Level::None);
        assert_eq!(caps(&[("NO_COLOR", "1"), ("CLICOLOR_FORCE", "1")], true).level, Level::None);
        assert_eq!(caps(&[("NO_COLOR", "1"), ("ADAGE_COLOR", "256")], true).level, Level::Ansi256);
        // An empty NO_COLOR is unset (no-color.org).
        assert_eq!(caps(&[("NO_COLOR", ""), ("TERM", "xterm")], true).level, Level::Ansi16);
    }

    #[test]
    fn pipes_are_plain_unless_forced() {
        assert_eq!(caps(&[("COLORTERM", "truecolor")], false).level, Level::None);
        assert_eq!(caps(&[("CLICOLOR_FORCE", "1"), ("TERM", "xterm")], false).level, Level::Ansi16);
        assert_eq!(caps(&[("FORCE_COLOR", "0")], false).level, Level::None);
    }

    #[test]
    fn depth_from_term_and_colorterm() {
        assert_eq!(caps(&[("TERM", "xterm-256color")], true).level, Level::Ansi256);
        assert_eq!(caps(&[("TERM", "xterm-256color"), ("COLORTERM", "truecolor")], true).level, Level::TrueColor);
        assert_eq!(caps(&[("TERM", "xterm")], true).level, Level::Ansi16);
        assert_eq!(caps(&[("TERM", "dumb")], true).level, Level::None);
        assert_eq!(caps(&[("WT_SESSION", "x")], true).level, Level::TrueColor);
    }

    #[test]
    fn no_color_keeps_bold_and_dim_at_a_terminal_but_not_in_a_pipe() {
        assert!(caps(&[("NO_COLOR", "1")], true).effects);
        assert!(!caps(&[("NO_COLOR", "1")], false).effects);
        assert!(!caps(&[], false).effects);
        assert!(!caps(&[("TERM", "dumb")], true).effects);
    }

    #[test]
    fn glyphs_fall_back_to_ascii_for_non_utf8_locales() {
        assert!(caps(&[], true).unicode);
        assert!(caps(&[("LANG", "en_US.UTF-8")], true).unicode);
        assert!(!caps(&[("LANG", "C")], true).unicode);
        assert!(caps(&[("LC_ALL", "C.UTF-8"), ("LANG", "C")], true).unicode);
        assert!(!caps(&[("ADAGE_ASCII", "1")], true).unicode);
        assert!(!caps(&[("TERM", "dumb")], true).unicode);
    }

    #[test]
    fn light_grounds_from_override_or_colorfgbg() {
        assert!(!caps(&[], true).day);
        assert!(caps(&[("ADAGE_THEME", "light")], true).day);
        assert!(caps(&[("COLORFGBG", "0;15")], true).day);
        assert!(!caps(&[("COLORFGBG", "15;0")], true).day);
        assert!(!caps(&[("COLORFGBG", "0;15"), ("ADAGE_THEME", "night")], true).day);
    }
}
