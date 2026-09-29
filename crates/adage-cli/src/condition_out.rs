//! What `adage condition` prints: per repository, the tracks it wrote (each reflex with its
//! strength and evidence, read back from disk), what changed, what binding instincts did, and
//! what to do next.
//!
//! ```text
//! adage condition · 57 traces read · 54 used · 0.24 s
//! thresholds  default
//! intent      potion-base-8M
//!
//! acme-billing · 54 traces · 1 intent family
//!   work-items@0.3.0 · updated
//!     ● fire     ◠ lookup-work-item        23/33  lb 0.53
//!   changed     read-linked-files  prime → fire
//!   instincts   2 bound · 1 advice
//!
//! index       gen 8dd5ec01
//!
//! next  adage review · 1 reflex waits for a decision
//! ```

use crate::words;
use adage_learn::diff::{Change, ChangeKind, Snapshot};
use adage_learn::instincts::InstinctReport;
use adage_term::{Glyph, Line, Out, Role};
use adage_tracks::RouteState;
use std::collections::BTreeMap;
use std::io::{self, Write};

pub struct Run<'a> {
    /// `"condition"` or `"condition --instincts-only"`.
    pub command: &'a str,
    pub report: &'a adage_learn::Report,
    /// The tracks each repository has now, as written.
    pub after: &'a BTreeMap<String, Snapshot>,
    pub changes: &'a BTreeMap<&'a String, Vec<Change>>,
    pub elapsed_ms: u64,
    /// Reflexes that wait for a decision in `adage review` after this run.
    pub waiting: usize,
}

pub fn render<W: Write>(o: &mut Out<W>, run: &Run) -> io::Result<()> {
    let rep = run.report;
    let secs = format!("{:.2} s", run.elapsed_ms as f64 / 1000.0);
    let read = format!("{} read", words::count(rep.sessions_read, "trace"));
    let used = format!("{} used", rep.sessions_used);
    let steered = format!("{} steered", rep.sessions_steered);
    let mut facts: Vec<&str> = Vec::new();
    if run.command == "condition" {
        facts.push(&read);
        facts.push(&used);
        if rep.sessions_steered > 0 {
            facts.push(&steered);
        }
    }
    facts.push(&secs);
    o.header(run.command, &facts)?;
    if run.command == "condition" {
        o.kv("thresholds", Line::new().text(thresholds(&rep.thresholds)))?;
        o.kv(
            "intent",
            match &rep.intent_model {
                Some(m) => Line::new().text(m.clone()),
                None => Line::new().push(Role::Prime, "skipped").sep().quiet("no embedding model"),
            },
        )?;
    }
    for (repo, rr) in &rep.repos {
        o.blank()?;
        let mut head = Line::new().bold(Role::Ink, repo.clone());
        if run.command == "condition" {
            head = head.sep().quiet(words::count(rr.sessions, "trace"));
            if rr.intent_families > 0 {
                head = head.sep().quiet(words::count(rr.intent_families, "intent family"));
            }
        }
        o.line(&head)?;
        let snap = run.after.get(repo);
        let id_w = snap
            .into_iter()
            .flat_map(|s| s.values())
            .flat_map(|(t, _)| t.routes.iter().map(|r| r.id.chars().count()))
            .max()
            .unwrap_or(16)
            .clamp(16, 34);
        for t in &rr.tracks {
            o.line(
                &Line::new()
                    .text("  ")
                    .text(short_name(&t.name, repo))
                    .faint(format!("@{}", t.version))
                    .sep()
                    .quiet(t.change.clone()),
            )?;
            let Some((track, _)) = snap.and_then(|s| s.get(&t.name)) else { continue };
            for r in &track.routes {
                let glyph = if r.is_instinct() { Glyph::Instinct } else { Glyph::Reflex };
                let mut l = Line::new()
                    .text("    ")
                    .strength(words::strength(r.strength))
                    .pad_to(15)
                    .glyph(Role::Accent, glyph)
                    .text(" ")
                    .text(r.id.clone())
                    .pad_to(18 + id_w)
                    .extend(words::evidence(&r.evidence));
                if r.state != RouteState::Firm {
                    l = l.sep().quiet(words::lifecycle(r.state));
                }
                o.line(&l)?;
            }
        }
        // A new reflex in a track created this run is already listed above.
        let created: Vec<&str> = rr.tracks.iter().filter(|t| t.change == "created").map(|t| t.name.as_str()).collect();
        for c in run.changes.get(repo).into_iter().flatten() {
            if c.change == ChangeKind::New && created.contains(&c.track.as_str()) {
                continue;
            }
            o.line(&Line::new().text("  ").extend(change_line(c, id_w)))?;
        }
        if let Some(ir) = &rr.instincts {
            o.line(&Line::new().text("  ").extend(instincts_line(ir)))?;
            if run.command != "condition" {
                for s in ir.skipped.iter().take(12) {
                    o.line(&Line::new().text("    ").faint("not bound").pad_to(16).quiet(s.clone()))?;
                }
            }
        }
        // What learning mined, compiled and kept (`adage_learn::engine::EngineReport`).
        if run.command == "condition" {
            let e = &rr.engine;
            let mut l = Line::new()
                .text("  ")
                .faint("conditioned")
                .pad_to(16)
                .text(words::count(e.sessions, "trace"))
                .sep()
                .text(format!("{} patterns selected", e.reflexes))
                .sep()
                .text(format!("{} compiled", words::count(e.compiled, "reflex")))
                .sep()
                .text(format!("{} kept", e.routes));
            if e.contexts > 0 {
                l = l.quiet(format!(" ({} with a context trigger)", e.contexts));
            }
            o.line(&l)?;
        }
    }
    if !rep.conflicts.is_empty() {
        o.blank()?;
        o.kv("interlocking", Line::new().text(format!("{} resolved", words::count(rep.conflicts.len(), "conflict"))))?;
        for c in &rep.conflicts {
            o.line(&Line::new().text("  ").quiet(words::prose(&c.describe())))?;
        }
    }
    // Nascent reflexes are formed but never delivered until a person decides.
    let nascent = rep
        .repos
        .values()
        .flat_map(|rr| rr.tracks.iter())
        .flat_map(|t| t.routes.iter())
        .filter(|l| l.contains(" NASCENT: "))
        .count();
    if nascent > 0 {
        o.blank()?;
        o.note(&format!(
            "{} formed but not delivered: each waits for a person.",
            words::count(nascent, "nascent reflex")
        ))?;
    }
    o.blank()?;
    o.kv("index", Line::new().text(format!("gen {}", &rep.index_generation[..rep.index_generation.len().min(8)])))?;
    // What to do next: decisions first, then what would make conditioning see more.
    let mut nexts: Vec<(String, String)> = Vec::new();
    if run.command == "condition" && rep.repos.is_empty() {
        o.blank()?;
        o.note("No reflexes formed: none of these traces could be used.")?;
        nexts.push(("adage sense".into(), "with the same sources: what those traces hold".into()));
    }
    if run.waiting > 0 {
        let why = if run.waiting == 1 {
            "1 reflex waits for a decision".to_string()
        } else {
            format!("{} reflexes wait for a decision", run.waiting)
        };
        nexts.push(("adage review".into(), why));
    }
    if run.command == "condition" && rep.intent_model.is_none() {
        nexts.push(("adage models fetch".into(), "similar prompts can then share a reflex".into()));
    }
    if nexts.is_empty() {
        nexts.push(("adage status".into(), "every reflex in effect".into()));
    }
    o.blank()?;
    for (i, (cmd, why)) in nexts.iter().enumerate() {
        if i == 0 {
            o.next(cmd, why)?;
        } else {
            o.also(cmd, why)?;
        }
    }
    Ok(())
}

/// The evidence bars in the lexicon's words: `min_support=3 fire_lower_bound=0.5
/// prime_rate=0.5` (or the older `green_lower_bound`, `yellow_rate`) → `support ≥ 3 · fire at lb ≥ 0.5 · prime at rate ≥ 0.5`.
fn thresholds(s: &str) -> String {
    s.split_whitespace()
        .map(|kv| match kv.split_once('=') {
            Some(("min_support", v)) => format!("support ≥ {v}"),
            Some(("fire_lower_bound" | "green_lower_bound", v)) => format!("fire at lb ≥ {v}"),
            Some(("prime_rate" | "yellow_rate", v)) => format!("prime at rate ≥ {v}"),
            _ => kv.to_string(),
        })
        .collect::<Vec<_>>()
        .join(" · ")
}

/// A learned track's name without its repository prefix (`acme-billing/work-items` → `work-items`).
fn short_name(name: &str, repo: &str) -> String {
    let tail = repo.rsplit('/').next().unwrap_or(repo);
    name.strip_prefix(&format!("{tail}/")).unwrap_or(name).to_string()
}

fn change_line(c: &Change, id_w: usize) -> Line {
    let l = Line::new().faint("changed").pad_to(14).text(c.route.clone()).pad_to(16 + id_w);
    match (c.change, c.from, c.to) {
        (ChangeKind::Strength, Some(a), Some(b)) => l
            .push(words::strength(a).role(), words::strength(a).word())
            .faint(" → ")
            .push(words::strength(b).role(), words::strength(b).word()),
        (ChangeKind::New, _, Some(b)) => l.quiet("new ").push(words::strength(b).role(), words::strength(b).word()),
        (ChangeKind::Nascent, _, _) => l.quiet("nascent").sep().quiet("waits for review"),
        (ChangeKind::Extinguished, _, _) => l.quiet(words::prose(&c.summary)),
        (ChangeKind::Dormant, _, _) => l.quiet("dormant"),
        _ => l.quiet(words::prose(&c.summary)),
    }
}

fn instincts_line(ir: &InstinctReport) -> Line {
    let l = Line::new().faint("instincts").pad_to(14);
    if let Some(why) = &ir.not_bound {
        return l.quiet("none bound").sep().quiet(words::prose(why));
    }
    let mut l = l.text(format!("{} bound", ir.bound)).sep().text(format!("{} advice", ir.advice));
    if ir.nascent > 0 {
        l = l.sep().push(Role::Prime, format!("{} wait for review", ir.nascent));
    }
    if !ir.extinguished.is_empty() {
        l = l.sep().quiet(format!("{} extinguished after failing", ir.extinguished.len()));
    }
    if !ir.confirmed.is_empty() {
        l = l.sep().quiet(format!("{} confirmed by traces", ir.confirmed.len()));
    }
    l
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thresholds_read_in_the_lexicon() {
        assert_eq!(
            thresholds("min_support=3 fire_lower_bound=0.5 prime_rate=0.5"),
            "support ≥ 3 · fire at lb ≥ 0.5 · prime at rate ≥ 0.5"
        );
        assert_eq!(
            thresholds("min_support=3 green_lower_bound=0.5 yellow_rate=0.5"),
            "support ≥ 3 · fire at lb ≥ 0.5 · prime at rate ≥ 0.5",
            "older learn.log lines"
        );
    }

    #[test]
    fn short_names_drop_the_repository() {
        assert_eq!(short_name("acme-billing/work-items", "acme-billing"), "work-items");
        assert_eq!(short_name("acme-billing/work-items", "github.com/acme/acme-billing"), "work-items");
        assert_eq!(short_name("other/x", "acme-billing"), "other/x");
    }
}
