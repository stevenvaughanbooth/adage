//! `procedure.json`: the compiled skill's procedure, for review (docs/packs.md, docs/training.md).
//!
//! A pack compiled from a skill (`adage train`, origin `trained`) carries the procedure it was
//! compiled from: every step of the skill, what became of it (compiled to which routes, left as
//! text and why, or dropped because its replay failed on a clean checkout), and the skill's hash,
//! which must equal `[provenance] skill`. A reviewer sees what stayed text, and why, next to the
//! routes. It is covered by the pack's signature like every other file.
//!
//! It is for people only: nothing reads it to set a strength, an execution level or an approval,
//! and nothing of it is rendered to an agent. It is read as untrusted input like the rest of the
//! pack: strict schema, bounded, printable text, and every route it names must be in the pack.

use crate::validate::{people_text_ok, safe_id, sha256_hex_ok};
use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// The file's name at the pack's root.
pub const PROCEDURE: &str = "procedure.json";
/// The `schema` of every `procedure.json` this build reads and writes.
pub const SCHEMA: &str = "adage-procedure/1";
/// At most this many steps, advice notes, commands per step, and replay records per step.
pub const MAX_STEPS: usize = 64;
pub const MAX_ADVICE: usize = 64;
pub const MAX_ITEMS: usize = 16;
/// Longest text field, in bytes.
pub const MAX_TEXT: usize = 600;

/// The procedure a pack was compiled from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Procedure {
    /// Always [`SCHEMA`].
    pub schema: String,
    pub skill: SkillRef,
    pub steps: Vec<ProcedureStep>,
    /// Blocks of the skill that direct no command (kept as text), and what became of each.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub advice: Vec<AdviceNote>,
}

/// The skill: its name, and the SHA-256 and size of the file compiled.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SkillRef {
    pub name: String,
    /// Lowercase hex SHA-256 of the skill file's bytes.
    pub sha256: String,
    pub bytes: u64,
}

/// One step of the skill's procedure.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProcedureStep {
    /// `s1`, `s2`, … in the skill's order.
    pub id: String,
    pub title: String,
    /// `lookup`, `read`, `search`, `edit`, `regenerate`, `verify`, `run`, `install`, `vcs`,
    /// `remove`, `note`.
    pub role: String,
    /// `always`, `after_edit <glob>`, `before_finish`, or `judgment`.
    pub when: String,
    /// The commands the step directs, as the skill writes them (`{slot}` placeholders).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub commands: Vec<String>,
    /// The commands the step forbids.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub avoid: Vec<String>,
    pub outcome: Outcome,
    /// What each command did when replayed on a clean checkout, one line each.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub replay: Vec<String>,
}

/// What became of a step.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub enum Outcome {
    /// Compiled into these routes of the pack.
    Compiled { routes: Vec<String> },
    /// Left as text: nothing Adage can deliver does what it says (`needs judgment`, …).
    Text { reason: String },
    /// Dropped: its command failed on a clean checkout, so the skill is stale here.
    Stale { reason: String },
}

/// A block of the skill that directs no command.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdviceNote {
    pub text: String,
    /// The route a fixed template made of it, if any (`leave changes uncommitted`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub route: Option<String>,
}

const ROLES: [&str; 11] =
    ["lookup", "read", "search", "edit", "regenerate", "verify", "run", "install", "vcs", "remove", "note"];

fn text_ok(t: &str) -> bool {
    people_text_ok(t, MAX_TEXT)
}

impl Procedure {
    /// Check the procedure against its pack: the schema, bounds and text, the skill hash against
    /// `[provenance] skill`, and every route it names against the pack's routes. Every compiled
    /// route must be one of the pack's, but a pack may hold routes no step names (a replay
    /// refinement, advice).
    pub fn check(&self, skill: Option<&str>, routes: &BTreeSet<&str>) -> Result<()> {
        if self.schema != SCHEMA {
            bail!("procedure.json: schema {:?} is not {SCHEMA:?}", self.schema);
        }
        let s = &self.skill;
        if !text_ok(&s.name) || s.name.is_empty() || s.name.len() > 128 {
            bail!("procedure.json: skill name {:?} is not allowed", s.name);
        }
        if !sha256_hex_ok(&s.sha256) {
            bail!("procedure.json: skill sha256 is not lowercase hex SHA-256");
        }
        match skill {
            Some(p) if p.strip_prefix("sha256:") == Some(s.sha256.as_str()) => {}
            Some(p) => bail!("procedure.json: the skill's sha256 differs from [provenance] skill {p:?}"),
            None => bail!("procedure.json needs [provenance] skill = \"sha256:<hex>\""),
        }
        if self.steps.len() > MAX_STEPS || self.advice.len() > MAX_ADVICE {
            bail!("procedure.json: at most {MAX_STEPS} steps and {MAX_ADVICE} advice notes");
        }
        let mut ids = BTreeSet::new();
        for st in &self.steps {
            let ctx = format!("procedure.json: step {:?}", st.id);
            if !safe_id(&st.id) || !ids.insert(st.id.as_str()) {
                bail!("{ctx}: the id is not allowed or is repeated");
            }
            if !ROLES.contains(&st.role.as_str()) {
                bail!("{ctx}: role {:?} is not one of {ROLES:?}", st.role);
            }
            let when_ok = matches!(st.when.as_str(), "always" | "before_finish" | "judgment")
                || st.when.strip_prefix("after_edit ").is_some_and(crate::validate::safe_glob);
            if !when_ok {
                bail!("{ctx}: when {:?} is not always, after_edit <glob>, before_finish or judgment", st.when);
            }
            for list in [&st.commands, &st.avoid, &st.replay] {
                if list.len() > MAX_ITEMS || !list.iter().all(|t| text_ok(t)) {
                    bail!("{ctx}: at most {MAX_ITEMS} printable lines of at most {MAX_TEXT} bytes each");
                }
            }
            if !text_ok(&st.title) {
                bail!("{ctx}: the title contains characters that may not be shown");
            }
            match &st.outcome {
                Outcome::Compiled { routes: rs } => {
                    if rs.is_empty() || rs.len() > MAX_ITEMS {
                        bail!("{ctx}: a compiled step names 1..={MAX_ITEMS} routes");
                    }
                    if let Some(r) = rs.iter().find(|r| !routes.contains(r.as_str())) {
                        bail!("{ctx}: route {r:?} is not in the pack (dangling reference)");
                    }
                }
                Outcome::Text { reason } | Outcome::Stale { reason } => {
                    if reason.is_empty() || !text_ok(reason) {
                        bail!("{ctx}: the reason is empty or contains characters that may not be shown");
                    }
                }
            }
        }
        for a in &self.advice {
            if !text_ok(&a.text) {
                bail!("procedure.json: an advice note contains characters that may not be shown");
            }
            if let Some(r) = &a.route {
                if !routes.contains(r.as_str()) {
                    bail!("procedure.json: advice route {r:?} is not in the pack (dangling reference)");
                }
            }
        }
        Ok(())
    }

    /// Steps compiled, left as text, and stale.
    pub fn counts(&self) -> (usize, usize, usize) {
        let n = |f: fn(&Outcome) -> bool| self.steps.iter().filter(|s| f(&s.outcome)).count();
        (
            n(|o| matches!(o, Outcome::Compiled { .. })),
            n(|o| matches!(o, Outcome::Text { .. })),
            n(|o| matches!(o, Outcome::Stale { .. })),
        )
    }
}

/// Text made safe for a procedure field: control and invisible characters dropped, whitespace
/// collapsed, and cut to [`MAX_TEXT`] bytes on a character boundary.
pub fn clean_text(s: &str) -> String {
    let kept: String = s
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .filter(|c| !adage_tracks::io::invisible(*c) && *c != '\u{2028}' && *c != '\u{2029}')
        .collect();
    let mut out = kept.split_whitespace().collect::<Vec<_>>().join(" ");
    if out.len() > MAX_TEXT {
        let mut cut = MAX_TEXT - 3;
        while !out.is_char_boundary(cut) {
            cut -= 1;
        }
        out.truncate(cut);
        out.push_str("...");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Procedure {
        Procedure {
            schema: SCHEMA.into(),
            skill: SkillRef { name: "acme-work-item".into(), sha256: "a".repeat(64), bytes: 10 },
            steps: vec![
                ProcedureStep {
                    id: "s1".into(),
                    title: "Run the tests".into(),
                    role: "verify".into(),
                    when: "after_edit schema/*.json".into(),
                    commands: vec!["make test".into()],
                    avoid: vec![],
                    outcome: Outcome::Compiled { routes: vec!["r1".into()] },
                    replay: vec!["make test: exit 0".into()],
                },
                ProcedureStep {
                    id: "s2".into(),
                    title: "Make the change".into(),
                    role: "edit".into(),
                    when: "always".into(),
                    commands: vec![],
                    avoid: vec![],
                    outcome: Outcome::Text { reason: "needs judgment".into() },
                    replay: vec![],
                },
            ],
            advice: vec![AdviceNote { text: "Keep changes minimal.".into(), route: None }],
        }
    }

    #[test]
    fn a_procedure_names_only_routes_of_its_pack_and_its_skill() {
        let p = sample();
        let skill = format!("sha256:{}", "a".repeat(64));
        let routes = BTreeSet::from(["r1"]);
        p.check(Some(&skill), &routes).unwrap();
        assert!(p.check(None, &routes).is_err(), "needs the provenance hash");
        assert!(p.check(Some(&format!("sha256:{}", "b".repeat(64))), &routes).is_err());
        assert!(p.check(Some(&skill), &BTreeSet::new()).is_err(), "dangling route");
        let mut bad = sample();
        bad.steps[0].title = "a\u{202E}b".into();
        assert!(bad.check(Some(&skill), &routes).is_err());
        let mut bad = sample();
        bad.steps[1].when = "after_edit /etc/*".into();
        assert!(bad.check(Some(&skill), &routes).is_err());
        assert_eq!(p.counts(), (1, 1, 0));
        let json = serde_json::to_string(&p).unwrap();
        assert!(serde_json::from_str::<Procedure>(&json.replace("\"schema\"", "\"x\":1,\"schema\"")).is_err());
    }

    #[test]
    fn clean_text_is_printable_and_bounded() {
        assert_eq!(clean_text("a\u{200B}b\n\tc"), "ab c");
        let long = "é".repeat(400);
        let c = clean_text(&long);
        assert!(c.len() <= MAX_TEXT && c.ends_with("..."));
    }
}
