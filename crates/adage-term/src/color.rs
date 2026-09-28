//! The value types [`crate::tokens`] is written in.

/// An sRGB colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgb(pub u8, pub u8, pub u8);

/// A role in the terminal's own 16-colour theme. `Default` is the terminal's foreground and
/// `Dim` is the default foreground dimmed: both follow the user's theme exactly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ansi16 {
    Default,
    Dim,
    Black,
    Red,
    Green,
    Yellow,
    Blue,
    Magenta,
    Cyan,
    White,
    BrightBlack,
    BrightRed,
    BrightGreen,
    BrightYellow,
    BrightBlue,
    BrightMagenta,
    BrightCyan,
    BrightWhite,
}

impl Ansi16 {
    /// The ANSI colour index (0–15), or `None` for the default foreground (plain or dimmed).
    pub fn index(self) -> Option<u8> {
        use Ansi16::*;
        Some(match self {
            Default | Dim => return None,
            Black => 0,
            Red => 1,
            Green => 2,
            Yellow => 3,
            Blue => 4,
            Magenta => 5,
            Cyan => 6,
            White => 7,
            BrightBlack => 8,
            BrightRed => 9,
            BrightGreen => 10,
            BrightYellow => 11,
            BrightBlue => 12,
            BrightMagenta => 13,
            BrightCyan => 14,
            BrightWhite => 15,
        })
    }
}

/// One colour token: its night (dark ground) and day (light ground) values, the nearest
/// xterm-256 entry for each, and its role in a 16-colour theme.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Swatch {
    pub night: Rgb,
    pub day: Rgb,
    pub night_256: u8,
    pub day_256: u8,
    pub ansi16: Ansi16,
}
