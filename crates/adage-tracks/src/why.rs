//! Why a reflex exists (Track G §6.3): the evidence behind each route the graph engine compiled,
//! kept beside the track in `.why.json` for people (`adage review`), **never rendered to the
//! agent**. The file is hidden, so it never travels in a shared pack; the index does not read it.
//!
//! One entry per route id: the mined pattern (its canonical string), the backoff depth (context
//! items), the compression gain in bits, lift, confidence and its lower bound, the members of
//! each prototype item with their counts, and up to three example traces (session ids).

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

pub const FILE: &str = ".why.json";

/// One pattern item's members: its label and the exact contents seen there, with counts.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Members {
    pub item: String,
    pub members: Vec<(String, u32)>,
}

/// The evidence behind one compiled route.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Why {
    /// The delivery shape (`run`, `obligation`, `avoid`, `read-batch`, `sequence`).
    pub shape: String,
    /// The mined pattern's canonical string.
    pub pattern: String,
    /// Context items the pattern conditions on (the variable-order backoff's depth).
    pub backoff: usize,
    /// Compression gain, in bits.
    pub gain: f64,
    pub lift: f64,
    pub conf: f64,
    pub conf_lb: f64,
    pub support: u32,
    pub days: u32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub members: Vec<Members>,
    /// At most three supporting traces (session ids).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub examples: Vec<String>,
    /// The instinct whose prior entered this reflex's evidence (Track G §6.3), if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prior: Option<String>,
}

impl Why {
    /// Lines for `adage review` (people only).
    pub fn lines(&self) -> Vec<String> {
        let mut out = vec![
            format!("why ({} reflex): {}", self.shape, self.pattern),
            format!(
                "  {} traces over {} days; confidence {:.2} (lower bound {:.2}); lift {:.2}; backoff depth {}; compression gain {:.1} bits",
                self.support, self.days, self.conf, self.conf_lb, self.lift, self.backoff, self.gain
            ),
        ];
        for m in &self.members {
            let list: Vec<String> = m.members.iter().take(6).map(|(k, n)| format!("{k} ×{n}")).collect();
            out.push(format!("  {}: {}", m.item, list.join(", ")));
        }
        if let Some(p) = &self.prior {
            out.push(format!("  instinct prior: {p}"));
        }
        if !self.examples.is_empty() {
            out.push(format!("  e.g. traces {}", self.examples.join(", ")));
        }
        out
    }
}

/// Every route's [`Why`] in a track directory (empty when there is no file or it is unreadable:
/// it explains, it never decides).
pub fn load(dir: &Path) -> BTreeMap<String, Why> {
    std::fs::read(dir.join(FILE)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

/// Write the track's `.why.json` (removed when there is nothing to explain).
pub fn save(dir: &Path, why: &BTreeMap<String, Why>) -> Result<()> {
    let p = dir.join(FILE);
    if why.is_empty() {
        if p.exists() {
            std::fs::remove_file(&p).with_context(|| p.display().to_string())?;
        }
        return Ok(());
    }
    std::fs::create_dir_all(dir)?;
    std::fs::write(&p, serde_json::to_string_pretty(why)? + "\n").with_context(|| p.display().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("track.toml"), "x\n").unwrap();
        let mut m = BTreeMap::new();
        m.insert(
            "finish-line-e7e809".to_string(),
            Why {
                shape: "obligation".into(),
                pattern: "last[edit/c:schema/a.json] || → [act/x:exec make gen✓]".into(),
                backoff: 1,
                gain: 12.5,
                lift: 4.0,
                conf: 0.9,
                conf_lb: 0.8,
                support: 9,
                days: 3,
                members: vec![Members { item: "edit".into(), members: vec![("schema/a.json".into(), 5)] }],
                examples: vec!["s1".into()],
                prior: None,
            },
        );
        save(d.path(), &m).unwrap();
        assert_eq!(load(d.path()), m);
        assert!(FILE.starts_with('.'), "hidden, so never shared");
        assert!(m["finish-line-e7e809"].lines()[0].starts_with("why (obligation reflex)"));
        save(d.path(), &BTreeMap::new()).unwrap();
        assert!(!d.path().join(FILE).exists());
    }
}
