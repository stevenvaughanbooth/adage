//! Prototypes: from node embeddings to labels (Track G §2).
//!
//! Distinct nodes of each family (prompt, act, art = read and edit, fail, obs) are clustered with
//! the method intent already uses (`adage_intent::cluster`): exact cosine kNN (k = 15), a graph
//! thresholded at τ, deterministic Louvain, and a minimum size. Then:
//!
//! - **Blocking.** Clusters form only inside one block: effect class and program for acts, top
//!   directory and extension family for artifacts. So `Verify` never merges with `Regenerate`.
//! - **Must-link:** commands equal under `pattern::meets` (they differ only by options) are joined
//!   by an edge of weight 1. **Cannot-link:** commands with different HeadSub identities never
//!   share a prototype (a community holding both is split, first fit in member order), so
//!   `make gen` and a decoy `make proto` stay apart, and every act prototype sits inside one
//!   identity (which keeps the taxonomy a tree).
//! - **Assignment** is hard: nearest centroid (quantised), ties by prototype name. A node whose
//!   best cosine is below τ, or whose top-2 margin is below δ = 0.02, is a singleton: it keeps its
//!   exact label and has no prototype label.
//! - **Taxonomy:** exact ⊂ prototype ⊂ coarse prototype (a second Louvain over level-1 labels at
//!   τ − 0.1, same blocks and cannot-links) ⊂ head (HeadSub identity; `top/**/*.<family>`) ⊂
//!   kind (`read`, `edit`, `prompt`).
//! - **τ calibration** per family by successor purity: on the grid {0.80, 0.85, 0.90, 0.95} pick
//!   the τ that maximises the held-out log-likelihood of the next event's exact label given the
//!   node's level-1 label (fit and held-out sessions split by a hash of the id). Ties go to the
//!   higher τ.
//! - **Embeddings.** An event's own embedding when it has one (node embeddings come from the
//!   substrate, `embed.rs`); otherwise a 256-d signed feature hash (SHA-256-derived, fixed) of
//!   the structural features Track G §1.2 lists, so prototypes work before the substrate lands.
//!
//! The result is an [`Epoch`]: per family its τ, prototypes (quantised centroids, members,
//! coarse group) and a label table. [`Epoch::label`] assigns sessions to it (new nodes by nearest
//! centroid), giving the [`Corpus`] the miner reads. A pure function of (sessions, params).

use crate::events::{self, Corpus, Event, EventSession, Level, NodeType, LEVELS};
use adage_core::pattern::Shape;
use adage_intent::cluster;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

pub const EPOCH_VERSION: u32 = 1;
/// Dimensions of the fallback feature-hash embedding.
pub const HASH_DIM: usize = 256;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ProtoParams {
    pub k: usize,
    pub tau_act: f32,
    pub tau_art: f32,
    pub tau_prompt: f32,
    pub tau_fail: f32,
    /// The coarse level clusters at τ − this.
    pub coarse_drop: f32,
    /// Top-2 margin below which a node is a singleton.
    pub delta: f32,
    /// Smallest prototype (members).
    pub min_size: usize,
    /// Only nodes seen in at least this many sessions are clustered (others are assigned).
    pub min_count: u32,
    pub resolution: f64,
    pub calibrate: bool,
    pub grid: Vec<f32>,
    pub threads: usize,
}

impl Default for ProtoParams {
    fn default() -> Self {
        ProtoParams {
            k: 15,
            tau_act: 0.90,
            tau_art: 0.85,
            tau_prompt: 0.80,
            tau_fail: 0.88,
            coarse_drop: 0.10,
            delta: 0.02,
            min_size: 2,
            min_count: 2,
            resolution: 1.0,
            calibrate: true,
            grid: vec![0.80, 0.85, 0.90, 0.95],
            threads: 1,
        }
    }
}

impl ProtoParams {
    pub fn tau(&self, family: &str) -> f32 {
        match family {
            "act" => self.tau_act,
            "art" => self.tau_art,
            "prompt" => self.tau_prompt,
            _ => self.tau_fail,
        }
    }
}

/// One prototype.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Proto {
    /// Content name: the dominant member's key (most sessions, ties by key).
    pub name: String,
    pub block: String,
    /// The members' head (one HeadSub identity for acts).
    pub head: String,
    pub centroid: Vec<i16>,
    /// Member keys with their session counts, by key.
    pub members: Vec<(String, u32)>,
}

/// One family's clustering.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct FamilyEpoch {
    pub tau: f32,
    /// (τ, held-out log-likelihood) per grid point, when calibrated.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub calibration: Vec<(f32, f64)>,
    pub protos: Vec<Proto>,
    /// Exact key → prototype name, for every clustered member.
    pub assign: BTreeMap<String, String>,
    /// Level-1 content (prototype name, or a singleton's own key) → coarse group name.
    pub coarse: BTreeMap<String, String>,
    /// Nodes seen, and how many ended as singletons.
    pub nodes: u32,
    pub singletons: u32,
}

/// A prototype epoch (`tracks/<repo>/.epoch.json`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Epoch {
    pub version: u32,
    pub params: ProtoParams,
    pub families: BTreeMap<String, FamilyEpoch>,
    /// Content-ranked label strings of the corpus the epoch was built on.
    pub labels: Vec<String>,
}

/// One distinct node of a family.
#[derive(Debug, Clone)]
struct Node {
    key: String,
    head: String,
    block: String,
    count: u32,
    emb: Vec<f32>,
}

fn hash_feature(v: &mut [f32], feature: &str, w: f32) {
    let h = Sha256::digest(feature.as_bytes());
    let i = u16::from_le_bytes([h[0], h[1]]) as usize % v.len();
    let sign = if h[2] & 1 == 0 { 1.0 } else { -1.0 };
    v[i] += sign * w;
}

fn path_features(v: &mut [f32], path: &str, w: f32) {
    let segs: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    let mut prefix = String::new();
    for (d, s) in segs.iter().enumerate() {
        if d + 1 == segs.len() {
            break;
        }
        prefix.push_str(s);
        prefix.push('/');
        hash_feature(v, &format!("dir:{prefix}"), w * (1.0 + 0.5 * d as f32));
    }
    let base = segs.last().copied().unwrap_or("");
    hash_feature(v, &format!("ext:{}", events::ext_family(base)), w);
    let stem = base.rsplit_once('.').map(|(s, _)| s).unwrap_or(base);
    for t in stem.split(['_', '-', '.']).filter(|t| !t.is_empty()) {
        hash_feature(v, &format!("stem:{t}"), 0.5 * w);
    }
}

/// The fallback structural embedding of a node (Track G §1.2, the hashed half), a unit vector.
pub fn hashed_embedding(ty: NodeType, key: &str, head: &str, block: &str) -> Vec<f32> {
    let mut v = vec![0f32; HASH_DIM];
    match ty {
        NodeType::Act => {
            let cmd = key.strip_prefix("exec ").unwrap_or(key);
            let toks: Vec<String> = cmd.split_whitespace().map(str::to_string).collect();
            if let Some(shape) = Shape::of_tokens(&toks) {
                hash_feature(&mut v, &format!("prog:{}", shape.program), 2.0);
                hash_feature(&mut v, &format!("id:{}", shape.identity.join(" ")), 3.0);
                for o in &shape.options {
                    let name = o.split([' ', '=']).next().unwrap_or(o);
                    hash_feature(&mut v, &format!("opt:{}|{name}", shape.identity.join(" ")), 1.0);
                }
                hash_feature(&mut v, &format!("npos:{}", shape.positionals.len()), 1.0);
                for p in &shape.positionals {
                    if p.contains('/') || p.contains('.') {
                        path_features(&mut v, p, 0.7);
                    } else {
                        hash_feature(&mut v, &format!("pos:{p}"), 1.0);
                    }
                }
            }
            for t in &toks {
                hash_feature(&mut v, &format!("t:{t}"), 0.5);
            }
            for w in toks.windows(2) {
                hash_feature(&mut v, &format!("b:{} {}", w[0], w[1]), 0.5);
            }
            hash_feature(&mut v, &format!("eff:{}", block.split(' ').next().unwrap_or("")), 1.0);
        }
        NodeType::Read | NodeType::Edit => {
            path_features(&mut v, key, 1.0);
            hash_feature(&mut v, &format!("head:{head}"), 1.0);
        }
        _ => {
            for w in key.split_whitespace() {
                hash_feature(&mut v, &format!("w:{w}"), 1.0);
            }
        }
    }
    adage_intent::unit(v)
}

fn event_embedding(e: &Event) -> Vec<f32> {
    match &e.embedding {
        Some(x) => x.to_unit(),
        None => hashed_embedding(e.ty, &e.key, &e.head, &e.block),
    }
}

fn session_in_fit(id: &str) -> bool {
    Sha256::digest(id.as_bytes())[0] & 1 == 0
}

/// Distinct nodes per family, sorted by key; the first embedding seen (in session-id order) wins.
fn collect(sessions: &[&EventSession]) -> BTreeMap<String, Vec<Node>> {
    let mut acc: BTreeMap<(String, String), (Node, BTreeSet<usize>)> = BTreeMap::new();
    for (si, s) in sessions.iter().enumerate() {
        for e in s.events.iter().filter(|e| e.ty != NodeType::Skill) {
            let fam = e.ty.family().to_string();
            let ent = acc.entry((fam, e.key.clone())).or_insert_with(|| {
                (
                    Node {
                        key: e.key.clone(),
                        head: e.head.clone(),
                        block: e.block.clone(),
                        count: 0,
                        emb: event_embedding(e),
                    },
                    BTreeSet::new(),
                )
            });
            ent.1.insert(si);
        }
    }
    let mut out: BTreeMap<String, Vec<Node>> = BTreeMap::new();
    for ((fam, _), (mut n, ss)) in acc {
        n.count = ss.len() as u32;
        out.entry(fam).or_default().push(n);
    }
    out
}

/// Identity of an act key (cannot-link and must-link read it).
fn shape_of(key: &str) -> Option<Shape> {
    Shape::of_key(key)
}

/// Split a community so no cannot-linked pair shares a part (first fit, member order).
fn split_cannot(members: &[usize], nodes: &[Node], family: &str) -> Vec<Vec<usize>> {
    if family != "act" {
        return vec![members.to_vec()];
    }
    let mut parts: Vec<(String, Vec<usize>)> = Vec::new();
    for &m in members {
        let id = nodes[m].head.clone();
        match parts.iter_mut().find(|(k, _)| *k == id) {
            Some((_, v)) => v.push(m),
            None => parts.push((id, vec![m])),
        }
    }
    parts.into_iter().map(|(_, v)| v).collect()
}

fn centroid(vs: &[&[f32]]) -> Vec<f32> {
    let d = vs.first().map(|v| v.len()).unwrap_or(0);
    let mut c = vec![0f32; d];
    for v in vs {
        for (x, y) in c.iter_mut().zip(v.iter()) {
            *x += *y;
        }
    }
    adage_intent::unit(c)
}

/// Communities of `items` (indices into `vecs`) at threshold `eps`, with must-links.
fn communities(
    vecs: &[Vec<f32>],
    items: &[usize],
    eps: f32,
    must: &[(usize, usize)],
    p: &ProtoParams,
) -> Vec<Vec<usize>> {
    let n = items.len();
    if n < 2 {
        return vec![];
    }
    let d = vecs[items[0]].len();
    if items.iter().any(|&i| vecs[i].len() != d) {
        return vec![];
    }
    let flat: Vec<f32> = items.iter().flat_map(|&i| vecs[i].iter().copied()).collect();
    let k = p.k.min(n - 1);
    let (idx, sim) = cluster::knn(&flat, n, d, k, p.threads.max(1));
    let mut adj = cluster::graph(&idx, &sim, n, k, eps);
    let pos: BTreeMap<usize, usize> = items.iter().enumerate().map(|(a, &b)| (b, a)).collect();
    for &(a, b) in must {
        if let (Some(&x), Some(&y)) = (pos.get(&a), pos.get(&b)) {
            if x != y {
                adj[x].push((y as u32, 1.0));
                adj[y].push((x as u32, 1.0));
            }
        }
    }
    for a in adj.iter_mut() {
        a.sort_by(|x, y| x.0.cmp(&y.0).then(y.1.total_cmp(&x.1)));
        a.dedup_by_key(|e| e.0);
    }
    let comm = cluster::louvain(adj, p.resolution, 10);
    let mut groups: BTreeMap<u32, Vec<usize>> = BTreeMap::new();
    for (i, c) in comm.into_iter().enumerate() {
        groups.entry(c).or_default().push(items[i]);
    }
    // Communities in order of their smallest member (content order: nodes are sorted by key).
    let mut out: Vec<Vec<usize>> = groups.into_values().collect();
    for g in out.iter_mut() {
        g.sort_unstable();
    }
    out.sort();
    out
}

fn dominant(members: &[(String, u32)]) -> String {
    let mut best: Option<&(String, u32)> = None;
    for m in members {
        if best.is_none_or(|b| m.1 > b.1 || (m.1 == b.1 && m.0 < b.0)) {
            best = Some(m);
        }
    }
    best.map(|b| b.0.clone()).unwrap_or_default()
}

fn nearest(protos: &[&Proto], emb: &[f32]) -> Option<(usize, f32, f32)> {
    let mut best: Option<(usize, f32)> = None;
    let mut second = f32::NEG_INFINITY;
    for (i, pr) in protos.iter().enumerate() {
        let c = adage_intent::dequantize(&pr.centroid);
        if c.len() != emb.len() {
            continue;
        }
        let s = cluster::dot(&c, emb);
        match best {
            None => best = Some((i, s)),
            Some((_, b)) if s > b => {
                second = b;
                best = Some((i, s));
            }
            Some(_) => {
                if s > second {
                    second = s;
                }
            }
        }
    }
    best.map(|(i, s)| (i, s, s - second))
}

/// Cluster one family at `tau`.
fn cluster_family(family: &str, nodes: &[Node], tau: f32, p: &ProtoParams) -> FamilyEpoch {
    let vecs: Vec<Vec<f32>> = nodes.iter().map(|n| n.emb.clone()).collect();
    let mut blocks: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
    for (i, n) in nodes.iter().enumerate() {
        blocks.entry(n.block.as_str()).or_default().push(i);
    }
    let mut fe = FamilyEpoch { tau, nodes: nodes.len() as u32, ..Default::default() };
    for (block, idxs) in &blocks {
        let eligible: Vec<usize> = idxs.iter().copied().filter(|&i| nodes[i].count >= p.min_count).collect();
        // Must-link: acts equal up to options (same identity and positionals).
        let mut must = Vec::new();
        if family == "act" {
            let mut by: BTreeMap<(Vec<String>, Vec<String>), usize> = BTreeMap::new();
            for &i in &eligible {
                if let Some(s) = shape_of(&nodes[i].key) {
                    match by.get(&(s.identity.clone(), s.positionals.clone())) {
                        Some(&j) => must.push((j, i)),
                        None => {
                            by.insert((s.identity, s.positionals), i);
                        }
                    }
                }
            }
        }
        let mut protos: Vec<Proto> = Vec::new();
        for comm in communities(&vecs, &eligible, tau, &must, p) {
            for part in split_cannot(&comm, nodes, family) {
                if part.len() < p.min_size.max(2) {
                    continue;
                }
                let members: Vec<(String, u32)> =
                    part.iter().map(|&i| (nodes[i].key.clone(), nodes[i].count)).collect();
                let c = centroid(&part.iter().map(|&i| vecs[i].as_slice()).collect::<Vec<_>>());
                protos.push(Proto {
                    name: dominant(&members),
                    block: block.to_string(),
                    head: nodes[part[0]].head.clone(),
                    centroid: adage_intent::quantize(&c),
                    members,
                });
            }
        }
        protos.sort_by(|a, b| a.name.cmp(&b.name));
        // Hard assignment, with the singleton rules.
        let refs: Vec<&Proto> = protos.iter().collect();
        let mut kept: BTreeMap<String, Vec<(String, u32)>> = BTreeMap::new();
        for &i in idxs {
            let n = &nodes[i];
            let hit = nearest(&refs, &n.emb)
                .filter(|&(j, s, margin)| s >= tau && margin >= p.delta && (family != "act" || refs[j].head == n.head));
            match hit {
                Some((j, _, _)) => kept.entry(refs[j].name.clone()).or_default().push((n.key.clone(), n.count)),
                None => fe.singletons += 1,
            }
        }
        for mut pr in protos {
            let Some(mem) = kept.remove(&pr.name) else { continue };
            if mem.len() < 2 {
                // A prototype left with one member is that member.
                fe.singletons += mem.len() as u32;
                continue;
            }
            pr.members = mem;
            for (k, _) in &pr.members {
                fe.assign.insert(k.clone(), pr.name.clone());
            }
            fe.protos.push(pr);
        }
    }
    fe.protos.sort_by(|a, b| a.name.cmp(&b.name).then(a.block.cmp(&b.block)));
    coarse_level(family, nodes, &vecs, &mut fe, tau - p.coarse_drop, p);
    fe
}

/// The coarse level: Louvain over level-1 labels (prototypes and singletons) at `eps`.
fn coarse_level(family: &str, nodes: &[Node], vecs: &[Vec<f32>], fe: &mut FamilyEpoch, eps: f32, p: &ProtoParams) {
    // Level-1 labels: (content, block, head, count, vector).
    let mut l1: Vec<Node> = Vec::new();
    let mut seen: BTreeSet<String> = BTreeSet::new();
    for pr in &fe.protos {
        l1.push(Node {
            key: pr.name.clone(),
            head: pr.head.clone(),
            block: pr.block.clone(),
            count: pr.members.iter().map(|m| m.1).sum(),
            emb: adage_intent::dequantize(&pr.centroid),
        });
        seen.insert(pr.name.clone());
    }
    for (i, n) in nodes.iter().enumerate() {
        if !fe.assign.contains_key(&n.key) && !seen.contains(&n.key) && n.count >= p.min_count {
            l1.push(Node { emb: vecs[i].clone(), ..n.clone() });
        }
    }
    l1.sort_by(|a, b| a.key.cmp(&b.key));
    let lv: Vec<Vec<f32>> = l1.iter().map(|n| n.emb.clone()).collect();
    let mut blocks: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
    for (i, n) in l1.iter().enumerate() {
        blocks.entry(n.block.as_str()).or_default().push(i);
    }
    for idxs in blocks.values() {
        for comm in communities(&lv, idxs, eps, &[], p) {
            for part in split_cannot(&comm, &l1, family) {
                if part.len() < 2 {
                    continue;
                }
                let members: Vec<(String, u32)> = part.iter().map(|&i| (l1[i].key.clone(), l1[i].count)).collect();
                let name = dominant(&members);
                for (k, _) in members {
                    fe.coarse.insert(k, name.clone());
                }
            }
        }
    }
}

/// Held-out log-likelihood of the next event's exact label given a node's level-1 label.
fn successor_ll(family: &str, sessions: &[&EventSession], fe: &FamilyEpoch) -> Option<f64> {
    let l1 = |k: &str| fe.assign.get(k).cloned().unwrap_or_else(|| k.to_string());
    let mut counts: BTreeMap<String, BTreeMap<String, u32>> = BTreeMap::new();
    let mut vocab: BTreeSet<String> = BTreeSet::new();
    let pairs = |s: &EventSession| -> Vec<(String, String)> {
        s.events
            .windows(2)
            .filter(|w| w[0].ty.family() == family)
            .map(|w| (l1(&w[0].key), format!("{}:{}", w[1].ty.name(), w[1].key)))
            .collect()
    };
    for s in sessions.iter().filter(|s| session_in_fit(&s.id)) {
        for (a, b) in pairs(s) {
            vocab.insert(b.clone());
            *counts.entry(a).or_default().entry(b).or_default() += 1;
        }
    }
    let v = vocab.len() as f64 + 1.0;
    let alpha = 0.1;
    let mut ll = 0.0;
    let mut n = 0u32;
    for s in sessions.iter().filter(|s| !session_in_fit(&s.id)) {
        for (a, b) in pairs(s) {
            let row = counts.get(&a);
            let c = row.and_then(|r| r.get(&b)).copied().unwrap_or(0) as f64;
            let tot = row.map(|r| r.values().sum::<u32>()).unwrap_or(0) as f64;
            ll += ((c + alpha) / (tot + alpha * v)).ln();
            n += 1;
        }
    }
    (n > 0).then_some(ll)
}

/// Build an epoch from sessions (a full learn).
pub fn build(sessions: &[EventSession], p: &ProtoParams) -> Epoch {
    let mut order: Vec<&EventSession> = sessions.iter().collect();
    order.sort_by(|a, b| a.id.cmp(&b.id));
    let fams = collect(&order);
    let mut families = BTreeMap::new();
    for (fam, nodes) in &fams {
        let default = p.tau(fam);
        let fe = if p.calibrate && (fam == "act" || fam == "art" || fam == "prompt") && !p.grid.is_empty() {
            let mut best: Option<(f64, FamilyEpoch)> = None;
            let mut cal = Vec::new();
            let mut grid = p.grid.clone();
            grid.sort_by(|a, b| b.total_cmp(a)); // descending: ties go to the higher τ
            for &tau in &grid {
                let fe = cluster_family(fam, nodes, tau, p);
                let Some(ll) = successor_ll(fam, &order, &fe) else {
                    best = None;
                    cal.clear();
                    break;
                };
                cal.push((tau, ll));
                if best.as_ref().is_none_or(|(b, _)| ll > *b + 1e-9) {
                    best = Some((ll, fe));
                }
            }
            match best {
                Some((_, mut fe)) => {
                    cal.sort_by(|a, b| a.0.total_cmp(&b.0));
                    fe.calibration = cal;
                    fe
                }
                None => cluster_family(fam, nodes, default, p),
            }
        } else {
            cluster_family(fam, nodes, default, p)
        };
        families.insert(fam.clone(), fe);
    }
    let mut epoch = Epoch { version: EPOCH_VERSION, params: p.clone(), families, labels: vec![] };
    epoch.labels = epoch.label(sessions).labels.names;
    epoch
}

impl Epoch {
    /// Level-1 content of a node: its prototype, or `None` (a singleton). New nodes are assigned
    /// to the nearest centroid of their block under the singleton rules.
    fn proto_of(&self, e: &Event) -> Option<String> {
        let fe = self.families.get(e.ty.family())?;
        if let Some(p) = fe.assign.get(&e.key) {
            return Some(p.clone());
        }
        let cands: Vec<&Proto> = fe.protos.iter().filter(|p| p.block == e.block).collect();
        if cands.is_empty() {
            return None;
        }
        let emb = event_embedding(e);
        let (j, s, margin) = nearest(&cands, &emb)?;
        let same = e.ty != NodeType::Act || cands[j].head == e.head;
        (s >= fe.tau && margin >= self.params.delta && same).then(|| cands[j].name.clone())
    }

    /// The label strings of one event at each level.
    pub fn names(&self, e: &Event) -> [Option<String>; LEVELS] {
        let ty = e.ty;
        let mut out: [Option<String>; LEVELS] = Default::default();
        out[0] = Some(events::label_name(ty, Level::Exact, &e.key));
        // Two skills are never one item: a skill directive names one skill, by name.
        if ty == NodeType::Skill {
            return out;
        }
        let proto = self.proto_of(e);
        if let Some(p) = &proto {
            out[1] = Some(events::label_name(ty, Level::Proto, p));
        }
        let l1 = proto.unwrap_or_else(|| e.key.clone());
        if let Some(c) = self.families.get(ty.family()).and_then(|f| f.coarse.get(&l1)) {
            out[2] = Some(events::label_name(ty, Level::Coarse, c));
        }
        match ty {
            NodeType::Act => {
                out[3] = Some(events::label_name(ty, Level::Head, &e.head));
                // A failed run's failure family: every runner that fails with this signature.
                if let Some(sig) = e.signature.as_deref().filter(|s| !e.ok && !s.is_empty()) {
                    out[4] = Some(events::label_name(ty, Level::Kind, &format!("fails: {sig}")));
                }
            }
            NodeType::Read | NodeType::Edit => {
                out[3] = Some(events::label_name(ty, Level::Head, &e.head));
                out[4] = Some(events::label_name(ty, Level::Kind, ty.name()));
            }
            NodeType::Prompt => out[4] = Some(events::label_name(ty, Level::Kind, ty.name())),
            NodeType::Obs | NodeType::Fail => out[4] = Some(events::label_name(ty, Level::Kind, ty.name())),
            // A skill is its name: no prototype, coarse, head or kind level (see `names`).
            NodeType::Skill => {}
        }
        out
    }

    /// Label sessions with this epoch.
    pub fn label(&self, sessions: &[EventSession]) -> Corpus {
        events::build_corpus(sessions, &|e| self.names(e))
    }

    /// Share of nodes that are singletons, over every family.
    pub fn singleton_share(&self) -> f64 {
        let (n, s) = self.families.values().fold((0u32, 0u32), |a, f| (a.0 + f.nodes, a.1 + f.singletons));
        if n == 0 {
            0.0
        } else {
            s as f64 / n as f64
        }
    }

    pub fn write(&self, path: &std::path::Path) -> std::io::Result<()> {
        std::fs::write(path, serde_json::to_vec_pretty(self).map_err(std::io::Error::other)?)
    }

    pub fn read(path: &std::path::Path) -> std::io::Result<Epoch> {
        serde_json::from_slice(&std::fs::read(path)?).map_err(std::io::Error::other)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::Embedding;

    fn ev(ty: NodeType, key: &str, head: &str, block: &str, emb: Option<Vec<f32>>) -> Event {
        Event {
            ty,
            key: key.into(),
            head: head.into(),
            block: block.into(),
            embedding: emb.map(Embedding::F32),
            batch: 0,
            ok: true,
            segment: 0,
            effect: None,
            signature: None,
        }
    }

    fn sess(id: &str, events: Vec<Event>) -> EventSession {
        EventSession {
            id: id.into(),
            day: 0,
            task: None,
            outcome: None,
            events,
            flows: vec![],
            same_art: vec![],
            exposed: vec![],
        }
    }

    #[test]
    fn close_nodes_share_a_prototype_and_cannot_links_hold() {
        // Three near-identical embeddings in one block and identity, one far away, and one with
        // the same embedding but another identity (cannot-link).
        let a = vec![1.0, 0.0, 0.0, 0.0];
        let b = vec![0.99, 0.05, 0.0, 0.0];
        let c = vec![0.98, 0.0, 0.08, 0.0];
        let far = vec![0.0, 0.0, 0.0, 1.0];
        let mk = |id: &str| {
            sess(
                id,
                vec![
                    ev(NodeType::Act, "exec pytest tests/a.py", "pytest", "verify pytest", Some(a.clone())),
                    ev(NodeType::Act, "exec pytest tests/b.py", "pytest", "verify pytest", Some(b.clone())),
                    ev(NodeType::Act, "exec pytest tests/c.py", "pytest", "verify pytest", Some(c.clone())),
                    ev(NodeType::Act, "exec pytest -x", "pytest", "verify pytest", Some(far.clone())),
                    ev(NodeType::Act, "exec make gen", "make gen", "regenerate make", Some(a.clone())),
                    ev(NodeType::Act, "exec make proto", "make proto", "regenerate make", Some(a.clone())),
                ],
            )
        };
        let sessions: Vec<EventSession> = (0..6).map(|i| mk(&format!("s{i}"))).collect();
        let p = ProtoParams { calibrate: false, ..Default::default() };
        let e = build(&sessions, &p);
        let act = &e.families["act"];
        assert_eq!(act.assign.get("exec pytest tests/a.py"), act.assign.get("exec pytest tests/b.py"));
        assert!(act.assign.contains_key("exec pytest tests/c.py"));
        assert!(!act.assign.contains_key("exec pytest -x"));
        // make gen and make proto: identical embeddings, different identities: never one prototype.
        assert_ne!(
            act.assign.get("exec make gen").cloned().unwrap_or("gen".into()),
            act.assign.get("exec make proto").cloned().unwrap_or("proto".into())
        );
        // Taxonomy: exact → proto → head.
        let corpus = e.label(&sessions);
        let t = &corpus.labels;
        let lab = corpus.sessions[0].events[0].lab;
        assert_ne!(lab[1], events::NONE);
        assert_eq!(t.parent[lab[0] as usize], lab[1]);
        assert!(t.is_ancestor_or_self(lab[3], lab[0]));
        // Deterministic and independent of input order and thread count.
        let mut rev = sessions.clone();
        rev.reverse();
        assert_eq!(build(&rev, &ProtoParams { threads: 4, ..p.clone() }).families, e.families);
    }

    #[test]
    fn hashed_embeddings_group_paths_of_one_directory() {
        let a = hashed_embedding(NodeType::Read, "schema/a.json", "schema/**/*.config", "schema:config");
        let b = hashed_embedding(NodeType::Read, "schema/b.json", "schema/**/*.config", "schema:config");
        let c = hashed_embedding(NodeType::Read, "src/deep/x.py", "src/**/*.py", "src:py");
        assert!(cluster::dot(&a, &b) > cluster::dot(&a, &c));
    }

    #[test]
    fn epochs_round_trip_and_assign_new_nodes() {
        let a = vec![1.0, 0.0, 0.0];
        let b = vec![0.99, 0.1, 0.0];
        let mk = |id: &str| {
            sess(
                id,
                vec![
                    ev(NodeType::Read, "schema/a.json", "schema/**/*.config", "schema:config", Some(a.clone())),
                    ev(NodeType::Read, "schema/b.json", "schema/**/*.config", "schema:config", Some(b.clone())),
                ],
            )
        };
        let sessions: Vec<EventSession> = (0..4).map(|i| mk(&format!("s{i}"))).collect();
        let e = build(&sessions, &ProtoParams { calibrate: false, ..Default::default() });
        let dir = std::env::temp_dir().join(format!("adage-epoch-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join(".epoch.json");
        e.write(&f).unwrap();
        assert_eq!(Epoch::read(&f).unwrap(), e);
        let new =
            ev(NodeType::Read, "schema/c.json", "schema/**/*.config", "schema:config", Some(vec![0.995, 0.05, 0.0]));
        assert!(e.names(&new)[1].is_some(), "a new node joins the nearest prototype");
        let _ = std::fs::remove_dir_all(dir);
    }
}
