//! `verified.json`: what `adage train` found when it replayed a skill's steps in this repository,
//! beside the staged pack (`$ADAGE_HOME/train/skills/<name>/verified.json`, plan §2.4).
//!
//! It is written by `adage-train` and read by the hook (`SkillPrompt`, which annotates the skill's
//! text as the model reads it, and `TurnStep`, which invokes only an inert skill), so its type and
//! reader live here, at a layer the hook may link. It sits outside the pack on purpose: it carries
//! a date, which would break the pack's deterministic digest (the `cards.json` precedent), and it
//! is never covered by the pack's signature.
//!
//! It is read as untrusted input: a strict schema, bounded sizes and counts. Its text is for the
//! model only after the hook passes every line through the runtime's `sanitise`; nothing in it
//! sets a strength, a grant or an approval. Only a file a person accepted (`adage train accept`
//! stamps `accepted_at`) is ever read for a session.

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// The file's name beside the staged pack.
pub const VERIFIED: &str = "verified.json";
/// The `schema` of every `verified.json` this build reads and writes.
pub const SCHEMA: &str = "adage-train-verified/1";
/// The largest file read, in bytes.
pub const MAX_BYTES: u64 = 256 * 1024;
/// At most this many steps.
pub const MAX_STEPS: usize = 64;
/// Longest text field, in bytes.
pub const MAX_TEXT: usize = 600;
/// At most this many staged skills are looked through for one name ([`find`]).
pub const MAX_SKILLS: usize = 256;

/// One skill's replay record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Verified {
    /// Always [`SCHEMA`].
    pub schema: String,
    pub skill: Skill,
    /// The repository key the steps were replayed in (`adage_core::repo::repo_key`).
    pub repo: String,
    /// The staged pack the record goes with, and its digest (empty when nothing compiled):
    /// accepting that pack is what accepts this record.
    #[serde(default)]
    pub pack: String,
    #[serde(default)]
    pub digest: String,
    pub steps: Vec<Step>,
    /// When a person accepted the pack (`adage train accept`), as `YYYY-MM-DD`; `None` until then.
    /// The hook reads only an accepted record.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accepted_at: Option<String>,
}

/// The skill: its name as the harness lists it, the SHA-256 of the file compiled, and whether
/// invoking it runs nothing by itself ([`inert`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Skill {
    pub name: String,
    pub sha256: String,
    pub inert: bool,
}

/// One compiled step and what its replay found.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Step {
    /// The skill's step ids it compiles (`s4`, or `s4+s5` for a sequence).
    pub id: String,
    /// The route it compiled to.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub route: String,
    /// The command the step runs, as the skill writes it (`{slot}` placeholders kept).
    pub command: String,
    pub result: Outcome,
    /// For a stale step, what the replay saw fail.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
    /// The replay's date, `YYYY-MM-DD`.
    pub date: String,
    /// The sandbox it ran in, in one line, or why there was none.
    pub sandbox: String,
}

/// What a step's replay found.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Outcome {
    /// Every check held on a clean checkout.
    Works,
    /// A check failed: the skill is stale here.
    Stale,
    /// Not replayed (no sandbox, nothing to run with, replay turned off).
    NotReplayed,
}

impl Outcome {
    pub fn as_str(self) -> &'static str {
        match self {
            Outcome::Works => "works",
            Outcome::Stale => "stale",
            Outcome::NotReplayed => "not-replayed",
        }
    }
}

fn date_ok(d: &str) -> bool {
    d.len() == 10 && d.bytes().enumerate().all(|(i, c)| if i == 4 || i == 7 { c == b'-' } else { c.is_ascii_digit() })
}

impl Verified {
    /// Check the schema, bounds and dates. Text fields are bounded, not judged: whoever renders
    /// them to a model sanitises every line.
    pub fn check(&self) -> Result<()> {
        if self.schema != SCHEMA {
            bail!("verified.json: schema {:?} is not {SCHEMA:?}", self.schema);
        }
        let s = &self.skill;
        if s.name.is_empty() || s.name.len() > 128 {
            bail!("verified.json: skill name {:?} is not allowed", s.name);
        }
        if !crate::validate::sha256_hex_ok(&s.sha256) {
            bail!("verified.json: skill sha256 is not lowercase hex SHA-256");
        }
        if self.repo.is_empty() || self.repo.len() > 256 {
            bail!("verified.json: repo {:?} is not allowed", self.repo);
        }
        if self.pack.len() > 128 || !(self.digest.is_empty() || crate::validate::sha256_hex_ok(&self.digest)) {
            bail!("verified.json: pack or digest is not allowed");
        }
        if self.steps.len() > MAX_STEPS {
            bail!("verified.json: at most {MAX_STEPS} steps");
        }
        if self.accepted_at.as_deref().is_some_and(|d| !date_ok(d)) {
            bail!("verified.json: accepted_at is not a YYYY-MM-DD date");
        }
        for st in &self.steps {
            let long = [st.id.as_str(), &st.route, &st.command, &st.sandbox, st.signature.as_deref().unwrap_or("")]
                .iter()
                .any(|t| t.len() > MAX_TEXT);
            if st.id.is_empty() || long || !date_ok(&st.date) {
                bail!("verified.json: step {:?} is not allowed", st.id);
            }
        }
        Ok(())
    }

    /// Whether a person accepted the pack this record goes with.
    pub fn accepted(&self) -> bool {
        self.accepted_at.is_some()
    }

    /// The latest replay date among the steps.
    pub fn date(&self) -> Option<&str> {
        self.steps.iter().map(|s| s.date.as_str()).max()
    }
}

/// Read and check `<dir>/verified.json`.
pub fn read(dir: &Path) -> Result<Verified> {
    let p = dir.join(VERIFIED);
    let meta = std::fs::symlink_metadata(&p).with_context(|| format!("reading {}", p.display()))?;
    if !meta.is_file() || meta.len() > MAX_BYTES {
        bail!("{} is not a regular file of at most {MAX_BYTES} bytes", p.display());
    }
    let v: Verified =
        serde_json::from_slice(&std::fs::read(&p)?).with_context(|| format!("{} is not valid", p.display()))?;
    v.check()?;
    Ok(v)
}

/// Write `<dir>/verified.json` (the directory must exist), checked first.
pub fn write(dir: &Path, v: &Verified) -> Result<()> {
    v.check()?;
    let p = dir.join(VERIFIED);
    let tmp = p.with_extension(format!("tmp{}", std::process::id()));
    std::fs::write(&tmp, serde_json::to_vec_pretty(v)?)?;
    std::fs::rename(&tmp, &p)?;
    Ok(())
}

/// Mark the record in `dir` accepted on `date`, when it goes with the pack of `digest` (the pack a
/// person just accepted). Returns whether it was marked; a record for another pack, or none, is
/// left as it is.
pub fn accept(dir: &Path, digest: &str, date: &str) -> Result<bool> {
    if !date_ok(date) {
        bail!("{date:?} is not a YYYY-MM-DD date");
    }
    if !dir.join(VERIFIED).exists() {
        return Ok(false);
    }
    let mut v = read(dir)?;
    if v.digest.is_empty() || v.digest != digest {
        return Ok(false);
    }
    v.accepted_at = Some(date.to_string());
    write(dir, &v)?;
    Ok(true)
}

/// The accepted record for the skill `name` in the repository `repo`, among the staged skills
/// under `skills` (`$ADAGE_HOME/train/skills`): its directory and record. Directories are looked
/// through in name order, at most [`MAX_SKILLS`], never through a symlink; an unreadable or
/// invalid record is passed over (fail open: nothing).
pub fn find(skills: &Path, name: &str, repo: &str) -> Option<(PathBuf, Verified)> {
    find_all(skills, repo).into_iter().find(|(_, v)| v.skill.name == name)
}

/// Every accepted record for the repository `repo` under `skills`, as [`find`] reads them.
pub fn find_all(skills: &Path, repo: &str) -> Vec<(PathBuf, Verified)> {
    let Ok(rd) = std::fs::read_dir(skills) else { return Vec::new() };
    let mut dirs: Vec<PathBuf> =
        rd.flatten().filter(|e| e.file_type().is_ok_and(|t| t.is_dir())).map(|e| e.path()).take(MAX_SKILLS).collect();
    dirs.sort();
    dirs.into_iter()
        .filter_map(|d| read(&d).ok().map(|v| (d, v)))
        .filter(|(_, v)| v.accepted() && v.repo == repo)
        .collect()
}

/// Whether invoking a skill whose `SKILL.md` is `text` runs nothing by itself (architecture
/// §3.11): it holds no dynamic-context block (`` !`command` ``, which the harness runs when the
/// skill expands) and its front matter does not say `context: fork` (the skill runs as a subagent).
/// Anything that might be either is not inert.
pub fn inert(text: &str) -> bool {
    if text.contains("!`") {
        return false;
    }
    let mut lines = text.lines();
    if lines.next().map(str::trim) != Some("---") {
        return true;
    }
    for l in lines {
        let l = l.trim();
        if l == "---" {
            break;
        }
        if let Some(v) = l.strip_prefix("context:") {
            let v = v.trim().trim_matches(|c| c == '"' || c == '\'');
            if v.eq_ignore_ascii_case("fork") {
                return false;
            }
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Verified {
        Verified {
            schema: SCHEMA.into(),
            skill: Skill { name: "acme-work-item".into(), sha256: "a".repeat(64), inert: true },
            repo: "acme@0123456789ab".into(),
            pack: "acme/skill-acme-work-item".into(),
            digest: "b".repeat(64),
            steps: vec![
                Step {
                    id: "s1".into(),
                    route: "acme-work-item-s1-lookup".into(),
                    command: "scripts/wi-lookup {id}".into(),
                    result: Outcome::Works,
                    signature: None,
                    date: "2026-10-03".into(),
                    sandbox: "bwrap".into(),
                },
                Step {
                    id: "s5".into(),
                    route: String::new(),
                    command: "make suite".into(),
                    result: Outcome::Stale,
                    signature: Some("exit 2: suite is broken".into()),
                    date: "2026-10-03".into(),
                    sandbox: "bwrap".into(),
                },
            ],
            accepted_at: None,
        }
    }

    #[test]
    fn a_record_round_trips_and_is_found_only_once_accepted_for_its_repository() {
        let home = tempfile::tempdir().unwrap();
        let skills = home.path().join("train/skills");
        let dir = skills.join("acme-work-item");
        std::fs::create_dir_all(&dir).unwrap();
        let v = sample();
        write(&dir, &v).unwrap();
        let text = std::fs::read_to_string(dir.join(VERIFIED)).unwrap();
        assert!(text.contains("\"schema\": \"adage-train-verified/1\""), "{text}");
        assert!(text.contains("\"result\": \"not-replayed\"") || text.contains("\"result\": \"works\""));
        assert_eq!(read(&dir).unwrap(), v);
        assert!(find(&skills, "acme-work-item", &v.repo).is_none(), "not accepted yet");
        assert!(!accept(&dir, &"c".repeat(64), "2026-10-04").unwrap(), "another pack's digest");
        assert!(accept(&dir, &v.digest, "2026-10-04").unwrap());
        let (d, got) = find(&skills, "acme-work-item", &v.repo).unwrap();
        assert_eq!(d, dir);
        assert_eq!(got.accepted_at.as_deref(), Some("2026-10-04"));
        assert_eq!(got.date(), Some("2026-10-03"));
        assert!(find(&skills, "acme-work-item", "other@0123456789ab").is_none(), "another repository");
        assert!(find(&skills, "other-skill", &v.repo).is_none());
        assert!(accept(&skills.join("missing"), &v.digest, "2026-10-04").is_ok_and(|m| !m));
    }

    #[test]
    fn a_record_is_untrusted_input() {
        let ok = sample();
        ok.check().unwrap();
        let json = serde_json::to_string(&ok).unwrap();
        assert!(serde_json::from_str::<Verified>(&json.replace("\"schema\"", "\"x\":1,\"schema\"")).is_err());
        let mut bad = sample();
        bad.schema = "adage-train-verified/2".into();
        assert!(bad.check().is_err());
        let mut bad = sample();
        bad.skill.sha256 = "x".into();
        assert!(bad.check().is_err());
        let mut bad = sample();
        bad.steps[0].date = "yesterday".into();
        assert!(bad.check().is_err());
        let mut bad = sample();
        bad.steps[1].signature = Some("x".repeat(MAX_TEXT + 1));
        assert!(bad.check().is_err());
        let mut bad = sample();
        bad.steps = vec![bad.steps[0].clone(); MAX_STEPS + 1];
        assert!(bad.check().is_err());
        let mut bad = sample();
        bad.accepted_at = Some("soon".into());
        assert!(bad.check().is_err());
    }

    #[test]
    fn a_skill_with_a_dynamic_block_or_a_fork_is_not_inert() {
        assert!(inert("---\nname: x\n---\n\nRun `make test`.\n"));
        assert!(inert("No front matter. context: fork is only words here.\n"));
        assert!(!inert("---\nname: x\n---\n\nThe branch: !`git branch --show-current`\n"));
        assert!(!inert("---\nname: x\ncontext: fork\n---\nbody\n"));
        assert!(!inert("---\nname: x\ncontext: \"Fork\"\n---\nbody\n"));
        assert!(inert("---\nname: x\ncontext: inline\n---\nbody\n"));
    }
}
