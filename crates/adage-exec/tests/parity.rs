//! `adage-exec hook` is a superset of `adage hook` (Track C §3.2): with nothing approved, its
//! output equals `adage hook`'s byte for byte. CI pins this with the existing golden files: this
//! compiles adage-cli's hook tests (`crates/adage-cli/tests/hooks.rs`) against the adage-exec
//! binary, so every golden scenario, hostile payload and fail-open case runs through
//! `adage-exec hook` and must produce exactly what the goldens hold.

#[allow(dead_code)]
#[path = "../../adage-cli/tests/hooks.rs"]
mod hooks;
