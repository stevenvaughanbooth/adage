//! `adage eval embeddings <manifest.json>… --models <dir>`: what the embedder resolved from
//! `<dir>` (`ADAGE_EMBED_MODEL` selects: `hashed`, `bundled`, `potion-base-8m`,
//! `potion-code-16m-v2`) is worth offline, on the replay manifests' held-out sessions
//! (`evals/launch/embeddings.md`). Reads only; spends nothing.
//!
//! - **Prompt → area hit@k** (monorepo routing): the area of an edited source file is its first two
//!   directories (`packages/runtime-core`, `src/click`). For each held-out control session with an
//!   edit, areas are ranked by (a) **kNN** over the training sessions' prompts (similarity² votes
//!   for the areas each neighbour edited), (b) **retrieval** over the checkout (cosine between the
//!   prompt and each area's path and file names), (c) their sum, and (d) the **frequency** prior
//!   (areas by how many training sessions edited them), which needs no embedding.
//! - **Prompt → file hit@k**: the same over files (a file's words: its directories and name).
//! - **Task identity**: every held-out session (all arms), pairs of sessions grouped as one task by
//!   prompt similarity ≥ τ ([`adage_learn::cases::task_groups`]), against the manifest's task ids;
//!   the exact-text rule the learner uses today is the baseline.
//! - **Quantisation**: the bundled int8 model's vectors against the f32 model's (when both exist).
//! - **Latency**: opening the embedder and embedding one prompt, in process.

use crate::savings_replay::Manifest;
use adage_intent::model::{self, Embedder};
use adage_learn::import::Session;
use anyhow::{bail, Context, Result};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::Instant;

const KS: [usize; 3] = [1, 3, 5];
/// Training neighbours that vote in the kNN ranking.
const NEIGHBOURS: usize = 10;
/// Files listed from a checkout, at most.
const MAX_FILES: usize = 20_000;

fn area(p: &str) -> Option<String> {
    let segs: Vec<&str> = p.split('/').collect();
    (segs.len() >= 3).then(|| segs[..2].join("/"))
}

fn edits(s: &Session) -> BTreeSet<String> {
    s.steps.iter().filter_map(adage_learn::outcome::source_edit).map(str::to_string).collect()
}

fn words(p: &str) -> String {
    adage_graph::embed::split_words(p)
}

/// Source files of a checkout, repository-relative, sorted.
fn list_files(root: &Path) -> Vec<String> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        let mut es: Vec<_> = rd.flatten().collect();
        es.sort_by_key(|e| e.file_name());
        for e in es {
            let name = e.file_name().to_string_lossy().to_string();
            if name.starts_with('.')
                || matches!(name.as_str(), "node_modules" | "dist" | "build" | "target" | "__pycache__")
            {
                continue;
            }
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if let Ok(rel) = p.strip_prefix(root) {
                let rel = rel.to_string_lossy().replace('\\', "/");
                if rel.contains('/') && !adage_learn::outcome::is_test_path(&rel) && is_code(&rel) {
                    out.push(rel);
                }
            }
            if out.len() >= MAX_FILES {
                return out;
            }
        }
    }
    out.sort();
    out
}

fn is_code(p: &str) -> bool {
    matches!(p.rsplit('.').next(), Some("py" | "ts" | "tsx" | "js" | "mjs" | "vue" | "go" | "rs"))
}

fn cos(a: &[f32], b: &[f32]) -> f64 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    adage_intent::cosine(a, b) as f64
}

/// Rank keys by score (ties by key), best first.
fn ranked(scores: &BTreeMap<String, f64>) -> Vec<String> {
    let mut v: Vec<(&String, &f64)> = scores.iter().collect();
    v.sort_by(|a, b| b.1.total_cmp(a.1).then(a.0.cmp(b.0)));
    v.into_iter().map(|(k, _)| k.clone()).collect()
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Hits {
    pub n: usize,
    /// hit@1, @3, @5.
    pub hit: [f64; 3],
}

impl Hits {
    fn add(&mut self, rank: &[String], truth: &BTreeSet<String>) {
        self.n += 1;
        for (i, k) in KS.iter().enumerate() {
            if rank.iter().take(*k).any(|r| truth.contains(r)) {
                self.hit[i] += 1.0;
            }
        }
    }
    fn done(mut self) -> Self {
        for h in &mut self.hit {
            *h = if self.n == 0 { 0.0 } else { (*h / self.n as f64 * 1000.0).round() / 1000.0 };
        }
        self
    }
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct PairScore {
    pub precision: f64,
    pub recall: f64,
    pub f1: f64,
}

fn pairs(groups: &[usize], truth: &[String]) -> PairScore {
    let (mut tp, mut fp, mut fneg) = (0.0, 0.0, 0.0);
    for i in 0..groups.len() {
        for j in (i + 1)..groups.len() {
            match (groups[i] == groups[j], truth[i] == truth[j]) {
                (true, true) => tp += 1.0,
                (true, false) => fp += 1.0,
                (false, true) => fneg += 1.0,
                _ => {}
            }
        }
    }
    let p = if tp + fp > 0.0 { tp / (tp + fp) } else { 1.0 };
    let r = if tp + fneg > 0.0 { tp / (tp + fneg) } else { 1.0 };
    let f = if p + r > 0.0 { 2.0 * p * r / (p + r) } else { 0.0 };
    let r3 = |x: f64| (x * 1000.0).round() / 1000.0;
    PairScore { precision: r3(p), recall: r3(r), f1: r3(f) }
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct RepoReport {
    pub name: String,
    pub areas: usize,
    pub files: usize,
    pub area_knn: Hits,
    pub area_retrieval: Hits,
    pub area_hybrid: Hits,
    pub area_frequency: Hits,
    pub file_knn: Hits,
    pub file_retrieval: Hits,
    pub file_hybrid: Hits,
    pub file_frequency: Hits,
    pub task_exact: PairScore,
    /// τ → pairwise scores.
    pub task_embed: BTreeMap<String, PairScore>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Report {
    pub embedder: String,
    pub source: String,
    pub open_ms: f64,
    pub embed_us_p50: f64,
    pub embed_us_p99: f64,
    /// Cosine of the bundled int8 model's prompt vectors with the f32 model's: min and median.
    pub int8_vs_f32: Option<[f64; 2]>,
    pub repos: Vec<RepoReport>,
}

fn sessions(paths: &[&PathBuf], repo: &str) -> Vec<Session> {
    let table = adage_core::CommandTable::load();
    paths.iter().filter_map(|p| adage_learn::import::parse_transcript(p, table, Some(repo))).collect()
}

fn embed(e: &Embedder, text: &str) -> Vec<f32> {
    e.embed(text).unwrap_or_default()
}

fn prompt_vec(e: &Embedder, s: &Session) -> Vec<f32> {
    embed(e, &adage_intent::normalize::normalize(&s.prompt))
}

pub fn repo_report(m: &Manifest, e: &Embedder) -> Result<RepoReport> {
    let train = sessions(&m.train.iter().collect::<Vec<_>>(), &m.repo_key);
    let held_meta: Vec<_> = m.sessions.iter().filter(|s| s.arm == "control").collect();
    let held = sessions(&held_meta.iter().map(|s| &s.transcript).collect::<Vec<_>>(), &m.repo_key);
    let files = list_files(&m.root);
    let areas: BTreeSet<String> = files.iter().filter_map(|f| area(f)).collect();
    // Documents: an area's path and its files' names; a file's path.
    let mut area_docs: BTreeMap<String, String> = BTreeMap::new();
    for f in &files {
        if let Some(a) = area(f) {
            let d = area_docs.entry(a.clone()).or_insert_with(|| words(&a));
            if d.len() < 4000 {
                let name = f.rsplit('/').next().unwrap_or(f);
                d.push(' ');
                d.push_str(&words(name.rsplit_once('.').map(|x| x.0).unwrap_or(name)));
            }
        }
    }
    let area_vecs: BTreeMap<String, Vec<f32>> = area_docs.iter().map(|(a, d)| (a.clone(), embed(e, d))).collect();
    let file_vecs: Vec<(String, Vec<f32>)> = files.iter().map(|f| (f.clone(), embed(e, &words(f)))).collect();
    let train_p: Vec<Vec<f32>> = train.iter().map(|s| prompt_vec(e, s)).collect();
    let train_edits: Vec<BTreeSet<String>> = train.iter().map(edits).collect();
    let mut freq_area: BTreeMap<String, f64> = BTreeMap::new();
    let mut freq_file: BTreeMap<String, f64> = BTreeMap::new();
    for ed in &train_edits {
        for a in ed.iter().filter_map(|f| area(f)).collect::<BTreeSet<_>>() {
            *freq_area.entry(a).or_default() += 1.0;
        }
        for f in ed {
            *freq_file.entry(f.clone()).or_default() += 1.0;
        }
    }
    let mut r = RepoReport { name: m.name.clone(), areas: areas.len(), files: files.len(), ..Default::default() };
    for s in &held {
        let truth_files = edits(s);
        if truth_files.is_empty() {
            continue;
        }
        let truth_areas: BTreeSet<String> = truth_files.iter().filter_map(|f| area(f)).collect();
        let pv = prompt_vec(e, s);
        // kNN over training prompts.
        let mut nb: Vec<(f64, usize)> = train_p.iter().enumerate().map(|(i, t)| (cos(&pv, t), i)).collect();
        nb.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
        let (mut knn_a, mut knn_f): (BTreeMap<String, f64>, BTreeMap<String, f64>) = Default::default();
        for &(sim, i) in nb.iter().take(NEIGHBOURS) {
            let w = sim.max(0.0).powi(2);
            for a in train_edits[i].iter().filter_map(|f| area(f)).collect::<BTreeSet<_>>() {
                *knn_a.entry(a).or_default() += w;
            }
            for f in &train_edits[i] {
                *knn_f.entry(f.clone()).or_default() += w;
            }
        }
        // Retrieval over the checkout.
        let ret_a: BTreeMap<String, f64> = area_vecs.iter().map(|(a, v)| (a.clone(), cos(&pv, v))).collect();
        let ret_f: BTreeMap<String, f64> = file_vecs.iter().map(|(f, v)| (f.clone(), cos(&pv, v))).collect();
        let hybrid = |knn: &BTreeMap<String, f64>, ret: &BTreeMap<String, f64>| -> BTreeMap<String, f64> {
            let top = knn.values().cloned().fold(0.0, f64::max).max(1e-9);
            let mut h: BTreeMap<String, f64> = ret.clone();
            for (k, v) in knn {
                *h.entry(k.clone()).or_default() += v / top;
            }
            h
        };
        if !truth_areas.is_empty() {
            r.area_knn.add(&ranked(&knn_a), &truth_areas);
            r.area_retrieval.add(&ranked(&ret_a), &truth_areas);
            r.area_hybrid.add(&ranked(&hybrid(&knn_a, &ret_a)), &truth_areas);
            r.area_frequency.add(&ranked(&freq_area), &truth_areas);
        }
        r.file_knn.add(&ranked(&knn_f), &truth_files);
        r.file_retrieval.add(&ranked(&ret_f), &truth_files);
        r.file_hybrid.add(&ranked(&hybrid(&knn_f, &ret_f)), &truth_files);
        r.file_frequency.add(&ranked(&freq_file), &truth_files);
    }
    for h in [
        &mut r.area_knn,
        &mut r.area_retrieval,
        &mut r.area_hybrid,
        &mut r.area_frequency,
        &mut r.file_knn,
        &mut r.file_retrieval,
        &mut r.file_hybrid,
        &mut r.file_frequency,
    ] {
        *h = std::mem::take(h).done();
    }
    // Task identity over every held-out session (all arms).
    let all = sessions(&m.sessions.iter().map(|s| &s.transcript).collect::<Vec<_>>(), &m.repo_key);
    let mut by_id: BTreeMap<String, &str> = BTreeMap::new();
    for x in &m.sessions {
        by_id.insert(x.id.clone(), x.task.as_str());
        if let Some(stem) = x.transcript.file_stem() {
            by_id.insert(stem.to_string_lossy().to_string(), x.task.as_str());
        }
    }
    let (mut truth, mut texts, mut vecs) = (Vec::new(), Vec::new(), Vec::new());
    for s in &all {
        let task = by_id.get(&s.id).copied().unwrap_or("");
        truth.push(task.to_string());
        texts.push(
            s.prompt
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
                .to_lowercase()
                .chars()
                .take(240)
                .collect::<String>(),
        );
        vecs.push(adage_learn::cases::vector(e, &adage_intent::normalize::normalize(&s.prompt)));
    }
    let exact: Vec<usize> = texts.iter().map(|t| texts.iter().position(|x| x == t).unwrap_or(0)).collect();
    r.task_exact = pairs(&exact, &truth);
    for tau in [0.8, 0.9, 0.95, 0.99] {
        r.task_embed.insert(format!("{tau}"), pairs(&adage_learn::cases::task_groups(&vecs, tau), &truth));
    }
    Ok(r)
}

pub fn run(manifests: &[Manifest], models: &Path) -> Result<Report> {
    let t0 = Instant::now();
    let r = model::resolve(models, false);
    let open_ms = t0.elapsed().as_secs_f64() * 1e3;
    // Per-prompt latency, over every training prompt of every manifest.
    let mut lat: Vec<f64> = Vec::new();
    let mut prompts: Vec<String> = Vec::new();
    for m in manifests {
        for s in sessions(&m.train.iter().collect::<Vec<_>>(), &m.repo_key) {
            prompts.push(adage_intent::normalize::normalize(&s.prompt));
        }
    }
    for p in &prompts {
        let t = Instant::now();
        let _ = r.embedder.embed(p);
        lat.push(t.elapsed().as_secs_f64() * 1e6);
    }
    lat.sort_by(|a, b| a.total_cmp(b));
    let pct = |q: f64| lat.get(((lat.len() as f64 - 1.0) * q).round() as usize).copied().unwrap_or(0.0);
    let int8_vs_f32 = match (model::bundled(), model::StaticModel::open(&model::dir_in(models))) {
        (Some(b), Ok(f)) => {
            let mut c: Vec<f64> = prompts.iter().filter_map(|p| Some(cos(&b.embed(p)?, &f.embed(p)?))).collect();
            c.sort_by(|a, b| a.total_cmp(b));
            (!c.is_empty()).then(|| [c[0], c[c.len() / 2]])
        }
        _ => None,
    };
    let e = model::resolve(models, true);
    let mut repos = Vec::new();
    for m in manifests {
        repos.push(repo_report(m, &e.embedder).with_context(|| m.name.clone())?);
    }
    Ok(Report {
        embedder: e.embedder.id(),
        source: e.describe(),
        open_ms: (open_ms * 100.0).round() / 100.0,
        embed_us_p50: pct(0.5).round(),
        embed_us_p99: pct(0.99).round(),
        int8_vs_f32,
        repos,
    })
}

pub fn render(r: &Report) -> String {
    use std::fmt::Write;
    let mut o = String::new();
    let _ = writeln!(o, "embeddings: {}", r.source);
    let _ = writeln!(
        o,
        "  open {:.2} ms; embed one prompt p50 {} µs, p99 {} µs{}",
        r.open_ms,
        r.embed_us_p50,
        r.embed_us_p99,
        r.int8_vs_f32.map(|c| format!("; int8 vs f32 cosine min {:.4}, median {:.5}", c[0], c[1])).unwrap_or_default()
    );
    let h = |x: &Hits| format!("{:.2}/{:.2}/{:.2}", x.hit[0], x.hit[1], x.hit[2]);
    for p in &r.repos {
        let _ = writeln!(
            o,
            "  {} ({} areas, {} files; hit@1/3/5 over {} sessions)",
            p.name, p.areas, p.files, p.area_knn.n
        );
        let _ = writeln!(
            o,
            "    area: kNN {}  retrieval {}  hybrid {}  frequency {}",
            h(&p.area_knn),
            h(&p.area_retrieval),
            h(&p.area_hybrid),
            h(&p.area_frequency)
        );
        let _ = writeln!(
            o,
            "    file: kNN {}  retrieval {}  hybrid {}  frequency {}",
            h(&p.file_knn),
            h(&p.file_retrieval),
            h(&p.file_hybrid),
            h(&p.file_frequency)
        );
        let t: Vec<String> =
            p.task_embed.iter().map(|(k, v)| format!("τ{k} P{:.2}/R{:.2}", v.precision, v.recall)).collect();
        let _ = writeln!(
            o,
            "    task identity: exact P{:.2}/R{:.2}; {}",
            p.task_exact.precision,
            p.task_exact.recall,
            t.join(", ")
        );
    }
    o
}

pub fn cli(args: &[String]) -> Result<()> {
    let flag = |n: &str| args.iter().position(|a| a == n).and_then(|i| args.get(i + 1).cloned());
    let models = flag("--models").context("usage: adage eval embeddings <manifest.json>… --models <dir> [--json]")?;
    let skip: BTreeSet<usize> = args.iter().enumerate().filter(|(_, a)| *a == "--models").map(|(i, _)| i + 1).collect();
    let mut ms = Vec::new();
    for (i, a) in args.iter().enumerate() {
        if a.starts_with("--") || skip.contains(&i) {
            continue;
        }
        ms.push(serde_json::from_slice::<Manifest>(&std::fs::read(a).with_context(|| a.clone())?)?);
    }
    if ms.is_empty() {
        bail!("usage: adage eval embeddings <manifest.json>… --models <dir> [--json]");
    }
    let r = run(&ms, Path::new(&models))?;
    if args.iter().any(|a| a == "--json") {
        println!("{}", serde_json::to_string_pretty(&r)?);
    } else {
        print!("{}", render(&r));
    }
    Ok(())
}
