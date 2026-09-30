//! Execution policy (Track C §4.5): an optional personal file, `$ADAGE_HOME/policy.toml`, and an
//! optional managed file (`/etc/adage/policy.toml`; macOS `/Library/Application Support/Adage/
//! policy.toml`; or the path in `ADAGE_POLICY`). The managed file's denies always win, and
//! `require_sandbox` cannot be lowered by the personal file.
//!
//! ```toml
//! [exec]
//! max_level = "E1"             # never above (E2: actions from hooks, a beta opt-in)
//! auto_approve = "off"         # or "inert-observe"
//! allow_vendor_scripts = false
//! allow_raw_output = false
//! require_sandbox = true       # the personal file cannot set this false
//! allow_environment_actions = false  # E2 actions with the environment effect; managed file only
//! env = ["JAVA_HOME"]          # variables routes may receive (never secrets: env::forbidden)
//! [publishers]
//! allow = ["SHA256:…"]
//! deny  = ["SHA256:…"]
//! [effects]
//! deny = ["environment"]
//! ```

use adage_core::Effect;
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::request::{ExecRequest, Level, Origin};

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawPolicy {
    #[serde(default)]
    pub exec: RawExec,
    #[serde(default)]
    pub publishers: RawPublishers,
    #[serde(default)]
    pub effects: RawEffects,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawExec {
    pub max_level: Option<String>,
    pub auto_approve: Option<String>,
    pub allow_vendor_scripts: Option<bool>,
    pub allow_raw_output: Option<bool>,
    pub require_sandbox: Option<bool>,
    /// E2 actions with the environment effect (Track C §3.1): only a managed (team) file may
    /// turn this on; the personal file's value is ignored.
    pub allow_environment_actions: Option<bool>,
    pub env: Option<Vec<String>>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawPublishers {
    #[serde(default)]
    pub allow: Vec<String>,
    #[serde(default)]
    pub deny: Vec<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawEffects {
    #[serde(default)]
    pub deny: Vec<String>,
}

/// The effective policy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Policy {
    pub max_level: Level,
    /// Automatic approval of the inert-observe class. Off by default until gate G3.
    pub auto_approve: bool,
    pub allow_vendor_scripts: bool,
    pub allow_raw_output: bool,
    pub require_sandbox: bool,
    /// E2 actions with the environment effect: a team-policy opt-in (managed file only).
    pub allow_environment_actions: bool,
    pub env: Vec<String>,
    pub publishers_allow: Vec<String>,
    pub publishers_deny: Vec<String>,
    pub effects_deny: Vec<Effect>,
    /// The files that contributed, for `doctor`.
    pub sources: Vec<String>,
}

/// The default ceiling. E2 actions (staged, verified, applied from hooks) are a beta behind gate
/// G4: a person opts in with `max_level = "E2"`, and each action is still approved explicitly.
pub const DEFAULT_MAX_LEVEL: Level = Level::E1;

impl Default for Policy {
    fn default() -> Self {
        merge(None, None).expect("the empty policy is valid")
    }
}

fn level(s: &Option<String>, what: &str) -> Result<Option<Level>> {
    match s {
        None => Ok(None),
        Some(v) => Level::parse(v).map(Some).with_context(|| format!("{what}: max_level {v:?} is not E1 or E2")),
    }
}

fn auto(s: &Option<String>, what: &str) -> Result<Option<bool>> {
    match s.as_deref() {
        None => Ok(None),
        Some("off") => Ok(Some(false)),
        Some("inert-observe") => Ok(Some(true)),
        Some(v) => bail!("{what}: auto_approve {v:?} is not \"off\" or \"inert-observe\""),
    }
}

/// Merge the personal and managed files. Unknown values refuse (fail closed).
pub fn merge(personal: Option<&RawPolicy>, managed: Option<&RawPolicy>) -> Result<Policy> {
    let p = personal.cloned().unwrap_or_default();
    let m = managed.cloned().unwrap_or_default();
    let p_level = level(&p.exec.max_level, "personal policy")?;
    let m_level = level(&m.exec.max_level, "managed policy")?;
    let max_level = p_level.unwrap_or(DEFAULT_MAX_LEVEL).min(m_level.unwrap_or(Level::E2));
    let p_auto = auto(&p.exec.auto_approve, "personal policy")?;
    let m_auto = auto(&m.exec.auto_approve, "managed policy")?;
    let auto_approve = m_auto != Some(false) && p_auto.or(m_auto).unwrap_or(false);
    // A managed `false` wins; otherwise the personal choice, then the managed one, else off.
    let allow = |pv: Option<bool>, mv: Option<bool>| mv != Some(false) && pv.or(mv).unwrap_or(false);
    let allow_vendor_scripts = allow(p.exec.allow_vendor_scripts, m.exec.allow_vendor_scripts);
    let allow_raw_output = allow(p.exec.allow_raw_output, m.exec.allow_raw_output);
    let allow_environment_actions = m.exec.allow_environment_actions == Some(true);
    // Only the managed file can lower it; the personal file can only raise it.
    let require_sandbox = m.exec.require_sandbox.unwrap_or(true) || p.exec.require_sandbox == Some(true);
    let mut env: Vec<String> = p
        .exec
        .env
        .iter()
        .flatten()
        .chain(m.exec.env.iter().flatten())
        .filter(|n| !crate::env::forbidden(n))
        .cloned()
        .collect();
    env.sort();
    env.dedup();
    let mut publishers_deny: Vec<String> = p.publishers.deny.iter().chain(&m.publishers.deny).cloned().collect();
    publishers_deny.sort();
    publishers_deny.dedup();
    let mut publishers_allow: Vec<String> = p
        .publishers
        .allow
        .iter()
        .chain(&m.publishers.allow)
        .filter(|f| !publishers_deny.contains(f))
        .cloned()
        .collect();
    publishers_allow.sort();
    publishers_allow.dedup();
    let mut effects_deny = Vec::new();
    for e in p.effects.deny.iter().chain(&m.effects.deny) {
        let eff = Effect::parse(e).with_context(|| format!("policy: unknown effect {e:?} in [effects] deny"))?;
        if !effects_deny.contains(&eff) {
            effects_deny.push(eff);
        }
    }
    effects_deny.sort();
    Ok(Policy {
        max_level,
        auto_approve,
        allow_vendor_scripts,
        allow_raw_output,
        require_sandbox,
        allow_environment_actions,
        env,
        publishers_allow,
        publishers_deny,
        effects_deny,
        sources: Vec::new(),
    })
}

/// The managed policy file: `ADAGE_POLICY`, else the system path.
pub fn managed_path() -> PathBuf {
    if let Some(p) = std::env::var_os("ADAGE_POLICY").filter(|p| !p.is_empty()) {
        return PathBuf::from(p);
    }
    if cfg!(target_os = "macos") {
        PathBuf::from("/Library/Application Support/Adage/policy.toml")
    } else {
        PathBuf::from("/etc/adage/policy.toml")
    }
}

fn read(path: &Path) -> Result<Option<RawPolicy>> {
    match std::fs::read_to_string(path) {
        Ok(text) => Ok(Some(toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))?)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e).with_context(|| format!("reading {}", path.display())),
    }
}

/// Load the effective policy. A file that exists but does not parse is an error: execution then
/// refuses rather than guessing.
pub fn load(adage_home: &Path) -> Result<Policy> {
    let personal_path = adage_home.join("policy.toml");
    let managed_path = managed_path();
    let personal = read(&personal_path)?;
    let managed = read(&managed_path)?;
    let mut p = merge(personal.as_ref(), managed.as_ref())?;
    if personal.is_some() {
        p.sources.push(personal_path.display().to_string());
    }
    if managed.is_some() {
        p.sources.push(format!("{} (managed)", managed_path.display()));
    }
    Ok(p)
}

impl Policy {
    /// Why policy refuses this request (with its locally classified effect), or `Ok`.
    pub fn permits(&self, req: &ExecRequest, classified: Effect) -> Result<(), String> {
        if req.level > self.max_level {
            return Err(format!("policy caps execution at {}", self.max_level.as_str().to_uppercase()));
        }
        if req.output.unstructured() && !self.allow_raw_output {
            return Err("policy does not allow raw output (exec.allow_raw_output)".into());
        }
        for e in [req.effect, classified] {
            if self.effects_deny.contains(&e) {
                return Err(format!("policy denies the {} effect", e.as_str()));
            }
        }
        if req.scripts.iter().any(|s| s.path.starts_with("cas:")) && !self.allow_vendor_scripts {
            return Err("policy does not allow vendored scripts (exec.allow_vendor_scripts)".into());
        }
        if let Origin::Pack { signer: Some(s), .. } = &req.origin {
            if self.publishers_deny.contains(s) {
                return Err(format!("policy denies publisher {s}"));
            }
            if !self.publishers_allow.is_empty() && !self.publishers_allow.contains(s) {
                return Err(format!("publisher {s} is not in policy's allow list"));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw(s: &str) -> RawPolicy {
        toml::from_str(s).unwrap()
    }

    #[test]
    fn defaults_are_conservative() {
        let p = Policy::default();
        assert_eq!(p.max_level, Level::E1);
        assert!(!p.auto_approve && !p.allow_raw_output && !p.allow_vendor_scripts);
        assert!(p.require_sandbox);
    }

    #[test]
    fn complete_stdout_uses_unstructured_output_policy() {
        let mut req = ExecRequest {
            repo_root: "/repo".into(),
            argv: vec!["git".into(), "status".into()],
            inputs: Default::default(),
            contracts: Default::default(),
            effect: Effect::Observe,
            level: Level::E1,
            output: Default::default(),
            scripts: vec![],
            profile: crate::sandbox::Profile::for_level(Level::E1),
            labels: Default::default(),
            origin: Origin::Local,
            route_exec_digest: None,
            action: None,
        };
        req.output.fields = serde_json::from_str(r#"{"result":{"type":"text","stdout":true}}"#).unwrap();
        assert!(Policy::default().permits(&req, Effect::Observe).unwrap_err().contains("allow_raw_output"));
        let allowed = merge(Some(&raw("[exec]\nallow_raw_output = true\n")), None).unwrap();
        assert!(allowed.permits(&req, Effect::Observe).is_ok());
    }

    #[test]
    fn managed_denies_win() {
        let personal = raw(
            "[exec]\nmax_level = \"E2\"\nauto_approve = \"inert-observe\"\nallow_raw_output = true\nrequire_sandbox = false\n\
             [publishers]\nallow = [\"SHA256:a\", \"SHA256:b\"]\n",
        );
        let managed = raw(
            "[exec]\nmax_level = \"E1\"\nauto_approve = \"off\"\nallow_raw_output = false\n[publishers]\ndeny = [\"SHA256:b\"]\n\
             [effects]\ndeny = [\"environment\"]\n",
        );
        let p = merge(Some(&personal), Some(&managed)).unwrap();
        assert_eq!(p.max_level, Level::E1);
        assert!(!p.auto_approve);
        assert!(!p.allow_raw_output);
        assert!(p.require_sandbox, "the personal file cannot lower require_sandbox");
        assert_eq!(p.publishers_allow, vec!["SHA256:a"]);
        assert_eq!(p.publishers_deny, vec!["SHA256:b"]);
        assert_eq!(p.effects_deny, vec![Effect::Environment]);
        let alone = merge(Some(&personal), None).unwrap();
        assert!(alone.auto_approve && alone.allow_raw_output && alone.require_sandbox);
        assert_eq!(alone.max_level, Level::E2);
        let lowered = merge(None, Some(&raw("[exec]\nrequire_sandbox = false\n"))).unwrap();
        assert!(!lowered.require_sandbox, "only the managed file can lower it");
        let env_actions = raw("[exec]\nallow_environment_actions = true\n");
        assert!(!merge(Some(&env_actions), None).unwrap().allow_environment_actions, "a team opt-in only");
        assert!(merge(None, Some(&env_actions)).unwrap().allow_environment_actions);
    }

    #[test]
    fn unknown_values_fail_closed() {
        assert!(merge(Some(&raw("[exec]\nmax_level = \"E9\"\n")), None).is_err());
        assert!(merge(Some(&raw("[exec]\nauto_approve = \"yes\"\n")), None).is_err());
        assert!(merge(Some(&raw("[effects]\ndeny = [\"everything\"]\n")), None).is_err());
        assert!(toml::from_str::<RawPolicy>("[exec]\nsurprise = 1\n").is_err());
    }

    #[test]
    fn policy_env_never_names_secrets() {
        let p =
            merge(Some(&raw("[exec]\nenv = [\"JAVA_HOME\", \"SSH_AUTH_SOCK\", \"GITHUB_TOKEN\"]\n")), None).unwrap();
        assert_eq!(p.env, vec!["JAVA_HOME"]);
    }
}
