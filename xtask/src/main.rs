//! Repository tasks, run as `cargo xtask <task>`.
//!
//! `affected [--base <ref>] [--files <path>...] [--json]`: the workspace packages a change can
//! affect, so CI tests only those (`affected.rs`).
//!
//! `test python [<suite>...]`: every Python test suite, each from its own import root (`suites.rs`).
//!
//! `tokens [--check]`: reads `design/tokens/adage.tokens.json` (W3C DTCG 2025.10) and writes
//!
//! * `crates/adage-term/src/tokens.rs`: every colour with its night and day values, the nearest
//!   xterm-256 index for each (computed here, never hand-picked), and its ANSI-16 role; the
//!   glyph table with ASCII fallbacks; the font families;
//! * `design/build/tokens.css`: CSS variables for the web map, night by default, day under
//!   `prefers-color-scheme: light` or `data-theme="day"`;
//! * `plugin/claude-code/hooks/tokens.ts`: for the Claude Code mod, every glyph with its ASCII
//!   twin, and Adage's roles as the engine's theme keys (a mod colours text by theme key, so the
//!   person's Claude Code theme wins, as the terminal's colours do).
//!
//! It also checks WCAG contrast (4.5:1) for every text colour on every ground in both modes.
//! `--check` writes nothing and fails if either output is stale, for CI. Node is not needed.

mod affected;
#[cfg(test)]
mod hygiene;
#[cfg(test)]
mod layers;
mod suites;

use anyhow::{anyhow, bail, Context, Result};
use serde_json::Value;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

const SOURCE: &str = "design/tokens/adage.tokens.json";
const RUST_OUT: &str = "crates/adage-term/src/tokens.rs";
const CSS_OUT: &str = "design/build/tokens.css";
const TS_OUT: &str = "plugin/claude-code/hooks/tokens.ts";

/// Adage's roles (`adage_term::Role`), the colour each takes in the terminal, and the Claude Code
/// theme key a mod draws it with: the nearest key in meaning, never a colour literal.
const ROLES: &[(&str, &str, &str)] = &[
    ("ink", "ink.1", "text"),
    ("ink2", "ink.2", "inactive"),
    ("ink3", "ink.3", "subtle"),
    ("accent", "accent.base", "suggestion"),
    ("command", "accent.soft", "permission"),
    ("fire", "lamp.fire", "success"),
    ("prime", "lamp.prime", "warning"),
    ("inhibit", "lamp.inhibit", "error"),
    ("cost", "cost", "claude"),
];

/// Text colours and the grounds they must be readable on (WCAG AA, 4.5:1).
const TEXT: &[&str] =
    &["ink.1", "ink.2", "ink.3", "accent.base", "accent.soft", "lamp.fire", "lamp.prime", "lamp.inhibit", "cost"];
const GROUNDS: &[&str] = &["bg.base", "bg.panel", "bg.raised"];
const MIN_CONTRAST: f64 = 4.5;

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("tokens") => tokens(args.iter().any(|a| a == "--check")),
        Some("affected") => affected::run(&root(), &args[1..]),
        Some("test") => suites::run(&root(), &args[1..]),
        _ => bail!(
            "usage: cargo xtask tokens [--check] | cargo xtask affected [--base <ref>] [--files <path>...] [--json]"
        ),
    }
}

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().expect("xtask sits in the workspace root").to_path_buf()
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Rgba {
    r: u8,
    g: u8,
    b: u8,
    a: f64,
}

struct Color {
    /// Dotted path under `color`, e.g. `lamp.fire`.
    path: String,
    description: String,
    night: Rgba,
    day: Rgba,
    /// `None` for web-only colours (grounds and lines).
    ansi16: Option<String>,
}

struct Glyph {
    name: String,
    unicode: String,
    ascii: String,
    description: String,
}

struct Tokens {
    colors: Vec<Color>,
    glyphs: Vec<Glyph>,
    fonts: Vec<(String, Vec<String>, String)>,
    /// `(css name, css value)` for dimensions, durations and easings.
    scalars: Vec<(String, String)>,
}

fn tokens(check: bool) -> Result<()> {
    let root = root();
    let src = std::fs::read_to_string(root.join(SOURCE)).with_context(|| format!("reading {SOURCE}"))?;
    let json: Value = serde_json::from_str(&src).with_context(|| format!("parsing {SOURCE}"))?;
    let t = parse(&json)?;
    contrast(&t)?;
    let outputs = [(RUST_OUT, rust(&t)), (CSS_OUT, css(&t)), (TS_OUT, ts(&t)?)];
    let mut stale = Vec::new();
    for (rel, text) in &outputs {
        let path = root.join(rel);
        let current = std::fs::read_to_string(&path).unwrap_or_default();
        if current == *text {
            continue;
        }
        if check {
            stale.push(*rel);
        } else {
            if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir)?;
            }
            std::fs::write(&path, text).with_context(|| format!("writing {rel}"))?;
            println!("wrote {rel}");
        }
    }
    if !stale.is_empty() {
        bail!("stale generated tokens: {} (run `cargo xtask tokens`)", stale.join(", "));
    }
    if check {
        println!("tokens: up to date, contrast ok");
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// Reading the DTCG file

fn parse(json: &Value) -> Result<Tokens> {
    let mut colors = Vec::new();
    walk_colors(json.get("color").ok_or_else(|| anyhow!("no `color` group"))?, "", &mut colors)?;
    let mut glyphs = Vec::new();
    if let Some(Value::Object(g)) = json.get("glyph") {
        for (name, v) in g {
            if name.starts_with('$') {
                continue;
            }
            glyphs.push(Glyph {
                name: name.clone(),
                unicode: str_at(v, "/$value")?,
                ascii: str_at(v, "/$extensions/adage.ascii")?,
                description: str_at(v, "/$description").unwrap_or_default(),
            });
        }
    }
    let mut fonts = Vec::new();
    if let Some(Value::Object(f)) = json.get("font") {
        for (name, v) in f {
            let stack = v
                .pointer("/$value")
                .and_then(Value::as_array)
                .ok_or_else(|| anyhow!("font.{name}: $value is not a list"))?
                .iter()
                .map(|s| s.as_str().map(str::to_string).ok_or_else(|| anyhow!("font.{name}: not a string")))
                .collect::<Result<Vec<_>>>()?;
            fonts.push((name.clone(), stack, str_at(v, "/$description").unwrap_or_default()));
        }
    }
    let mut scalars = Vec::new();
    for group in ["radius", "space", "duration"] {
        if let Some(Value::Object(g)) = json.get(group) {
            for (name, v) in g {
                let value = v
                    .pointer("/$value/value")
                    .and_then(Value::as_f64)
                    .ok_or_else(|| anyhow!("{group}.{name}: no value"))?;
                let unit = str_at(v, "/$value/unit")?;
                scalars.push((format!("{group}-{name}"), format!("{}{unit}", fmt_num(value))));
            }
        }
    }
    if let Some(Value::Object(g)) = json.get("easing") {
        for (name, v) in g {
            let p =
                v.pointer("/$value").and_then(Value::as_array).ok_or_else(|| anyhow!("easing.{name}: not a list"))?;
            let nums: Vec<String> = p.iter().filter_map(Value::as_f64).map(fmt_num).collect();
            scalars.push((format!("easing-{name}"), format!("cubic-bezier({})", nums.join(", "))));
        }
    }
    Ok(Tokens { colors, glyphs, fonts, scalars })
}

fn walk_colors(v: &Value, prefix: &str, out: &mut Vec<Color>) -> Result<()> {
    let Value::Object(map) = v else { return Ok(()) };
    if map.contains_key("$value") {
        let path = prefix.to_string();
        let night = rgba(v.pointer("/$extensions/mode/night").unwrap_or(&map["$value"]), &path)?;
        let day = rgba(v.pointer("/$extensions/mode/day").ok_or_else(|| anyhow!("color.{path}: no day mode"))?, &path)?;
        let ansi16 = v.pointer("/$extensions/adage.terminal/ansi16").and_then(Value::as_str).map(str::to_string);
        if let Some(a) = &ansi16 {
            if !ANSI16.contains(&a.as_str()) {
                bail!("color.{path}: unknown ansi16 role {a:?} (one of {})", ANSI16.join(", "));
            }
        }
        out.push(Color { path, description: str_at(v, "/$description").unwrap_or_default(), night, day, ansi16 });
        return Ok(());
    }
    for (k, child) in map {
        if k.starts_with('$') {
            continue;
        }
        let p = if prefix.is_empty() { k.clone() } else { format!("{prefix}.{k}") };
        walk_colors(child, &p, out)?;
    }
    Ok(())
}

fn rgba(v: &Value, path: &str) -> Result<Rgba> {
    let hex = v.pointer("/hex").and_then(Value::as_str).ok_or_else(|| anyhow!("color.{path}: no hex"))?;
    let h = hex.trim_start_matches('#');
    if h.len() != 6 {
        bail!("color.{path}: hex {hex:?} is not #rrggbb");
    }
    let byte = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).map_err(|_| anyhow!("color.{path}: bad hex {hex:?}"));
    let a = v.pointer("/alpha").and_then(Value::as_f64).unwrap_or(1.0);
    Ok(Rgba { r: byte(0)?, g: byte(2)?, b: byte(4)?, a })
}

fn str_at(v: &Value, ptr: &str) -> Result<String> {
    v.pointer(ptr).and_then(Value::as_str).map(str::to_string).ok_or_else(|| anyhow!("missing {ptr}"))
}

fn fmt_num(n: f64) -> String {
    if n.fract() == 0.0 {
        format!("{}", n as i64)
    } else {
        format!("{n}")
    }
}

// ---------------------------------------------------------------------------------------------
// Colour maths

/// WCAG relative luminance.
fn luminance(c: Rgba) -> f64 {
    let ch = |v: u8| {
        let s = v as f64 / 255.0;
        if s <= 0.03928 {
            s / 12.92
        } else {
            ((s + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * ch(c.r) + 0.7152 * ch(c.g) + 0.0722 * ch(c.b)
}

fn contrast_ratio(a: Rgba, b: Rgba) -> f64 {
    let (x, y) = (luminance(a), luminance(b));
    let (hi, lo) = if x > y { (x, y) } else { (y, x) };
    (hi + 0.05) / (lo + 0.05)
}

fn contrast(t: &Tokens) -> Result<()> {
    let get = |p: &str| t.colors.iter().find(|c| c.path == p).ok_or_else(|| anyhow!("contrast check: no color.{p}"));
    let mut fails = Vec::new();
    for fg in TEXT {
        let f = get(fg)?;
        for bg in GROUNDS {
            let b = get(bg)?;
            for (mode, fc, bc) in [("night", f.night, b.night), ("day", f.day, b.day)] {
                let r = contrast_ratio(fc, bc);
                if r < MIN_CONTRAST {
                    fails.push(format!("{fg} on {bg} ({mode}): {r:.2}:1"));
                }
            }
        }
    }
    if !fails.is_empty() {
        bail!("contrast below {MIN_CONTRAST}:1:\n  {}", fails.join("\n  "));
    }
    Ok(())
}

/// The xterm-256 palette entry (16..=255; the first 16 are the user's theme) nearest to `c`,
/// by the "redmean" weighted distance.
fn nearest_256(c: Rgba) -> u8 {
    const LEVELS: [u8; 6] = [0, 95, 135, 175, 215, 255];
    let mut best = (u32::MAX, 16u8);
    let mut consider = |idx: u8, r: u8, g: u8, b: u8| {
        let rm = (c.r as i32 + r as i32) / 2;
        let (dr, dg, db) = (c.r as i32 - r as i32, c.g as i32 - g as i32, c.b as i32 - b as i32);
        let d = (((512 + rm) * dr * dr) >> 8) + 4 * dg * dg + (((767 - rm) * db * db) >> 8);
        if (d as u32) < best.0 {
            best = (d as u32, idx);
        }
    };
    for (i, r) in LEVELS.iter().enumerate() {
        for (j, g) in LEVELS.iter().enumerate() {
            for (k, b) in LEVELS.iter().enumerate() {
                consider((16 + 36 * i + 6 * j + k) as u8, *r, *g, *b);
            }
        }
    }
    for i in 0..24u8 {
        let v = 8 + 10 * i;
        consider(232 + i, v, v, v);
    }
    best.1
}

const ANSI16: &[&str] = &[
    "default",
    "dim",
    "black",
    "red",
    "green",
    "yellow",
    "blue",
    "magenta",
    "cyan",
    "white",
    "bright-black",
    "bright-red",
    "bright-green",
    "bright-yellow",
    "bright-blue",
    "bright-magenta",
    "bright-cyan",
    "bright-white",
];

// ---------------------------------------------------------------------------------------------
// Writers

fn ident(path: &str) -> String {
    path.replace(['.', '-'], "_").to_uppercase()
}

fn ansi16_variant(s: &str) -> String {
    s.split('-').map(|w| w[..1].to_uppercase() + &w[1..]).collect()
}

fn rust(t: &Tokens) -> String {
    let mut s = String::new();
    s.push_str(
        "// @generated by `cargo xtask tokens` from design/tokens/adage.tokens.json. Do not edit.\n\
         //! Design tokens: the terminal half of the one palette the web map also uses.\n\n\
         use crate::color::{Ansi16, Rgb, Swatch};\n\n",
    );
    for c in &t.colors {
        let Some(a16) = &c.ansi16 else { continue };
        let _ = writeln!(s, "/// `color.{}`: {}", c.path, c.description);
        let _ = writeln!(
            s,
            "pub const {}: Swatch = Swatch {{\n    night: Rgb(0x{:02x}, 0x{:02x}, 0x{:02x}),\n    day: Rgb(0x{:02x}, 0x{:02x}, 0x{:02x}),\n    night_256: {},\n    day_256: {},\n    ansi16: Ansi16::{},\n}};\n",
            ident(&c.path),
            c.night.r,
            c.night.g,
            c.night.b,
            c.day.r,
            c.day.g,
            c.day.b,
            nearest_256(c.night),
            nearest_256(c.day),
            ansi16_variant(a16),
        );
    }
    s.push_str("/// Glyphs as `(unicode, ascii)`.\npub mod glyph {\n");
    for g in &t.glyphs {
        let _ = writeln!(
            s,
            "    /// {}\n    pub const {}: (&str, &str) = ({:?}, {:?});",
            g.description,
            ident(&g.name),
            g.unicode,
            g.ascii
        );
    }
    s.push_str("}\n\n/// Web font stacks (the terminal keeps the user's own font).\npub mod font {\n");
    for (name, stack, desc) in &t.fonts {
        let _ = writeln!(s, "    /// {desc}\n    pub const {}: &[&str] = &{stack:?};", ident(name));
    }
    s.push_str("}\n");
    s
}

fn css_color(c: Rgba) -> String {
    if c.a >= 1.0 {
        format!("#{:02x}{:02x}{:02x}", c.r, c.g, c.b)
    } else {
        format!("rgb({} {} {} / {})", c.r, c.g, c.b, fmt_num((c.a * 1000.0).round() / 1000.0))
    }
}

fn css(t: &Tokens) -> String {
    let var = |p: &str| format!("--color-{}", p.replace('.', "-"));
    let colors = |day: bool| {
        let mut s = String::new();
        for c in &t.colors {
            let _ = writeln!(s, "  {}: {};", var(&c.path), css_color(if day { c.day } else { c.night }));
        }
        s
    };
    let mut s = String::from("/* @generated by `cargo xtask tokens` from design/tokens/adage.tokens.json. Do not edit. */\n\n:root {\n  color-scheme: dark;\n");
    s.push_str(&colors(false));
    for (name, stack, _) in &t.fonts {
        let list: Vec<String> =
            stack.iter().map(|f| if f.contains(' ') { format!("\"{f}\"") } else { f.clone() }).collect();
        let _ = writeln!(s, "  --font-{name}: {};", list.join(", "));
    }
    for (name, value) in &t.scalars {
        let _ = writeln!(s, "  --{name}: {value};");
    }
    s.push_str("}\n\n@media (prefers-color-scheme: light) {\n  :root:not([data-theme=\"night\"]) {\n    color-scheme: light;\n");
    for line in colors(true).lines() {
        let _ = writeln!(s, "  {line}");
    }
    s.push_str("  }\n}\n\n:root[data-theme=\"day\"] {\n  color-scheme: light;\n");
    s.push_str(&colors(true));
    s.push_str("}\n\n:root[data-theme=\"night\"] {\n  color-scheme: dark;\n");
    s.push_str(&colors(false));
    s.push_str("}\n");
    s
}

/// Camel case for a TypeScript key: `lamp` stays, `my-glyph` becomes `myGlyph`.
fn camel(name: &str) -> String {
    let mut out = String::new();
    let mut up = false;
    for c in name.chars() {
        if c == '-' || c == '_' || c == '.' {
            up = true;
        } else if up {
            out.extend(c.to_uppercase());
            up = false;
        } else {
            out.push(c);
        }
    }
    out
}

fn ts(t: &Tokens) -> Result<String> {
    let mut s = String::from(
        "// @generated by `cargo xtask tokens` from design/tokens/adage.tokens.json. Do not edit.\n\
         // The mod's half of the design tokens (docs/design/style.md): glyphs with their ASCII twins\n\
         // (`ADAGE_ASCII=1` picks the twin) and Adage's roles as Claude Code theme keys.\n\n\
         /** Glyphs as `[unicode, ascii]`; every one is one column wide. */\n\
         export const GLYPHS = {\n",
    );
    for g in &t.glyphs {
        let _ = writeln!(s, "  /** {} */\n  {}: [{:?}, {:?}],", g.description, camel(&g.name), g.unicode, g.ascii);
    }
    s.push_str("} as const\n\nexport type GlyphName = keyof typeof GLYPHS\n\n");
    s.push_str("/** A glyph, or its ASCII twin. */\nexport function glyph(name: GlyphName, ascii: boolean): string {\n  return GLYPHS[name][ascii ? 1 : 0]\n}\n\n");
    s.push_str("/** Adage's roles as the engine's theme keys: `Text` takes these, never a colour. */\nexport const ROLES = {\n");
    for (role, path, key) in ROLES {
        let c = t
            .colors
            .iter()
            .find(|c| c.path == *path)
            .ok_or_else(|| anyhow!("role {role}: no color.{path} in {SOURCE}"))?;
        let _ = writeln!(s, "  /** `color.{path}`: {} */\n  {role}: {key:?},", c.description);
    }
    s.push_str("} as const\n\nexport type Role = keyof typeof ROLES\n");
    Ok(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nearest_256_hits_exact_cube_and_gray_entries() {
        assert_eq!(nearest_256(Rgba { r: 0xaf, g: 0x87, b: 0xff, a: 1.0 }), 141);
        assert_eq!(nearest_256(Rgba { r: 0x80, g: 0x80, b: 0x80, a: 1.0 }), 244);
    }

    #[test]
    fn contrast_matches_wcag_examples() {
        let black = Rgba { r: 0, g: 0, b: 0, a: 1.0 };
        let white = Rgba { r: 255, g: 255, b: 255, a: 1.0 };
        assert!((contrast_ratio(black, white) - 21.0).abs() < 0.01);
    }

    #[test]
    fn the_committed_tokens_are_current() {
        let root = root();
        let json: Value = serde_json::from_str(&std::fs::read_to_string(root.join(SOURCE)).unwrap()).unwrap();
        let t = parse(&json).unwrap();
        contrast(&t).unwrap();
        assert_eq!(std::fs::read_to_string(root.join(RUST_OUT)).unwrap(), rust(&t), "run `cargo xtask tokens`");
        assert_eq!(std::fs::read_to_string(root.join(CSS_OUT)).unwrap(), css(&t), "run `cargo xtask tokens`");
        let ts_text = ts(&t).unwrap();
        assert_eq!(std::fs::read_to_string(root.join(TS_OUT)).unwrap(), ts_text, "run `cargo xtask tokens`");
        // Every glyph with its ASCII twin, every role with a theme key.
        for g in &t.glyphs {
            assert!(ts_text.contains(&format!("{}: [{:?}, {:?}]", camel(&g.name), g.unicode, g.ascii)), "{}", g.name);
        }
        for (role, _, key) in ROLES {
            assert!(ts_text.contains(&format!("{role}: {key:?}")), "{role}");
        }
    }
}
