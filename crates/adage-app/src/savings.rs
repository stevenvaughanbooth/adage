//! The savings estimate (docs/zero-config.md §4.3): what Adage saves per session, measured against
//! the sessions where it stayed quiet (the control arm), with an interval, or nothing at all.
//!
//! **Population.** Sessions of repository R that have a `session_start` spool record (exposed to
//! an index with at least one reflex for R), are finished (a `SessionEnd`/`Stop` record, or last
//! active more than 30 minutes ago), and appear in R's ledger ([`adage_learn::ledger`]).
//!
//! **Estimand.** Intent-to-treat per session, Δ = E[Y | control] − E[Y | treated], so a positive Δ
//! is a saving. The arm is randomised at session start, before anything downstream, and each
//! session's recorded propensity weights it (Hajek IPW), so a changed `runtime.control` stays
//! valid. Outcomes: API-list-equivalent cost, model turns, and turns to the first edit. Each is
//! winsorised at its pooled 95th percentile (the cap is in `--json`).
//!
//! **Interval.** A bootstrap of 2,000 resamples, stratified by arm, seeded from the sorted session
//! ids (every surface shows the same numbers). With arms this small a plain percentile interval
//! is too narrow, so it is the *expanded* percentile interval (Hesterberg 2015): the percentiles
//! taken are widened by √(n/(n−1))·t(n−1) in place of 1.96, n the smaller arm.
//!
//! **Gate.** At least 10 control and 30 treated sessions in the repository. Below it, only counts
//! of observed events are shown, never dollars. Above it: a lower bound above zero is a saving;
//! an interval across zero is "no measurable difference yet"; an upper bound below zero says,
//! plainly, that Adage appears to add cost. Beside it always: how often each arm's agent ended by
//! asking the person for help, so a saving never hides worse outcomes.

use adage_learn::ledger::{Ledger, SessionSummary};
use adage_store::Paths;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::Path;

pub const NEED_CONTROL: usize = 10;
pub const NEED_TREATED: usize = 30;
pub const RESAMPLES: usize = 2000;
/// A session last active this long ago is finished even without an end record.
const IDLE_MS: u64 = 30 * 60 * 1000;
/// Counts cover this many days.
const COUNT_DAYS: u64 = 30;
/// Spools older than this are not read.
const SPOOL_DAYS: u64 = 180;

/// One session in the population.
#[derive(Debug, Clone, PartialEq)]
pub struct Unit {
    pub id: String,
    pub control: bool,
    pub propensity: f64,
    pub cost: Option<f64>,
    pub turns: f64,
    pub first_edit: Option<f64>,
    pub asked_for_help: bool,
}

/// One outcome's estimate.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Estimate {
    /// Δ = control − treated: positive is a saving per session.
    pub delta: f64,
    pub lo: f64,
    pub hi: f64,
    pub mean_control: f64,
    pub mean_treated: f64,
    pub n_control: usize,
    pub n_treated: usize,
    /// The pooled 95th percentile each value was capped at.
    pub winsorized_at: f64,
}

/// Counts observed directly in the spools (never converted to dollars).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Counts {
    pub days: u64,
    pub sessions: usize,
    pub directives: usize,
    pub followed: usize,
    pub dead_ends_headed_off: usize,
    pub finish_lines_met: usize,
    /// Commands run with the project's environment instead of the system Python (`PreToolUse`
    /// rewrites), and `pip install`s into the system Python refused.
    #[serde(default)]
    pub rewritten: usize,
    /// When the earliest session counted was first recorded (ms since the epoch; 0 for none). A
    /// count that began inside the window says so (docs/design/everyday-ux.md, rule 3).
    #[serde(default)]
    pub since: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    /// Below the gate: counts only.
    Measuring,
    Saved,
    NoDifference,
    AddsCost,
}

/// What every surface reads (`ledger/<slug>.summary.json`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Summary {
    pub repo: String,
    pub computed_at: u64,
    /// The pinned price table's date: dollar figures are API-list equivalents as of it.
    pub pricing_as_of: String,
    pub n_treated: usize,
    pub n_control: usize,
    pub gate_met: bool,
    pub verdict: Verdict,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost_usd: Option<Estimate>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_turns: Option<Estimate>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turns_to_first_edit: Option<Estimate>,
    /// Sessions whose model has no listed price (tokens and turns only): the cost estimate
    /// leaves them out.
    pub unpriced: usize,
    /// Share of sessions whose agent ended by asking the person for help, per arm.
    pub asked_for_help_treated: f64,
    pub asked_for_help_control: f64,
    pub counts: Counts,
}

// --- The estimator ----------------------------------------------------------------------------

/// SplitMix64: a small, fixed generator, so resamples are the same on every platform.
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

/// The seed from the sorted session ids.
pub fn seed(ids: &[&str]) -> u64 {
    let mut v: Vec<&str> = ids.to_vec();
    v.sort();
    let d = Sha256::digest(v.join("\n").as_bytes());
    u64::from_le_bytes(d[..8].try_into().expect("8 bytes"))
}

fn quantile(sorted: &[f64], q: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let pos = q.clamp(0.0, 1.0) * (sorted.len() - 1) as f64;
    let (i, frac) = (pos.floor() as usize, pos - pos.floor());
    if i + 1 < sorted.len() {
        sorted[i] + frac * (sorted[i + 1] - sorted[i])
    } else {
        sorted[i]
    }
}

/// Student's t 0.975 quantile.
fn t975(df: usize) -> f64 {
    const T: [f64; 30] = [
        12.706, 4.303, 3.182, 2.776, 2.571, 2.447, 2.365, 2.306, 2.262, 2.228, 2.201, 2.179, 2.160, 2.145, 2.131,
        2.120, 2.110, 2.101, 2.093, 2.086, 2.080, 2.074, 2.069, 2.064, 2.060, 2.056, 2.052, 2.048, 2.045, 2.042,
    ];
    match df {
        0 => T[0],
        1..=30 => T[df - 1],
        _ => 1.96 + 2.4 / df as f64,
    }
}

/// The standard normal CDF (Abramowitz and Stegun 7.1.26, error below 1.5e-7).
fn phi(x: f64) -> f64 {
    let t = 1.0 / (1.0 + 0.327_591_1 * x.abs() / std::f64::consts::SQRT_2);
    let y = 1.0
        - (((((1.061_405_429 * t - 1.453_152_027) * t) + 1.421_413_741) * t - 0.284_496_736) * t + 0.254_829_592)
            * t
            * (-(x * x) / 2.0).exp();
    if x >= 0.0 {
        0.5 * (1.0 + y)
    } else {
        0.5 * (1.0 - y)
    }
}

/// Hajek (self-normalised inverse-propensity) mean.
fn hajek(ys: &[(f64, f64)]) -> f64 {
    let (num, den) = ys.iter().fold((0.0, 0.0), |(n, d), (y, p)| (n + y / p, d + 1.0 / p));
    if den > 0.0 {
        num / den
    } else {
        0.0
    }
}

/// Estimate Δ for `(control?, y, propensity)` values. `None` with an empty arm.
pub fn estimate(values: &[(bool, f64, f64)], seed: u64) -> Option<Estimate> {
    let mut pooled: Vec<f64> = values.iter().map(|v| v.1).collect();
    pooled.sort_by(|a, b| a.total_cmp(b));
    let cap = quantile(&pooled, 0.95);
    let arm = |c: bool| -> Vec<(f64, f64)> {
        values.iter().filter(|v| v.0 == c).map(|v| (v.1.min(cap), v.2.clamp(1e-6, 1.0))).collect()
    };
    let (ctl, trt) = (arm(true), arm(false));
    if ctl.is_empty() || trt.is_empty() {
        return None;
    }
    let (mc, mt) = (hajek(&ctl), hajek(&trt));
    let delta = mc - mt;
    let mut rng = Rng(seed);
    let mut draws: Vec<f64> = Vec::with_capacity(RESAMPLES);
    let mut buf: Vec<(f64, f64)> = Vec::new();
    let mut resample = |xs: &[(f64, f64)], rng: &mut Rng| {
        buf.clear();
        for _ in 0..xs.len() {
            buf.push(xs[rng.below(xs.len())]);
        }
        hajek(&buf)
    };
    for _ in 0..RESAMPLES {
        let c = resample(&ctl, &mut rng);
        let t = resample(&trt, &mut rng);
        draws.push(c - t);
    }
    draws.sort_by(|a, b| a.total_cmp(b));
    // Expanded percentiles for small samples (Hesterberg 2015).
    let n = ctl.len().min(trt.len()).max(2);
    let z = (n as f64 / (n as f64 - 1.0)).sqrt() * t975(n - 1);
    let tail = phi(-z);
    Some(Estimate {
        delta,
        lo: quantile(&draws, tail),
        hi: quantile(&draws, 1.0 - tail),
        mean_control: mc,
        mean_treated: mt,
        n_control: ctl.len(),
        n_treated: trt.len(),
        winsorized_at: cap,
    })
}

fn verdict_of(e: &Estimate) -> Verdict {
    if e.lo > 0.0 {
        Verdict::Saved
    } else if e.hi < 0.0 {
        Verdict::AddsCost
    } else {
        Verdict::NoDifference
    }
}

/// The estimate for these sessions (already the population), with the gate applied.
pub fn summarise(repo: &str, units: &[Unit], counts: Counts, now: u64) -> Summary {
    let ids: Vec<&str> = units.iter().map(|u| u.id.as_str()).collect();
    let s = seed(&ids);
    let n_control = units.iter().filter(|u| u.control).count();
    let n_treated = units.len() - n_control;
    let gate_met = n_control >= NEED_CONTROL && n_treated >= NEED_TREATED;
    let rate = |c: bool| {
        let arm: Vec<&Unit> = units.iter().filter(|u| u.control == c).collect();
        if arm.is_empty() {
            0.0
        } else {
            arm.iter().filter(|u| u.asked_for_help).count() as f64 / arm.len() as f64
        }
    };
    let priced: Vec<(bool, f64, f64)> =
        units.iter().filter_map(|u| u.cost.map(|c| (u.control, c, u.propensity))).collect();
    let priced_gate =
        priced.iter().filter(|v| v.0).count() >= NEED_CONTROL && priced.iter().filter(|v| !v.0).count() >= NEED_TREATED;
    let turns: Vec<(bool, f64, f64)> = units.iter().map(|u| (u.control, u.turns, u.propensity)).collect();
    let edits: Vec<(bool, f64, f64)> =
        units.iter().filter_map(|u| u.first_edit.map(|t| (u.control, t, u.propensity))).collect();
    let (cost_usd, model_turns, turns_to_first_edit) = if gate_met {
        (if priced_gate { estimate(&priced, s) } else { None }, estimate(&turns, s), estimate(&edits, s ^ 0x5eed))
    } else {
        (None, None, None)
    };
    let verdict = match (gate_met, cost_usd.as_ref().or(model_turns.as_ref())) {
        (true, Some(e)) => verdict_of(e),
        _ => Verdict::Measuring,
    };
    Summary {
        repo: repo.to_string(),
        computed_at: now,
        pricing_as_of: adage_core::pricing::table().as_of.clone(),
        n_treated,
        n_control,
        gate_met,
        verdict,
        cost_usd,
        model_turns,
        turns_to_first_edit,
        unpriced: units.iter().filter(|u| u.cost.is_none()).count(),
        asked_for_help_treated: rate(false),
        asked_for_help_control: rate(true),
        counts,
    }
}

// --- The population, from spools and the ledger -------------------------------------------------

/// What one spool says about a session.
#[derive(Debug, Default)]
struct SpoolSession {
    started: bool,
    arm_control: Option<bool>,
    propensity: Option<f64>,
    ended: bool,
    first_ts: u64,
    last_ts: u64,
    directives: usize,
    followed: usize,
    headed_off: usize,
    finish_lines: usize,
    rewritten: usize,
}

fn read_spool(path: &Path, repo: &str) -> Option<SpoolSession> {
    let recs: Vec<Value> = adage_store::spool::read_all(path).ok()?;
    let mut s = SpoolSession::default();
    let mut mine = false;
    let mut inhibits: Vec<String> = Vec::new();
    let mut ignored: Vec<String> = Vec::new();
    for r in &recs {
        if r["repo"].as_str() != Some(repo) {
            continue;
        }
        mine = true;
        let ts = r["ts"].as_u64().unwrap_or(0);
        s.last_ts = s.last_ts.max(ts);
        if ts > 0 && (s.first_ts == 0 || ts < s.first_ts) {
            s.first_ts = ts;
        }
        if s.arm_control.is_none() {
            s.arm_control = r["arm"].as_str().map(|a| a == "control");
        }
        if s.propensity.is_none() {
            s.propensity = r["propensity"].as_f64().filter(|p| *p > 0.0 && *p <= 1.0);
        }
        match r["kind"].as_str() {
            Some("session_start") => s.started = true,
            // Rewrites only: advice changes no command.
            Some("pretool") if r["withheld"] != true && r["pretool"].get("rewrite").is_some() => s.rewritten += 1,
            Some("session_end") | Some("stop") => s.ended = true,
            // A step boundary neither starts nor ends a session; a step Adage predicted is judged
            // `predicted`, which is not a follow (below).
            Some("turn_step") => {}
            _ => {}
        }
        let d = &r["decision"];
        for dir in d["directives"].as_array().into_iter().flatten() {
            s.directives += 1;
            if dir["strength"] == "inhibit" {
                inhibits.push(dir["route"].as_str().unwrap_or("").to_string());
            }
        }
        for j in d["judgements"].as_array().into_iter().flatten() {
            match j["kind"].as_str() {
                Some("followed") => s.followed += 1,
                Some("obligation_met") => s.finish_lines += 1,
                // Followed, and the directed command failed: followed all the same (the card
                // counts what agents did), but no dead end was headed off.
                Some("misfired") => {
                    s.followed += 1;
                    ignored.push(j["route"].as_str().unwrap_or("").to_string());
                }
                Some("ignored") => ignored.push(j["route"].as_str().unwrap_or("").to_string()),
                // A dead end replaced before it ran: headed off whether or not it was announced.
                Some("substituted") => {
                    let route = j["route"].as_str().unwrap_or("").to_string();
                    if !inhibits.contains(&route) {
                        inhibits.push(route);
                    }
                }
                _ => {}
            }
        }
    }
    s.headed_off = inhibits.iter().filter(|r| !ignored.contains(r)).count();
    mine.then_some(s)
}

/// The population and the counts for `repo`, from the spools and this ledger.
pub fn gather(paths: &Paths, repo: &str, ledger: &BTreeMap<String, SessionSummary>, now: u64) -> (Vec<Unit>, Counts) {
    let by_spool: BTreeMap<String, &SessionSummary> =
        ledger.values().map(|s| (adage_store::paths::sanitise(&s.session_id), s)).collect();
    let mut units = Vec::new();
    let mut counts = Counts { days: COUNT_DAYS, ..Default::default() };
    let since = now.saturating_sub(COUNT_DAYS * 86_400_000);
    let oldest = std::time::SystemTime::now() - std::time::Duration::from_secs(SPOOL_DAYS * 86_400);
    for e in std::fs::read_dir(paths.sessions()).into_iter().flatten().flatten() {
        let p = e.path();
        if p.extension().is_none_or(|x| x != "spool") {
            continue;
        }
        if e.metadata().and_then(|m| m.modified()).is_ok_and(|t| t < oldest) {
            continue;
        }
        let Some(s) = read_spool(&p, repo) else { continue };
        if s.last_ts >= since {
            counts.sessions += 1;
            if s.first_ts > 0 && (counts.since == 0 || s.first_ts < counts.since) {
                counts.since = s.first_ts;
            }
            counts.directives += s.directives;
            counts.followed += s.followed;
            counts.dead_ends_headed_off += s.headed_off;
            counts.finish_lines_met += s.finish_lines;
            counts.rewritten += s.rewritten;
        }
        let stem = p.file_stem().map(|x| x.to_string_lossy().to_string()).unwrap_or_default();
        let (Some(l), true, Some(control)) = (by_spool.get(&stem), s.started, s.arm_control) else { continue };
        let last = s.last_ts.max(l.ended_at);
        if !(s.ended || now.saturating_sub(last) > IDLE_MS) {
            continue;
        }
        units.push(Unit {
            id: l.session_id.clone(),
            control,
            propensity: s.propensity.unwrap_or(if control { 0.1 } else { 0.9 }),
            cost: l.cost.usd,
            turns: f64::from(l.model_turns),
            first_edit: l.first_edit_turn.map(f64::from),
            asked_for_help: l.asked_for_help,
        });
    }
    units.sort_by(|a, b| a.id.cmp(&b.id));
    (units, counts)
}

/// Compute and write `ledger/<slug>.summary.json` for `repo` from its ledger.
pub fn refresh(paths: &Paths, repo: &str, ledger: &BTreeMap<String, SessionSummary>) -> Summary {
    let now = now_ms();
    let (units, counts) = gather(paths, repo, ledger, now);
    let mut s = summarise(repo, &units, counts, now);
    // Every trace on an unlisted model, not only those in the population.
    s.unpriced = ledger.values().filter(|l| l.cost.usd.is_none()).count();
    let l = Ledger::new(&paths.ledger(), repo);
    if let Ok(b) = serde_json::to_vec_pretty(&s) {
        let _ = adage_store::config::write_atomic(&l.summary_path(), &b);
    }
    s
}

/// The summary for `repo`: the last one written, recomputed first when the ledger is newer than it
/// (or there is a ledger and no summary yet). `None` when the repository has no ledger.
pub fn read(paths: &Paths, repo: &str) -> Option<Summary> {
    let l = Ledger::new(&paths.ledger(), repo);
    let mtime = |p: &Path| std::fs::metadata(p).and_then(|m| m.modified()).ok();
    let ledger_at = mtime(&l.path())?;
    let fresh = mtime(&l.summary_path()).is_some_and(|s| s >= ledger_at);
    if !fresh {
        return Some(refresh(paths, repo, &l.read()));
    }
    serde_json::from_slice(&std::fs::read(l.summary_path()).ok()?).ok()
}

// --- Wording ------------------------------------------------------------------------------------

pub fn usd(x: f64) -> String {
    let a = x.abs();
    if a >= 100.0 {
        format!("${a:.0}")
    } else if a >= 1.0 {
        format!("${a:.2}")
    } else {
        format!("${a:.3}")
    }
}

/// The card's savings lines (one or two), in the lexicon's words: dollars only past the gate and
/// always with an interval, labelled API-list equivalent.
pub fn lines(s: &Summary) -> Vec<String> {
    if !s.gate_met {
        let line = if s.n_control < NEED_CONTROL {
            format!(
                "measuring savings: {} of {NEED_CONTROL} quiet sessions so far (Adage stays quiet in 1 of 10 sessions so its effect can be measured)",
                s.n_control
            )
        } else {
            format!("measuring savings: {} of {NEED_TREATED} sessions with Adage so far", s.n_treated)
        };
        return vec![line];
    }
    let turns = s.model_turns.as_ref();
    let turn_text = |e: &Estimate| format!("{:.1} turns", e.delta.abs());
    match (s.verdict, s.cost_usd.as_ref()) {
        (Verdict::Saved, Some(c)) => vec![
            format!(
                "savings: ~{} {}per session, API-list equivalent (95% CI {}–{})",
                usd(c.delta),
                turns.filter(|t| t.lo > 0.0).map(|t| format!("and {} ", turn_text(t))).unwrap_or_default(),
                usd(c.lo),
                usd(c.hi)
            ),
            format!(
                "         ≈ {} over {} sessions, vs {} where Adage stayed quiet (priced as of {})",
                usd(c.delta * s.n_treated as f64),
                s.n_treated,
                s.n_control,
                s.pricing_as_of
            ),
        ],
        (Verdict::Saved, None) => vec![format!(
            "savings: ~{} per session (95% CI {:.1}–{:.1}), vs {} sessions where Adage stayed quiet; no dollar figure (model not in the price table)",
            turns.map(turn_text).unwrap_or_default(),
            turns.map(|t| t.lo).unwrap_or(0.0),
            turns.map(|t| t.hi).unwrap_or(0.0),
            s.n_control
        )],
        (Verdict::AddsCost, Some(c)) => vec![format!(
            "Adage appears to add ~{} per session, API-list equivalent (95% CI {}–{}). See adage review.",
            usd(-c.delta),
            usd(-c.hi),
            usd(-c.lo)
        )],
        (Verdict::AddsCost, None) => vec![format!(
            "Adage appears to add ~{} per session. See adage review.",
            turns.map(turn_text).unwrap_or_default()
        )],
        (_, Some(c)) => vec![format!(
            "savings: no measurable difference yet (±{} per session, API-list equivalent; {} sessions vs {} quiet)",
            usd((c.hi - c.lo) / 2.0),
            s.n_treated,
            s.n_control
        )],
        (_, None) => vec![format!(
            "savings: no measurable difference yet (±{:.1} turns per session; {} sessions vs {} quiet)",
            turns.map(|t| (t.hi - t.lo) / 2.0).unwrap_or(0.0),
            s.n_treated,
            s.n_control
        )],
    }
}

/// The quality guardrail line, shown beside a figure past the gate.
pub fn guardrail(s: &Summary) -> Option<String> {
    s.gate_met.then(|| {
        format!(
            "ended asking you for help: {:.0}% with Adage, {:.0}% without",
            s.asked_for_help_treated * 100.0,
            s.asked_for_help_control * 100.0
        )
    })
}

/// A signed dollar amount: `$0.034`, `−$0.010`.
pub fn usd_signed(x: f64) -> String {
    if x < 0.0 && usd(x) != usd(0.0) {
        format!("−{}", usd(x))
    } else {
        usd(x)
    }
}

pub fn traces(n: usize) -> String {
    format!("{n} trace{}", if n == 1 { "" } else { "s" })
}

/// The verdict in words.
pub fn verdict_word(v: Verdict) -> &'static str {
    match v {
        Verdict::Measuring => "measuring",
        Verdict::Saved => "saved",
        Verdict::NoDifference => "no measurable difference",
        Verdict::AddsCost => "adds cost",
    }
}

/// The estimate in one line, for `adage watch` and `adage ui`: below the gate only what is being
/// counted (`measuring: 4 control / 25 treated traces`), never dollars; past it the figure per
/// trace, always with its 95% interval, labelled an API-list equivalent.
pub fn brief(s: &Summary) -> String {
    if !s.gate_met {
        return format!("measuring: {} control / {} treated traces", s.n_control, s.n_treated);
    }
    let vs = format!("{} vs {} control traces", s.n_treated, s.n_control);
    let turns = s.model_turns.as_ref();
    match (s.verdict, s.cost_usd.as_ref(), turns) {
        (Verdict::Saved, Some(c), _) => format!(
            "saved ~{} per trace, API-list equivalent (95% CI {}–{}) · {vs}",
            usd(c.delta),
            usd_signed(c.lo),
            usd_signed(c.hi)
        ),
        (Verdict::AddsCost, Some(c), _) => format!(
            "adds ~{} per trace, API-list equivalent (95% CI {}–{}) · {vs}",
            usd(-c.delta),
            usd_signed(-c.hi),
            usd_signed(-c.lo)
        ),
        (Verdict::NoDifference, Some(c), _) => {
            format!("no measurable difference yet (95% CI {}–{} per trace) · {vs}", usd_signed(c.lo), usd_signed(c.hi))
        }
        (v, None, Some(t)) => {
            let what = match v {
                Verdict::Saved => format!("saved ~{:.1} turns per trace", t.delta),
                Verdict::AddsCost => format!("adds ~{:.1} turns per trace", -t.delta),
                _ => "no measurable difference yet".to_string(),
            };
            format!("{what} (95% CI {:.1}–{:.1} turns) · {vs}; no dollar figure", t.lo, t.hi)
        }
        _ => format!("no measurable difference yet · {vs}"),
    }
}

pub use adage_core::time::now_ms;

#[cfg(test)]
mod tests {
    use super::*;

    fn unit(i: usize, control: bool, cost: f64) -> Unit {
        Unit {
            id: format!("s{i:03}"),
            control,
            propensity: if control { 0.1 } else { 0.9 },
            cost: Some(cost),
            turns: cost * 100.0,
            first_edit: None,
            asked_for_help: false,
        }
    }

    #[test]
    fn hajek_weights_by_propensity() {
        assert!((hajek(&[(1.0, 0.5), (3.0, 0.25)]) - (2.0 + 12.0) / 6.0).abs() < 1e-12);
    }

    #[test]
    fn t_and_normal_helpers() {
        assert!((phi(0.0) - 0.5).abs() < 1e-7);
        assert!((phi(1.96) - 0.975).abs() < 1e-4);
        assert_eq!(t975(11), 2.201);
        assert!((t975(60) - 2.0).abs() < 0.01);
    }

    /// Acceptance test 4: with 40 treated and 12 control sessions and a known Δ, the interval
    /// covers the true Δ in at least 93 of 100 seeded simulations (costs skewed, as real ones are),
    /// and the same inputs give the same numbers.
    #[test]
    fn the_interval_covers_the_true_difference() {
        let true_delta = 0.05;
        let mut covered = 0;
        for sim in 0..100u64 {
            let mut rng = Rng(sim.wrapping_mul(0x9E37_79B9) + 7);
            // Lognormal-ish costs around $0.20: exp of a sum of uniforms.
            let mut draw = |shift: f64| {
                let z: f64 = (0..6).map(|_| (rng.next() % 10_000) as f64 / 10_000.0).sum::<f64>() - 3.0;
                0.2 * (0.35 * z).exp() / (0.35f64 * 0.35 / 2.0).exp() + shift
            };
            let mut v: Vec<(bool, f64, f64)> = Vec::new();
            for _ in 0..40 {
                v.push((false, draw(0.0), 0.9));
            }
            for _ in 0..12 {
                v.push((true, draw(true_delta), 0.1));
            }
            // The winsorised mean differs slightly from the raw one; the target is the true Δ.
            let e = estimate(&v, sim).unwrap();
            if e.lo <= true_delta && true_delta <= e.hi {
                covered += 1;
            }
            assert_eq!(estimate(&v, sim), estimate(&v, sim), "deterministic");
        }
        eprintln!("covered the true difference in {covered} of 100");
        assert!(covered >= 93, "covered the true difference in {covered} of 100");
    }

    #[test]
    fn a_negative_difference_says_adage_appears_to_add_cost() {
        let units: Vec<Unit> = (0..52)
            .map(|i| {
                unit(i, i >= 40, if i >= 40 { 0.1 + (i % 3) as f64 * 0.001 } else { 0.3 + (i % 5) as f64 * 0.001 })
            })
            .collect();
        let s = summarise("r", &units, Counts::default(), 0);
        assert_eq!(s.verdict, Verdict::AddsCost);
        assert!(lines(&s)[0].starts_with("Adage appears to add ~$0.20"), "{:?}", lines(&s));
    }

    #[test]
    fn below_the_gate_there_are_no_dollars() {
        let units: Vec<Unit> = (0..29).map(|i| unit(i, i < 4, 0.1)).collect();
        let s = summarise("r", &units, Counts::default(), 0);
        assert_eq!((s.n_control, s.n_treated, s.gate_met, s.verdict), (4, 25, false, Verdict::Measuring));
        assert!(s.cost_usd.is_none());
        let l = lines(&s).join("\n");
        assert!(!l.contains('$') && l.contains("4 of 10 quiet sessions"), "{l}");
    }
}
