//! `adage train <skill>`: compile a skill into one reviewed pack of reflexes.
//!
//! 1. **Compile** ([`crate::procedure`], [`crate::compile`]): the skill's procedure graph, and the
//!    routes its steps compile to, deterministically and for free (no model, no session).
//! 2. **Verify** ([`crate::verify`]): every compiled command replayed on a clean checkout in
//!    adage-exec's sandbox. Stale commands are dropped and reported; only verified routes may fire.
//! 3. **Pack** ([`pack`]): one pack, origin `trained`, with `procedure.json` (every step and what
//!    became of it) and the skill's hash in `[provenance] skill`, written to
//!    `$ADAGE_HOME/train/skills/<name>/` with the review. Nothing is installed: the person reads
//!    the review and runs `adage train accept <name>` (docs/training.md).
//! 4. **Record** ([`verified`]): beside the pack, `verified.json` (`adage_pack::verified`): each
//!    compiled step's command and what its replay found, dated, and whether the skill is inert.
//!    The hook reads it once the person has accepted the pack.

use crate::compile::{Compiled, Verified};
use crate::procedure::{self, Graph};
use crate::verify::{self, Report};
use adage_core::CommandTable;
use adage_pack::procedure::{clean_text, AdviceNote, Outcome as StepOutcome, Procedure, ProcedureStep, SkillRef};
use adage_pack::{
    Applicability, ExecCeiling, Pack, PackInfo, PackManifest, PackOrigin, Provenance, Requires, RouteV2, Shape,
    ToolReq, TriggerV2, FORMAT,
};
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// The largest skill file read.
pub const MAX_SKILL_BYTES: u64 = 1 << 20;

/// Where compiled skills are staged: `$ADAGE_HOME/train/skills/<name>/`.
pub fn skills_dir(home: &Path) -> PathBuf {
    home.join("train").join("skills")
}

/// The review record beside a staged pack (`review.json`): what was reviewed, so `adage train
/// accept` installs exactly that.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Review {
    pub schema: String,
    /// The pack's name (`owner/skill-name`) and digest.
    pub pack: String,
    pub digest: String,
    /// The repository compiled against: its key and checkout.
    pub repo_key: String,
    pub root: String,
    pub skill: String,
    pub skill_sha256: String,
    /// The sandbox the replays ran in (or why none).
    pub sandbox: String,
    /// Route id → strength, verified or not, as reviewed.
    pub routes: BTreeMap<String, ReviewedRoute>,
    /// The review as printed.
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReviewedRoute {
    pub strength: String,
    pub verified: bool,
}

pub const REVIEW_SCHEMA: &str = "adage-train-review/1";
pub const REVIEW_FILE: &str = "review.json";

/// Options for [`run`].
#[derive(Debug, Clone)]
pub struct Options {
    pub skill: PathBuf,
    /// Any path inside the repository the skill is for; by default the current directory's, else
    /// the skill's.
    pub repo: Option<PathBuf>,
    pub replay: verify::Options,
    pub home: PathBuf,
}

/// What `adage train <skill>` produced.
#[derive(Debug, Clone, Serialize)]
pub struct Outcome {
    pub compiled: Compiled,
    pub report: Report,
    pub repo_key: String,
    pub root: PathBuf,
    /// The staged pack (none when no step compiled).
    #[serde(skip)]
    pub pack: Option<Pack>,
    pub pack_name: String,
    pub digest: Option<String>,
    pub dir: PathBuf,
    /// The skill runs nothing when it is invoked ([`adage_pack::verified::inert`]).
    pub inert: bool,
    /// The skill file compiled, absolute: `verified.json` names it, and the hook reads it again
    /// at every use.
    #[serde(skip)]
    pub skill_file: PathBuf,
}

/// The repository a skill is compiled for.
pub fn repo_root(skill: &Path, repo: Option<&Path>) -> Result<PathBuf> {
    let find = |p: &Path| {
        let p = p.canonicalize().ok()?;
        adage_core::repo::find_root(if p.is_file() { p.parent()? } else { &p })
    };
    if let Some(r) = repo {
        return find(r).with_context(|| format!("{} is not inside a git repository", r.display()));
    }
    std::env::current_dir()
        .ok()
        .and_then(|d| find(&d))
        .or_else(|| find(skill))
        .context("run adage train inside the repository the skill is for, or pass --repo <path>")
}

fn slug(s: &str) -> String {
    let s: String = s
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-' { c } else { '-' })
        .collect();
    let s = s.trim_matches(|c: char| !c.is_ascii_alphanumeric()).to_string();
    let mut s: String = s.chars().take(48).collect();
    while s.ends_with(|c: char| !c.is_ascii_alphanumeric()) {
        s.pop();
    }
    if s.is_empty() {
        "repo".into()
    } else {
        s
    }
}

/// The pack's owner: the repository's name (`github.com/pallets/click` → `click`,
/// `acme-billing@1a2b3c` → `acme-billing`).
fn owner(repo_key: &str) -> String {
    let last = repo_key.rsplit('/').next().unwrap_or(repo_key);
    slug(last.split('@').next().unwrap_or(last))
}

fn procedure_of(c: &Compiled) -> Procedure {
    let g = &c.graph;
    let steps = g
        .steps
        .iter()
        .map(|s| {
            let routes = c.routes_of(&s.id);
            let text = c.text.get(&s.id).cloned().unwrap_or_default();
            let stale = c.stale.get(&s.id).cloned().unwrap_or_default();
            let outcome = if !routes.is_empty() {
                StepOutcome::Compiled { routes: routes.clone() }
            } else if let Some(w) = stale.first() {
                StepOutcome::Stale { reason: clean_text(w) }
            } else {
                StepOutcome::Text { reason: clean_text(text.first().map(String::as_str).unwrap_or("not compiled")) }
            };
            let replay: Vec<String> = c
                .candidates
                .iter()
                .filter(|x| x.steps.contains(&s.id))
                .flat_map(|x| x.replay.iter().map(|l| clean_text(l)))
                .chain(stale.iter().map(|w| clean_text(&format!("dropped: {w}"))))
                .chain(if routes.is_empty() {
                    Vec::new()
                } else {
                    text.iter().map(|t| clean_text(&format!("text: {t}"))).collect()
                })
                .take(adage_pack::procedure::MAX_ITEMS)
                .collect();
            ProcedureStep {
                id: s.id.clone(),
                title: clean_text(&s.title),
                role: s.role.as_str().into(),
                when: match &s.pre {
                    procedure::Pre::AfterEdit(g) if adage_pack::validate::safe_glob(g) => format!("after_edit {g}"),
                    procedure::Pre::AfterEdit(_) => "always".into(),
                    p => p.describe(),
                },
                commands: s.commands.iter().map(|x| clean_text(&x.template)).take(16).collect(),
                avoid: s.avoid.iter().map(|x| clean_text(&x.command)).take(16).collect(),
                outcome,
                replay,
            }
        })
        .collect();
    Procedure {
        schema: adage_pack::procedure::SCHEMA.into(),
        skill: SkillRef { name: clean_text(&g.skill.name), sha256: g.skill.sha256.clone(), bytes: g.skill.bytes },
        steps,
        advice: c
            .advice
            .iter()
            .take(adage_pack::procedure::MAX_ADVICE)
            .map(|a| AdviceNote { text: clean_text(&a.text), route: a.route.clone() })
            .collect(),
    }
}

/// Build the pack for a compiled skill (`None` when nothing compiled).
pub fn pack(c: &Compiled, root: &Path, table: &CommandTable) -> Result<Option<Pack>> {
    if c.candidates.is_empty() {
        return Ok(None);
    }
    let key = adage_core::repo::repo_key(root);
    let mut keys: Vec<String> =
        adage_pack::bind::local_keys(root).into_iter().filter(|k| adage_pack::validate::safe_repo_key(k)).collect();
    keys.sort();
    keys.truncate(16);
    if keys.is_empty() {
        bail!("the repository's key {key:?} cannot name a pack's applicability");
    }
    let routes: Vec<RouteV2> = c.candidates.iter().map(|x| x.route.clone()).collect();
    let mut templates: BTreeSet<String> = routes.iter().map(|r| r.shape.template()).collect();
    for r in &routes {
        for s in &r.steps {
            if let Some(a) = &s.advise {
                templates.insert(format!("advise:{}@1", a.template));
            }
        }
    }
    // Only the repository files some kept route runs.
    let used: BTreeSet<&str> =
        routes.iter().flat_map(|r| r.steps.iter()).filter_map(|s| s.script.as_ref()).map(|s| s.path.as_str()).collect();
    let manifest = PackManifest {
        format: FORMAT.into(),
        pack: PackInfo {
            name: format!("{}/skill-{}", owner(&key), c.prefix),
            version: "0.1.0".into(),
            origin: PackOrigin::Trained,
            description: clean_text(&format!(
                "Compiled from the skill {} (sha256 {}) by adage train",
                c.graph.skill.name,
                &c.graph.skill.sha256[..12]
            )),
        },
        applicability: Applicability::Repo { repo_keys: keys },
        requires: Requires {
            os: Vec::new(),
            tools: c.tools(table).into_iter().map(|name| ToolReq { name, version: None }).collect(),
            harness: Vec::new(),
            models: Vec::new(),
            templates: templates.into_iter().collect(),
            command_table: None,
            repo_files: c.repo_files.values().filter(|f| used.contains(f.path.as_str())).cloned().collect(),
        },
        exec: ExecCeiling::default(),
        provenance: Provenance {
            tool: format!("adage {}", env!("CARGO_PKG_VERSION")),
            origins: Default::default(),
            skill: Some(format!("sha256:{}", c.graph.skill.sha256)),
        },
    };
    let types = c
        .types
        .iter()
        .filter(|(id, _)| routes.iter().any(|r| r.inputs.values().any(|i| &i.ty == *id)))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    let mut p = Pack::new(manifest, routes, types, BTreeMap::new()).context("the compiled pack fails validation")?;
    p.procedure = Some(procedure_of(c));
    p.check().context("the compiled pack's procedure fails validation")?;
    Ok(Some(p))
}

/// Compile, verify and stage a skill.
pub fn run(o: &Options) -> Result<Outcome> {
    let meta = std::fs::metadata(&o.skill).with_context(|| format!("reading {}", o.skill.display()))?;
    if !meta.is_file() {
        bail!("{} is not a file", o.skill.display());
    }
    if meta.len() > MAX_SKILL_BYTES {
        bail!("{} is larger than {MAX_SKILL_BYTES} bytes", o.skill.display());
    }
    let bytes = std::fs::read(&o.skill)?;
    let root = repo_root(&o.skill, o.repo.as_deref())?;
    let table = CommandTable::load();
    let file = o.skill.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "SKILL.md".into());
    // A skill named by its directory (`skills/acme-work-item/SKILL.md`) when its front matter
    // names none.
    let fallback = o
        .skill
        .canonicalize()
        .ok()
        .and_then(|p| {
            let stem = p.file_stem()?.to_string_lossy().into_owned();
            if stem.eq_ignore_ascii_case("skill") {
                p.parent()?.file_name().map(|n| n.to_string_lossy().into_owned())
            } else {
                Some(stem)
            }
        })
        .unwrap_or_else(|| "skill".into());
    let graph: Graph = procedure::parse(&bytes, &file, &fallback, table);
    let mut compiled = crate::compile::compile(graph, &root, table);
    let report = verify::verify(&mut compiled, &root, &o.replay);
    let repo_key = adage_core::repo::repo_key(&root);
    let pack = pack(&compiled, &root, table)?;
    let dir = skills_dir(&o.home).join(&compiled.prefix);
    let pack_name = pack.as_ref().map(|p| p.manifest.pack.name.clone()).unwrap_or_default();
    let inert = adage_pack::verified::inert(&String::from_utf8_lossy(&bytes));
    let mut out = Outcome {
        compiled,
        report,
        repo_key,
        root: root.clone(),
        digest: None,
        pack,
        pack_name,
        dir: dir.clone(),
        inert,
        skill_file: o.skill.canonicalize().unwrap_or_else(|_| std::path::absolute(&o.skill).unwrap_or_default()),
    };
    stage(&mut out)?;
    Ok(out)
}

/// Write the staged pack, its review and `verified.json` under the outcome's directory, replacing
/// any earlier. A record a person accepted stays accepted when the pack staged now is the very
/// pack accepted (same skill, repository and digest); a changed skill is staged unaccepted.
fn stage(out: &mut Outcome) -> Result<()> {
    let before = adage_pack::verified::read(&out.dir).ok();
    let _ = std::fs::remove_dir_all(&out.dir);
    std::fs::create_dir_all(&out.dir).with_context(|| format!("creating {}", out.dir.display()))?;
    if let Some(p) = &out.pack {
        let pd = out.dir.join("pack");
        p.write(&pd)?;
        out.digest = Some(adage_pack::digest_dir(&pd)?);
    }
    let text = render(out);
    let review = Review {
        schema: REVIEW_SCHEMA.into(),
        pack: out.pack_name.clone(),
        digest: out.digest.clone().unwrap_or_default(),
        repo_key: out.repo_key.clone(),
        root: out.root.display().to_string(),
        skill: out.compiled.graph.skill.name.clone(),
        skill_sha256: out.compiled.graph.skill.sha256.clone(),
        sandbox: out.report.sandbox.clone(),
        routes: out
            .compiled
            .candidates
            .iter()
            .map(|c| {
                (
                    c.route.id.clone(),
                    ReviewedRoute {
                        strength: format!("{:?}", c.route.strength).to_lowercase(),
                        verified: c.verified == Verified::Works,
                    },
                )
            })
            .collect(),
        text,
    };
    std::fs::write(out.dir.join(REVIEW_FILE), serde_json::to_vec_pretty(&review)?)?;
    std::fs::write(out.dir.join("procedure.graph.json"), serde_json::to_vec_pretty(&out.compiled.graph)?)?;
    let mut record = verified(out, &adage_tracks::review::today());
    if let Some(b) = before.filter(|b| {
        b.skill.sha256 == record.skill.sha256
            && b.repo == record.repo
            && !b.digest.is_empty()
            && b.digest == record.digest
    }) {
        record.accepted_at = b.accepted_at;
    }
    adage_pack::verified::write(&out.dir, &record)?;
    Ok(())
}

/// The command a candidate runs, as the skill writes it: its run steps' argv, in order (a
/// sequence's `make test, then make ci`). Empty for one that runs nothing of its own (a read of
/// what a lookup lists, fixed advice, a prohibition).
fn command_of(c: &crate::compile::Candidate) -> String {
    let runs: Vec<String> = c.route.steps.iter().filter_map(|s| s.run.as_ref()).map(|r| r.argv.join(" ")).collect();
    match c.route.shape {
        Shape::Avoid | Shape::Read | Shape::Advise => String::new(),
        _ => runs.join(", then "),
    }
}

/// The failure a stale replay saw, from its reason (`` `make suite` fails on a clean checkout
/// (exit 2: suite is broken) `` → `exit 2: suite is broken`).
fn signature_of(why: &str) -> String {
    let inner =
        why.strip_suffix(')').and_then(|w| w.rfind(" (").map(|i| &w[i + 2..])).filter(|s| !s.is_empty()).unwrap_or(why);
    clean_text(&inner.replace('`', ""))
}

/// `verified.json` for an outcome, dated `date`: one step per candidate that runs a command, kept
/// or dropped as stale, with what its replay found (plan §2.4).
pub fn verified(out: &Outcome, date: &str) -> adage_pack::verified::Verified {
    use adage_pack::verified::{Outcome as Result_, Skill, Step, Verified as Record, SCHEMA};
    let c = &out.compiled;
    let sandbox = clean_text(&out.report.sandbox);
    let steps = c
        .candidates
        .iter()
        .chain(&c.dropped)
        .filter_map(|x| {
            let command = command_of(x);
            if command.is_empty() {
                return None;
            }
            let (result, signature) = match &x.verified {
                Verified::Works => (Result_::Works, None),
                Verified::Stale(why) => (Result_::Stale, Some(signature_of(why))),
                Verified::NotReplayed(_) => (Result_::NotReplayed, None),
            };
            Some(Step {
                id: x.steps.join("+"),
                // A stale candidate's route is not in the pack.
                route: if matches!(x.verified, Verified::Stale(_)) { String::new() } else { x.route.id.clone() },
                command: clean_text(&command),
                result,
                signature,
                date: date.to_string(),
                sandbox: sandbox.clone(),
            })
        })
        .take(adage_pack::verified::MAX_STEPS)
        .collect();
    Record {
        schema: SCHEMA.into(),
        // The name as the harness lists it is short; a longer one is cut to what the record holds.
        skill: Skill {
            name: clean_text(&c.graph.skill.name).chars().take(64).collect(),
            sha256: c.graph.skill.sha256.clone(),
            inert: out.inert,
            path: out.skill_file.to_string_lossy().into_owned(),
        },
        repo: out.repo_key.clone(),
        pack: out.pack_name.clone(),
        digest: out.digest.clone().unwrap_or_default(),
        steps,
        accepted_at: None,
    }
}

/// One line for a route: what triggers it and what it says.
pub fn describe(r: &RouteV2) -> String {
    let run = |i: usize| r.steps.get(i).and_then(|s| s.run.as_ref()).map(|x| x.argv.join(" ")).unwrap_or_default();
    let when = match &r.trigger {
        TriggerV2::Prompt { .. } => "on a work item id in the prompt".to_string(),
        TriggerV2::AfterCommand { command } => format!("after `{}`", command.trim_start_matches("exec ")),
        TriggerV2::AfterFailure { command, .. } => format!("after `{}` fails", command.trim_start_matches("exec ")),
        TriggerV2::AfterEdit { paths } if paths == "*" && r.shape == Shape::Obligation => {
            "after any edit, before finishing".to_string()
        }
        TriggerV2::AfterEdit { paths } if paths == "*" => "after any edit".to_string(),
        TriggerV2::AfterEdit { paths } => format!("after editing {paths}"),
        TriggerV2::SessionStart => "from the session's start".to_string(),
        TriggerV2::Intent { family, .. } => format!("on prompts like the {family} family"),
    };
    let what = match r.shape {
        Shape::Run | Shape::After | Shape::Obligation => format!("run `{}`", run(0)),
        Shape::Sequence => {
            let cmds: Vec<String> = (0..r.steps.len()).map(|i| format!("`{}`", run(i))).collect();
            format!("run {}", cmds.join(", then "))
        }
        Shape::Read => "read the files it lists, together".to_string(),
        Shape::Avoid => {
            let a = r.steps[0].avoid.as_ref();
            let runners: Vec<String> =
                a.map(|a| a.runners.iter().map(|x| format!("`{}`", x.join(" "))).collect()).unwrap_or_default();
            format!(
                "not {} (fails with `{}`): run `{}`",
                runners.join(" or "),
                a.map(|a| a.signature.as_str()).unwrap_or(""),
                a.map(|a| a.instead.join(" ")).unwrap_or_default()
            )
        }
        Shape::Advise => {
            let t = r.steps[0].advise.as_ref().map(|a| a.template.as_str()).unwrap_or("");
            match t {
                "uncommitted" => "advise: leave changes uncommitted".into(),
                other => format!("advise: {other}"),
            }
        }
    };
    format!("{when}: {what}")
}

fn strength(s: adage_pack::Strength) -> &'static str {
    match s {
        adage_pack::Strength::Fire => "fire",
        adage_pack::Strength::Prime => "prime",
        adage_pack::Strength::Inhibit => "inhibit",
    }
}

/// The review: every step, compiled (to what, verified how) or not (and why).
pub fn render(o: &Outcome) -> String {
    let c = &o.compiled;
    let g = &c.graph;
    let mut s = String::new();
    let line = |s: &mut String, t: String| {
        s.push_str(&t);
        s.push('\n');
    };
    line(
        &mut s,
        format!(
            "adage train: {} ({}, {} bytes, about {} tokens in context on every call)",
            g.skill.name, g.skill.file, g.skill.bytes, g.skill.approx_tokens
        ),
    );
    line(&mut s, format!("repository  {}  ({})", o.repo_key, o.root.display()));
    line(&mut s, format!("replay      {}", o.report.sandbox));
    if !o.report.lent.is_empty() {
        line(&mut s, format!("            lent read-only to each checkout: {}", o.report.lent.join(", ")));
    }
    if !o.report.unlent.is_empty() {
        line(
            &mut s,
            format!(
                "            not lent: {} (the sandbox runs as nobody, who cannot read it: an ancestor directory is private)",
                o.report.unlent.join(", ")
            ),
        );
    }
    line(&mut s, String::new());
    for st in &g.steps {
        line(&mut s, format!("{:<3} {:<10} {}", st.id, st.role.as_str(), st.title));
        let routes: Vec<&crate::compile::Candidate> =
            c.candidates.iter().filter(|x| x.steps.contains(&st.id)).collect();
        for r in &routes {
            let v = match &r.verified {
                Verified::Works => "verified".to_string(),
                Verified::NotReplayed(w) => format!("not replayed: {w}"),
                Verified::Stale(w) => format!("stale: {w}"),
            };
            line(&mut s, format!("    compiled  {:<8} {}", strength(r.route.strength), describe(&r.route)));
            line(&mut s, format!("              {}  [{}]", v, r.route.id));
            for l in &r.replay {
                line(&mut s, format!("              replay: {l}"));
            }
        }
        for w in c.stale.get(&st.id).into_iter().flatten() {
            line(&mut s, format!("    STALE     {w}: dropped (your skill is stale here)"));
        }
        for w in c.text.get(&st.id).into_iter().flatten() {
            line(&mut s, format!("    text      {w}"));
        }
    }
    if !c.advice.is_empty() {
        line(&mut s, "advice".into());
        for a in &c.advice {
            let t: String = a.text.chars().take(100).collect();
            let more = if a.text.chars().count() > 100 { "..." } else { "" };
            match &a.route {
                Some(id) => {
                    let r = c.candidates.iter().find(|x| &x.route.id == id);
                    let d = r.map(|r| describe(&r.route)).unwrap_or_default();
                    line(&mut s, format!("    compiled  prime    {d}  [{id}]"));
                    line(&mut s, format!("              from: {t}{more}"));
                }
                None => line(&mut s, format!("    text      {t}{more}")),
            }
        }
    }
    line(&mut s, String::new());
    let steps = g.steps.len();
    let compiled = g.steps.iter().filter(|x| !c.routes_of(&x.id).is_empty()).count();
    let stale = g.steps.iter().filter(|x| c.routes_of(&x.id).is_empty() && c.stale.contains_key(&x.id)).count();
    let verified = c.candidates.iter().filter(|x| x.verified == Verified::Works).count();
    line(
        &mut s,
        format!(
            "{compiled} of {steps} steps compiled into {} reflexes ({verified} verified by replay); {} left as text; {stale} stale.",
            c.candidates.len(),
            steps - compiled - stale,
        ),
    );
    match (&o.pack, &o.digest) {
        (Some(p), Some(d)) => {
            line(
                &mut s,
                format!("pack        {} {} (sha256 {})", p.manifest.pack.name, p.manifest.pack.version, &d[..16]),
            );
            line(&mut s, format!("            {}", o.dir.join("pack").display()));
            line(&mut s, String::new());
            line(&mut s, "Nothing is installed yet. Read the review above; to install the pack as reviewed:".into());
            line(&mut s, format!("  adage train accept {}", c.prefix));
            line(
                &mut s,
                "Verified reflexes install at the strength shown; the rest at prime. Keep the skill's text only for"
                    .into(),
            );
            line(&mut s, "what stayed text.".into());
        }
        _ => line(&mut s, "Nothing compiled, so there is no pack: the skill stays text.".into()),
    }
    s
}

/// Read a staged review (`$ADAGE_HOME/train/skills/<name>/review.json`).
pub fn load_review(dir: &Path) -> Result<Review> {
    let p = dir.join(REVIEW_FILE);
    let r: Review = serde_json::from_slice(&std::fs::read(&p).with_context(|| format!("reading {}", p.display()))?)
        .with_context(|| format!("{} is not a review", p.display()))?;
    if r.schema != REVIEW_SCHEMA {
        bail!("{}: schema {:?} is not {REVIEW_SCHEMA:?}", p.display(), r.schema);
    }
    Ok(r)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn owners_are_the_repository_name() {
        assert_eq!(owner("github.com/pallets/click"), "click");
        assert_eq!(owner("acme-billing@1a2b3c4d5e6f"), "acme-billing");
        assert_eq!(owner("Weird Name@x"), "weird-name");
    }
}
