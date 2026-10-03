//! `SkillPrompt` (plan §2.2.7, architecture §3.11 "verify in place"): the Claude Code mod's
//! `skill.prompt` asks, as the engine expands a skill's text for the model, whether Adage has
//! anything to add. For a skill `adage train` compiled and replayed in this repository, and whose
//! pack a person accepted, it answers the text with one plain line per step the replay verified or
//! found stale, under a fixed heading. The skill's file is untouched; the model reads the
//! repository's truth beside it.
//!
//! The record is `verified.json` beside the staged pack (`adage_pack::verified`); every line of it
//! that reaches the model passes `render::sanitise`. In the control arm, and in the mod's observe
//! mode, the text is unchanged and `would_annotate` is spooled. Like every hook path it fails
//! open: anything missing or unreadable answers nothing.
//!
//! This module also says which skills a `skill` step may invoke at a `TurnStep`
//! ([`invocable`]): only an inert skill in an accepted record, and only while the person has
//! turned `skills.invoke` on.
//!
//! Both read the skill file again first (`verified::current`): a skill whose file no longer hashes
//! to its record (edited since it was compiled and accepted) is neither annotated nor invoked,
//! and its inertness is read from the file, never taken from the record alone.

use crate::failopen::{self, Failure};
use crate::harness::{Adapter, Harness};
use adage_core::repo;
use adage_pack::verified::{self, Outcome, Verified};
use adage_runtime::{assign, render, Arm, SessionState};
use adage_store::{spool, state, Paths};
use serde_json::{json, Value};
use std::path::Path;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

/// The native event name.
pub const EVENT: &str = "SkillPrompt";
/// At most this many bytes are added to a skill's text.
pub const MAX_ADDED: usize = 2_400;
/// The heading's fixed words, before the date.
pub const HEADING: &str = "Adage verified these steps in this repository on";

/// Whether a harness hands Adage a skill's text as the model is about to read it: only Claude
/// Code, through the mod's `skill.prompt`. Every other harness's skills are read as written.
pub fn rewrites_skill_text(h: Harness) -> bool {
    matches!(h, Harness::ClaudeCode)
}

/// Where `adage train` stages compiled skills (`$ADAGE_HOME/train/skills`).
pub fn skills_dir(paths: &Paths) -> std::path::PathBuf {
    paths.root.join("train").join("skills")
}

/// What is appended to a skill's text: the heading with the replay's date, then one line per
/// step the replay verified (`make test: works`) or found stale (`make suite: stale (exit 2: …)`),
/// every interpolated value sanitised, at most [`MAX_ADDED`] bytes in all. Steps not replayed say
/// nothing. `None` when no step has anything to say.
pub fn annotation(v: &Verified) -> Option<(String, usize, usize)> {
    let date = render::sanitise(v.date()?);
    let mut text = format!("{HEADING} {date}:");
    let (mut works, mut stale) = (0, 0);
    for s in &v.steps {
        let command = render::sanitise(&s.command);
        if command.is_empty() {
            continue;
        }
        let line = match s.result {
            Outcome::Works => format!("\n- {command}: works"),
            Outcome::Stale => match s.signature.as_deref().map(render::sanitise).filter(|x| !x.is_empty()) {
                Some(sig) => format!("\n- {command}: stale ({sig})"),
                None => format!("\n- {command}: stale"),
            },
            Outcome::NotReplayed => continue,
        };
        // Two bytes for the blank line that separates the annotation from the skill's text.
        if text.len() + line.len() + 2 > MAX_ADDED {
            break;
        }
        text.push_str(&line);
        match s.result {
            Outcome::Works => works += 1,
            _ => stale += 1,
        }
    }
    (works + stale > 0).then_some((text, works, stale))
}

/// The skills a `skill` step may invoke in this repository at a `TurnStep` (architecture §3.11,
/// level 2): none unless the person turned `skills.invoke` on; then each skill whose replay record
/// a person accepted here and which runs nothing when it expands (`inert`). The predict grant on
/// the route is still required (`Policy::predict_grants`).
pub fn invocable(paths: &Paths, root: &Path) -> Vec<String> {
    if !adage_store::config::load(paths).config.skills_invoke {
        return Vec::new();
    }
    let key = repo::repo_key(root);
    verified::find_all(&skills_dir(paths), &key)
        .into_iter()
        .filter(|(_, v)| v.skill.inert && verified::current(v) == Some(true))
        .map(|(_, v)| v.skill.name)
        .collect()
}

/// `SkillPrompt`: the annotated text, or nothing. See the module documentation.
pub fn prompt(
    adapter: &dyn Adapter,
    p: &Value,
    session: &str,
    root: &Path,
    paths: &Paths,
    started: Instant,
) -> Result<Option<String>, Failure> {
    if !rewrites_skill_text(adapter.harness()) {
        return Ok(None);
    }
    let (Some(skill), Some(text)) = (p.get("skill").and_then(Value::as_str), p.get("text").and_then(Value::as_str))
    else {
        return Ok(None);
    };
    if skill.is_empty() || skill.len() > 128 {
        return Ok(None);
    }
    let key = repo::repo_key(root);
    let Some((_, record)) = verified::find(&skills_dir(paths), skill, &key) else { return Ok(None) };
    if verified::current(&record).is_none() {
        return Ok(None);
    }
    let Some((added, works, stale)) = annotation(&record) else { return Ok(None) };
    // The arm, assigned here if this is the session's first decision (as `PreToolUse` does).
    let arm_env = std::env::var("ADAGE_ARM").ok();
    let share: f64 = adage_store::config::load(paths).config.runtime_control;
    let (arm, propensity) = state::update::<SessionState, _>(&paths.state(session), |st| {
        if st.arm.is_none() {
            let (arm, pr) = assign(session, share, arm_env.as_deref());
            st.arm = Some(arm);
            st.propensity = Some(pr);
            st.repo = Some(key.clone());
        }
        (st.arm, st.propensity)
    })
    .map_err(|e| Failure::io("state", "assigning the session's arm", &e))?;
    let observe = p.get("observe").and_then(Value::as_bool).unwrap_or(false);
    let act = arm != Some(Arm::Control) && !observe;
    let what = json!({ "skill": record.skill.name, "verified": works, "stale": stale, "bytes": added.len() + 2 });
    let ts = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
    let mut rec = json!({ "ts": ts, "event": EVENT, "kind": "skill_prompt", "arm": arm, "propensity": propensity,
                          "repo": key, "would_annotate": what });
    if act {
        rec["annotated"] = what;
    }
    if observe {
        rec["observe"] = json!(true);
    }
    rec["duration_us"] = json!(u64::try_from(started.elapsed().as_micros()).unwrap_or(u64::MAX));
    if let Err(e) = spool::append(&paths.spool(session), &rec) {
        failopen::record(
            &paths.root,
            adapter.name(),
            EVENT,
            &Failure::io("spool", "appending to the session spool", &e),
        );
    }
    if !act {
        return Ok(None);
    }
    Ok(Some(json!({ "text": format!("{text}\n\n{added}") }).to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use adage_pack::verified::{Skill, Step};

    fn record(steps: Vec<Step>) -> Verified {
        Verified {
            schema: verified::SCHEMA.into(),
            skill: Skill { name: "acme-work-item".into(), sha256: "a".repeat(64), inert: true, path: String::new() },
            repo: "acme@0123456789ab".into(),
            pack: "acme/skill-acme-work-item".into(),
            digest: "b".repeat(64),
            steps,
            accepted_at: Some("2026-10-03".into()),
        }
    }

    fn step(command: &str, result: Outcome, signature: Option<&str>, date: &str) -> Step {
        Step {
            id: "s1".into(),
            route: String::new(),
            command: command.into(),
            result,
            signature: signature.map(str::to_string),
            date: date.into(),
            sandbox: "test".into(),
        }
    }

    #[test]
    fn the_annotation_says_what_was_verified_or_stale_and_nothing_else() {
        let v = record(vec![
            step("make test", Outcome::Works, None, "2026-10-02"),
            step("make gen", Outcome::NotReplayed, None, "2026-10-02"),
            step("make suite", Outcome::Stale, Some("exit 2: suite is broken"), "2026-10-03"),
            step("make lint", Outcome::Stale, None, "2026-10-03"),
        ]);
        let (text, works, stale) = annotation(&v).unwrap();
        assert_eq!(
            text,
            "Adage verified these steps in this repository on 2026-10-03:\n- make test: works\n- make suite: stale (exit 2: suite is broken)\n- make lint: stale"
        );
        assert_eq!((works, stale), (1, 2));
        let quiet = record(vec![step("make gen", Outcome::NotReplayed, None, "2026-10-02")]);
        assert!(annotation(&quiet).is_none(), "nothing replayed, nothing to say");
        let long = record(
            (0..64)
                .map(|i| {
                    step(
                        &format!("make a-target-whose-name-is-long-enough-to-matter-{i}"),
                        Outcome::Works,
                        None,
                        "2026-10-03",
                    )
                })
                .collect(),
        );
        let (text, works, _) = annotation(&long).unwrap();
        assert!(text.len() + 2 <= MAX_ADDED && works < 64);
    }

    #[test]
    fn nothing_is_invocable_until_skills_invoke_is_on_and_then_only_inert_accepted_skills() {
        let home = tempfile::tempdir().unwrap();
        let paths = Paths::at(home.path().join("adage"));
        let repo = home.path().join("acme");
        std::fs::create_dir_all(&repo).unwrap();
        let key = repo::repo_key(&repo);
        let stage = |name: &str, inert: bool, accepted: bool| {
            let dir = skills_dir(&paths).join(name);
            std::fs::create_dir_all(&dir).unwrap();
            let file = repo.join(".claude/skills").join(name).join("SKILL.md");
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            let text = if inert { "Run `make test`.\n" } else { "---\ncontext: fork\n---\nRun `make test`.\n" };
            std::fs::write(&file, text).unwrap();
            let mut v = record(vec![step("make test", Outcome::Works, None, "2026-10-03")]);
            v.skill.name = name.into();
            v.skill.inert = inert;
            v.skill.sha256 = adage_pack::digest::sha256_hex(text.as_bytes());
            v.skill.path = file.to_string_lossy().into_owned();
            v.repo = key.clone();
            v.accepted_at = accepted.then(|| "2026-10-03".into());
            verified::write(&dir, &v).unwrap();
            file
        };
        stage("release-checklist", true, true);
        stage("forked", false, true);
        stage("unreviewed", true, false);
        let edited = stage("edited", true, true);
        assert!(invocable(&paths, &repo).is_empty(), "skills.invoke is off by default");
        adage_store::config::set(&paths, "skills.invoke", "true").unwrap();
        assert_eq!(invocable(&paths, &repo), ["edited", "release-checklist"]);
        // Review M5: a skill edited after its record was accepted (a dynamic-context block added)
        // is not invoked; neither is one whose file is gone.
        std::fs::write(&edited, "Run `make test`.\nThe branch: !`git branch --show-current`\n").unwrap();
        assert_eq!(invocable(&paths, &repo), ["release-checklist"]);
        std::fs::remove_file(repo.join(".claude/skills/release-checklist/SKILL.md")).unwrap();
        assert!(invocable(&paths, &repo).is_empty());
        assert!(rewrites_skill_text(Harness::ClaudeCode));
        assert!(!rewrites_skill_text(Harness::Codex) && !rewrites_skill_text(Harness::OpenCode));
    }
}
