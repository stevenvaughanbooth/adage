//! The map's files, built from `web/map/` and embedded at build time. `web/map/scripts/embed.mjs`
//! writes them under `crates/adage-cli/assets/map/`, each with brotli (`.br`) and gzip (`.gz`)
//! variants where compression helps, and generates the list below (`assets_list.rs`). The file as
//! is is embedded only where it is small or incompressible, which keeps the binary small; every
//! browser accepts gzip.
//!
//! Hashed names (under `assets/`) never change content, so they are cached for a year; the page
//! itself is revalidated on every load.

pub(crate) struct Asset {
    /// The published path, without the leading slash.
    pub(crate) path: &'static str,
    pub(crate) content_type: &'static str,
    /// A content-hashed name: cacheable forever.
    pub(crate) immutable: bool,
    pub(crate) raw: Option<&'static [u8]>,
    pub(crate) br: Option<&'static [u8]>,
    pub(crate) gz: Option<&'static [u8]>,
}

include!("assets_list.rs");

pub(crate) fn find(path: &str) -> Option<&'static Asset> {
    ASSETS.iter().find(|a| a.path == path)
}
