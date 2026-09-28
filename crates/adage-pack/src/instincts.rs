//! The core instincts as a pack: `adage/core-instincts` (Track C §1.1), with detector
//! applicability.
//!
//! The detectors are Rust compiled into Adage, so a detector pack names them and never supplies
//! them. The core pack's routes are therefore not fixed bytes: for a repository, the compiled
//! detectors run over its declared facts (`adage_instincts::bind`, the same binder `adage condition`
//! uses), and the routes they propose become the pack's routes for that repository, upgraded to
//! route v2, validated and closed like any pack's, with applicability `detector = "*"` over this
//! build's catalogue. Binding that pack ([`crate::bind`]) gives back exactly the binder's routes
//! (a test pins this).

use crate::model::*;
use crate::upgrade::{upgrade, Upgraded};
use adage_tracks::{Gauge, Manifest, Route, Scope, Track, TrackInfo};
use anyhow::Result;
use std::collections::BTreeSet;
use std::path::Path;

/// The core instincts pack's name.
pub const CORE_PACK: &str = "adage/core-instincts";

/// The core instincts pack for the repository at `root`, and the candidates it could not carry
/// (instinct or route id, reason). `None` when no detector proposes anything there. `existing`
/// and `rejections` are as for `adage_instincts::bind`.
pub fn core_instincts(root: &Path, existing: &[Route], rejections: &BTreeSet<String>) -> Result<Option<Upgraded>> {
    let bound = adage_instincts::bind(root, existing, rejections);
    if bound.routes.is_empty() {
        return Ok(None);
    }
    let cat = adage_instincts::catalogue();
    let track = Track {
        manifest: Manifest {
            track: TrackInfo {
                name: CORE_PACK.into(),
                version: cat.pack.version.clone(),
                description: "Adage's built-in defaults, bound from a repository's declared files".into(),
                provenance: cat.provenance(),
            },
            scope: Scope { repo: "instincts".into() },
            gauge: Gauge::default(),
        },
        routes: bound.routes,
    };
    let up = upgrade(&track, Some(root))?;
    let mut pack = up.pack;
    pack.manifest.pack.origin = PackOrigin::Instinct;
    pack.manifest.applicability =
        Applicability::Detector { detector: "*".into(), catalogue: crate::bind::catalogue_id() };
    pack.check()?;
    Ok(Some(Upgraded { pack, skipped: up.skipped }))
}
