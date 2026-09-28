//! `adage eval types`: learned value types (Track G §4.4) against today's hard-coded ticket
//! detector, on generated sandboxes or on a dataset.
//!
//! ```text
//! adage eval types <gen-dir | dataset-dir> [--json]
//! ```
//!
//! - **Generated sandboxes** (`evals/gen`, a bundle or a suite): the sessions are the rehearsal's
//!   scripted transcripts (`<bundle>/rehearsal/transcripts/*/*.jsonl`) and the held-out set is
//!   `<bundle>/rehearsal/held_out.json` (200 fresh ids from the generator's id distribution and 200
//!   near misses: a wrong prefix, one character too few or too many, a leading `-`, shell
//!   metacharacters). Both come from `python3 evals/gen/rehearse.py <dir> --no-learn`.
//! - **A dataset** (`evals/datasets/*`): every session in the manifest; the held-out set is the lookup
//!   convention's `samples` (to accept) and `counter_samples` (to reject).
//!
//! For each, value types are learned from every session (`adage_graph::learn`), and the lookup
//! convention's command names the sink (`./scripts/wi-lookup {item}` → `scripts/wi-lookup #0`).
//! Reported per bundle and per id shape, for the learned type and for the hard-coded detector
//! (`adage_core::spans` TICKET spans in the prompt that reappear in the lookup's argv, then
//! `spans::ticket_contract`, as the learner does today):
//!
//! - **slot found**: a slot at the lookup's sink with a safe type (learned), or a contract
//!   (hard-coded). Recall over bundles is the share with one;
//! - **accept**: the share of fresh ids the type accepts (0 without a type);
//! - **reject**: the share of near misses it rejects (1 without a type: nothing is accepted);
//! - **unsafe**: acceptances, by *any* learned type in the bundle, of the adversarial set (the
//!   leading-`-` and shell near misses, and fixed hostile strings);
//! - **agreement**: the learned regex (compiled here, never at runtime) and the O(len) matcher
//!   agree on every held-out string.
//!
//! Sessions come through adage-learn's importer (`adage_learn::read_session`) and its bridge to
//! the graph engine (`adage_learn::bridge`), with every prompt anchored where it was typed.

use adage_core::spans::{self, SpanKind};
use adage_core::types::CanonicalAction;
use adage_core::{pattern::Shape, vtype, CommandTable};
use adage_graph::flow::At;
use adage_graph::input;
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// Convert an imported session into the graph engine's input: `adage_learn::bridge::to_input`, every
/// prompt anchored where it was typed (G0).
pub fn to_input(s: &adage_learn::import::Session) -> input::Session {
    adage_learn::bridge::to_input(s)
}

/// One arm (learned or hard-coded) on one bundle.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Arm {
    pub slot_found: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub regex: Option<String>,
    /// Why there is no type, when a slot was found without one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub no_type: Option<String>,
    pub accepted: u32,
    pub fresh: u32,
    pub rejected: u32,
    pub near_misses: u32,
    /// Near misses accepted, by kind.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub accepted_near_misses: BTreeMap<String, u32>,
}

impl Arm {
    pub fn accept(&self) -> f64 {
        ratio(self.accepted, self.fresh)
    }
    pub fn reject(&self) -> f64 {
        ratio(self.rejected, self.near_misses)
    }
}

fn ratio(a: u32, b: u32) -> f64 {
    if b == 0 {
        0.0
    } else {
        a as f64 / b as f64
    }
}

/// A learned slot, summarised.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlotSummary {
    pub sink: String,
    pub contract: String,
    pub distinct: u32,
    pub flow_rate: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BundleReport {
    pub name: String,
    pub id_shape: String,
    pub sessions: usize,
    pub lookup_sink: String,
    pub learned: Arm,
    pub hardcoded: Arm,
    /// Every slot learned in the bundle.
    pub slots: Vec<SlotSummary>,
    /// (type regex, adversarial value) pairs a learned type accepted. Must be empty.
    pub unsafe_accepts: Vec<(String, String)>,
    /// Held-out strings on which a learned regex and the matcher disagree. Must be empty.
    pub disagreements: Vec<(String, String)>,
    /// Learning, then the hook, when asked (`--end-to-end`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_to_end: Option<crate::value_types_e2e::EndToEnd>,
}

/// Totals for one id shape.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ShapeSummary {
    pub bundles: u32,
    pub learned_recall: f64,
    pub learned_accept: f64,
    pub learned_reject: f64,
    pub hardcoded_recall: f64,
    pub hardcoded_accept: f64,
    pub hardcoded_reject: f64,
    pub unsafe_accepts: u32,
    /// End to end (`--end-to-end`): the mean recall over bundles, and unsafe binds in all.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub e2e_recall: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub e2e_unsafe: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Report {
    pub bundles: Vec<BundleReport>,
    pub by_shape: BTreeMap<String, ShapeSummary>,
    /// Totals over every id shape except `prefix_dash` (and datasets).
    pub new_shapes: ShapeSummary,
    pub all: ShapeSummary,
}

#[derive(Debug, Clone, Deserialize)]
struct NearMiss {
    value: String,
    kind: String,
}

#[derive(Debug, Clone, Deserialize)]
struct HeldOut {
    id_shape: String,
    fresh: Vec<String>,
    near_miss: Vec<NearMiss>,
}

/// Fixed hostile strings every learned type must reject.
pub const HOSTILE: &[&str] = &[
    "-rf",
    "--exec=sh",
    "-o/tmp/x",
    "$(id)",
    "`id`",
    "a;id",
    "a|sh",
    "a&&id",
    "a b",
    "a\nb",
    "a>b",
    "../../etc/passwd;",
    "BIL-1041;whoami",
    "W-2041 --config=/x",
];

/// The lookup's sink, from a convention command such as `./scripts/wi-lookup {item}`:
/// (identity, positional index of the placeholder).
pub fn lookup_sink(command: &str) -> Option<(String, u8)> {
    let toks: Vec<String> = command.split_whitespace().map(str::to_string).collect();
    let shape = Shape::of_tokens(&toks)?;
    let i = shape.positionals.iter().position(|t| adage_core::pattern::is_slot(t))?;
    Some((shape.identity.join(" "), i as u8))
}

fn identity_of(argv: &[String]) -> Option<String> {
    Shape::of_tokens(argv).map(|s| s.identity.join(" "))
}

/// Today's detector: TICKET spans of the prompt found in the lookup's argv, then
/// `ticket_contract` over those values.
fn hardcoded_contract(sessions: &[adage_learn::import::Session], identity: &str) -> Option<String> {
    let mut values = Vec::new();
    for s in sessions {
        let tickets: Vec<String> =
            spans::extract(&s.prompt).into_iter().filter(|x| x.kind == SpanKind::Ticket).map(|x| x.value).collect();
        if tickets.is_empty() {
            continue;
        }
        for st in &s.steps {
            let CanonicalAction::Exec { argv, .. } = &st.norm.action else { continue };
            if identity_of(argv).as_deref() != Some(identity) {
                continue;
            }
            if let Some(t) = tickets.iter().find(|t| argv.contains(t)) {
                values.push(t.clone());
            }
        }
    }
    if values.is_empty() {
        return None;
    }
    spans::ticket_contract(&values)
}

fn score_arm(arm: &mut Arm, accepts: &dyn Fn(&str) -> bool, fresh: &[String], near: &[(String, String)]) {
    arm.fresh = fresh.len() as u32;
    arm.near_misses = near.len() as u32;
    arm.accepted = fresh.iter().filter(|v| accepts(v)).count() as u32;
    arm.rejected = 0;
    for (v, kind) in near {
        if accepts(v) {
            *arm.accepted_near_misses.entry(kind.clone()).or_default() += 1;
        } else {
            arm.rejected += 1;
        }
    }
}

/// Evaluate one set of sessions against one held-out set.
fn evaluate(
    name: &str,
    id_shape: &str,
    sessions: &[adage_learn::import::Session],
    lookup_command: &str,
    fresh: &[String],
    near: &[(String, String)],
    e2e_repo: Option<&str>,
) -> Result<BundleReport> {
    let (identity, pos) =
        lookup_sink(lookup_command).with_context(|| format!("{name}: no placeholder in {lookup_command:?}"))?;
    let inputs: Vec<input::Session> = sessions.iter().map(to_input).collect();
    let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1).min(8);
    let learned = adage_graph::learn(&inputs, &adage_graph::Params { threads, ..Default::default() });

    let mut arm = Arm::default();
    let slot = learned.slots.iter().find(|s| s.slot.sink.identity == identity && s.slot.sink.at == At::Pos(pos));
    let vt = slot.and_then(|s| s.vtype());
    if let Some(s) = slot {
        arm.slot_found = vt.is_some();
        arm.regex = vt.map(|t| t.regex.clone());
        if let adage_graph::Contract::None(r) = &s.contract {
            arm.no_type = Some(format!("{r:?}"));
        }
    }
    match vt {
        Some(t) => score_arm(&mut arm, &|v| vtype::matches(&t.runs, v), fresh, near),
        None => score_arm(&mut arm, &|_| false, fresh, near),
    }

    let mut hard = Arm::default();
    let contract = hardcoded_contract(sessions, &identity);
    let re = contract.as_deref().map(regex::Regex::new).transpose()?;
    hard.slot_found = re.is_some();
    hard.regex = contract;
    match &re {
        Some(re) => score_arm(&mut hard, &|v| re.is_match(v), fresh, near),
        None => score_arm(&mut hard, &|_| false, fresh, near),
    }

    // Safety and agreement over every learned type.
    let adversarial: Vec<String> = HOSTILE
        .iter()
        .map(|s| s.to_string())
        .chain(near.iter().filter(|(_, k)| k == "leading_dash" || k == "shell").map(|(v, _)| v.clone()))
        .chain([format!("{}1", "1".repeat(200)), "BÏL-1041".to_string(), "١٢٣".to_string()])
        .collect();
    let mut unsafe_accepts = Vec::new();
    let mut disagreements = Vec::new();
    let mut slots = Vec::new();
    for s in &learned.slots {
        let contract = match &s.contract {
            adage_graph::Contract::Pattern(i) => i.vtype.regex.clone(),
            adage_graph::Contract::RepoPath => "repo_path".into(),
            adage_graph::Contract::None(r) => format!("none: {r:?}"),
        };
        slots.push(SlotSummary {
            sink: s.slot.sink.name(),
            contract,
            distinct: s.slot.distinct,
            flow_rate: s.slot.flow_rate,
        });
        let Some(t) = s.vtype() else { continue };
        for a in &adversarial {
            if vtype::matches(&t.runs, a) {
                unsafe_accepts.push((t.regex.clone(), a.clone()));
            }
        }
        let re = regex::Regex::new(&t.regex).with_context(|| t.regex.clone())?;
        for v in fresh.iter().chain(near.iter().map(|(v, _)| v)).chain(adversarial.iter()) {
            if re.is_match(v) != vtype::matches(&t.runs, v) {
                disagreements.push((t.regex.clone(), v.clone()));
            }
        }
    }
    let end_to_end = match e2e_repo {
        Some(repo) => Some(crate::value_types_e2e::run(repo, sessions, &(identity.clone(), pos), fresh, &adversarial)?),
        None => None,
    };
    Ok(BundleReport {
        name: name.to_string(),
        id_shape: id_shape.to_string(),
        sessions: sessions.len(),
        lookup_sink: format!("{identity} #{pos}"),
        learned: arm,
        hardcoded: hard,
        slots,
        unsafe_accepts,
        disagreements,
        end_to_end,
    })
}

fn read_sessions(files: &[PathBuf], repo: &str) -> Vec<adage_learn::import::Session> {
    let table = CommandTable::load();
    let mut out: Vec<_> = files.iter().filter_map(|f| adage_learn::read_session(f, &table, Some(repo))).collect();
    out.sort_by(|a, b| a.id.cmp(&b.id));
    out
}

fn gen_bundle(dir: &Path, e2e: bool) -> Result<BundleReport> {
    let name = dir.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let conv =
        crate::conventions::Conventions::from_bundle(dir).with_context(|| format!("{name}: conventions.json"))?;
    let look = conv
        .conventions
        .iter()
        .find(|c| c.kind == crate::conventions::Kind::Lookup)
        .with_context(|| format!("{name}: no lookup convention"))?;
    let command = look.command.clone();
    let tasks: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(dir.join("tasks.json"))?)?;
    let repo = tasks["repo"].as_str().unwrap_or_default().to_string();
    let reh = dir.join("rehearsal");
    let hp = reh.join("held_out.json");
    if !hp.is_file() {
        bail!(
            "{name}: no rehearsal ({}): run `python3 evals/gen/rehearse.py {} --no-learn` first",
            hp.display(),
            dir.display()
        );
    }
    let ho: HeldOut = serde_json::from_str(&std::fs::read_to_string(&hp)?).with_context(|| hp.display().to_string())?;
    let mut files = Vec::new();
    for p in std::fs::read_dir(reh.join("transcripts"))?.flatten() {
        if p.path().is_dir() {
            for f in std::fs::read_dir(p.path())?.flatten() {
                if f.path().extension().is_some_and(|e| e == "jsonl") {
                    files.push(f.path());
                }
            }
        }
    }
    files.sort();
    let sessions = read_sessions(&files, &repo);
    let near: Vec<(String, String)> = ho.near_miss.into_iter().map(|n| (n.value, n.kind)).collect();
    evaluate(&name, &ho.id_shape, &sessions, &command, &ho.fresh, &near, e2e.then_some(repo.as_str()))
}

fn dataset(dir: &Path, e2e: bool) -> Result<BundleReport> {
    let ds = crate::dataset::Dataset::load(dir)?;
    let cf = ds.conventions_file()?.with_context(|| format!("{}: no conventions.json", dir.display()))?;
    let conv = crate::conventions::Conventions::load(&cf)?;
    let look = conv
        .conventions
        .iter()
        .find(|c| c.kind == crate::conventions::Kind::Lookup)
        .with_context(|| format!("{}: no lookup convention", dir.display()))?;
    let files: Vec<PathBuf> = ds.manifest.sessions.iter().map(|s| ds.path(s)).collect();
    let sessions = read_sessions(&files, &ds.manifest.repo);
    let near: Vec<(String, String)> =
        look.counter_samples.iter().map(|v| (v.clone(), "counter_sample".into())).collect();
    let repo = ds.manifest.repo.clone();
    evaluate(&ds.manifest.name, "dataset", &sessions, &look.command, &look.samples, &near, e2e.then_some(repo.as_str()))
}

fn summarise<'a>(rs: impl Iterator<Item = &'a BundleReport>) -> ShapeSummary {
    let rs: Vec<&BundleReport> = rs.collect();
    let n = rs.len().max(1) as f64;
    let mean = |f: &dyn Fn(&BundleReport) -> f64| rs.iter().map(|r| f(r)).sum::<f64>() / n;
    ShapeSummary {
        bundles: rs.len() as u32,
        learned_recall: mean(&|r| f64::from(u8::from(r.learned.slot_found))),
        learned_accept: mean(&|r| r.learned.accept()),
        learned_reject: mean(&|r| r.learned.reject()),
        hardcoded_recall: mean(&|r| f64::from(u8::from(r.hardcoded.slot_found))),
        hardcoded_accept: mean(&|r| r.hardcoded.accept()),
        hardcoded_reject: mean(&|r| r.hardcoded.reject()),
        unsafe_accepts: rs.iter().map(|r| r.unsafe_accepts.len() as u32).sum(),
        e2e_recall: rs
            .iter()
            .any(|r| r.end_to_end.is_some())
            .then(|| mean(&|r| r.end_to_end.as_ref().map(|e| e.recall()).unwrap_or(0.0))),
        e2e_unsafe: rs
            .iter()
            .any(|r| r.end_to_end.is_some())
            .then(|| rs.iter().filter_map(|r| r.end_to_end.as_ref()).map(|e| e.unsafe_binds.len() as u32).sum()),
    }
}

/// Run the eval on a gen bundle, a gen suite, or a dataset.
pub fn run(dir: &Path) -> Result<Report> {
    run_with(dir, false)
}

/// [`run`], and with `e2e` learning and replaying the prompts through the
/// hook's decision (`crate::value_types_e2e`).
pub fn run_with(dir: &Path, e2e: bool) -> Result<Report> {
    let mut bundles = Vec::new();
    if dir.join("manifest.json").is_file() {
        bundles.push(dataset(dir, e2e)?);
    } else if dir.join("spec.json").is_file() {
        bundles.push(gen_bundle(dir, e2e)?);
    } else {
        let mut dirs: Vec<PathBuf> = std::fs::read_dir(dir)
            .with_context(|| dir.display().to_string())?
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.join("spec.json").is_file())
            .collect();
        dirs.sort();
        if dirs.is_empty() {
            bail!("{}: not a dataset, a generated bundle or a suite", dir.display());
        }
        for d in dirs {
            bundles.push(gen_bundle(&d, e2e)?);
        }
    }
    let shapes: BTreeSet<String> = bundles.iter().map(|b| b.id_shape.clone()).collect();
    let by_shape = shapes.iter().map(|s| (s.clone(), summarise(bundles.iter().filter(|b| &b.id_shape == s)))).collect();
    let new_shapes = summarise(bundles.iter().filter(|b| b.id_shape != "prefix_dash" && b.id_shape != "dataset"));
    let all = summarise(bundles.iter());
    Ok(Report { bundles, by_shape, new_shapes, all })
}

pub fn render_text(r: &Report) -> String {
    let mut s = String::new();
    s.push_str(&format!(
        "{:<26} {:<12} {:>4}  {:<44} {:>6} {:>6}   {:<24} {:>6} {:>6}\n",
        "bundle", "id shape", "sess", "learned type", "accept", "reject", "hard-coded contract", "accept", "reject"
    ));
    for b in &r.bundles {
        let lt = b.learned.regex.clone().or_else(|| b.learned.no_type.clone()).unwrap_or_else(|| "(no slot)".into());
        let ht = b.hardcoded.regex.clone().unwrap_or_else(|| "(none)".into());
        s.push_str(&format!(
            "{:<26} {:<12} {:>4}  {:<44} {:>6.3} {:>6.3}   {:<24} {:>6.3} {:>6.3}\n",
            b.name,
            b.id_shape,
            b.sessions,
            lt,
            b.learned.accept(),
            b.learned.reject(),
            ht,
            b.hardcoded.accept(),
            b.hardcoded.reject()
        ));
        for (t, v) in &b.unsafe_accepts {
            s.push_str(&format!("    UNSAFE: {t} accepts {v:?}\n"));
        }
        for (t, v) in &b.disagreements {
            s.push_str(&format!("    DISAGREE: {t} on {v:?}\n"));
        }
        for (k, n) in &b.learned.accepted_near_misses {
            s.push_str(&format!("    learned type accepts {n} near misses of kind {k}\n"));
        }
        if let Some(e) = &b.end_to_end {
            s.push_str(&format!(
                "    end to end (engine graph): {} · sessions {}/{} · fresh {}/{} · controls fired {}/{} · unsafe {}\n",
                e.route.as_deref().unwrap_or("no lookup route"),
                e.sessions_bound,
                e.sessions,
                e.fresh_bound,
                e.fresh,
                e.controls_fired,
                e.controls,
                e.unsafe_binds.len()
            ));
            for (p, v) in &e.unsafe_binds {
                s.push_str(&format!("    UNSAFE BIND: {p:?} -> {v:?}\n"));
            }
        }
    }
    s.push_str("\nper id shape (mean over bundles): recall = lookup slot found with a safe type\n");
    s.push_str(&format!(
        "{:<14} {:>3}  {:>7} {:>7} {:>7}   {:>7} {:>7} {:>7}  {:>6}\n",
        "id shape", "n", "L.rec", "L.acc", "L.rej", "H.rec", "H.acc", "H.rej", "unsafe"
    ));
    let row = |name: &str, x: &ShapeSummary| {
        format!(
            "{:<14} {:>3}  {:>7.3} {:>7.3} {:>7.3}   {:>7.3} {:>7.3} {:>7.3}  {:>6}\n",
            name,
            x.bundles,
            x.learned_recall,
            x.learned_accept,
            x.learned_reject,
            x.hardcoded_recall,
            x.hardcoded_accept,
            x.hardcoded_reject,
            x.unsafe_accepts
        )
    };
    for (k, v) in &r.by_shape {
        s.push_str(&row(k, v));
    }
    if r.new_shapes.bundles > 0 {
        s.push_str(&row("non-X-123", &r.new_shapes));
    }
    s.push_str(&row("all", &r.all));
    if let (Some(rec), Some(bad)) = (r.all.e2e_recall, r.all.e2e_unsafe) {
        s.push_str(&format!("end to end (learn with engine graph, then the hook): recall {rec:.3}, unsafe {bad}\n"));
    }
    s.push_str(
        "targets (Track G §4.4): accept >= 0.99, reject = 1.00, recall on non-X-123 shapes >= 0.95, unsafe = 0\n",
    );
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookup_sinks() {
        assert_eq!(lookup_sink("./scripts/wi-lookup {item}"), Some(("scripts/wi-lookup".into(), 0)));
        assert_eq!(lookup_sink("python3 scripts/wi-lookup {id}"), Some(("scripts/wi-lookup".into(), 0)));
        assert_eq!(lookup_sink("gh issue view {id} --json"), Some(("gh issue view".into(), 0)));
        assert_eq!(lookup_sink("make check"), None);
    }

    #[test]
    fn datasets_evaluate() {
        // The demo dataset: `W-1041`-style ids, which today's detector already handles.
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../evals/datasets/demo-haiku45");
        if !dir.join("manifest.json").is_file() {
            return;
        }
        let r = run(&dir).unwrap();
        let b = &r.bundles[0];
        assert!(b.unsafe_accepts.is_empty(), "{:?}", b.unsafe_accepts);
        assert!(b.disagreements.is_empty(), "{:?}", b.disagreements);
        assert_eq!(b.learned.reject(), 1.0, "{b:?}");
    }
}
