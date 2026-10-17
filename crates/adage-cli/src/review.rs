//! `adage review` (Design J1, §10.4, §10.9; Architecture §6.3): the reflexes that wait for a
//! person, and the decisions a person makes about them.
//!
//! ```text
//! adage review [--json] [--verbose]                         list the queue (agents may read it);
//!                                                           --verbose adds the statistics
//! adage review approve    <track> <route> [--by <name>]     accept as is (restores an extinguished route at
//!                                                           forming; puts a nascent environment step into service)
//! adage review extinguish <track> <route> [--by <name>]     take it out of service, and keep it out
//! adage review weaken     <track> <route> [--by <name>]     fire → prime, held across conditioning
//! adage review strengthen <track> <route> [--by <name>]     prime → fire, within the effect ceiling only
//! adage review pin        <track> <route> [--by <name>]     edit-lock: mark `authored`
//! adage review dismiss    <track> <route>                   keep a pinned route; silence its proposed change
//! adage review delegate   <track> <route> --level e1|e2 …   let Adage run it (`crate::exec`)
//! adage review revoke     <track> <route>                   withdraw that grant
//! ```
//!
//! A pinned route whose learned version drifted materially waits with the reason "proposed" and a
//! diff (see [`crate::proposal`]); `approve` applies the proposed version, which stays pinned.
//!
//! Instinct routes (Adage's built-in defaults, bound from a repository's files; track
//! `<repo>/instincts`) are listed with their origin until a person decides, once per instinct
//! and step with what triggers each; one in effect waits for no decision (it is not counted as
//! waiting). They stay prime at most and can never be strengthened.
//!
//! A learned routine with execution evidence (execution only: never said to the agent) waits
//! for a decision on its evidence once; after `approve` it is listed as ready to delegate, and
//! no longer waits. `adage review delegate` grants it, separately. Approving an environment-effect instinct (an install step,
//! nascent until then) approves every environment step of its pack for that repository, recorded
//! in the track's `review.yaml` (`packs`); extinguishing one keeps it out of later binds.
//!
//! `<track>` is a track name (a learned track wins over an import of the same name) or its
//! directory under the tracks directory, e.g. `@imported/acme/work-items`.
//!
//! Decisions change trust, so they refuse to run inside an agent session (review finding F11),
//! unless a person made them in the session with a press (mod architecture §3.12): a verb run by the
//! Claude Code mod reads `{ "press", "session" }` on stdin, and on success prints `{ "next":
//! <token> }` as its first line, before anything else ([`person_or_press`]).
//! They are recorded in the track's `review.yaml` (see [`adage_tracks::review`]); every decision
//! re-validates the track and recompiles the index atomically, as `adage condition` and `adage add` do.

use crate::share::describe_step;
use crate::words;
use adage_app::person::PersonVerb;
use adage_app::reflex::{Decisions, Reflex};
pub(crate) use adage_app::review::{
    ceiling_str, first_hand, needs_person, origin_label, preview, waiting_counts, waits, Installed, Standing,
    LISTED_ONLY,
};
use adage_app::review::{judgements, queue, verbs, Already, Outcome, QueueItem as Item, Verb};
use adage_store::Paths;
use adage_term::out::KEY_WIDTH;
use adage_term::{Glyph, Line, Out, Role};
use adage_tracks::{Index, Step, Strength, Trigger};
use anyhow::{bail, Result};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const USAGE: &str = "usage: adage review [--json] [--verbose]\n       adage review <approve|extinguish|weaken|strengthen|pin|dismiss> <track> <reflex> [--by <name>]\n       adage review delegate <track> <reflex> --level e1|e2 [...]\n       adage review revoke <track> <reflex>";

pub fn review(args: &[String], paths: &Paths) -> Result<()> {
    match args.first().map(String::as_str) {
        None | Some("list" | "--json" | "--verbose" | "-v") => {
            list(paths, args.iter().any(|a| a == "--json"), args.iter().any(|a| a == "--verbose" || a == "-v"))
        }
        Some(verb @ ("approve" | "extinguish" | "weaken" | "strengthen" | "pin" | "dismiss")) => {
            decide(verb, &args[1..], paths)
        }
        _ => bail!("{USAGE}"),
    }
}

fn list(paths: &Paths, as_json: bool, verbose: bool) -> Result<()> {
    let all = discover(&paths.tracks())?;
    let judged = judgements(paths);
    let index = Index::compile_root(&paths.tracks(), &all.iter().map(|i| i.track.clone()).collect::<Vec<_>>()).0;
    // The checkout the person is in, once: the queue's grants and the lapsed ones are both for it.
    let here = here();
    let items = queue(&all, &judged, &index, paths, here.as_deref());
    let inert = crate::packs::inert_routes(paths);
    let attested = attestations(paths);
    let store = adage_exec::approvals::Store::load(&adage_exec::approvals::path(&paths.root)).unwrap_or_default();
    // Grants here that no longer match what would run: a person's decision again.
    let lapsed = crate::exec::lapsed_here(paths, here.as_deref(), &store);
    let revoked = store.revoked;
    if as_json {
        let live = adage_app::graph::live(paths);
        let rows: Vec<Value> = items
            .iter()
            .map(|it| {
                let (t, r) = (&it.inst.track, it.route);
                let whys = adage_tracks::why::load(&it.inst.dir);
                let mut row = serde_json::to_value(Reflex::of(it.inst, r, whys.get(&r.id))).unwrap_or_default();
                row["review"] = json!(Decisions::of(&it.inst.reference, r, verbs(r, it.proposal.is_some()), &it.reasons));
                row["waits"] = json!(waits(&it.reasons));
                row["live"] = json!(live.get(&(it.inst.reference.clone(), r.id.clone())).cloned().unwrap_or_default());
                row["example_values"] = json!(preview(t, r).1);
                row["proposal"] = json!(it.proposal.map(|p| {
                    let diff: Vec<Value> = crate::proposal::diff(r, &adage_app::review::proposed_route(it.inst, p))
                        .into_iter()
                        .map(|(field, pinned, proposed)| json!({ "field": field, "pinned": pinned, "proposed": proposed }))
                        .collect();
                    json!({ "on": p.on, "reason": p.reason, "diff": diff, "directive": adage_app::review::review_preview(it.inst, r).ok().and_then(|v| v.proposal).map(|v| v.directive) })
                }));
                row
            })
            .collect();
        let attestations: Vec<Value> = attested
            .iter()
            .map(|(pack, lines)| json!({ "pack": pack, "attestations": lines, "note": "self-reported by signer; informational only" }))
            .collect();
        let revoked: Vec<Value> = revoked
            .iter()
            .map(|r| json!({ "kind": "exec-revoked", "route": r.route, "repo": r.repo, "level": r.level.as_str(), "on": r.on, "cause": r.cause }))
            .collect();
        println!(
            "{}",
            serde_json::to_string_pretty(
                &json!({ "count": rows.len(), "waiting": items.iter().filter(|it| waits(&it.reasons)).count(),
                         "queue": rows, "inert": inert, "attestations": attestations, "exec_revoked": revoked, "exec_lapsed": lapsed,
                         "exec_last_withheld": adage_exec::last_withheld(&paths.root) })
            )?
        );
        return Ok(());
    }
    let mut o = Out::stdout();
    // What waits for a decision, and what is only listed: instincts in effect, and approved
    // routines that run once delegated. Of what waits: what needs a person, and what is in use
    // and only worth a look (docs/design/everyday-ux.md §5.3).
    let (deciding, listed): (Vec<&Item>, Vec<&Item>) = items.iter().partition(|it| waits(&it.reasons));
    let (needs, look) = waiting_counts(deciding.iter().map(|it| it.reasons.as_slice()));
    let waiting = waiting_words(needs, look).unwrap_or_else(|| "nothing waits for a decision".to_string());
    // Quiet reflexes wait too, but they are not install steps: agents already take their step.
    let quiet_n = deciding.iter().filter(|it| it.reasons.iter().any(|(k, _)| *k == "quiet")).count();
    let quiet_fact = if quiet_n > 0 { format!("{quiet_n} quiet") } else { String::new() };
    let is = |it: &&Item, kind: &str| it.reasons.iter().any(|(k, _)| *k == kind);
    let instincts: Vec<&Item> = listed.iter().copied().filter(|it| is(it, "instinct")).collect();
    let delegable: Vec<&Item> = listed.iter().copied().filter(|it| is(it, "delegable")).collect();
    let instinct_fact = if instincts.is_empty() {
        String::new()
    } else {
        format!("{} in effect", words::count(instincts.len(), "instinct"))
    };
    o.header("review", &[&waiting, &quiet_fact, &instinct_fact])?;
    print_inert(&mut o, &inert)?;
    print_revoked(&mut o, &revoked)?;
    crate::exec::print_lapsed(&mut o, &lapsed)?;
    print_attested(&mut o, &attested)?;
    // Why the last prefetch or action was not delivered (every withheld stage records a reason).
    if let Some(w) = adage_exec::last_withheld(&paths.root) {
        o.blank()?;
        o.note(&format!("Last reflex run by Adage {}", w.line()))?;
    }
    if items.is_empty() {
        o.blank()?;
        return Ok(o.next("adage status", "the reflexes in effect")?);
    }
    // What Adage learned here first: a person's decision matters most there.
    let mut ordered = deciding.clone();
    ordered.sort_by_key(|it| it.route.is_instinct());
    for it in &ordered {
        print_item(&mut o, it, verbose)?;
    }
    if !delegable.is_empty() {
        o.blank()?;
        o.line(
            &Line::new()
                .bold(Role::Ink, "Approved, and run only once delegated")
                .sep()
                .quiet("nothing waits: delegate one to let Adage run it on this machine"),
        )?;
        for it in &delegable {
            print_item(&mut o, it, verbose)?;
        }
    }
    if !instincts.is_empty() {
        o.blank()?;
        o.line(&Line::new().bold(Role::Ink, "Instincts in effect").sep().quiet(
            "Adage's built-in defaults, bound from this repository's files: prime at most, and any conditioned reflex overrides them. Nothing waits: extinguish one to turn it off.",
        ))?;
        // One block per instinct and step; its reflexes differ only in what triggers each.
        let groups = words::group(instincts.iter().copied(), |it| {
            format!(
                "{}\n{}\n{}\n{:?}",
                it.inst.reference,
                origin_label(it.route).unwrap_or_default(),
                describe_step(&it.route.step),
                it.route.state
            )
        });
        for g in &groups {
            print_instinct_group(&mut o, g, verbose)?;
        }
    }
    o.blank()?;
    // Next: the first decision, a learned reflex before an instinct; else the first routine to
    // delegate; else what is in effect.
    match ordered.first() {
        Some(first) if first.reasons.iter().any(|(k, _)| *k == "execution") => o.next(
            &format!("adage review approve {} {}", first.inst.reference, first.route.id),
            "accepts its evidence (it still runs nothing); or another decision listed above",
        )?,
        Some(first) => {
            let why = if first.reasons.iter().any(|(k, _)| *k == "quiet") {
                "delivers this quiet reflex anyway; or another decision listed above"
            } else {
                "or another decision listed above"
            };
            o.next(&format!("adage review approve {} {}", first.inst.reference, first.route.id), why)?
        }
        None => match delegable.first() {
            Some(d) => o.next(
                &format!("adage review delegate {} {} --level e2", d.inst.reference, d.route.id),
                "review the exact command and its limits, from its repository, to let Adage run it",
            )?,
            None => o.next("adage status", "the reflexes in effect")?,
        },
    }
    if verbose {
        return Ok(());
    }
    o.also("adage review --verbose", "the statistics behind each reflex")?;
    Ok(())
}

/// One reflex in the review listing: why it is listed, what it does, its evidence (in plain words;
/// `--verbose` adds the statistics and the graph engine's reasons), and the decisions it takes.
fn print_item<W: std::io::Write>(o: &mut Out<W>, it: &Item, verbose: bool) -> Result<()> {
    let (t, r) = (&it.inst.track, it.route);
    o.blank()?;
    let quiet = it.reasons.iter().any(|(k, _)| *k == "quiet");
    let mut head = Line::new();
    if quiet {
        head = head.glyph(Role::Ink3, Glyph::Quiet).text(" ");
    } else if matches!(r.step, Step::Card { .. }) {
        head = head.glyph(Role::Accent, Glyph::Card).text(" ");
    }
    head = head
        .bold(Role::Ink, it.inst.reference.clone())
        .faint(format!("@{}", t.manifest.track.version))
        .sep()
        .text(r.id.clone())
        .sep()
        .quiet(t.manifest.scope.repo.clone());
    match origin_label(r) {
        Some(l) => head = head.sep().quiet(l),
        None if it.inst.imported => head = head.sep().quiet("imported"),
        None => {}
    }
    o.line(&head)?;
    for (kind, detail) in &it.reasons {
        // Quiet: the one-line reason; the uplift row below says the rest.
        let q = if verbose { crate::evidence::quiet_line(r) } else { crate::evidence::quiet_plain(r) };
        let line = match q.filter(|_| *kind == "quiet") {
            Some(q) => q,
            None => format!("{kind}: {}", words::prose(detail)),
        };
        o.line(&row("why", Line::new().text(line)))?;
    }
    if !matches!(r.trigger, Trigger::Intent { .. }) {
        o.line(&row("when", Line::new().text(words::trigger(&r.trigger))))?;
    }
    let execution = r.evidence.execution.is_some();
    let mut strength =
        if execution { Line::new().text("Execution only") } else { Line::new().strength(words::strength(r.strength)) };
    if r.strength != Strength::Inhibit && !execution {
        strength = strength.quiet(format!(" (ceiling {})", ceiling_str(r)));
    }
    strength = strength.sep().quiet(format!("effect {}", r.effect.as_str())).sep().quiet(words::lifecycle(r.state));
    if r.authored {
        strength = strength.sep().quiet("pinned");
    }
    o.line(&row("strength", strength))?;
    let whys = adage_tracks::why::load(&it.inst.dir);
    let evidence = if verbose {
        crate::evidence::describe(r, whys.get(&r.id))
    } else {
        crate::evidence::plain(r, whys.get(&r.id))
    };
    o.line(&row("evidence", Line::new().text(evidence)))?;
    if let Some(ob) = crate::evidence::obligation(r) {
        o.line(&row("enforced", Line::new().quiet(ob)))?;
    }
    // An execution-only routine is never said to the agent: what it does with and without a grant.
    if let Some((without, with)) = crate::evidence::execution(r) {
        o.line(&row("no grant", Line::new().text(without)))?;
        o.line(&row("with grant", Line::new().quiet(with)))?;
    } else {
        o.line(&row(
            "in use",
            Line::new()
                .text(format!("followed {}", it.followed))
                .sep()
                .text(format!("ignored {}", it.ignored))
                .faint(" (treated traces)"),
        ))?;
    }
    let uplift = if verbose { r.evidence.uplift.as_ref().map(|u| u.line()) } else { crate::evidence::uplift_plain(r) };
    if let Some(u) = uplift {
        o.line(&row(if verbose { "uplift" } else { "unaided" }, Line::new().quiet(u)))?;
    }
    // The graph engine's reasons for a reflex (Track G §6.3): for people, never the agent.
    if let Some(w) = whys.get(&r.id).filter(|_| verbose) {
        for (i, line) in w.lines().iter().enumerate() {
            o.line(&row(if i == 0 { "reflex" } else { "" }, Line::new().quiet(line.to_string())))?;
        }
    }
    o.line(&row("step", Line::new().text(describe_step(&r.step))))?;
    if let Some(e) = &r.evidence.execution {
        let text = if verbose {
            format!(
                "{} / {} matching situations supported by a compatible successful continuation before further mutation; {} sessions",
                e.successful, e.opportunities, e.sessions
            )
        } else {
            format!(
                "{} / {} times it came up, the agent then ran it successfully before changing anything else ({})",
                e.successful,
                e.opportunities,
                words::count(e.sessions as usize, "trace")
            )
        };
        o.line(&row("CPU evidence", Line::new().text(text)))?;
        o.line(&row(
            "delegate",
            Line::new().cmd(format!("adage review delegate {} {} --level e2", it.inst.reference, r.id)),
        ))?;
    }
    let (directive, examples) = preview(t, r);
    let ex = if examples.is_empty() {
        String::new()
    } else {
        format!("example values: {}", examples.iter().map(|(k, v)| format!("{k} = {v}")).collect::<Vec<_>>().join(", "))
    };
    if r.evidence.execution.is_none() {
        o.line(&row("directive", Line::new().faint(ex)))?;
        for line in directive.lines() {
            o.line(&Line::new().text("    ").quiet(line))?;
        }
    }
    if let Some(p) = it.proposal {
        o.line(&row(
            "proposed",
            Line::new().text("`adage review approve` applies these changes (the reflex stays pinned):"),
        ))?;
        for (field, pinned, proposed) in crate::proposal::diff(r, &adage_app::review::proposed_route(it.inst, p)) {
            o.line(&Line::new().text("    ").push(Role::Inhibit, format!("- {field:<9} {pinned}")))?;
            o.line(&Line::new().text("    ").push(Role::Fire, format!("+ {field:<9} {proposed}")))?;
        }
        if let Some(proposed) = adage_app::review::review_preview(it.inst, r)?.proposal {
            o.line(&row("proposed response", Line::new()))?;
            for line in proposed.directive.lines() {
                o.line(&Line::new().text("    ").quiet(line))?;
            }
        }
    }
    let decide = verbs(r, it.proposal.is_some()).join("|");
    o.line(&row("decide", Line::new().cmd(format!("adage review <{decide}> {} {}", it.inst.reference, r.id))))?;
    Ok(())
}

/// Instinct reflexes that do the same thing (one instinct, one step), once: what each answers
/// (its trigger) with its id, one directive as an example, and the decisions they take.
fn print_instinct_group<W: std::io::Write>(o: &mut Out<W>, g: &[&Item], verbose: bool) -> Result<()> {
    let (first, r) = (g[0], g[0].route);
    let t = &first.inst.track;
    o.blank()?;
    let mut head = Line::new()
        .bold(Role::Ink, first.inst.reference.clone())
        .faint(format!("@{}", t.manifest.track.version))
        .sep()
        .text(origin_label(r).unwrap_or_else(|| "instinct".into()))
        .sep()
        .text(describe_step(&r.step));
    if g.len() > 1 {
        head = head.sep().quiet(words::count(g.len(), "reflex"));
    }
    o.line(&head)?;
    let id_w = g.iter().map(|it| it.route.id.chars().count()).max().unwrap_or(0) + 2;
    for (i, it) in g.iter().enumerate() {
        let when = Line::new().text(format!("{:<id_w$}", it.route.id)).quiet(words::trigger(&it.route.trigger));
        o.line(&row(if i == 0 { "when" } else { "" }, when))?;
    }
    let judged: (u32, u32) = g.iter().fold((0, 0), |(f, i), it| (f + it.followed, i + it.ignored));
    o.line(&row(
        "strength",
        Line::new()
            .strength(words::strength(r.strength))
            .sep()
            .quiet(format!("effect {}", r.effect.as_str()))
            .sep()
            .quiet(words::lifecycle(r.state)),
    ))?;
    o.line(&row(
        "in use",
        Line::new()
            .text(format!("followed {}", judged.0))
            .sep()
            .text(format!("ignored {}", judged.1))
            .faint(" (treated traces)"),
    ))?;
    for it in g {
        let u = if verbose {
            it.route.evidence.uplift.as_ref().map(|u| u.line())
        } else {
            crate::evidence::uplift_plain(it.route)
        };
        if let Some(u) = u {
            let key = if verbose { "uplift" } else { "unaided" };
            o.line(&row(key, Line::new().quiet(format!("{}: {u}", it.route.id))))?;
        }
    }
    let (directive, _) = preview(t, r);
    o.line(&row("directive", Line::new().faint(if g.len() > 1 { "each says, for example:" } else { "" })))?;
    for line in directive.lines() {
        o.line(&Line::new().text("    ").quiet(line))?;
    }
    let decide = verbs(r, false).join("|");
    let reflex = if g.len() > 1 { "<reflex>" } else { r.id.as_str() };
    o.line(&row("decide", Line::new().cmd(format!("adage review <{decide}> {} {reflex}", first.inst.reference))))?;
    Ok(())
}

/// A row under a listed reflex: `  why         new: …`.
fn row(key: &str, value: Line) -> Line {
    Line::new().text("  ").faint(key).pad_to(2 + KEY_WIDTH).extend(value)
}

/// Each installed pack's attestations (Track C §1.8), one line each, marked self-reported by its
/// signer.
fn attestations(paths: &Paths) -> Vec<(String, Vec<String>)> {
    let mut packs: Vec<String> = crate::packs::bindings(paths).into_iter().map(|(_, b)| b.pack).collect();
    packs.sort();
    packs.dedup();
    packs
        .into_iter()
        .map(|p| {
            let lines = crate::packs::attestation_lines(paths, &p);
            (p, lines)
        })
        .filter(|(_, l)| !l.is_empty())
        .collect()
}

fn print_attested<W: std::io::Write>(o: &mut Out<W>, attested: &[(String, Vec<String>)]) -> Result<()> {
    for (pack, lines) in attested {
        o.blank()?;
        o.line(
            &Line::new()
                .bold(Role::Ink, pack.clone())
                .text(format!(" carries {}, self-reported by its signer", words::count(lines.len(), "attestation"))),
        )?;
        for l in lines {
            o.line(&row("attested", Line::new().quiet(l.clone())))?;
        }
        o.line(&row(
            "note",
            Line::new()
                .faint("attestations are informational only: they never set a reflex's strength or an execution level"),
        ))?;
    }
    Ok(())
}

/// Execution grants Adage withdrew by itself (Track C §3.6): losing authority is automatic,
/// regaining it never is.
fn print_revoked<W: std::io::Write>(o: &mut Out<W>, revoked: &[adage_exec::approvals::Revocation]) -> Result<()> {
    if revoked.is_empty() {
        return Ok(());
    }
    o.blank()?;
    o.warn(&format!(
        "{} automatically: losing authority is automatic, regaining it never is",
        if revoked.len() == 1 {
            "1 execution grant was revoked".to_string()
        } else {
            format!("{} execution grants were revoked", revoked.len())
        }
    ))?;
    for r in revoked {
        let (track, route) = r.route.rsplit_once('/').unwrap_or((&r.route, ""));
        o.blank()?;
        o.line(
            &Line::new()
                .bold(Role::Ink, r.route.clone())
                .sep()
                .text(r.level.as_str().to_uppercase())
                .sep()
                .quiet(r.repo.clone()),
        )?;
        o.line(&row("why", Line::new().text(format!("exec-revoked: {}", r.cause)).faint(format!(" (on {})", r.on))))?;
        o.line(&row(
            "decide",
            Line::new()
                .cmd(format!("adage review delegate {track} {route} --level {}", r.level.as_str()))
                .faint(" (after fixing what it changes)"),
        ))?;
    }
    Ok(())
}

/// Reflexes of installed packs that are inert here (Track C §2.3): never delivered, and no decision
/// changes that; restoring what they need (a script, a tool) and running `adage add` again does.
fn print_inert<W: std::io::Write>(o: &mut Out<W>, inert: &[crate::packs::InertRoute]) -> Result<()> {
    if inert.is_empty() {
        return Ok(());
    }
    o.blank()?;
    o.warn(&format!(
        "{} inert here: never delivered until what they need is restored and `adage add` runs again",
        words::count(inert.len(), "pack reflex")
    ))?;
    for i in inert {
        o.blank()?;
        o.line(
            &Line::new()
                .bold(Role::Ink, i.pack.clone())
                .faint(format!("@{}", i.version))
                .sep()
                .text(i.route.clone())
                .sep()
                .quiet(i.repo.clone())
                .sep()
                .faint(format!("pack, sha256 {}", &i.digest[..i.digest.len().min(16)])),
        )?;
        o.line(&row("why", Line::new().text(format!("inert: {}", i.reason))))?;
    }
    Ok(())
}

/// What `adage review` has, by what it asks of a person: `1 needs you · 3 to look over`
/// (docs/design/everyday-ux.md §5.3). `None` when there is nothing.
pub(crate) fn waiting_words(needs: usize, look: usize) -> Option<String> {
    let mut parts = Vec::new();
    if needs > 0 {
        parts.push(format!("{needs} need{} you", if needs == 1 { "s" } else { "" }));
    }
    if look > 0 {
        parts.push(format!("{look} to look over"));
    }
    (!parts.is_empty()).then(|| parts.join(" · "))
}

/// The person check at the sites that take a press (mod architecture §3.12; the list is
/// [`PersonVerb`]): stdin is read once, a valid press stands in for the
/// terminal, and the next token of the chain is printed at once, as the first line on stdout,
/// since the one just spent is refused from now on, whatever the verb goes on to do. Every other
/// person-only command keeps [`crate::share::require_person`], which never reads a press.
pub(crate) fn person_or_press(
    verb: PersonVerb,
    paths: &Paths,
) -> Result<(adage_app::Person, adage_app::person::Stdin)> {
    person_from(verb, paths, adage_app::person::read_press())
}

/// [`person_or_press`] over a stdin already read.
pub(crate) fn person_from(
    verb: PersonVerb,
    paths: &Paths,
    stdin: adage_app::person::Stdin,
) -> Result<(adage_app::Person, adage_app::person::Stdin)> {
    let person = adage_app::person::require_or_press(verb, stdin.press.as_ref(), paths)?;
    if let Some(next) = person.next() {
        use std::io::Write;
        let mut out = std::io::stdout().lock();
        writeln!(out, "{}", json!({ "next": next }))?;
        out.flush()?;
    }
    Ok((person, stdin))
}

fn decide(verb: &str, args: &[String], paths: &Paths) -> Result<()> {
    let Some(verb) = Verb::parse(verb) else { bail!("{USAGE}") };
    let (person, _) = person_or_press(PersonVerb::Review(verb), paths)?;
    let pos: Vec<&String> = {
        let mut v = Vec::new();
        let mut i = 0;
        while i < args.len() {
            if args[i] == "--by" {
                i += 2;
                continue;
            }
            v.push(&args[i]);
            i += 1;
        }
        v
    };
    let (Some(spec), Some(route_id), None) = (pos.first(), pos.get(1), pos.get(2)) else { bail!("{USAGE}") };
    let by = crate::helpers::flag(args, "--by");
    let d = report(adage_app::review::decide(&person, verb, spec, route_id, by, paths)?);
    let mut o = Out::stdout();
    let facts: Vec<&str> = d.facts.iter().map(String::as_str).collect();
    o.header(&format!("review {}", verb.as_str()), &facts)?;
    o.blank()?;
    for (k, v) in d.rows {
        o.kv(k, v)?;
    }
    o.blank()?;
    // What waits now, after this decision.
    let all = discover(&paths.tracks())?;
    let index = Index::load(&paths.index());
    let left = queue(&all, &judgements(paths), &index, paths, here().as_deref())
        .iter()
        .filter(|it| waits(&it.reasons))
        .count();
    if left > 0 {
        let why = if left == 1 { "1 reflex still waits".to_string() } else { format!("{left} reflexes still wait") };
        o.next("adage review", &why)?;
    } else {
        o.next("adage status", "the reflexes in effect")?;
    }
    Ok(())
}

/// A decision from `adage ui`: the same guard and the same code path as `adage review <verb>`.
pub(crate) fn decide_quietly(
    verb: &str,
    spec: &str,
    route_id: &str,
    expected_revision: &str,
    paths: &Paths,
) -> Result<Outcome> {
    // `adage ui` owns the terminal, so no press can reach it on stdin: the check is the terminal's.
    let Some(verb) = Verb::parse(verb) else { bail!("{USAGE}") };
    let person = adage_app::person::require_or_press(PersonVerb::Review(verb), None, paths)?;
    // `adage ui` decides on its event loop: it waits a moment for a running learner, never longer.
    let wait = std::time::Duration::from_secs(2);
    adage_app::review::decide_reviewed_within(&person, verb, spec, route_id, expected_revision, paths, wait)
}

/// What a decision did: the header's facts and the rows under it.
pub(crate) struct Decided {
    pub(crate) facts: Vec<String>,
    pub(crate) rows: Vec<(&'static str, Line)>,
}

/// A decision's outcome as `adage review <verb>` prints it.
fn report(o: Outcome) -> Decided {
    for w in &o.warnings {
        crate::ui::warn(w.clone());
    }
    let (before, after) = (&o.before, &o.after);
    let mut facts = vec![o.reference.clone(), o.route.clone()];
    if o.verb == Verb::Dismiss {
        facts.push("proposed change dismissed".into());
        return Decided {
            facts,
            rows: vec![
                (
                    "kept",
                    Line::new().text(
                        "the pinned reflex as it is; conditioning proposes again only if its version changes materially",
                    ),
                ),
                ("recorded", Line::new().text(o.recorded.clone())),
            ],
        };
    }
    match o.already {
        _ if o.proposal_applied => facts.push("proposed change applied; it stays pinned".into()),
        Some(Already::Extinguished) => facts.push("already extinguished".into()),
        Some(Already::Approved) => facts.push("already approved; nothing changed".into()),
        None => {}
    }
    let mut rows: Vec<(&'static str, Line)> = Vec::new();
    if o.proposal_applied {
        let (a, b) = (describe_step(&before.step), describe_step(&after.step));
        rows.push((
            "step",
            if a == b {
                Line::new().text(a)
            } else {
                Line::new().quiet(a).text(" ").glyph(Role::Ink3, Glyph::Arrow).text(" ").text(b)
            },
        ));
    }
    let mut strength = Line::new().strength(words::strength(before.strength));
    if after.strength != before.strength {
        strength =
            strength.text(" ").glyph(Role::Ink3, Glyph::Arrow).text(" ").strength(words::strength(after.strength));
    }
    rows.push(("strength", strength));
    let lifecycle = if before.state == after.state {
        Line::new().text(words::lifecycle(after.state))
    } else {
        Line::new()
            .quiet(words::lifecycle(before.state))
            .text(" ")
            .glyph(Role::Ink3, Glyph::Arrow)
            .text(" ")
            .text(words::lifecycle(after.state))
    };
    rows.push(("lifecycle", lifecycle));
    if after.evidence.execution.is_some() {
        rows.push((
            "execution",
            Line::new().text("Evidence decision only; execution still requires a valid local E2 grant"),
        ));
        rows.push((
            "delegate",
            Line::new().cmd(format!("adage review delegate {} {} --level e2", o.reference, o.route)),
        ));
    }
    if after.authored != before.authored {
        rows.push(("pinned", Line::new().text("yes: `adage condition` will not overwrite it")));
    }
    if let Some((pack, others)) = o.pack_approved.split_first() {
        let also =
            if others.is_empty() { String::new() } else { format!("; also in service now: {}", others.join(", ")) };
        rows.push(("approved", Line::new().text(format!("{pack}'s environment steps for {}{also}", o.repo))));
    }
    rows.push(("track", Line::new().text(o.track.clone())));
    rows.push(("recorded", Line::new().text(o.recorded.clone())));
    if let Some(ix) = &o.index {
        rows.push((
            "index",
            Line::new()
                .text(format!("gen {}", &ix.generation[..ix.generation.len().min(8)]))
                .sep()
                .quiet(words::count(ix.reflexes, "reflex")),
        ));
    }
    Decided { facts, rows }
}

/// The checkout the person is in, if any: execution grants are per checkout.
fn here() -> Option<PathBuf> {
    std::env::current_dir().ok().and_then(|p| adage_core::repo::find_root(&p))
}

/// Every installed track (`adage_app::review::discover`), warning about each one skipped.
pub(crate) fn discover(root: &Path) -> Result<Vec<Installed>> {
    let (all, warnings) = adage_app::review::discover(root)?;
    for w in warnings {
        crate::ui::warn(w);
    }
    Ok(all)
}

/// What `adage ui` shows beside each installed route (`adage_app::review::standings`), for the
/// checkout the person is in.
pub(crate) fn standings(paths: &Paths) -> Result<BTreeMap<(String, String), Standing>> {
    adage_app::review::standings(paths, here().as_deref())
}
