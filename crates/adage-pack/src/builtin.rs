//! The marketplace built into this binary: the Adage repository's own (`.adage/marketplace.toml`
//! and `packs/`), embedded at compile time, so the first run has Adage's base instincts with
//! nothing to download (`docs/marketplace.md`).
//!
//! It is the first marketplace Adage knows, and is read like any other ([`Loaded`]): each pack must
//! be exactly the one its manifest pins. [`install`] syncs it to this machine
//! ([`Loaded::sync`]), as learning does before it binds instincts; binding reads its packs from
//! here ([`crate::instincts::available`]), so a deleted or damaged installed copy changes nothing
//! until the next sync restores it.

use crate::marketplace::{Loaded, Synced};
use anyhow::{anyhow, Result};
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::LazyLock;

/// The built-in marketplace's name.
pub const NAME: &str = "adage";

/// Every file of the built-in marketplace, by its path in the Adage repository. A test checks this
/// is exactly what the repository holds under `packs/`, beside the manifest.
pub const FILES: &[(&str, &str)] = &[
    (".adage/marketplace.toml", include_str!("../../../.adage/marketplace.toml")),
    ("packs/generated-files/CLOSURE", include_str!("../../../packs/generated-files/CLOSURE")),
    ("packs/generated-files/pack.toml", include_str!("../../../packs/generated-files/pack.toml")),
    (
        "packs/generated-files/routes/generated-file.yaml",
        include_str!("../../../packs/generated-files/routes/generated-file.yaml"),
    ),
    ("packs/lockfiles/CLOSURE", include_str!("../../../packs/lockfiles/CLOSURE")),
    ("packs/lockfiles/pack.toml", include_str!("../../../packs/lockfiles/pack.toml")),
    ("packs/lockfiles/routes/lockfile.yaml", include_str!("../../../packs/lockfiles/routes/lockfile.yaml")),
    ("packs/package-manager/CLOSURE", include_str!("../../../packs/package-manager/CLOSURE")),
    ("packs/package-manager/pack.toml", include_str!("../../../packs/package-manager/pack.toml")),
    (
        "packs/package-manager/routes/install-with-declared-manager.yaml",
        include_str!("../../../packs/package-manager/routes/install-with-declared-manager.yaml"),
    ),
    (
        "packs/package-manager/routes/test-with-declared-manager.yaml",
        include_str!("../../../packs/package-manager/routes/test-with-declared-manager.yaml"),
    ),
    (
        "packs/package-manager/routes/uses-declared-manager.yaml",
        include_str!("../../../packs/package-manager/routes/uses-declared-manager.yaml"),
    ),
    ("packs/pytest-wrapper/CLOSURE", include_str!("../../../packs/pytest-wrapper/CLOSURE")),
    ("packs/pytest-wrapper/pack.toml", include_str!("../../../packs/pytest-wrapper/pack.toml")),
    (
        "packs/pytest-wrapper/routes/run-pytest-wrapper.yaml",
        include_str!("../../../packs/pytest-wrapper/routes/run-pytest-wrapper.yaml"),
    ),
    ("packs/python-env/CLOSURE", include_str!("../../../packs/python-env/CLOSURE")),
    ("packs/python-env/pack.toml", include_str!("../../../packs/python-env/pack.toml")),
    (
        "packs/python-env/routes/name-project-interpreter.yaml",
        include_str!("../../../packs/python-env/routes/name-project-interpreter.yaml"),
    ),
    (
        "packs/python-env/routes/run-tests-in-project-env.yaml",
        include_str!("../../../packs/python-env/routes/run-tests-in-project-env.yaml"),
    ),
    ("packs/regenerate/CLOSURE", include_str!("../../../packs/regenerate/CLOSURE")),
    ("packs/regenerate/pack.toml", include_str!("../../../packs/regenerate/pack.toml")),
    (
        "packs/regenerate/routes/regenerate-after-input-edit.yaml",
        include_str!("../../../packs/regenerate/routes/regenerate-after-input-edit.yaml"),
    ),
    ("packs/test-command/CLOSURE", include_str!("../../../packs/test-command/CLOSURE")),
    ("packs/test-command/pack.toml", include_str!("../../../packs/test-command/pack.toml")),
    (
        "packs/test-command/routes/run-declared-test-command.yaml",
        include_str!("../../../packs/test-command/routes/run-declared-test-command.yaml"),
    ),
    ("packs/test-wrapper/CLOSURE", include_str!("../../../packs/test-wrapper/CLOSURE")),
    ("packs/test-wrapper/pack.toml", include_str!("../../../packs/test-wrapper/pack.toml")),
    (
        "packs/test-wrapper/routes/run-declared-wrapper.yaml",
        include_str!("../../../packs/test-wrapper/routes/run-declared-wrapper.yaml"),
    ),
];

static BUILTIN: LazyLock<Result<Loaded, String>> = LazyLock::new(|| {
    let files: BTreeMap<String, Vec<u8>> = FILES.iter().map(|(p, t)| (p.to_string(), t.as_bytes().to_vec())).collect();
    Loaded::from_files(files).map_err(|e| format!("{e:#}"))
});

/// The built-in marketplace, read and verified. An error (which the tests rule out for a build)
/// says why it cannot be read; nothing of it is then installed or bound.
pub fn marketplace() -> Result<&'static Loaded, &'static str> {
    BUILTIN.as_ref().map_err(String::as_str)
}

/// A short fingerprint of the built-in marketplace's bytes, read without parsing them: it changes
/// when a build ships other base packs, which binds each repository's instincts again. Cheap
/// enough for the session-start hook (`adage_learn::instincts::stale_binding`).
pub fn fingerprint() -> &'static str {
    static FINGERPRINT: LazyLock<String> = LazyLock::new(|| {
        use sha2::{Digest, Sha256};
        let mut h = Sha256::new();
        for (path, text) in FILES {
            h.update(path.as_bytes());
            h.update([0]);
            h.update(text.as_bytes());
            h.update([0]);
        }
        crate::digest::hex(&h.finalize())[..16].to_string()
    });
    &FINGERPRINT
}

/// Whether `name` is a pack of the built-in marketplace: no other bytes may be installed under it.
pub fn reserved(name: &str) -> bool {
    marketplace().is_ok_and(|b| b.get(name).is_some())
}

/// Sync the built-in marketplace to the machine whose tracks are at `tracks_root`
/// ([`Loaded::sync`]): its packs installed where `adage add` installs a pack, each at its pinned
/// digest and recorded as from it, and the ones a newer build no longer lists removed. The caller
/// holds the tracks lock.
pub fn install(tracks_root: &Path) -> Result<Synced> {
    marketplace().map_err(|e| anyhow!("the built-in marketplace cannot be read: {e}"))?.sync(tracks_root)
}
