//! Adage's terminal design system.
//!
//! One palette, defined once in `design/tokens/adage.tokens.json`, drives every terminal cell
//! and every pixel of the web map. This crate is the terminal half:
//!
//! * [`tokens`]: generated from the token file (`cargo xtask tokens`); never edited by hand.
//! * [`caps`]: what the terminal can do (colour level, UTF-8, light or dark ground,
//!   hyperlinks), decided once from the environment.
//! * [`theme`]: semantic roles (`Role::Fire`, `Role::Ink3`, …) resolved against the
//!   capabilities, as `anstyle` styles for printed output and as ratatui styles for the
//!   full-screen views (feature `ratatui`).
//! * [`out`]: the output grammar every command prints through: a header line, sections,
//!   key–value rows with one column rule, lamp rows, notes, warnings, errors, and the `next`
//!   hint that ends every command.
//!
//! Rules (docs/design/style.md): call sites ask for roles, never colours; body text uses the
//! terminal's own foreground; lamp colours mean strength only; violet is the one accent;
//! `NO_COLOR` and pipes get plain text with strengths spelled out.
//!
//! Nothing here is used on the hook path.

pub mod caps;
pub mod color;
pub mod out;
pub mod theme;
pub mod tokens;

pub use caps::{Caps, Level};
pub use out::{Line, Out};
pub use theme::{Glyph, Role, Strength, Theme};
