//! T-W1-01 to T-W1-06: the resolver over table-driven worlds (`cases/resolve/*.json`), the
//! no-signal set, determinism under shuffled inputs, the classifier, the scheduler and the
//! budgets. Embeddings come from the hashed fallback, which every binary has, so the cases hold
//! without a model on disk.

use adage_intent::model::hashed_embed;
use adage_intent::normalize::normalize;
use adage_intent::quantize;
use adage_trace::{Artifact, ArtifactKind, CalendarBlock, WorldView};
use adage_world::text::tokens;
use adage_world::{classify, render, resolve, schedule, Decision, Liveness, Params, Ttl, WorldCtx};
use serde::Deserialize;
use std::path::Path;

const MIN: u64 = 60_000;

#[derive(Deserialize)]
struct World {
    title: String,
    now: u64,
    #[serde(default)]
    tz_offset_min: i32,
    artifacts: Vec<ArtifactCase>,
    #[serde(default)]
    blocks: Vec<BlockCase>,
    prompts: Vec<PromptCase>,
}

#[derive(Deserialize)]
struct ArtifactCase {
    id: String,
    kind: ArtifactKind,
    title: String,
    locator: String,
    #[serde(default)]
    summary: Option<String>,
    ago_min: u64,
    #[serde(default)]
    state: Option<String>,
    #[serde(default)]
    people: Vec<String>,
    #[serde(default)]
    related: Vec<String>,
}

#[derive(Deserialize)]
struct BlockCase {
    id: String,
    title: String,
    start_min_ago: i64,
    end_min_ago: i64,
    busy: bool,
    attendees: u32,
}

#[derive(Deserialize)]
struct PromptCase {
    prompt: String,
    expect: String,
    #[serde(default)]
    artifact: Option<String>,
    #[serde(default)]
    any_of: Vec<String>,
}

fn embed(text: &str) -> Option<Vec<f32>> {
    hashed_embed(&normalize(text))
}

fn artifact(c: &ArtifactCase, now: u64) -> Artifact {
    let mut a = Artifact {
        id: c.id.clone(),
        kind: c.kind,
        title: c.title.clone(),
        locator: c.locator.clone(),
        summary: c.summary.clone(),
        embedding: None,
        first_seen: now - c.ago_min * MIN,
        last_touched: now - c.ago_min * MIN,
        related: c.related.clone(),
        state: c.state.clone(),
        people: c.people.clone(),
    };
    a.embedding = embed(&a.text()).map(|v| quantize(&v));
    a
}

fn block(c: &BlockCase, now: u64) -> CalendarBlock {
    let at = |m: i64| (now as i64 - m * MIN as i64) as u64;
    CalendarBlock {
        id: c.id.clone(),
        start: at(c.start_min_ago),
        end: at(c.end_min_ago),
        title: c.title.clone(),
        busy: c.busy,
        accepted: true,
        focus: false,
        attendees: c.attendees,
    }
}

fn worlds() -> Vec<(String, World)> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/cases/resolve");
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "json"))
        .collect();
    files.sort();
    files
        .into_iter()
        .map(|p| {
            let w: World = serde_json::from_str(&std::fs::read_to_string(&p).unwrap())
                .unwrap_or_else(|e| panic!("{}: {e}", p.display()));
            (p.file_name().unwrap().to_string_lossy().into_owned(), w)
        })
        .collect()
}

fn ctx<'a>(w: &World, artifacts: &'a [Artifact], blocks: &'a [CalendarBlock], params: &'a Params) -> WorldCtx<'a> {
    WorldCtx { now: w.now, tz_offset_min: w.tz_offset_min, artifacts, events: &[], blocks, embed: &embed, params }
}

/// T-W1-01, T-W1-02: every prompt of every world resolves as its case expects; the hint names
/// the expected artifact first.
#[test]
fn every_case_resolves_as_expected() {
    let params = Params::default();
    let mut failures = Vec::new();
    for (file, w) in worlds() {
        let artifacts: Vec<Artifact> = w.artifacts.iter().map(|a| artifact(a, w.now)).collect();
        let blocks: Vec<CalendarBlock> = w.blocks.iter().map(|b| block(b, w.now)).collect();
        let c = ctx(&w, &artifacts, &blocks, &params);
        for p in &w.prompts {
            let d = resolve(&c, &p.prompt);
            let got = match &d {
                Decision::Attach(_) => "attach",
                Decision::Disambiguate(_) => "disambiguate",
                Decision::None { .. } => "none",
            };
            let mut ok = got == p.expect;
            if let Some(h) = d.hint() {
                if let Some(want) = &p.artifact {
                    ok &= h.artifacts.first() == Some(want);
                }
                if !p.any_of.is_empty() {
                    ok &= h.artifacts.iter().any(|a| p.any_of.contains(a));
                }
                ok &= tokens(&h.text) <= params.hint_tokens;
                ok &= h.text.contains("from Adage, not from the user");
            }
            if !ok {
                let detail = match &d {
                    Decision::None { reason } => reason.to_string(),
                    _ => d
                        .hint()
                        .map(|h| {
                            format!(
                                "{:?} top scores {:?}",
                                h.artifacts,
                                h.scored
                                    .iter()
                                    .take(3)
                                    .map(|s| (s.id.clone(), (s.score * 100.0).round() / 100.0))
                                    .collect::<Vec<_>>()
                            )
                        })
                        .unwrap_or_default(),
                };
                failures
                    .push(format!("{file} ({}): {:?} expected {} got {got}: {detail}", w.title, p.prompt, p.expect));
            }
        }
    }
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
}

/// T-W1-03: shuffled artifacts and blocks give the same decision and the same text.
#[test]
fn the_decision_does_not_depend_on_input_order() {
    let params = Params::default();
    for (_, w) in worlds() {
        let mut artifacts: Vec<Artifact> = w.artifacts.iter().map(|a| artifact(a, w.now)).collect();
        let mut blocks: Vec<CalendarBlock> = w.blocks.iter().map(|b| block(b, w.now)).collect();
        let before: Vec<Decision> = {
            let c = ctx(&w, &artifacts, &blocks, &params);
            w.prompts.iter().map(|p| resolve(&c, &p.prompt)).collect()
        };
        artifacts.reverse();
        blocks.reverse();
        let c = ctx(&w, &artifacts, &blocks, &params);
        let after: Vec<Decision> = w.prompts.iter().map(|p| resolve(&c, &p.prompt)).collect();
        assert_eq!(before, after, "{}", w.title);
    }
}

fn pr(id: &str, title: &str, state: &str, now: u64) -> Artifact {
    Artifact {
        id: id.into(),
        kind: ArtifactKind::Pr,
        title: title.into(),
        locator: format!("#{}", id.rsplit(':').next().unwrap()),
        summary: None,
        embedding: None,
        first_seen: now - 60 * MIN,
        last_touched: now - 5 * MIN,
        related: vec![],
        state: Some(state.into()),
        people: vec![],
    }
}

fn cal(id: &str, start: u64, end: u64, focus: bool) -> CalendarBlock {
    CalendarBlock { id: id.into(), start, end, title: id.into(), busy: true, accepted: true, focus, attendees: 3 }
}

/// T-W1-04: CI red on the session's branch is an interrupt; red elsewhere, a moved ticket and a
/// new pull request are deltas in one block; a meeting in four minutes is an interrupt; a focus
/// block holds interrupts back; the per-session cap holds.
#[test]
fn changes_are_classified_by_focus_and_urgency() {
    let now = 1_792_000_000_000;
    let params = Params::default();
    let prev = WorldView {
        ts: now - 10 * MIN,
        states: vec![
            ("github:pr:412".into(), "ci:green".into()),
            ("github:pr:418".into(), "ci:green".into()),
            ("jira:MKT-1182".into(), "open".into()),
        ],
        ..WorldView::default()
    };
    let mut ticket = pr("jira:MKT-1182", "Tracking spec", "in_review", now);
    ticket.kind = ArtifactKind::Ticket;
    let artifacts = vec![
        pr("github:pr:412", "Schema evolution", "ci:red", now),
        pr("github:pr:418", "Retry budget", "ci:red", now),
        pr("github:pr:420", "New thing", "open", now),
        ticket,
    ];
    let blocks = vec![cal("calendar:soon", now + 4 * MIN, now + 30 * MIN, false)];
    let embed = |_: &str| None;
    let c = WorldCtx {
        now,
        tz_offset_min: 0,
        artifacts: &artifacts,
        events: &[],
        blocks: &blocks,
        embed: &embed,
        params: &params,
    };
    let out = classify(&c, &prev, &["github:pr:412".to_string()]);
    assert_eq!(out.interrupts.len(), 2, "{:?}", out.changes);
    assert!(out.interrupts[0].contains("#412") && out.interrupts[0].contains("ci red"), "{}", out.interrupts[0]);
    assert!(out.interrupts[1].contains("Meeting starts in 4 min"), "{}", out.interrupts[1]);
    let delta = out.delta.as_deref().unwrap();
    assert!(delta.contains("#418") && delta.contains("new pull request #420") && delta.contains("MKT-1182"), "{delta}");
    assert!(delta.starts_with("<adage-world kind=\"delta\">") && delta.ends_with("</adage-world>"));
    assert!(tokens(delta) <= params.delta_tokens);
    assert_eq!(out.next.interrupts, 2);
    assert_eq!(out.next.states.len(), 4);
    // The same world seen again changes nothing.
    let again = classify(&c, &out.next, &["github:pr:412".to_string()]);
    assert!(again.delta.is_none() && again.interrupts.is_empty(), "{:?}", again.changes);
    // Inside a focus block every urgent change becomes a delta.
    let focus_blocks = vec![
        cal("calendar:focus", now - MIN, now + 30 * MIN, true),
        cal("calendar:soon", now + 4 * MIN, now + 30 * MIN, false),
    ];
    let c2 = WorldCtx { blocks: &focus_blocks, ..c };
    let held = classify(&c2, &prev, &["github:pr:412".to_string()]);
    assert!(held.interrupts.is_empty() && held.delta.as_deref().unwrap().contains("#412"));
    // The cap: a session that had its three gets deltas.
    let capped = classify(&c, &WorldView { interrupts: 3, ..prev.clone() }, &["github:pr:412".to_string()]);
    assert!(capped.interrupts.is_empty());
}

/// T-W1-05: the TTL flips across a predicted gap on a heavy session only; a long block gets the
/// long-block note; an idle session inside a block gets the autonomy text once and a finalize
/// time three minutes before the end; the person speaking reverts the TTL; two briefs and a free
/// quarter hour propose a review slot.
#[test]
fn the_scheduler_follows_the_calendar() {
    let now = 1_792_000_000_000;
    let params = Params::default();
    let embed = |_: &str| None;
    let soon = vec![cal("calendar:soon", now + 8 * MIN, now + 38 * MIN, false)];
    let c =
        WorldCtx { now, tz_offset_min: 0, artifacts: &[], events: &[], blocks: &soon, embed: &embed, params: &params };
    let heavy = Liveness {
        idle_since: Some(now),
        cached_tokens: 60_000,
        brief_path: ".adage/review-s.md".into(),
        ..Liveness::default()
    };
    assert_eq!(schedule(&c, &heavy).ttl, Some(Ttl::OneHour));
    let light = Liveness { cached_tokens: 20_000, ..heavy.clone() };
    assert_eq!(schedule(&c, &light).ttl, None);
    let long = vec![cal("calendar:long", now + 8 * MIN, now + 98 * MIN, false)];
    let cl = WorldCtx { blocks: &long, ..c };
    let plan = schedule(&cl, &heavy);
    assert!(plan.long_block && plan.ttl.is_none());
    let inside = vec![cal("calendar:now", now - 2 * MIN, now + 28 * MIN, false)];
    let ci = WorldCtx { blocks: &inside, ..c };
    let plan = schedule(&ci, &heavy);
    let a = plan.autonomy.as_ref().expect("an autonomy message");
    assert!(
        a.text.contains(&format!("unavailable until {}", adage_world::text::hhmm(now + 28 * MIN, 0)))
            && a.text.contains(".adage/review-s.md"),
        "{}",
        a.text
    );
    assert_eq!(plan.finalize_at.as_ref().map(|f| f.0), Some(now + 25 * MIN));
    let sent = Liveness { autonomy_sent_for: Some("calendar:now".into()), ..heavy.clone() };
    assert!(schedule(&ci, &sent).autonomy.is_none(), "once per block");
    let spoke = Liveness { idle_since: None, ..heavy.clone() };
    assert_eq!(schedule(&c, &spoke).ttl, Some(Ttl::FiveMinutes));
    let briefs = Liveness { briefs_pending: 2, ..heavy.clone() };
    let review = schedule(&c, &briefs).review.expect("a slot after the block: eight minutes before it is too short");
    assert_eq!((review.start, review.end), (now + 38 * MIN, now + 53 * MIN));
    assert_eq!(review.title, "Agent review: 2 briefs");
    let packed =
        vec![cal("calendar:a", now, now + 60 * MIN, false), cal("calendar:b", now + 60 * MIN, now + 125 * MIN, false)];
    let cp = WorldCtx { blocks: &packed, ..c };
    assert!(schedule(&cp, &briefs).review.is_none(), "no quarter hour free in the next two hours");
}

/// T-W1-06: every rendered text is inside its budget, closes its tag and says who it is from.
#[test]
fn every_text_fits_its_budget() {
    let now = 1_792_000_000_000;
    let params = Params::default();
    let embed = |_: &str| None;
    let artifacts: Vec<Artifact> =
        (0..40).map(|i| pr(&format!("github:pr:{i}"), &"a long title ".repeat(8), "ci:green", now)).collect();
    let blocks: Vec<CalendarBlock> =
        (0..30).map(|i| cal(&format!("calendar:{i}"), now + i * 10 * MIN, now + (i + 1) * 10 * MIN, false)).collect();
    let c = WorldCtx {
        now,
        tz_offset_min: 0,
        artifacts: &artifacts,
        events: &[],
        blocks: &blocks,
        embed: &embed,
        params: &params,
    };
    let snap = render::snapshot(&c);
    assert!(
        tokens(&snap) <= params.snapshot_tokens && snap.ends_with("</adage-world>") && snap.contains("30 meetings"),
        "{snap}"
    );
    let big: Vec<String> =
        (0..200).map(|i| format!("pull request #{i} \"{}\" is now ci red", "x".repeat(40))).collect();
    let delta = render::delta(&c, &big);
    assert!(tokens(&delta) <= params.delta_tokens && delta.ends_with("</adage-world>"));
    let interrupt = render::interrupt(&c, &"word ".repeat(400));
    assert!(tokens(&interrupt) <= params.interrupt_tokens && interrupt.ends_with("</adage-world>"));
    assert!(
        snap.contains("from Adage, not from the user")
            && delta.contains("from Adage")
            && interrupt.contains("from Adage")
    );
}
