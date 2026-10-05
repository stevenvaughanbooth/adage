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
//!     ● fire     ◠ on a prompt that names a ticket, run bin/wi-lookup {item}   23/33  lb 0.53
//!   changed     on a prompt that names a ticket, read the files it names      prime → fire
//!   instincts   3 in use (2 steps to run, 1 advice)
//!
//! index       gen 8dd5ec01
//!
//! next  adage review · 1 needs you · 2 to look over
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
    /// What `adage review` has after this run: `(needs you, to look over)`.
    pub waiting: (usize, usize),
    /// End with what to run next (`adage init`, which runs it as a step, prints its own).
    pub next: bool,
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
        for t in &rr.tracks {
            let track = snap.and_then(|s| s.get(&t.name)).map(|(track, _)| track);
            let mut head = Line::new()
                .text("  ")
                .text(short_name(&t.name, repo))
                .faint(format!("@{}", t.version))
                .sep()
                .quiet(t.change.clone());
            // A track this run left as it was: its count, not every reflex again.
            let unchanged = t.change == "unchanged";
            if let Some(track) = track.filter(|_| unchanged) {
                head = head.sep().quiet(words::count(track.routes.len(), "reflex"));
            }
            o.line(&head)?;
            let Some(track) = track.filter(|_| !unchanged) else { continue };
            if track.routes.iter().all(|r| r.is_instinct()) {
                instinct_lines(o, &track.routes, "    ")?;
                continue;
            }
            for r in &track.routes {
                let glyph = if r.is_instinct() { Glyph::Instinct } else { Glyph::Reflex };
                let mut l = Line::new()
                    .text("    ")
                    .strength(words::strength(r.strength))
                    .pad_to(15)
                    .glyph(Role::Accent, glyph)
                    .text(" ")
                    .text(clip(&r.gist(), GIST))
                    .pad_to(18 + GIST)
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
            // Each reflex by what it tells the agent (`Route::gist`), its id only if unknown.
            let name = if c.gist.is_empty() { &c.route } else { &c.gist };
            o.line(&Line::new().text("  ").extend(change_line(c, name)))?;
        }
        if let Some(ir) = &rr.instincts {
            o.line(&Line::new().text("  ").extend(instincts_line(ir)))?;
            if run.command != "condition" {
                // `U3: after pytest fails with "…", make test: already bound by U11`, once per
                // instinct and reason, with how many triggers it covers.
                let key = |s: &&String| {
                    let (id, rest) = s.split_once(": ").unwrap_or(("", s));
                    format!("{id}\n{}", rest.rsplit_once(": ").map_or(rest, |(_, why)| why))
                };
                for g in words::group(ir.skipped.iter(), key).into_iter().take(12) {
                    let text = match g.len() {
                        1 => g[0].clone(),
                        n => match g[0].split_once(": ") {
                            Some((id, rest)) => format!("{id}: {n} triggers, e.g. {rest}"),
                            None => g[0].clone(),
                        },
                    };
                    o.line(&Line::new().text("    ").faint("not bound").pad_to(16).quiet(text))?;
                }
            }
        }
        // What learning mined, compiled and kept (`adage_learn::engine::EngineReport`): the
        // patterns, what they compiled to, and the reflexes kept, summed by where each came from
        // (a pattern, a value estimate, a procedure card…), so the numbers add up.
        if run.command == "condition" {
            let e = &rr.engine;
            let mut l = Line::new()
                .text("  ")
                .faint("conditioned")
                .pad_to(16)
                .text(words::count(e.sessions, "trace"))
                .sep()
                .text(format!("{} → {} compiled", words::count(e.reflexes, "pattern"), e.compiled))
                .sep()
                .text(format!("{} kept", words::count(e.routes, "reflex")));
            let from: Vec<String> = e.kept_by.iter().map(|(k, n)| kept_from(k, *n)).collect();
            if !from.is_empty() && e.kept_by.values().sum::<usize>() == e.routes {
                l = l.quiet(format!(": {}", from.join(", ")));
            }
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
    if let Some(why) = crate::review::waiting_words(run.waiting.0, run.waiting.1) {
        nexts.push(("adage review".into(), why));
    }
    if run.command == "condition" && rep.intent_model.is_none() {
        nexts.push(("adage models fetch".into(), "similar prompts can then share a reflex".into()));
    }
    if nexts.is_empty() {
        nexts.push(("adage status".into(), "every reflex in effect".into()));
    }
    if !run.next {
        return Ok(());
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

/// `1 from a mined pattern`, `2 procedure cards`: one source of the reflexes a run kept.
fn kept_from(kind: &str, n: usize) -> String {
    match kind {
        "procedure card" => words::count(n, "procedure card"),
        "mined pattern" => format!("{n} from {}", if n == 1 { "a mined pattern" } else { "mined patterns" }),
        "value estimate" => format!("{n} from {}", if n == 1 { "a value estimate" } else { "value estimates" }),
        "lookup" => words::count(n, "lookup"),
        "prompt intent" => format!("{n} from {}", if n == 1 { "a prompt intent" } else { "prompt intents" }),
        other => format!("{n} {other}"),
    }
}

/// An instinct track's reflexes, one line per instinct and step (`U11 run .venv/bin/python -m
/// pytest · 9 triggers, e.g. …`): its reflexes differ only in the failure each answers.
pub(crate) fn instinct_lines<W: Write>(o: &mut Out<W>, routes: &[adage_tracks::Route], indent: &str) -> io::Result<()> {
    let label = |r: &adage_tracks::Route| match r.origin() {
        adage_tracks::Origin::Instinct { instinct, .. } if !instinct.is_empty() => instinct,
        _ => "instinct".to_string(),
    };
    let groups = words::group(routes.iter(), |r| {
        format!("{}\n{}\n{:?}\n{:?}", label(r), crate::share::describe_step(&r.step), r.strength, r.state)
    });
    for g in groups {
        let r = g[0];
        let when = words::trigger(&r.trigger);
        let w = indent.chars().count();
        let mut l = Line::new()
            .text(indent)
            .strength(words::strength(r.strength))
            .pad_to(w + 11)
            .glyph(Role::Accent, Glyph::Instinct)
            .text(" ")
            .text(label(r))
            .pad_to(w + 18)
            .text(crate::share::describe_step(&r.step))
            .sep()
            .quiet(if g.len() == 1 { format!("when {when}") } else { format!("{} triggers, e.g. {when}", g.len()) });
        if r.state != RouteState::Firm {
            l = l.sep().quiet(words::lifecycle(r.state));
        }
        o.line(&l)?;
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

/// How wide a reflex's gist may be on a line before it is clipped.
const GIST: usize = 58;

/// `s` in at most `n` characters, ending in `…` when cut.
fn clip(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        return s.to_string();
    }
    let mut out: String = s.chars().take(n.saturating_sub(1)).collect();
    out.truncate(out.trim_end().len());
    out.push('…');
    out
}

fn change_line(c: &Change, gist: &str) -> Line {
    let l = Line::new().faint("changed").pad_to(14).text(clip(gist, GIST)).pad_to(16 + GIST);
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
    // In use, and of what kind: a run step the agent is told to take, or advice.
    let kinds = match (ir.bound, ir.advice) {
        (0, 0) => String::new(),
        (0, _) => " (advice)".into(),
        (_, 0) => " (steps to run)".into(),
        (b, a) => format!(" ({b} steps to run, {a} advice)"),
    };
    let mut l = l.text(format!("{} in use{kinds}", ir.bound + ir.advice));
    if ir.nascent > 0 {
        l = l.sep().push(Role::Prime, format!("{} need{} you", ir.nascent, if ir.nascent == 1 { "s" } else { "" }));
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
