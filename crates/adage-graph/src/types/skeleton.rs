//! Run skeletons: a value read as alternating alphanumeric runs and punctuation literals
//! (`BIL-1041` → `U{3} '-' D{4}`), and the character-set lattice that classifies a run.

use adage_core::vtype::{segments, Class};

/// One element of a value's skeleton.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Item<'a> {
    Alnum(&'a str),
    Punct(char),
}

/// The shape of an item, without its content: what anti-unification aligns on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Sig {
    Alnum,
    Punct(char),
}

pub fn items(v: &str) -> Vec<Item<'_>> {
    segments(v)
        .into_iter()
        .map(|s| {
            if s.as_bytes()[0].is_ascii_alphanumeric() {
                Item::Alnum(s)
            } else {
                Item::Punct(s.chars().next().unwrap_or(' '))
            }
        })
        .collect()
}

pub fn signature(items: &[Item<'_>]) -> Vec<Sig> {
    items
        .iter()
        .map(|i| match i {
            Item::Alnum(_) => Sig::Alnum,
            Item::Punct(c) => Sig::Punct(*c),
        })
        .collect()
}

/// A readable form of a signature: `a-a`, `a/a-a`.
pub fn sig_text(sig: &[Sig]) -> String {
    sig.iter()
        .map(|s| match s {
            Sig::Alnum => 'a',
            Sig::Punct(c) => *c,
        })
        .collect()
}

/// The set of ASCII alphanumerics in a string, as a bitmask: digits, then A-Z, then a-z.
pub fn charset(s: &str) -> u64 {
    s.bytes().fold(0u64, |m, b| {
        m | match b {
            b'0'..=b'9' => 1 << (b - b'0'),
            b'A'..=b'Z' => 1 << (10 + b - b'A'),
            b'a'..=b'z' => 1 << (36 + b - b'a'),
            _ => 0,
        }
    })
}

const DIGITS: u64 = (1 << 10) - 1;
const UPPER: u64 = ((1u64 << 26) - 1) << 10;
const LOWER: u64 = ((1u64 << 26) - 1) << 36;
const HEX: u64 = DIGITS | (((1u64 << 6) - 1) << 36);

/// The least general class covering a character set: `D`, then `U`, `L`, then `H` (digits mixed
/// with `a-f`), else `W`. So `U ∪ D` is `W`, and `D ∪ {a-f}` is `H`.
pub fn min_class(mask: u64) -> Class {
    if mask & !DIGITS == 0 {
        Class::D
    } else if mask & !UPPER == 0 {
        Class::U
    } else if mask & !LOWER == 0 {
        Class::L
    } else if mask & !HEX == 0 {
        Class::H
    } else {
        Class::W
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skeletons_of_the_design_examples() {
        let show = |v: &str| {
            items(v)
                .iter()
                .map(|i| match i {
                    Item::Alnum(s) => format!("{}{{{}}}", min_class(charset(s)).name(), s.len()),
                    Item::Punct(c) => format!("'{c}'"),
                })
                .collect::<Vec<_>>()
                .join(" ")
        };
        assert_eq!(show("BIL-1041"), "U{3} '-' D{4}");
        assert_eq!(show("#123"), "'#' D{3}");
        assert_eq!(show("3fa85f64-5717-4562-b3fc-2c963f66afa6"), "H{8} '-' D{4} '-' D{4} '-' H{4} '-' H{12}");
        assert_eq!(show("fix/login-42"), "L{3} '/' L{5} '-' D{2}");
        assert_eq!(min_class(charset("BIL1041")), Class::W);
        assert_eq!(min_class(charset("abc")), Class::L);
        assert_eq!(min_class(charset("ab12")), Class::H);
    }
}
