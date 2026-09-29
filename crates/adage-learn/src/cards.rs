//! Procedure cards for recurring work (evals/lab/research/recurring-workflows): a repository's
//! traces grouped into **families** of similar tasks, and for each family with a passing trace a
//! **card** of what its traces had in common, delivered once at the prompt of the next task of
//! the family (an `intent` trigger, [`adage_tracks::Step::Card`]).
//!
//! - **Families.** Two traces are of one family when their first prompts are similar (cosine of
//!   the pinned static model's embeddings, the one the hook embeds with, at least
//!   [`CardParams::family_similarity`]) *and* they edited overlapping files (Jaccard at least
//!   [`CardParams::file_overlap`]). Families are the connected components, in canonical order.
//! - **Cards** (a port of the research's `procedure.py`): every file edited in at least half of the
//!   family's traces (all of them with one trace), in the median order of first edit, a file new in
//!   each trace under a different name anti-unified to its directory; for each, the lines the most
//!   recent passing trace added (clipped); the commands run in at least half of the traces
//!   (normalised to their head), kept when they mostly worked and listed to avoid when they mostly
//!   failed, with the first line of the error; and the check the passing traces finished with.
//! - **A family with no passing trace makes no card**: most of a card's value is there after one
//!   passing trace, and it does not grow much after that (the research's learning curve).
//! - **Triggering.** The family's centroid, and a similarity threshold that is a parameter
//!   ([`CardParams::threshold`], `learn.card_threshold`) until the confirming experiment calibrates
//!   it. A card is prime, observe, and said once per session.
//! - **Verification.** `adage train cards` (the adage-train binary: `adage` spawns nothing) replays
//!   a card's commands on a clean checkout in adage-exec's sandbox and records the verdicts
//!   ([`VERDICTS_FILE`]); the next learn marks those that worked and drops those that did not from
//!   the commands that worked.
//!
//! Nothing here names a repository's files, commands or errors.

use crate::import::Session;
use adage_intent::model::StaticModel;
use adage_intent::normalize::normalize;
use adage_tracks::card::{Card, CardCommand, CardFile, Exemplar, MAX_EXEMPLAR_LINES, MAX_LINE};
use adage_tracks::why::Why;
use adage_tracks::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

/// Under `$ADAGE_HOME`: card id → command → whether its replay on a clean checkout worked.
pub const VERDICTS_FILE: &str = "train/cards.json";

pub type Verdicts = BTreeMap<String, BTreeMap<String, bool>>;

/// Read the verdicts `adage train cards` recorded (none when absent or unreadable).
pub fn load_verdicts(home: &Path) -> Verdicts {
    std::fs::read(home.join(VERDICTS_FILE)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

/// How families form and when a card is delivered.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CardParams {
    /// A new prompt's similarity to the family's centroid that delivers the card.
    pub threshold: f32,
    /// Two traces' first prompts must be at least this similar to be of one family.
    pub family_similarity: f32,
    /// ... and their edited files must overlap at least this much (Jaccard).
    pub file_overlap: f64,
    #[serde(default)]
    pub verdicts: Verdicts,
}

impl Default for CardParams {
    fn default() -> Self {
        CardParams { threshold: 0.8, family_similarity: 0.6, file_overlap: 0.25, verdicts: Verdicts::new() }
    }
}

const EDIT_TOOLS: [&str; 4] = ["Edit", "Write", "MultiEdit", "NotebookEdit"];

struct EditView {
    k: usize,
    path: String,
    write: bool,
    text: String,
}

struct CmdView {
    k: usize,
    norm: String,
    failed: bool,
    out: String,
}

struct View<'a> {
    s: &'a Session,
    edits: Vec<EditView>,
    cmds: Vec<CmdView>,
    passed: bool,
}

/// The lines an edit added (all of a written file's): what to imitate, without the context.
fn added(old: &str, new: &str) -> String {
    if old.is_empty() {
        return new.to_string();
    }
    let olds: BTreeSet<&str> = old.lines().collect();
    new.lines()
        .filter(|l| l.trim().is_empty() || !olds.contains(l))
        .collect::<Vec<_>>()
        .join("\n")
        .trim_matches('\n')
        .to_string()
}

/// A shell command's stable head: the non-`cd` segments of its first line, pipes and redirections
/// dropped, at most six tokens, a test id cut to its file.
pub fn norm_cmd(cmd: &str) -> String {
    let first = cmd.trim().lines().next().unwrap_or("");
    let first = first.split("<<").next().unwrap_or("");
    let segs: Vec<&str> =
        first.split("&&").flat_map(|s| s.split(';')).map(str::trim).filter(|s| !s.is_empty()).collect();
    let kept: Vec<&str> = segs.iter().copied().filter(|s| !s.starts_with("cd ")).collect();
    let segs = if kept.is_empty() { segs } else { kept };
    let joined = segs.iter().map(|s| s.split('|').next().unwrap_or("").trim()).collect::<Vec<_>>().join(" && ");
    let mut out: Vec<String> = Vec::new();
    for t in joined.split_whitespace() {
        if out.len() >= 6 {
            break;
        }
        // Redirections (`2>&1`, `>/dev/null`, `> out.txt`) are not the command.
        if t.contains('>') || t.contains('<') {
            break;
        }
        let t = t.trim_matches(|c| c == '"' || c == '\'');
        let t = t.split("::").next().unwrap_or(t);
        if !t.is_empty() {
            out.push(t.to_string());
        }
    }
    out.join(" ")
}

fn view<'a>(s: &'a Session) -> View<'a> {
    let root = s.root.to_string_lossy().to_string();
    let mut edits = Vec::new();
    let mut cmds = Vec::new();
    for (k, st) in s.steps.iter().enumerate() {
        let c = &st.call;
        if EDIT_TOOLS.contains(&c.tool.as_str()) {
            let Some(path) = st.rel_path.clone() else { continue };
            if c.failed {
                continue;
            }
            let i = &c.input;
            let s_ = |v: &serde_json::Value, k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string();
            let text = if c.tool == "MultiEdit" {
                i.get("edits")
                    .and_then(|e| e.as_array())
                    .map(|es| {
                        es.iter()
                            .map(|e| added(&s_(e, "old_string"), &s_(e, "new_string")))
                            .collect::<Vec<_>>()
                            .join("\n")
                    })
                    .unwrap_or_default()
            } else {
                let new = if c.tool == "Write" { s_(i, "content") } else { s_(i, "new_string") };
                added(&s_(i, "old_string"), &new)
            };
            edits.push(EditView { k, path, write: c.tool == "Write", text });
        } else if c.tool == "Bash" {
            let Some(cmd) = c.input.get("command").and_then(|x| x.as_str()) else { continue };
            let cmd = if root.is_empty() {
                cmd.to_string()
            } else {
                cmd.replace(&format!("{root}/"), "").replace(&root, ".")
            };
            cmds.push(CmdView { k, norm: norm_cmd(&cmd), failed: c.failed, out: c.output.clone() });
        }
    }
    let passed = s.outcome.or_else(|| s.proxy_outcome()) == Some(true);
    View { s, edits, cmds, passed }
}

fn files_of(v: &View) -> BTreeSet<String> {
    v.edits.iter().map(|e| e.path.clone()).collect()
}

fn jaccard(a: &BTreeSet<String>, b: &BTreeSet<String>) -> f64 {
    let u = a.union(b).count();
    if u == 0 {
        0.0
    } else {
        a.intersection(b).count() as f64 / u as f64
    }
}

fn median(v: &mut [f64]) -> f64 {
    v.sort_by(|a, b| a.total_cmp(b));
    if v.is_empty() {
        0.0
    } else {
        v[v.len() / 2]
    }
}

/// Text safe for a card's error line: printable, no quotes or shell syntax, bounded.
fn error_line(out: &str) -> String {
    let l = out.lines().map(str::trim).find(|l| !l.is_empty() && !l.starts_with("Exit code")).unwrap_or("");
    let l: String = l.chars().filter(|c| !"`<>\"\\$;|&{}".contains(*c) && !c.is_control()).take(150).collect();
    let l = l.trim().to_string();
    if adage_tracks::io::safe_text(&l) {
        l
    } else {
        String::new()
    }
}

/// A command a card may name: plain, no absolute path, no placeholder.
fn plain(c: &str) -> bool {
    adage_tracks::io::safe_command(c) && !c.contains('{')
}

/// Learn one family's card (views in chronological order). `None` without a passing trace or a
/// file edited by enough of them.
fn learn(views: &[&View], verdicts: Option<&BTreeMap<String, bool>>) -> Option<Card> {
    let n = views.len();
    let passed = views.iter().filter(|v| v.passed).count();
    if passed == 0 {
        return None;
    }
    let need = if n == 1 { 1 } else { n.div_ceil(2) };
    // 1. Files, in the median order of their first edit; new files named differently each time
    //    are anti-unified to their directory.
    let mut per_file: BTreeMap<String, BTreeSet<usize>> = BTreeMap::new();
    let mut first_pos: BTreeMap<String, Vec<f64>> = BTreeMap::new();
    let mut new_files: BTreeMap<String, BTreeMap<usize, String>> = BTreeMap::new();
    for (i, v) in views.iter().enumerate() {
        let mut seen: Vec<&str> = Vec::new();
        for e in &v.edits {
            if !seen.contains(&e.path.as_str()) {
                seen.push(&e.path);
            }
        }
        for (j, p) in seen.iter().enumerate() {
            per_file.entry(p.to_string()).or_default().insert(i);
            first_pos.entry(p.to_string()).or_default().push(j as f64 / seen.len().max(1) as f64);
        }
    }
    for (i, v) in views.iter().enumerate() {
        for e in v.edits.iter().filter(|e| e.write) {
            if per_file.get(&e.path).is_some_and(|s| s.len() == 1) {
                let dir = e.path.rsplit_once('/').map(|x| x.0).unwrap_or("").to_string();
                new_files.entry(dir).or_default().insert(i, e.path.clone());
            }
        }
    }
    let mut files: Vec<(String, usize, f64)> = per_file
        .iter()
        .filter(|(_, s)| s.len() >= need)
        .map(|(p, s)| (p.clone(), s.len(), median(&mut first_pos[p].clone())))
        .collect();
    let mut anti: BTreeMap<String, Vec<String>> = BTreeMap::new();
    if n > 1 {
        for (dir, by) in &new_files {
            if by.len() >= need {
                let ps: Vec<String> = by.values().cloned().collect();
                let exts: BTreeSet<String> =
                    ps.iter().map(|p| p.rsplit_once('.').map(|x| format!(".{}", x.1)).unwrap_or_default()).collect();
                let ext = if exts.len() == 1 { exts.into_iter().next().unwrap_or_default() } else { String::new() };
                let pat = if dir.is_empty() { format!("<new>{ext}") } else { format!("{dir}/<new>{ext}") };
                let mut pos: Vec<f64> =
                    ps.iter().filter_map(|p| first_pos.get(p).and_then(|v| v.first().copied())).collect();
                files.push((pat.clone(), by.len(), median(&mut pos)));
                anti.insert(pat, ps);
            }
        }
    }
    files.sort_by(|a, b| a.2.total_cmp(&b.2).then_with(|| a.0.cmp(&b.0)));
    files.truncate(adage_tracks::card::MAX_FILES);
    if files.is_empty() {
        return None;
    }
    // 2. Exemplars, from the most recent passing trace (then the rest, most recent first).
    let order: Vec<&&View> =
        views.iter().rev().filter(|v| v.passed).chain(views.iter().rev().filter(|v| !v.passed)).collect();
    let mut exemplars = Vec::new();
    for (p, _, _) in &files {
        let members = anti.get(p);
        for v in &order {
            let hunks: Vec<&str> = v
                .edits
                .iter()
                .filter(|e| &e.path == p || (e.write && members.is_some_and(|m| m.contains(&e.path))))
                .map(|e| e.text.as_str())
                .collect();
            let Some(h) = hunks.iter().max_by_key(|h| h.lines().count()) else { continue };
            let mut lines: Vec<String> = h
                .trim_matches('\n')
                .lines()
                .map(|l| {
                    let l = l.replace('\t', "    ");
                    let l: String = l.chars().take(MAX_LINE).collect();
                    if adage_tracks::card::line_ok(&l) {
                        l
                    } else {
                        "…".to_string()
                    }
                })
                .collect();
            if lines.len() > MAX_EXEMPLAR_LINES {
                lines.truncate(MAX_EXEMPLAR_LINES - 1);
                lines.push("…".into());
            }
            if !lines.is_empty() && !lines.iter().all(|l| l.trim().is_empty()) {
                exemplars.push(Exemplar { path: p.clone(), lines });
            }
            break;
        }
    }
    // 3. Commands: those run in at least half of the traces; mostly working, or to avoid.
    #[derive(Default)]
    struct Agg {
        sessions: BTreeSet<usize>,
        runs: u32,
        fails: u32,
        err: String,
    }
    let mut per_cmd: BTreeMap<String, Agg> = BTreeMap::new();
    for (i, v) in views.iter().enumerate() {
        for c in &v.cmds {
            if c.norm.is_empty() {
                continue;
            }
            let a = per_cmd.entry(c.norm.clone()).or_default();
            a.sessions.insert(i);
            a.runs += 1;
            if c.failed {
                a.fails += 1;
                if a.err.is_empty() {
                    a.err = error_line(&c.out);
                }
            }
        }
    }
    let verdict = |c: &str| verdicts.and_then(|m| m.get(c).copied());
    let mut commands: Vec<CardCommand> = Vec::new();
    let mut avoid: Vec<CardCommand> = Vec::new();
    for (k, a) in &per_cmd {
        if a.sessions.len() < need || !plain(k) {
            continue;
        }
        let c = CardCommand {
            command: k.clone(),
            sessions: a.sessions.len() as u32,
            runs: a.runs,
            failed: a.fails,
            error: String::new(),
            verified: verdict(k),
        };
        if a.fails * 2 < a.runs {
            commands.push(c);
        } else {
            avoid.push(CardCommand { error: a.err.clone(), verified: None, ..c });
        }
    }
    let by_use = |a: &CardCommand, b: &CardCommand| b.sessions.cmp(&a.sessions).then_with(|| a.command.cmp(&b.command));
    commands.sort_by(by_use);
    avoid.sort_by(by_use);
    commands.truncate(adage_tracks::card::MAX_COMMANDS);
    avoid.truncate(adage_tracks::card::MAX_AVOID);
    // 4. The finishing check: the command passing traces ran successfully after their last edit,
    //    of a head they also ran more than once before it (a verify loop).
    let head2 = |c: &str| c.split_whitespace().take(2).collect::<Vec<_>>().join(" ");
    let mut loops: BTreeSet<String> = BTreeSet::new();
    for v in views {
        let last_edit = v.edits.iter().map(|e| e.k).max().unwrap_or(0);
        let heads: Vec<String> = v.cmds.iter().filter(|c| c.k < last_edit).map(|c| head2(&c.norm)).collect();
        loops.extend(heads.iter().filter(|h| heads.iter().filter(|x| x == h).count() >= 2).cloned());
    }
    let mut finals: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for v in views.iter().filter(|v| v.passed) {
        let last_edit = v.edits.iter().map(|e| e.k).max().unwrap_or(0);
        let mut seen = BTreeSet::new();
        for c in v.cmds.iter().filter(|c| c.k > last_edit && !c.failed && loops.contains(&head2(&c.norm))) {
            if seen.insert(head2(&c.norm)) {
                finals.entry(head2(&c.norm)).or_default().push(c.norm.clone());
            }
        }
    }
    let finish = finals
        .iter()
        .max_by(|a, b| a.1.len().cmp(&b.1.len()).then_with(|| b.0.cmp(a.0)))
        .and_then(|(_, cs)| {
            // The tokens every passing trace agreed on, up to the first that differed.
            let toks: Vec<Vec<&str>> = cs.iter().map(|c| c.split_whitespace().collect()).collect();
            let m = toks.iter().map(|t| t.len()).min().unwrap_or(0);
            let common: Vec<&str> =
                (0..m).take_while(|&i| toks.iter().all(|t| t[i] == toks[0][i])).map(|i| toks[0][i]).collect();
            (common.len() >= 2).then(|| common.join(" "))
        })
        .filter(|f| plain(f) && verdict(f) != Some(false));
    // A command the replay found not to work on a clean checkout is not listed as working.
    let card = Card {
        sessions: n as u32,
        passed: passed as u32,
        files: files.iter().map(|(p, k, _)| CardFile { path: p.clone(), sessions: *k as u32 }).collect(),
        exemplars,
        commands,
        avoid,
        finish,
    };
    let mut card = card;
    // Keep within the size bound: drop exemplars from the end until it fits.
    while adage_tracks::card::check(&card).is_err() && !card.exemplars.is_empty() {
        card.exemplars.pop();
    }
    adage_tracks::card::check(&card).is_ok().then_some(card)
}

/// A card's id: stable across relearns as the family grows (from its earliest trace).
fn card_id(first: &str) -> String {
    let h = Sha256::digest(first.as_bytes());
    format!("card-{}", h.iter().take(5).map(|b| format!("{b:02x}")).collect::<String>())
}

/// Families and their cards for one repository's sessions (see the module documentation).
/// Returns the card routes, each with why, and the number of families found.
pub fn compile(ss: &[&Session], model: &StaticModel, p: &CardParams, provenance: &str) -> (Vec<(Route, Why)>, usize) {
    let mut items: Vec<(View, Vec<f32>)> = ss
        .iter()
        .filter_map(|s| {
            let v = view(s);
            if v.edits.is_empty() {
                return None;
            }
            let e = model.embed(&normalize(&s.prompt))?;
            Some((v, e))
        })
        .collect();
    // Chronological, then by id: the order exemplars and ids are read in.
    items.sort_by(|a, b| (&a.0.s.day, &a.0.s.id).cmp(&(&b.0.s.day, &b.0.s.id)));
    let n = items.len();
    let files: Vec<BTreeSet<String>> = items.iter().map(|(v, _)| files_of(v)).collect();
    let mut parent: Vec<usize> = (0..n).collect();
    fn find(p: &mut Vec<usize>, i: usize) -> usize {
        let mut r = i;
        while p[r] != r {
            r = p[r];
        }
        let mut i = i;
        while p[i] != r {
            let next = p[i];
            p[i] = r;
            i = next;
        }
        r
    }
    for i in 0..n {
        for j in i + 1..n {
            let sim = adage_intent::cosine(&items[i].1, &items[j].1);
            if sim >= p.family_similarity && jaccard(&files[i], &files[j]) >= p.file_overlap {
                let (a, b) = (find(&mut parent, i), find(&mut parent, j));
                if a != b {
                    parent[a.max(b)] = a.min(b);
                }
            }
        }
    }
    let mut groups: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for i in 0..n {
        let r = find(&mut parent, i);
        groups.entry(r).or_default().push(i);
    }
    let threshold = p.threshold.clamp(INTENT_MIN_THRESHOLD, INTENT_MAX_THRESHOLD);
    let mut out = Vec::new();
    for members in groups.values() {
        let views: Vec<&View> = members.iter().map(|&i| &items[i].0).collect();
        let id = card_id(&views[0].s.id);
        let Some(card) = learn(&views, p.verdicts.get(&id)) else { continue };
        let dim = items[members[0]].1.len();
        let mut mean = vec![0f32; dim];
        for &i in members {
            for (m, x) in mean.iter_mut().zip(&items[i].1) {
                *m += x;
            }
        }
        let centroid = adage_intent::quantize(&adage_intent::unit(mean));
        let passed = card.passed;
        let route = Route {
            id: id.clone(),
            summary: "What earlier sessions of this family of tasks had in common".into(),
            trigger: Trigger::Intent { family: id.clone(), model: model.id.clone(), centroid, threshold },
            step: Step::Card { card },
            slots: vec![],
            effect: adage_core::Effect::Observe,
            strength: Strength::Prime,
            state: RouteState::Forming,
            exit: vec![Exit::IntentChanges],
            evidence: Evidence {
                opportunities: members.len() as u32,
                support: passed,
                days: views.iter().map(|v| v.s.day.as_str()).collect::<BTreeSet<_>>().len() as u32,
                rate: passed as f64 / members.len() as f64,
                provenance: provenance.to_string(),
                ..Default::default()
            },
            authored: false,
        };
        let probe = Track {
            manifest: Manifest {
                track: TrackInfo {
                    name: "probe/learned".into(),
                    version: "0.0.0".into(),
                    description: String::new(),
                    provenance: String::new(),
                },
                scope: Scope { repo: String::new() },
                gauge: Gauge::default(),
            },
            routes: vec![route.clone()],
        };
        if io::validate(&probe).is_err() {
            continue;
        }
        let why = Why {
            shape: "card".into(),
            pattern: format!("[prompt ~ {id} (τ {threshold:.2})] → [card of {} trace(s)]", members.len()),
            backoff: 1,
            gain: 0.0,
            lift: 0.0,
            conf: (route.evidence.rate * 1e4).round() / 1e4,
            conf_lb: 0.0,
            support: passed,
            days: route.evidence.days,
            members: vec![],
            examples: views.iter().take(3).map(|v| v.s.id.clone()).collect(),
            prior: None,
        };
        out.push((route, why));
    }
    let families = groups.len();
    (out, families)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commands_are_normalised_to_their_head() {
        assert_eq!(
            norm_cmd("cd /r && python -m pytest tests/test_v.py::test_x -xvs 2>&1 | tail -20"),
            "python -m pytest tests/test_v.py -xvs"
        );
        assert_eq!(norm_cmd("make test > out.txt"), "make test");
        assert_eq!(norm_cmd("cat <<EOF > x\nhello\nEOF"), "cat");
    }

    #[test]
    fn added_lines_are_what_to_imitate() {
        assert_eq!(added("a\nb", "a\nb\nc"), "c");
        assert_eq!(added("", "x\ny"), "x\ny");
    }
}
