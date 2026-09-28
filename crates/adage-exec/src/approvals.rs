//! Approvals (Track C §4): the key, the store, matching, lapse detection and automatic approval of
//! the inert-observe class.
//!
//! An approval is per repository and per exact route: a match needs equality on **every** field
//! of [`ApprovalKey`]. When any hash changes the approval stops matching, the route drops to E0,
//! and [`Store::lapsed`] explains what changed. Re-approval is explicit.
//!
//! The store, `$ADAGE_HOME/approvals.json`, is plain JSON, mode 0600, written atomically, and only
//! by commands that pass `require_person` (§4.3). It is not signed: whoever can write
//! `$ADAGE_HOME` can already rewrite `trusted-signers` and `trust.json`.

use adage_core::{CommandTable, Effect};
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::classify;
use crate::request::{ExecRequest, Level, Origin, OutputMode};

/// §4.1.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ApprovalKey {
    /// `repo_key`: approvals are per repository, never global.
    pub repo: String,
    /// §1.5: shape, types and script hashes inlined.
    pub route_exec_digest: String,
    pub argv_templates: Vec<Vec<String>>,
    /// (path or `cas:<hex>`, SHA-256 hex).
    pub scripts: Vec<(String, String)>,
    /// The effect the person approved (may be lower than the classifier's for L-ref).
    pub effect: Effect,
    pub level: Level,
    pub output_mode: OutputMode,
    /// The sandbox profile's hash.
    pub profile_id: String,
    /// Signer fingerprint, for imported packs.
    pub signer: Option<String>,
}

impl ApprovalKey {
    pub fn for_request(req: &ExecRequest, repo: &str) -> ApprovalKey {
        let mut scripts: Vec<(String, String)> =
            req.scripts.iter().map(|s| (s.path.clone(), s.sha256.clone())).collect();
        scripts.sort();
        ApprovalKey {
            repo: repo.to_string(),
            route_exec_digest: req.exec_digest(),
            argv_templates: vec![req.argv.clone()],
            scripts,
            effect: req.effect,
            level: req.level,
            output_mode: req.output.mode,
            profile_id: req.profile.id(),
            signer: match &req.origin {
                Origin::Pack { signer, .. } => signer.clone(),
                Origin::Local => None,
            },
        }
    }

    /// What differs from `newer`, field by field, for review's `exec-changed` screen.
    pub fn diff(&self, newer: &ApprovalKey) -> Vec<Change> {
        let mut out = Vec::new();
        let mut cmp = |field: &'static str, a: String, b: String| {
            if a != b {
                out.push(Change { field, before: a, after: b });
            }
        };
        cmp("repo", self.repo.clone(), newer.repo.clone());
        cmp("route_exec_digest", self.route_exec_digest.clone(), newer.route_exec_digest.clone());
        let argv = |v: &[Vec<String>]| v.iter().map(|a| a.join(" ")).collect::<Vec<_>>().join(" ; ");
        cmp("argv_templates", argv(&self.argv_templates), argv(&newer.argv_templates));
        let scripts = |v: &[(String, String)]| v.iter().map(|(p, h)| format!("{p}@{h}")).collect::<Vec<_>>().join(", ");
        cmp("scripts", scripts(&self.scripts), scripts(&newer.scripts));
        cmp("effect", self.effect.as_str().into(), newer.effect.as_str().into());
        cmp("level", self.level.as_str().into(), newer.level.as_str().into());
        cmp("output_mode", self.output_mode.as_str().into(), newer.output_mode.as_str().into());
        cmp("profile_id", self.profile_id.clone(), newer.profile_id.clone());
        cmp("signer", self.signer.clone().unwrap_or_default(), newer.signer.clone().unwrap_or_default());
        out
    }
}

/// One changed field of a lapsed approval.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Change {
    pub field: &'static str,
    pub before: String,
    pub after: String,
}

/// One approval.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Approval {
    pub key: ApprovalKey,
    /// `track/route`, to find the approval a changed key used to match.
    pub route: String,
    pub approved_on: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub by: Option<String>,
    /// Evidence snapshot at approval time.
    #[serde(default)]
    pub basis: serde_json::Value,
    #[serde(default)]
    pub auto: bool,
    /// What the person declared beside the route (output fields, exit predicates, timeout), for
    /// requests derived from a compiled route (`crate::route`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terms: Option<crate::route::Terms>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Store {
    pub version: u32,
    #[serde(default)]
    pub approvals: Vec<Approval>,
}

impl Default for Store {
    fn default() -> Self {
        Store { version: 1, approvals: Vec::new() }
    }
}

pub fn path(adage_home: &Path) -> PathBuf {
    adage_home.join("approvals.json")
}

/// The route label an approval is filed under: `<track>/<route>`, without the track's version
/// (a version bump alone, as evidence changes, is not a new route).
pub fn route_label(req: &ExecRequest) -> String {
    let track = req.labels.track.split('@').next().unwrap_or_default();
    format!("{track}/{}", req.labels.route)
}

impl Store {
    /// Load the store; a missing file is an empty store. A symlink, a file that does not parse,
    /// or an unknown version is an error (execution then refuses).
    pub fn load(path: &Path) -> Result<Store> {
        match std::fs::symlink_metadata(path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Store::default()),
            Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
            Ok(m) if !m.file_type().is_file() => bail!("{} is not a regular file", path.display()),
            Ok(m) if m.len() > 16 << 20 => bail!("{} is larger than 16 MiB", path.display()),
            Ok(_) => {}
        }
        let text = std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        let s: Store = serde_json::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
        if s.version != 1 {
            bail!("{}: unknown approvals version {}", path.display(), s.version);
        }
        Ok(s)
    }

    /// Write atomically (a temporary file in the same directory, mode 0600, then rename).
    pub fn save(&self, path: &Path) -> Result<()> {
        use std::io::Write;
        let dir = path.parent().context("approvals path has no directory")?;
        std::fs::create_dir_all(dir)?;
        let tmp = dir.join(format!(".approvals.json.tmp{}", std::process::id()));
        let _ = std::fs::remove_file(&tmp);
        let mut opts = std::fs::OpenOptions::new();
        opts.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        let mut f = opts.open(&tmp).with_context(|| format!("creating {}", tmp.display()))?;
        let text = serde_json::to_string_pretty(self)? + "\n";
        f.write_all(text.as_bytes())?;
        f.sync_all()?;
        drop(f);
        std::fs::rename(&tmp, path).with_context(|| format!("replacing {}", path.display()))?;
        Ok(())
    }

    /// The approval whose key equals `key` on every field.
    pub fn find(&self, key: &ApprovalKey) -> Option<&Approval> {
        self.approvals.iter().find(|a| &a.key == key)
    }

    /// Approvals for the same repository, route and level whose key no longer matches, each with
    /// what changed.
    pub fn lapsed(&self, key: &ApprovalKey, route: &str) -> Vec<(&Approval, Vec<Change>)> {
        self.approvals
            .iter()
            .filter(|a| a.route == route && a.key.repo == key.repo && a.key.level == key.level && &a.key != key)
            .map(|a| (a, a.key.diff(key)))
            .collect()
    }

    /// Record an approval, replacing any older one for the same repository, route and level.
    pub fn approve(&mut self, a: Approval) {
        self.approvals.retain(|x| !(x.route == a.route && x.key.repo == a.key.repo && x.key.level == a.key.level));
        self.approvals.push(a);
        self.approvals.sort_by(|x, y| (&x.key.repo, &x.route).cmp(&(&y.key.repo, &y.route)));
    }

    /// Remove approvals for a repository (and route, when given). Returns how many.
    pub fn revoke(&mut self, repo: &str, route: Option<&str>) -> usize {
        let before = self.approvals.len();
        self.approvals.retain(|a| !(a.key.repo == repo && route.is_none_or(|r| a.route == r)));
        before - self.approvals.len()
    }
}

/// Facts about this machine and repository that automatic approval depends on.
pub struct AutoContext<'a> {
    pub table: &'a CommandTable,
    pub git_hazards: &'a [String],
    pub sandbox_enforced: bool,
    pub policy_auto: bool,
}

/// Whether `req` is in the inert-observe class (§4.2); every reason it is not, otherwise.
pub fn auto_eligible(req: &ExecRequest, ctx: &AutoContext) -> Result<(), Vec<String>> {
    let mut why = Vec::new();
    if !matches!(req.level, Level::E1 | Level::E2) {
        why.push(format!("level {} is never approved automatically", req.level.as_str()));
    }
    let classified = classify::effect(&req.argv, ctx.table);
    if classified != Effect::Observe || req.effect != Effect::Observe {
        why.push(format!("effect is {} (classified {}), not observe", req.effect.as_str(), classified.as_str()));
    }
    if let Err(e) = classify::inert(&req.argv, ctx.table, ctx.git_hazards) {
        why.push(e);
    }
    if !req.scripts.is_empty() {
        why.push("the route runs scripts (L-ref or L-vendor)".into());
    }
    for name in crate::request::slots(&req.argv) {
        match req.contracts.get(&name) {
            Some(re) if crate::request::contract_is_strict(re) => {}
            _ => why.push(format!("slot {{{name}}} has no strict contract")),
        }
    }
    if req.output.mode != OutputMode::Typed {
        why.push("output mode is raw".into());
    }
    if !ctx.sandbox_enforced {
        why.push("no sandbox is enforced on this machine".into());
    }
    match &req.origin {
        Origin::Local => {}
        Origin::Pack { trusted: true, probation_passed: true, .. } => {}
        Origin::Pack { .. } => why.push("the pack's signer is not trusted or it has not passed probation".into()),
    }
    if !ctx.policy_auto {
        why.push("automatic approval is off (policy exec.auto_approve; off by default until gate G3)".into());
    }
    if why.is_empty() {
        Ok(())
    } else {
        Err(why)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::request::{Labels, OutputSpec};
    use crate::sandbox::Profile;
    use std::collections::BTreeMap;

    type Mutation = Box<dyn Fn(&mut ExecRequest)>;

    fn req() -> ExecRequest {
        let fields = serde_json::from_str(r#"{"n":{"type":"int"}}"#).unwrap();
        ExecRequest {
            repo_root: PathBuf::from("/r"),
            argv: vec!["jq".into(), ".".into(), "{f}".into()],
            inputs: BTreeMap::new(),
            contracts: [("f".to_string(), r"^[a-z]{1,8}\.json$".to_string())].into(),
            effect: Effect::Observe,
            level: Level::E1,
            output: OutputSpec { mode: OutputMode::Typed, fields },
            scripts: vec![],
            profile: Profile::for_level(Level::E1),
            labels: Labels::default(),
            origin: Origin::Local,
            route_exec_digest: None,
        }
    }

    #[test]
    fn every_mutation_lapses_the_key() {
        let base = req();
        let k = ApprovalKey::for_request(&base, "github.com/acme/x");
        let mut store = Store::default();
        store.approve(Approval {
            key: k.clone(),
            route: route_label(&base),
            approved_on: crate::time::now(),
            by: None,
            basis: serde_json::Value::Null,
            auto: false,
            terms: None,
        });
        assert!(store.find(&k).is_some());
        let mutations: Vec<(&str, Mutation)> = vec![
            ("argv", Box::new(|r| r.argv[1] = ".x".into())),
            (
                "contract",
                Box::new(|r| {
                    r.contracts.insert("f".into(), r"^[a-z]{1,9}\.json$".into());
                }),
            ),
            ("output", Box::new(|r| r.output.fields.get_mut("n").unwrap().min = Some(0))),
            (
                "script",
                Box::new(|r| r.scripts.push(crate::request::ScriptRef { path: "s".into(), sha256: "00".into() })),
            ),
            ("effect", Box::new(|r| r.effect = Effect::Verify)),
            ("level", Box::new(|r| r.level = Level::E2)),
            ("mode", Box::new(|r| r.output.mode = OutputMode::Raw)),
            ("profile", Box::new(|r| r.profile.limits.wall_ms = 2_000)),
            (
                "signer",
                Box::new(|r| {
                    r.origin = Origin::Pack { signer: Some("SHA256:x".into()), trusted: true, probation_passed: true }
                }),
            ),
            ("digest", Box::new(|r| r.route_exec_digest = Some("ab".repeat(32)))),
        ];
        for (what, m) in mutations {
            let mut r = base.clone();
            m(&mut r);
            let k2 = ApprovalKey::for_request(&r, "github.com/acme/x");
            assert!(store.find(&k2).is_none(), "{what} did not lapse the approval");
            if r.level == base.level {
                let lapsed = store.lapsed(&k2, &route_label(&r));
                assert_eq!(lapsed.len(), 1, "{what}");
                assert!(!lapsed[0].1.is_empty(), "{what}: no diff");
            }
        }
        let other_repo = ApprovalKey::for_request(&base, "github.com/acme/y");
        assert!(store.find(&other_repo).is_none(), "approvals are per repository");
        // Bound values are not part of the key.
        let mut v = base.clone();
        v.inputs.insert("f".into(), "a.json".into());
        assert!(store.find(&ApprovalKey::for_request(&v, "github.com/acme/x")).is_some());
    }

    #[test]
    fn store_round_trips_with_mode_0600() {
        let d = tempfile::tempdir().unwrap();
        let p = path(d.path());
        let mut s = Store::load(&p).unwrap();
        let k = ApprovalKey::for_request(&req(), "r");
        s.approve(Approval {
            key: k.clone(),
            route: "t/r".into(),
            approved_on: "x".into(),
            by: Some("me".into()),
            basis: serde_json::json!({"sessions": 3}),
            auto: false,
            terms: None,
        });
        s.save(&p).unwrap();
        let back = Store::load(&p).unwrap();
        assert_eq!(back, s);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&p).unwrap().permissions().mode() & 0o777, 0o600);
        }
        let mut back = back;
        assert_eq!(back.revoke("r", Some("t/other")), 0);
        assert_eq!(back.revoke("r", None), 1);
        std::fs::write(&p, "{not json").unwrap();
        assert!(Store::load(&p).is_err());
    }

    #[test]
    fn inert_observe_class() {
        let table = CommandTable::load();
        let ctx = AutoContext { table: &table, git_hazards: &[], sandbox_enforced: true, policy_auto: true };
        assert!(auto_eligible(&req(), &ctx).is_ok());
        let off = AutoContext { policy_auto: false, ..ctx };
        assert!(auto_eligible(&req(), &off).is_err(), "off by default");
        let ctx = AutoContext { table: &table, git_hazards: &[], sandbox_enforced: true, policy_auto: true };
        let cases: Vec<Mutation> = vec![
            Box::new(|r| r.level = Level::E3),
            Box::new(|r| r.argv = vec!["make".into(), "test".into()]),
            Box::new(|r| r.argv = vec!["./scripts/x".into()]),
            Box::new(|r| r.scripts.push(crate::request::ScriptRef { path: "s".into(), sha256: "00".into() })),
            Box::new(|r| {
                r.contracts.insert("f".into(), ".*".into());
            }),
            Box::new(|r| r.output.mode = OutputMode::Raw),
            Box::new(|r| r.origin = Origin::Pack { signer: None, trusted: true, probation_passed: false }),
            Box::new(|r| r.effect = Effect::Verify),
        ];
        for (i, m) in cases.iter().enumerate() {
            let mut r = req();
            m(&mut r);
            assert!(auto_eligible(&r, &ctx).is_err(), "case {i}");
        }
        let unsandboxed = AutoContext { sandbox_enforced: false, ..ctx };
        assert!(auto_eligible(&req(), &unsandboxed).is_err());
        let mut git = req();
        git.argv = vec!["git".into(), "status".into()];
        git.contracts.clear();
        let hazards = vec!["core.fsmonitor".to_string()];
        let hostile = AutoContext { table: &table, git_hazards: &hazards, sandbox_enforced: true, policy_auto: true };
        assert!(auto_eligible(&git, &hostile).is_err());
        let clean = AutoContext { table: &table, git_hazards: &[], sandbox_enforced: true, policy_auto: true };
        assert!(auto_eligible(&git, &clean).is_ok());
    }
}
