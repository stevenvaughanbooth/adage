//! What every hook path asks about the session and the checkout before it decides: the key the
//! index routes this checkout by, the session's arm (assigned at its first decision), and whether
//! the mod asked in observe mode.

use adage_core::repo;
use adage_runtime::{assign, SessionState};
use adage_store::Paths;
use adage_tracks::Index;
use serde_json::Value;
use std::collections::BTreeSet;
use std::path::Path;

/// The key the index names this checkout's repository by, when it holds reflexes for it (and,
/// `with_candidates`, candidates: a repository with candidates and no reflexes yet still notices,
/// plan D2). A checkout without a remote is keyed by its path until an alias resolves it to the
/// learned key. `None`: nothing installed or counted here.
pub(crate) fn routed_key(index: &Index, root: &Path, paths: &Paths, with_candidates: bool) -> Option<String> {
    let routes = index.routes.iter().map(|r| r.repo.as_str());
    let keys: BTreeSet<&str> = if with_candidates {
        routes.chain(index.candidates.keys().map(String::as_str)).collect()
    } else {
        routes.collect()
    };
    repo::lookup_key(root, Some(&paths.repo_keys()), &keys)
}

/// What a session's arm is drawn with: the control share (`ADAGE_CONTROL`, else `runtime.control`
/// in config.toml, else 0.1) and `ADAGE_ARM`, which forces it (an evaluation knob).
pub(crate) struct ArmInputs {
    pub(crate) share: f64,
    pub(crate) forced: Option<String>,
}

impl ArmInputs {
    /// `ADAGE_ARM`, and the share from the configuration (one read of it).
    pub(crate) fn load(paths: &Paths) -> Self {
        let forced = std::env::var("ADAGE_ARM").ok();
        Self { share: adage_store::config::load(paths).config.runtime_control, forced }
    }

    /// `ADAGE_ARM`, and a share from a configuration the caller read already.
    pub(crate) fn with_share(share: f64) -> Self {
        Self { share, forced: std::env::var("ADAGE_ARM").ok() }
    }

    /// Assign the session's arm if it has none (an arm once drawn, the control arm included, is
    /// kept), with its propensity and the repository it was drawn in.
    pub(crate) fn ensure(&self, st: &mut SessionState, session: &str, key: &str) {
        if st.arm.is_none() {
            let (arm, propensity) = assign(session, self.share, self.forced.as_deref());
            st.arm = Some(arm);
            st.propensity = Some(propensity);
            st.repo = Some(key.to_string());
        }
    }
}

/// Whether the mod asked in observe mode (`ADAGE_MOD_OBSERVE`): decide, and record what would have
/// been done as the control arm records it, whatever the session's arm.
pub(crate) fn observed(p: &Value) -> bool {
    p.get("observe").and_then(Value::as_bool) == Some(true)
}
