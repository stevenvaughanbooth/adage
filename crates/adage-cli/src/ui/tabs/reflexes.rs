//! The Reflexes tab: every reflex by track, then the ones waiting for review (install steps), the
//! quiet ones (uplift below the bar: agents already do this, so they wait for a person), the
//! instincts and the extinguished; filter chips for strength and lifecycle; a fuzzy `/` filter;
//! and a card for the selection.
//!
//! A row reads as the reflex does, trigger → action: when it fires (dim), what it has the agent
//! do (a command in the command colour), and how much evidence it rests on as a small meter
//! (`▰▰▰▰▱▱ 22/35`), with its lifecycle only while it is not yet firm. Groups are named in plain
//! words (`Learned here · v17.0.12`, `Instincts · v3.0.0`, `From pack · work-items`), never by a
//! track's directory.
//!
//! The card, in sections: the reflex in plain words (its gist, never its id) with its strength and
//! what it may do (suggest, or run with your grant); the arc it learned, drawn as the Map draws it
//! (`when ──▶ ◠ ──▶ do`); the evidence as meters (the traces that took it, the confidence that
//! rests on, what has happened since it was installed, how much telling the agent changes); what
//! the agent sees, quoted (`r` shows the wrapper as delivered); a procedure card's contents; the
//! review actions; and the details a command needs (id, track, thresholds, the `adage review`
//! command each action equals), closed until enter opens them.
//!
//! When the selection changes, the card's meters fill from empty over [`FILL_MS`] (not under
//! `ADAGE_REDUCED_MOTION`, and not in tests, which compare the settled frame).

use super::empty;
use crate::ui::app::{Action, AppState, Cmd, Modal, WantKind};
use crate::ui::feed::{self, Act};
use crate::ui::fuzzy;
use crate::ui::kit::{self, KeyValue, Section, Tone};
use crate::ui::routes::{RouteRow, ACTIONS};
use crate::ui::scope;
use crate::ui::theme::Theme;
use crate::ui::view::{self, Ctx, NBSP};
use crate::ui::words;
use adage_term::Glyph;
use adage_tracks::Strength;
use ratatui::crossterm::event::KeyCode;
use ratatui::layout::{Constraint, Layout, Margin, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Wrap};
use ratatui::Frame;
use std::collections::BTreeSet;
use std::time::{Duration, Instant};

/// The lifecycle filter's choices, in the order `S` cycles them.
pub const STATES: [&str; 7] = ["all states", "firm", "forming", "nascent", "quiet", "dormant", "extinguished"];

/// How long the card's meters take to fill after the selection changes.
pub const FILL_MS: u64 = 200;

/// The frame interval while they fill.
pub const FRAME: Duration = Duration::from_millis(30);

#[derive(Debug, Default)]
pub struct State {
    /// The selected node's key: a group's, or a reflex's (`reference\0id`).
    pub sel: Option<String>,
    /// Groups opened or closed against their default.
    pub toggled: BTreeSet<String>,
    pub filter: String,
    pub filtering: bool,
    /// Strengths hidden by the chips: fire, prime, inhibit.
    pub hidden: [bool; 3],
    /// Index into [`STATES`].
    pub states: usize,
    pub detail_scroll: u16,
    /// The card's Details section is open (`v`). It stays open from reflex to reflex.
    pub details: bool,
    /// What the agent sees, exactly as delivered: the wrapper's tags too (`r`).
    pub raw: bool,
    /// When the selection last changed: the card's meters fill from then.
    pub moved_at: Option<Instant>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Node {
    Group {
        key: String,
        title: String,
        sub: String,
        count: usize,
        open: bool,
    },
    /// An index into the rows.
    Reflex(usize),
}

fn reflex_key(r: &RouteRow) -> String {
    format!("{}\u{0}{}", r.reference, r.id)
}

fn group_of(r: &RouteRow) -> String {
    if r.extinguished() {
        "extinguished".into()
    } else if r.quiet() {
        "quiet".into()
    } else if r.nascent() {
        "nascent".into()
    } else if r.instinct {
        "instincts".into()
    } else {
        format!("track:{}", r.reference)
    }
}

fn default_open(group: &str) -> bool {
    group != "extinguished"
}

fn strength_index(a: Strength) -> usize {
    match a {
        Strength::Fire => 0,
        Strength::Prime => 1,
        Strength::Inhibit => 2,
    }
}

/// Whether a row passes the chips and the filter.
fn passes(st: &State, r: &RouteRow) -> bool {
    !st.hidden[strength_index(r.strength)]
        && (st.states == 0 || r.stage() == STATES[st.states])
        && matches_filter(&st.filter, &r.id, &r.haystack())
}

/// The `/` filter: fuzzy on the reflex id (`npmchk` finds `start-with-npm-check`), a plain
/// substring on everything else it carries (`npm run` finds the reflex that says it). Fuzzy
/// matching across long directive text would match almost anything.
pub fn matches_filter(filter: &str, id: &str, haystack: &str) -> bool {
    filter.is_empty()
        || fuzzy::score(filter, id).is_some()
        || haystack.to_lowercase().contains(&filter.trim().to_lowercase())
}

/// The tree as shown: groups (open or closed) and the reflexes of open ones.
pub fn tree(s: &AppState) -> Vec<Node> {
    let st = &s.reflexes;
    let rows = s.route_rows();
    let mut order: Vec<String> = Vec::new();
    for r in rows.iter().filter(|r| s.in_scope(r)) {
        let g = group_of(r);
        if !order.contains(&g) {
            order.push(g);
        }
    }
    let rank = |g: &str| match g {
        "nascent" => 1,
        "quiet" => 2,
        "instincts" => 3,
        "extinguished" => 4,
        _ => 0,
    };
    order.sort_by_key(|g| rank(g));
    let mut out = Vec::new();
    for g in order {
        let members: Vec<usize> = (0..rows.len())
            .filter(|&i| group_of(&rows[i]) == g && s.in_scope(&rows[i]) && passes(st, &rows[i]))
            .collect();
        if members.is_empty() {
            continue;
        }
        let first = &rows[members[0]];
        // Plain words, the version beside them; a track's directory (what `adage review` takes)
        // is in the group's card, and each reflex's Details.
        let (title, sub) = match g.as_str() {
            "nascent" => ("Waiting for review".to_string(), String::new()),
            "quiet" => ("Quiet".to_string(), " · agents already do this; approve to deliver".to_string()),
            "instincts" => {
                let versions: BTreeSet<&str> = members.iter().map(|&i| rows[i].version.as_str()).collect();
                let sub = match versions.iter().next() {
                    Some(v) if versions.len() == 1 && !v.is_empty() => format!(" · v{v}"),
                    _ => String::new(),
                };
                ("Instincts".to_string(), sub)
            }
            "extinguished" => ("Extinguished".to_string(), String::new()),
            _ => (scope::track_title(first, s.scope_key().is_some()), format!(" · v{}", first.version)),
        };
        let open = default_open(&g) != st.toggled.contains(&g) || !st.filter.is_empty();
        out.push(Node::Group { key: g.clone(), title, sub, count: members.len(), open });
        if open {
            out.extend(members.into_iter().map(Node::Reflex));
        }
    }
    out
}

fn node_key(s: &AppState, n: &Node) -> String {
    match n {
        Node::Group { key, .. } => format!("group:{key}"),
        Node::Reflex(i) => reflex_key(&s.route_rows()[*i]),
    }
}

/// The selected node's position in `nodes`: the stored key's, else the first reflex.
fn position(s: &AppState, nodes: &[Node]) -> usize {
    s.reflexes
        .sel
        .as_ref()
        .and_then(|k| nodes.iter().position(|n| &node_key(s, n) == k))
        .or_else(|| nodes.iter().position(|n| matches!(n, Node::Reflex(_))))
        .unwrap_or(0)
}

pub fn selected(s: &AppState) -> Option<&RouteRow> {
    let nodes = tree(s);
    match nodes.get(position(s, &nodes))? {
        Node::Reflex(i) => s.route_rows().get(*i),
        Node::Group { .. } => None,
    }
}

/// The selection moved: the card starts at its top, and its meters fill again.
fn moved(st: &mut State) {
    st.detail_scroll = 0;
    st.moved_at = Some(Instant::now());
}

/// Select a reflex (from the palette or the map), opening its group.
pub fn select(s: &mut AppState, reference: &str, id: &str) {
    let Some(r) = s.route_rows().iter().find(|r| r.reference == reference && r.id == id).cloned() else { return };
    // A reflex of another repository than the one shown: show its repository.
    if !s.in_scope(&r) {
        s.live.all_repos = false;
        s.live.repo = Some(r.repo.clone());
        s.refresh_counts();
    }
    let (key, g) = (reflex_key(&r), group_of(&r));
    let st = &mut s.reflexes;
    if default_open(&g) {
        st.toggled.remove(&g);
    } else {
        st.toggled.insert(g);
    }
    if !passes(st, &r) {
        st.filter.clear();
        st.hidden = [false; 3];
        st.states = 0;
    }
    st.sel = Some(key);
    moved(st);
}

fn move_by(s: &mut AppState, delta: isize) {
    let nodes = tree(s);
    if nodes.is_empty() {
        return;
    }
    let i = position(s, &nodes) as isize + delta;
    let i = i.clamp(0, nodes.len() as isize - 1) as usize;
    let key = node_key(s, &nodes[i]);
    if s.reflexes.sel.as_deref() != Some(key.as_str()) {
        moved(&mut s.reflexes);
    }
    s.reflexes.sel = Some(key);
}

/// How full the card's meters are `ms` after the selection changed: eased out (quick, then
/// settling), and full from [`FILL_MS`] on.
pub fn fill(ms: u64) -> f64 {
    if ms >= FILL_MS {
        return 1.0;
    }
    let x = ms as f64 / FILL_MS as f64;
    1.0 - (1.0 - x).powi(3)
}

/// Whether the card's meters are still filling (the app then draws every [`FRAME`]).
pub fn animating(st: &State, now: Instant) -> bool {
    st.moved_at.is_some_and(|at| now.saturating_duration_since(at) < Duration::from_millis(FILL_MS))
}

/// How full the card's meters are drawn now: full under reduced motion, and in tests (frames are
/// compared once they have settled).
fn progress(st: &State, t: &Theme) -> f64 {
    match st.moved_at {
        Some(at) if t.motion && !cfg!(test) => fill(Instant::now().saturating_duration_since(at).as_millis() as u64),
        _ => 1.0,
    }
}

/// The traces (in the live feed) where a reflex fired or was judged.
fn traces<'a>(s: &'a AppState, id: &str, repo: &str) -> Vec<&'a feed::FeedItem> {
    s.live.feed.iter().filter(|i| i.repo == repo && i.reflexes().any(|r| r == id)).collect()
}

fn last_fired(s: &AppState, id: &str) -> Option<u64> {
    s.live
        .feed
        .iter()
        .rev()
        .find(|i| i.acts.iter().any(|a| matches!(a, Act::Directed { route, .. } if route == id)))
        .map(|i| i.ts)
}

pub(crate) fn show_traces(s: &mut AppState, r: &RouteRow) {
    let lines: Vec<String> = traces(s, &r.id, &r.repo)
        .iter()
        .map(|i| {
            let what: Vec<String> = i
                .acts
                .iter()
                .filter_map(|a| match a {
                    Act::Directed { route, strength, .. } if *route == r.id => {
                        Some(format!("fired {}", Theme::strength_word(*strength)))
                    }
                    Act::Withheld { route, .. } if *route == r.id => Some("withheld (control)".into()),
                    Act::Followed(x) if *x == r.id => Some("followed".into()),
                    Act::Ignored(x) if *x == r.id => Some("not observed within 3 batches".into()),
                    Act::Misfired(x) if *x == r.id => Some("misfired".into()),
                    Act::Met(x) if *x == r.id => Some("finish line met".into()),
                    Act::Unaided(x) if *x == r.id => Some("found unaided (control)".into()),
                    Act::NotReached(x) if *x == r.id => Some("missed (control)".into()),
                    _ => None,
                })
                .collect();
            format!(
                "{}  {}  {:<7} {:<7} {}",
                view::clock(i.ts, view::local_offset(), true),
                feed::short_session(&i.session, 6),
                feed::harness_name(&i.harness),
                i.event,
                what.join(", ")
            )
        })
        .collect();
    let mut lines = if lines.is_empty() {
        vec![format!("{} has not fired in the traces this screen has seen.", r.id)]
    } else {
        lines
    };
    lines.insert(0,"Recent feed only, matched by reflex ID in this repository. Historical and duplicate collection versions may appear; use Explore complete traces in browser for attributed evidence.".into());
    s.modal = Some(Modal::Viewer { title: format!("recent traces · {}", r.id), source: None, lines, scroll: 0 });
}

pub fn key(s: &mut AppState, code: KeyCode) -> Option<Cmd> {
    let st = &mut s.reflexes;
    if st.filtering {
        match code {
            KeyCode::Esc => {
                st.filter.clear();
                st.hidden = [false; 3];
                st.states = 0;
                st.filtering = false;
            }
            KeyCode::Enter => st.filtering = false,
            KeyCode::Backspace => {
                st.filter.pop();
            }
            KeyCode::Char(c) if st.filter.chars().count() < 60 => st.filter.push(c),
            _ => {}
        }
        return None;
    }
    match code {
        KeyCode::Up | KeyCode::Char('k') => move_by(s, -1),
        KeyCode::Down | KeyCode::Char('j') => move_by(s, 1),
        KeyCode::Home => move_by(s, -10_000),
        KeyCode::End => move_by(s, 10_000),
        KeyCode::PageDown => s.reflexes.detail_scroll = s.reflexes.detail_scroll.saturating_add(5),
        KeyCode::PageUp => s.reflexes.detail_scroll = s.reflexes.detail_scroll.saturating_sub(5),
        KeyCode::Esc => {
            s.reflexes.filter.clear();
            s.reflexes.hidden = [false; 3];
            s.reflexes.states = 0;
            s.reflexes.detail_scroll = 0;
        }
        KeyCode::Char('/') => s.reflexes.filtering = true,
        KeyCode::Char('f') => s.open_repos(),
        KeyCode::Char('F') => s.reflexes.hidden[0] = !s.reflexes.hidden[0],
        KeyCode::Char('P') => s.reflexes.hidden[1] = !s.reflexes.hidden[1],
        KeyCode::Char('I') => s.reflexes.hidden[2] = !s.reflexes.hidden[2],
        KeyCode::Char('S') => s.reflexes.states = (s.reflexes.states + 1) % STATES.len(),
        KeyCode::Char('R') => return Some(Cmd::Want(WantKind::Routes)),
        KeyCode::Char('r') => s.reflexes.raw = !s.reflexes.raw,
        KeyCode::Char('v') => s.reflexes.details = !s.reflexes.details,
        KeyCode::Enter | KeyCode::Left | KeyCode::Right | KeyCode::Char('h') | KeyCode::Char('l') => {
            let nodes = tree(s);
            let n = nodes.get(position(s, &nodes)).cloned()?;
            // On a reflex, Enter opens a dedicated review without changing it.
            if code == KeyCode::Enter && matches!(n, Node::Reflex(_)) {
                crate::ui::reflex_review::open(s);
                return None;
            }
            let group = match &n {
                Node::Group { key, .. } => Some(key.clone()),
                Node::Reflex(i) if matches!(code, KeyCode::Left | KeyCode::Char('h')) => {
                    Some(group_of(&s.route_rows()[*i]))
                }
                Node::Reflex(_) => None,
            };
            if let Some(g) = group {
                let open = default_open(&g) != s.reflexes.toggled.contains(&g);
                let want_open = match code {
                    KeyCode::Left | KeyCode::Char('h') => false,
                    KeyCode::Right | KeyCode::Char('l') => true,
                    _ => !open,
                };
                if want_open != open && !s.reflexes.toggled.remove(&g) {
                    s.reflexes.toggled.insert(g.clone());
                }
                let key = format!("group:{g}");
                if s.reflexes.sel.as_deref() != Some(key.as_str()) {
                    moved(&mut s.reflexes);
                }
                s.reflexes.sel = Some(key);
            }
        }
        KeyCode::Char('t') => {
            let r = s.selected_route()?.clone();
            show_traces(s, &r);
        }
        KeyCode::Char('o') => {
            let r = s.selected_route()?;
            return Some(Cmd::OpenWeb {
                focus: Some(serde_json::json!([r.repo, r.reference, r.id]).to_string()),
                traces: false,
            });
        }
        KeyCode::Char('d') => {
            return s.run_entry(crate::ui::palette::Run::Form(crate::ui::forms::Kind::Delegate));
        }
        KeyCode::Char(c) => {
            let (_, verb) = *ACTIONS.iter().find(|(k, _)| *k == c)?;
            let r = s.selected_route()?.clone();
            confirm_review(s, &r, verb);
        }
        _ => {}
    }
    None
}

/// Prepare a revision-bound decision on exactly the reflex the person inspected.
pub(crate) fn confirm_review(s: &mut AppState, r: &RouteRow, verb: &'static str) {
    let word = words::verb(verb);
    if !r.verbs.contains(&verb) {
        let open: Vec<&str> = r.verbs.iter().map(|v| words::verb(v)).collect();
        s.message(
            "Not available",
            vec![format!("{word} does not apply to {} as it stands.", r.id), format!("Available: {}", open.join(", "))],
            true,
        );
        return;
    }
    let t = Theme::from_env();
    let mut title = word.to_string();
    title[..1].make_ascii_uppercase();
    let mut lines = vec![
        Line::from(vec![
            Span::raw(format!("{title} ")),
            Span::styled(r.id.clone(), t.bold()),
            Span::raw(format!(" in {}?", r.reference)),
        ]),
        Line::raw(""),
        Line::raw(words::verb_effect(verb)),
        Line::raw("The decision is recorded in the track's review.yaml and the index is recompiled."),
        Line::raw(""),
        Line::from(vec![
            Span::styled("same as ", t.dim()),
            Span::styled(words::review_command(verb, &r.reference, &r.id), t.command()),
            Span::styled(" from any shell", t.dim()),
        ]),
        Line::from(vec![
            Span::styled("y", t.key()),
            Span::styled(format!(" confirm{}", t.sep()), t.dim()),
            Span::styled("n", t.key()),
            Span::styled(" cancel", t.dim()),
        ]),
    ];
    let Some(preview) = &r.review_preview else {
        s.message(
            "Review unavailable",
            vec!["The proposal could not be read safely. Recheck health and reload before reviewing.".into()],
            true,
        );
        return;
    };
    lines.push(Line::raw(""));
    lines.push(Line::raw("Current response"));
    lines.extend(preview.current_directive.lines().map(|l| Line::raw(l.to_string())));
    if let Some(proposal) = &preview.proposal {
        lines.push(Line::raw(format!("Proposed on {}: {}", proposal.on, proposal.reason)));
        for (field, before, after) in &proposal.diff {
            lines.push(Line::raw(format!("{field}: {before} → {after}")));
        }
        lines.push(Line::raw("Proposed response (approve applies this; dismiss keeps the current response)"));
        lines.extend(proposal.directive.lines().map(|l| Line::raw(l.to_string())));
    }
    lines.push(Line::raw("Confirmation applies only to this exact reviewed version. Enter/y confirms; Esc/n cancels."));
    s.confirm(
        &title,
        lines,
        Action::Review {
            verb,
            reference: r.reference.clone(),
            route: r.id.clone(),
            expected_revision: preview.revision.clone(),
        },
    );
}

// ---------------------------------------------------------------------------------------------
// Words: what a row and the card say

/// What kind of reflex a row is, as a glyph: a procedure card, one compiled from a skill, an
/// instinct, or a learned reflex.
pub fn kind_glyph(r: &RouteRow) -> Glyph {
    if r.is_card() {
        Glyph::Card
    } else if r.more.skill {
        Glyph::Skill
    } else if r.instinct {
        Glyph::Instinct
    } else {
        Glyph::Reflex
    }
}

/// How a reflex reads in a row: when it fires, what it has the agent do, and whether that is a
/// command. From its gist, which says it in plain words (`after editing src/**, run make test` →
/// `after editing src/**`, `make test`); a gist of another shape (an instinct's advice,
/// `Cargo.lock: let cargo update it; don't edit it by hand`) is all action, after its trigger.
pub fn says(r: &RouteRow) -> (String, String, bool) {
    let gist = r.gist.trim();
    if let Some((when, what)) = gist.rsplit_once(", run ").filter(|(w, x)| !w.is_empty() && !x.is_empty()) {
        return (when.to_string(), what.to_string(), true);
    }
    for verb in [", read ", ", show ", ", use ", ", advise "] {
        if let Some(i) = gist.rfind(verb).filter(|&i| i > 0) {
            return (gist[..i].to_string(), gist[i + 2..].to_string(), false);
        }
    }
    // Advice that names what its trigger already names says the rest: `edit Cargo.lock` → `let cargo
    // update it; don't edit it by hand`.
    let when = r.trigger.replace('`', "");
    let what = match gist.split_once(": ") {
        Some((subject, rest)) if !rest.is_empty() && when.contains(subject) => rest.to_string(),
        _ if gist.is_empty() => r.id.clone(),
        _ => gist.to_string(),
    };
    (when, what, false)
}

/// `s` with its first letter capitalised when it starts with a word (never a path's).
fn capitalised(s: &str) -> String {
    let first = s.split(' ').next().unwrap_or_default();
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if first.chars().all(|c| c.is_ascii_lowercase()) => {
            format!("{}{}", c.to_ascii_uppercase(), chars.as_str())
        }
        _ => s.to_string(),
    }
}

/// The card's title: the reflex in plain words (its gist, the first letter capitalised when it
/// starts with a word rather than a path), else its id.
pub fn title_of(r: &RouteRow) -> String {
    let gist = r.gist.trim();
    if gist.is_empty() {
        r.id.clone()
    } else {
        capitalised(gist)
    }
}

/// A trigger as briefly as a row can say it, still in words: `after editing crates/**/*.rs` is
/// `edit crates/**/*.rs`, `when npm test fails` is `npm test fails`. The card says it in full.
pub fn short_when(when: &str) -> String {
    for (long, short) in [
        ("after editing ", "edit "),
        ("an edit touches ", "edit "),
        ("on a prompt that names ", "a prompt names "),
        ("on requests like these", "requests like these"),
        ("at the start of a session", "session start"),
        ("when ", ""),
    ] {
        if let Some(rest) = when.strip_prefix(long) {
            return format!("{short}{rest}");
        }
    }
    when.to_string()
}

/// `when` cut to `n` columns. A path at its end keeps its tail, which is what tells two rows apart
/// (`edit …/src/*.rs`, not `edit crates/adage-c…`).
pub fn fit_when(when: &str, n: usize, t: &Theme) -> String {
    if when.chars().count() <= n {
        return when.to_string();
    }
    if let Some((head, last)) = when.rsplit_once(' ') {
        let head_w = head.chars().count() + 1;
        let more = t.more();
        if (last.contains('/') || last.contains('*')) && n >= head_w + more.chars().count() + 5 {
            let chars: Vec<char> = last.chars().collect();
            let keep = n - head_w - more.chars().count();
            let mut start = chars.len().saturating_sub(keep).max(1);
            // Cut inside a path part: start at the next one instead (`…/src/*.rs`, not `…i/src/*.rs`).
            if chars[start - 1] != '/' {
                if let Some(i) = chars[start..].iter().position(|c| *c == '/') {
                    if chars.len() - (start + i) >= 4 {
                        start += i;
                    }
                }
            }
            return format!("{head} {more}{}", chars[start..].iter().collect::<String>());
        }
    }
    t.fit(when, n)
}

/// The evidence a row shows: a share for its meter, with what the share is of; or a word where a
/// share would mislead (the strength rests on a value estimate) or there is none (an instinct).
#[derive(Debug, Clone, PartialEq)]
enum Ev {
    Share(f64, String),
    Word(&'static str),
}

fn ev(r: &RouteRow) -> Ev {
    let share = |a: u64, b: u64| a as f64 / b.max(1) as f64;
    if let Some(c) = &r.more.card {
        let n = c.sessions;
        return Ev::Share(share(c.passed.into(), n.into()), format!("{n} trace{}", if n == 1 { "" } else { "s" }));
    }
    if r.more.value.is_some() {
        return Ev::Word("by value");
    }
    if let Some(e) = r.more.execution.as_ref().filter(|e| e.opportunities > 0) {
        return Ev::Share(
            share(e.successful.into(), e.opportunities.into()),
            format!("{}/{}", e.successful, e.opportunities),
        );
    }
    if r.opportunities > 0 {
        Ev::Share(share(r.support, r.opportunities), format!("{}/{}", r.support, r.opportunities))
    } else if r.instinct {
        Ev::Word("built in")
    } else {
        Ev::Word("–")
    }
}

/// A row's lifecycle badge: only while it is not firm, and not where its group already says it
/// (every reflex under `Waiting for review` is nascent).
fn badge(r: &RouteRow) -> Option<&str> {
    let stage = r.stage();
    (stage != "firm" && stage != group_of(r)).then_some(stage)
}

/// What a reflex may do, for its card: the chip's words and tone, and whether the reason belongs
/// beside it (it stops the reflex, or needs a person). It suggests (the agent decides), runs with
/// your grant, or is held by what the mode names.
fn mode_chip(mode: &str) -> (&str, Tone, bool) {
    match mode {
        "Suggest" => ("Suggests", Tone::Muted, false),
        "Automatic" => ("Runs with your grant", Tone::Ok, false),
        "Predicted" => ("adage makes the call", Tone::Accent, false),
        "Observe" => ("Observes only", Tone::Muted, true),
        "Needs approval" | "Needs review" | "Needs recovery" | "Unavailable" => (mode, Tone::Warn, true),
        "Inactive" => (mode, Tone::Muted, false),
        _ => (mode, Tone::Muted, true),
    }
}

/// Where a reflex came from, when not learned here: a chip on its card.
fn origin(r: &RouteRow) -> Option<&'static str> {
    if r.is_card() {
        Some("procedure card")
    } else if r.more.skill {
        Some("from a skill")
    } else if r.instinct {
        Some("instinct")
    } else if r.imported {
        Some("from a pack")
    } else {
        None
    }
}

/// Two decimals, signed, never `-0.00`.
fn signed(x: f64) -> String {
    format!("{:+.2}", if x.abs() < 0.005 { 0.0 } else { x })
}

// ---------------------------------------------------------------------------------------------
// Pieces (each a pure function of data and the theme, written to move into `kit`)

/// The cells of a list row's evidence meter filled for share `x`: any evidence fills at least
/// one, and only all of it fills every cell, so a little never reads as none, nor most as all.
pub fn cells(x: f64, n: usize) -> usize {
    if x.is_nan() || x <= 0.0 || n == 0 {
        return 0;
    }
    if x >= 1.0 {
        return n;
    }
    ((x * n as f64).round() as usize).clamp(1, n.saturating_sub(1).max(1))
}

/// A list row's evidence meter, `n` cells: `▰▰▰▰▱▱` (`####..` in ASCII), filled in `style`.
pub fn cell_meter(x: f64, n: usize, style: Style, t: &Theme) -> Vec<Span<'static>> {
    let k = cells(x, n);
    vec![Span::styled(t.ch("▰", "#").repeat(k), style), Span::styled(t.ch("▱", ".").repeat(n - k), t.rule())]
}

/// Chips laid out in rows `width` wide: the first row starts with `lead`, the rest at its width
/// (a hanging indent), `sep` between chips on a row. A chip never breaks across rows.
fn chip_rows(
    lead: Vec<Span<'static>>,
    chips: Vec<Vec<Span<'static>>>,
    sep: Span<'static>,
    width: usize,
    t: &Theme,
) -> Vec<Line<'static>> {
    let indent = kit::spans_width(&lead);
    let sep_w = sep.width();
    let mut out = Vec::new();
    let mut cur = lead;
    let mut used = indent;
    let mut empty = true;
    for chip in chips {
        let w = kit::spans_width(&chip);
        if !empty && used + sep_w + w > width {
            out.push(Line::from(std::mem::take(&mut cur)));
            cur = vec![Span::raw(" ".repeat(indent))];
            used = indent;
            empty = true;
        }
        if !empty {
            cur.push(sep.clone());
            used += sep_w;
        }
        let chip = if used + w > width { kit::clip_spans(chip, width.saturating_sub(used), t) } else { chip };
        used += kit::spans_width(&chip);
        cur.extend(chip);
        empty = false;
    }
    if !empty {
        out.push(Line::from(cur));
    }
    out
}

/// A section's heading with a key hint at its end: `What the agent sees ──────────  r  raw`.
fn section(head: Section, hint: Option<(&str, &str)>, width: usize, t: &Theme) -> Line<'static> {
    let Some((key, what)) = hint else { return head.indent(0).line(width as u16, t) };
    let hint = vec![kit::keycap(key, t), Span::styled(format!(" {what}"), t.dim())];
    let hint_w = kit::spans_width(&hint);
    let mut line = head.indent(0).line(width.saturating_sub(hint_w + 1) as u16, t);
    line.spans.push(Span::raw(" "));
    line.spans.extend(hint);
    line
}

/// The width of the label column in the card's evidence.
const LABEL: usize = 15;
/// The width of a meter's own label (`when told`, `held back`) inside the value column.
const SUB: usize = 11;

/// The evidence's meters at one size: the card's width, the meters' width, and how full they are
/// drawn while the card settles (`p`, 1 once it has).
struct Meters<'a> {
    t: &'a Theme,
    width: usize,
    bar: usize,
    p: f64,
}

impl Meters<'_> {
    /// One row of evidence as a meter: its label, an optional label of its own (`when told`), the
    /// share `x` filled, and what the share is of: `Learned from    ━━━━━━━━━━╸─────  22 of 35 traces`.
    fn row(&self, label: &str, sub: &str, x: f64, tone: Tone, value: Vec<Span<'static>>) -> Vec<Line<'static>> {
        let t = self.t;
        let mut spans = vec![Span::styled(t.pad(label, LABEL + 2), t.dim())];
        let w = if sub.is_empty() {
            self.bar
        } else {
            spans.push(Span::styled(t.pad(sub, SUB), t.ink2()));
            self.bar.saturating_sub(SUB).max(6)
        };
        spans.extend(kit::meter(x * self.p, w, tone, t));
        spans.push(Span::raw("  "));
        // What the share is of stays whole: short of room it moves under the meter, not in pieces.
        spans.extend(value.into_iter().map(|mut s| {
            s.content = s.content.replace(' ', NBSP).into();
            s
        }));
        kit::wrap_spans(spans, self.width, LABEL + 2)
    }
}

/// `n` and what it counts, the number bold: `22` ` of 35 traces`.
fn counted(n: impl ToString, rest: impl Into<String>, t: &Theme) -> Vec<Span<'static>> {
    vec![Span::styled(n.to_string(), t.bold()), Span::styled(rest.into(), t.ink2())]
}

/// The reflex as an arc, as the Map draws it: what sets it off, the reflex, what it has the agent
/// do. Two boxes side by side where the card has the room (`boxes`), else one line:
///
/// ```text
/// ╭─ WHEN ────────────────────╮         ╭─ DO ─────────────────────╮
/// │ an edit touches           │─── ◠ ──▶│ cargo build -p adage-cli │
/// │ crates/adage-cli/src/*.rs │         │                          │
/// ╰───────────────────────────╯         ╰──────────────────────────╯
/// ```
fn arc(r: &RouteRow, t: &Theme, width: usize, boxes: bool) -> Vec<Line<'static>> {
    let when = r.trigger.replace('`', "");
    let what = r.step.replace('`', "");
    let do_style = if matches!(r.step_kind.as_str(), "run" | "avoid") { t.command() } else { Style::default() };
    let hot = if t.color { t.strength(r.strength).patch(t.bold()) } else { t.bold() };
    let glyph = t.glyph(kind_glyph(r));
    let (h, head) = (t.ch("─", "-"), t.ch("▶", ">"));
    if !boxes || width < 56 {
        // One line, broken only between its three parts: the arrow, and what it does, stay whole.
        let link = format!("{h}{h}{head}");
        let spans = vec![
            Span::styled("when ", t.dim()),
            Span::raw(when),
            Span::raw(" "),
            Span::styled(format!("{link}{NBSP}{glyph}{NBSP}{link}"), hot),
            Span::raw(" "),
            Span::styled(format!("do{NBSP}"), t.dim()),
            Span::styled(what.replace(' ', NBSP), do_style),
        ];
        return kit::wrap_spans(spans, width, 2);
    }
    // Each box as wide as its words need, sharing the room when they need more than there is;
    // a box never breaks a word (a path) the room would let it keep whole.
    let gap = 8 + kit::width(glyph);
    let avail = width.saturating_sub(gap + 8);
    let (need_w, need_d) = (kit::width(&when), kit::width(&what));
    let longest = |s: &str| s.split(' ').map(kit::width).max().unwrap_or(0);
    let half = avail / 2;
    let (mut iw, mut id) = if need_w <= half && need_d <= half {
        (need_w.max(need_d), need_w.max(need_d))
    } else if need_d <= half {
        (avail - need_d, need_d)
    } else if need_w <= half {
        (need_w, avail - need_w)
    } else {
        (half, avail - half)
    };
    if iw < longest(&when) && longest(&when) + longest(&what).max(8) <= avail {
        (iw, id) = (longest(&when), avail - longest(&when));
    } else if id < longest(&what) && longest(&what) + longest(&when).max(8) <= avail {
        (iw, id) = (avail - longest(&what), longest(&what));
    }
    let (iw, id) = (iw.clamp(8, 36), id.clamp(8, 36));
    let rows = |text: &str, inner: usize| -> Vec<String> {
        let mut v = kit::wrap(text, inner);
        if v.len() > 3 {
            let rest = v[2..].join(" ");
            v.truncate(2);
            v.push(t.fit(&rest, inner));
        }
        v
    };
    let (wl, dl) = (rows(&when, iw), rows(&what, id));
    // Then each hugs what it holds.
    let hug = |rows: &[String]| rows.iter().map(|r| kit::width(r)).max().unwrap_or(0).max(8);
    let (iw, id) = (hug(&wl), hug(&dl));
    let n = wl.len().max(dl.len()).max(1);
    let (tl, tr, bl, br, vt) = (t.ch("╭", "+"), t.ch("╮", "+"), t.ch("╰", "+"), t.ch("╯", "+"), t.ch("│", "|"));
    let top = |title: &str, inner: usize| {
        vec![
            Span::styled(format!("{tl}{h}"), t.rule()),
            Span::styled(format!(" {title} "), t.dim().patch(t.bold())),
            Span::styled(format!("{}{tr}", h.repeat((inner + 4).saturating_sub(5 + title.len()))), t.rule()),
        ]
    };
    let cell = |text: Option<&String>, inner: usize, style: Style| {
        vec![
            Span::styled(format!("{vt} "), t.rule()),
            Span::styled(col(text.map_or("", String::as_str), inner), style),
            Span::styled(format!(" {vt}"), t.rule()),
        ]
    };
    let bottom = |inner: usize| Span::styled(format!("{bl}{}{br}", h.repeat(inner + 2)), t.rule());
    let mut v = Vec::new();
    let mut first = top("WHEN", iw);
    first.push(Span::raw(" ".repeat(gap)));
    first.extend(top("DO", id));
    v.push(Line::from(first));
    for k in 0..n {
        let mut spans = cell(wl.get(k), iw, Style::default());
        if k == 0 {
            spans.push(Span::styled(format!("{} {glyph} {}{head}", h.repeat(3), h.repeat(2)), hot));
        } else {
            spans.push(Span::raw(" ".repeat(gap)));
        }
        spans.extend(cell(dl.get(k), id, do_style));
        v.push(Line::from(spans));
    }
    v.push(Line::from(vec![bottom(iw), Span::raw(" ".repeat(gap)), bottom(id)]));
    v
}

// ---------------------------------------------------------------------------------------------
// The list

/// The lamp of a row: its strength's colour (its initial without colour); quiet and off for the
/// reflexes not in use.
fn lamp(r: &RouteRow, t: &Theme) -> Span<'static> {
    if r.quiet() {
        Span::styled(t.glyph(Glyph::Quiet), t.dim())
    } else if r.nascent() || r.extinguished() {
        Span::styled(t.glyph(Glyph::Off), t.dim())
    } else if t.color {
        Span::styled(t.glyph(Glyph::Lamp), t.strength(r.strength))
    } else {
        Span::styled(Theme::strength_word(r.strength)[..1].to_ascii_uppercase(), t.bold())
    }
}

fn chips(s: &AppState, t: &Theme, width: usize) -> Line<'static> {
    let rows: Vec<&RouteRow> = s.route_rows().iter().filter(|r| s.in_scope(r)).collect();
    let mut spans = vec![Span::raw(" ")];
    for (k, a) in [Strength::Fire, Strength::Prime, Strength::Inhibit].into_iter().enumerate() {
        let n = rows.iter().filter(|r| r.strength == a && !r.extinguished()).count();
        let on = !s.reflexes.hidden[k];
        let word = Theme::strength_word(a);
        if t.color {
            let lamp = if on { t.glyph(Glyph::Lamp) } else { t.glyph(Glyph::Off) };
            spans.push(Span::styled(format!("{lamp} "), if on { t.strength(a) } else { t.dim() }));
            spans.push(Span::styled(format!("{word} {n}  "), if on { t.ink2() } else { t.dim() }));
        } else {
            let w = if on { word.to_ascii_uppercase() } else { format!("({word})") };
            spans.push(Span::styled(format!("{w} {n}  "), if on { t.bold() } else { t.dim() }));
        }
    }
    let quiet = rows.iter().filter(|r| r.quiet()).count();
    if quiet > 0 {
        spans.push(Span::styled(format!("{} ", t.glyph(Glyph::Quiet)), t.dim()));
        spans.push(Span::styled(format!("quiet {quiet}  "), t.ink2()));
    }
    let states = format!("{} {}", STATES[s.reflexes.states], t.ch("▾", "v"));
    spans.push(Span::styled(states, if s.reflexes.states == 0 { t.dim() } else { t.accent() }));
    let used: usize = spans.iter().map(|s| s.width()).sum();
    if used > width {
        spans.truncate(spans.len() - 1);
    }
    Line::from(spans)
}

/// A group's heading: its name in plain words, a rule, and how many it holds at the right edge
/// (`▾ Learned here · v17.0.12 ───────────── 5 reflexes`).
fn group_line(n: &Node, chosen: bool, width: usize, t: &Theme) -> Line<'static> {
    let Node::Group { key, title, sub, count, open } = n else { return Line::raw("") };
    let noun = match (key.as_str(), *count == 1) {
        ("instincts", true) => "instinct",
        ("instincts", false) => "instincts",
        (_, true) => "reflex",
        (_, false) => "reflexes",
    };
    let right = format!("{count} {noun}");
    let right_w = right.chars().count();
    let arrow = if *open { t.ch("▾", "v") } else { t.ch("▸", ">") };
    let title_style = if key == "extinguished" { t.dim().patch(t.bold()) } else { t.bold() };
    let lead = 4;
    let room = width.saturating_sub(lead + right_w + 4);
    let title = t.fit(&t.text(title), room.max(4));
    let sub = t.fit(&t.text(sub), room.saturating_sub(title.chars().count()));
    let mut spans = vec![
        if chosen { Span::styled(t.ch("▌", ">"), t.accent()) } else { Span::raw(" ") },
        Span::raw(" "),
        Span::styled(format!("{arrow} "), t.dim()),
        Span::styled(title, title_style),
        Span::styled(sub, t.dim()),
    ];
    let used = kit::spans_width(&spans);
    let rule = width.saturating_sub(used + right_w + 3);
    if rule >= 2 {
        spans.push(Span::styled(format!(" {} ", t.ch("─", "-").repeat(rule)), t.rule()));
        spans.push(Span::styled(right, t.dim()));
    }
    Line::from(spans)
}

const METER: usize = 6;

/// `s` (already cut to fit) as a column `n` wide.
fn col(s: &str, n: usize) -> String {
    format!("{s}{}", " ".repeat(n.saturating_sub(s.chars().count())))
}

fn tree_lines(s: &AppState, nodes: &[Node], sel: usize, t: &Theme, width: usize) -> (Vec<Line<'static>>, usize) {
    let rows = s.route_rows();
    let shown: Vec<&RouteRow> =
        nodes.iter().filter_map(|n| if let Node::Reflex(i) = n { Some(&rows[*i]) } else { None }).collect();
    let said: Vec<(String, String, bool)> = shown
        .iter()
        .map(|r| {
            let (when, what, command) = says(r);
            (short_when(&when), what, command)
        })
        .collect();
    let evs: Vec<Ev> = shown.iter().map(|r| ev(r)).collect();
    // One set of columns for the whole tree, so every row lines up: the lamp, when, the arrow,
    // what, then the evidence (a meter and its count) and the lifecycle at the right edge.
    let lamp_w = if t.unicode() { 1 } else { 3 };
    let arrow = format!(" {} ", t.glyph(Glyph::Arrow));
    let num_w = evs.iter().map(|e| if let Ev::Share(_, s) = e { s.chars().count() } else { 0 }).max().unwrap_or(0);
    let word_w = evs.iter().map(|e| if let Ev::Word(w) = e { w.chars().count() } else { 0 }).max().unwrap_or(0);
    let ev_w = if num_w > 0 { METER + 1 + num_w } else { 0 }.max(word_w);
    let fixed = 2 + lamp_w + 1 + arrow.chars().count() + 2 + ev_w + 1;
    let room = width.saturating_sub(fixed).max(12);
    // A command is shown whole where it can be (with its lifecycle, when it has one); advice
    // (prose) gives way first.
    let badge_w = |r: &RouteRow| badge(r).map_or(0, |b| b.chars().count() + 2);
    let max_when = said.iter().map(|s| s.0.chars().count()).max().unwrap_or(0);
    let max_what = said
        .iter()
        .zip(&shown)
        .map(|((_, what, command), r)| what.chars().count().min(if *command { 60 } else { 24 }) + badge_w(r))
        .max()
        .unwrap_or(0);
    let when_w = if max_when + max_what <= room {
        max_when
    } else {
        max_when.min((room * 2 / 5).max(room.saturating_sub(max_what)))
    };
    let what_w = room.saturating_sub(when_w);
    let mut out = Vec::new();
    let mut sel_line = 0;
    let mut k_row = 0;
    for (k, n) in nodes.iter().enumerate() {
        let chosen = k == sel;
        match n {
            Node::Group { .. } => {
                if k > 0 {
                    out.push(Line::raw(""));
                }
                if chosen {
                    sel_line = out.len();
                }
                out.push(group_line(n, chosen, width, t));
            }
            Node::Reflex(_) => {
                let (r, (when, what, command), e) = (shown[k_row], &said[k_row], &evs[k_row]);
                k_row += 1;
                if chosen {
                    sel_line = out.len();
                }
                let lit = !(r.nascent() || r.extinguished());
                let what_style = match (*command && lit, chosen) {
                    (true, true) => t.command().patch(t.bold()),
                    (true, false) => t.command(),
                    (false, true) => t.bold(),
                    (false, false) if lit => Style::default(),
                    _ => t.ink2(),
                };
                let mut lamp = lamp(r, t);
                lamp.content = t.pad(&lamp.content, lamp_w + 1).into();
                let b = badge(r);
                let what = t.fit(&t.text(what), what_w.saturating_sub(badge_w(r)));
                let mut spans = vec![
                    if chosen { Span::styled(t.ch("▌", ">"), t.accent()) } else { Span::raw(" ") },
                    Span::raw(" "),
                    lamp,
                    Span::styled(
                        col(&fit_when(&t.text(when), when_w, t), when_w),
                        if chosen { t.ink2() } else { t.dim() },
                    ),
                    Span::styled(arrow.clone(), t.dim()),
                    Span::styled(what.clone(), what_style),
                ];
                // Its lifecycle, while it is not firm, right after what it does.
                if let Some(b) = b {
                    let style = match b {
                        "forming" => t.accent(),
                        "dormant" => t.yellow(),
                        _ => t.dim(),
                    };
                    spans.push(Span::styled(format!("  {b}"), style));
                }
                spans.push(Span::raw(" ".repeat((what_w + 2).saturating_sub(what.chars().count() + badge_w(r)))));
                match e {
                    Ev::Share(x, text) => {
                        let fill = if chosen { t.accent() } else { t.ink2() };
                        spans.push(Span::raw(" ".repeat(ev_w - METER - 1 - num_w)));
                        spans.extend(cell_meter(*x, METER, fill, t));
                        spans.push(Span::styled(format!(" {text:>num_w$}"), if chosen { t.bold() } else { t.ink2() }));
                    }
                    Ev::Word(w) => spans.push(Span::styled(format!("{:>ev_w$}", t.text(w)), t.dim())),
                }
                out.push(Line::from(spans));
            }
        }
    }
    (out, sel_line)
}

// ---------------------------------------------------------------------------------------------
// The card

/// The list keeps a compact summary and the trigger-to-response graph. Full review is Enter.
fn overview(r: &RouteRow, s: &AppState, ctx: &Ctx, width: usize, tall: bool, p: f64) -> Vec<Line<'static>> {
    let t = &ctx.theme;
    let mut lines = hero(r, s, ctx, width);
    lines.push(Line::from(Span::styled(crate::ui::reflex_review::service(r), t.ink2())));
    if r.approved && r.strength == Strength::Prime && r.ceiling == "prime" {
        lines
            .push(Line::from(Span::styled("Ceiling: Prime. Approval does not raise this reflex's strength.", t.dim())));
    }
    lines.push(Line::raw(""));
    lines.extend(arc(r, t, width, tall));
    let evidence = if let Some(e) = &r.more.execution {
        Some((
            e.successful as f64 / e.opportunities.max(1) as f64,
            format!("{} of {} situations", e.successful, e.opportunities),
        ))
    } else if let Some(value) = &r.more.value {
        lines.push(Line::from(Span::styled(value.clone(), t.ink2())));
        None
    } else if let Some(card) = &r.more.card {
        Some((
            card.passed as f64 / card.sessions.max(1) as f64,
            format!("{} of {} traces passed", card.passed, card.sessions),
        ))
    } else if r.opportunities > 0 {
        Some((r.rate, format!("{} of {} traces", r.support, r.opportunities)))
    } else {
        None
    };
    if let Some((rate, label)) = evidence {
        let mut spans = cell_meter(rate * p, 6, t.accent(), t);
        spans.push(Span::styled(format!("  {label}"), t.ink2()));
        lines.push(Line::from(spans));
    }
    lines.push(Line::raw(""));
    lines.push(Line::from(vec![kit::keycap("enter", t), Span::styled(" Review evidence and decisions", t.accent())]));
    lines
}

/// The card for one reflex, `width` columns wide, its meters drawn to `p` of their value. The
/// arc is drawn as boxes when `tall` (the card has the rows to spare).
pub fn card(r: &RouteRow, s: &AppState, ctx: &Ctx, width: usize, tall: bool, p: f64) -> Vec<Line<'static>> {
    let t = &ctx.theme;
    let mut v = hero(r, s, ctx, width);
    v.push(Line::raw(""));
    v.extend(arc(r, t, width, tall));
    v.push(Line::raw(""));
    v.extend(evidence(r, t, width, p));
    v.push(Line::raw(""));
    v.extend(advisory(r, s.reflexes.raw, t, width));
    if let Some(c) = &r.more.card {
        v.push(Line::raw(""));
        v.extend(card_lines(c, r.more.card_threshold, t, width));
    }
    v.push(Line::raw(""));
    v.extend(review(r, s, t, width));
    v.push(Line::raw(""));
    v.extend(details(r, s.reflexes.details, t, width));
    v
}

/// The card's head: the reflex in plain words, then chips for its strength, what it may do, its
/// lifecycle and when it last fired; then whatever needs a person (a grant to review, an
/// uncertain last run, a review it waits for), above everything else.
fn hero(r: &RouteRow, s: &AppState, ctx: &Ctx, width: usize) -> Vec<Line<'static>> {
    let t = &ctx.theme;
    let glyph = t.glyph(kind_glyph(r));
    let lead = kit::width(glyph) + 1;
    // The gist; a command in it stays whole, in the command colour.
    let mut title = vec![Span::styled(format!("{glyph} "), t.accent())];
    match says(r) {
        (when, what, true) if !r.gist.trim().is_empty() => {
            title.push(Span::styled(format!("{}, run ", capitalised(&when)), t.bold()));
            title.push(Span::styled(what.replace(' ', NBSP), t.command().patch(t.bold())));
        }
        _ => title.push(Span::styled(title_of(r), t.bold())),
    }
    let mut v = kit::wrap_spans(title, width, lead);
    let (mode, tone, why) = mode_chip(&r.execution.mode);
    let mut chips = vec![
        // Muted while it is not in service (waiting, quiet, extinguished): the strength it would have.
        vec![kit::chip(
            &Theme::strength_word(r.strength).to_ascii_uppercase(),
            if r.nascent() || r.extinguished() { Tone::Muted } else { Tone::of_strength(r.strength) },
            t,
        )],
        vec![kit::chip(mode, tone, t)],
        stepper(r, t),
    ];
    if let Some(o) = origin(r) {
        chips.push(vec![Span::styled(o, t.ink2())]);
    }
    if let Some(ts) = last_fired(s, &r.id) {
        chips.push(vec![Span::styled(format!("last fired {}", view::clock(ts, ctx.tz, true)), t.dim())]);
    }
    v.extend(chip_rows(vec![Span::raw(" ".repeat(lead))], chips, Span::raw("  "), width, t));
    let indent = Span::raw(" ".repeat(lead));
    // Execution authority is separate from learned strength: what holds it, and a result that is
    // not known to have succeeded, come before the evidence (in the stacked layout too).
    if why {
        v.extend(kit::wrap_spans(
            vec![indent.clone(), Span::styled(r.execution.reason.clone(), tone.style(t))],
            width,
            lead,
        ));
    }
    if let Some(result) = &r.execution.last_result {
        let tone = if result.contains("Completed") { Tone::Ok } else { Tone::Warn };
        v.extend(kit::wrap_spans(
            vec![indent.clone(), Span::styled("last run ", t.dim()), Span::styled(result.clone(), tone.style(t))],
            width,
            lead + 9,
        ));
    }
    // Quiet is not an install step: its own mark, and why, in one line.
    if let Some(q) = r.more.quiet_line.as_ref().filter(|_| r.quiet()) {
        v.extend(kit::wrap_spans(
            vec![
                indent.clone(),
                Span::styled(format!("{} ", t.glyph(Glyph::Quiet)), t.dim()),
                Span::styled(q.clone(), t.ink2()),
            ],
            width,
            lead + 2,
        ));
    }
    for (k, d) in r.reasons.iter().filter(|(k, _)| !(k == "quiet" && r.more.quiet_line.is_some())) {
        v.extend(kit::wrap_spans(
            vec![
                indent.clone(),
                Span::styled(format!("{} waits for review: ", t.glyph(Glyph::Off)), t.yellow()),
                Span::styled(format!("{k}{}", t.sep()), t.yellow().patch(t.bold())),
                Span::styled(d.clone(), t.ink2()),
            ],
            width,
            lead + 2,
        ));
    }
    v
}

/// The lifecycle as steps: `○ nascent ── ● forming ── ● firm`, the reached ones lit; a reflex that
/// has fallen from firm adds where it is now.
fn stepper(r: &RouteRow, t: &Theme) -> Vec<Span<'static>> {
    let now = r.lifecycle();
    let rank = words::lifecycle_rank(now);
    let dash = format!(" {} ", t.ch("──", "--"));
    let mut spans = Vec::new();
    for (k, stage) in ["nascent", "forming", "firm"].into_iter().enumerate() {
        if k > 0 {
            spans.push(Span::styled(dash.clone(), t.rule()));
        }
        let (mark, style) = match (k <= rank, stage) {
            (false, _) => (t.glyph(Glyph::Off), t.dim()),
            (true, "firm") => (t.glyph(Glyph::Lamp), t.green()),
            (true, "forming") => (t.glyph(Glyph::Lamp), t.accent()),
            (true, _) => (t.glyph(Glyph::Off), t.dim()),
        };
        spans.push(Span::styled(format!("{mark} "), style));
        spans.push(Span::styled(stage, if stage == now { t.bold() } else { t.ink2() }));
    }
    if rank == 2 && now != "firm" {
        spans.push(Span::styled(dash, t.rule()));
        spans.push(Span::styled(format!("{} ", t.glyph(Glyph::Lamp)), t.dim()));
        spans.push(Span::styled(now.to_string(), t.bold()));
    }
    spans
}

/// The evidence as meters: what conditioning read (the traces that took it and the confidence
/// that rests on, a value estimate, a procedure card's family), what the hooks have recorded
/// since it was installed, how much telling the agent changes, and a finish line's obligation.
fn evidence(r: &RouteRow, t: &Theme, width: usize, p: f64) -> Vec<Line<'static>> {
    let mut v = vec![Section::new("Evidence").indent(0).line(width as u16, t)];
    let m = Meters { t, width, bar: width.saturating_sub(LABEL + 2 + 2 + 22).clamp(10, 26), p };
    let kv =
        |label: &str, value: Vec<Span<'static>>| KeyValue::new().label_width(LABEL).row(label, value).lines(width, t);
    if let Some(e) = &r.more.execution {
        let x = e.successful as f64 / e.opportunities.max(1) as f64;
        let value = counted(e.successful, format!(" of {} situations", e.opportunities), t);
        v.extend(m.row("Learned from", "", x, Tone::Accent, value));
        v.extend(kv(
            "",
            vec![Span::styled(
                format!(
                    "across {} traces. Observed routine; avoided agent work has not been established by these counts.",
                    e.sessions
                ),
                t.dim(),
            )],
        ));
    } else if let (Some(_), Some(e)) = (&r.more.value, r.more.saves) {
        // Decided on the calls it saves, not on how often traces took it: a follow count would
        // say nothing (a desire path has none).
        v.extend(kv("Saves", counted(format!("~{}", adage_app::evidence::num(e.calls)), " calls when followed", t)));
        let value = counted(
            adage_app::evidence::num(e.p),
            format!("{}lower bound {}", t.sep(), adage_app::evidence::num(e.lower)),
            t,
        );
        v.extend(m.row("Confidence", "", e.p, Tone::Accent, value));
        let mut seen = counted(r.support, format!(" of {} triggers took it", r.opportunities), t);
        if r.support == 0 {
            seen.push(Span::styled(format!("{}a desire path", t.sep()), t.dim()));
        }
        v.extend(kv("Learned from", seen));
    } else if let Some(value) = &r.more.value {
        v.extend(kv("Value", vec![Span::raw(value.trim_start_matches("value: ").to_string())]));
    } else if let Some(c) = &r.more.card {
        let x = c.passed as f64 / c.sessions.max(1) as f64;
        let value = counted(c.passed, format!(" of {} traces passed", c.sessions), t);
        v.extend(m.row("Learned from", "", x, Tone::Accent, value));
    } else if r.opportunities == 0 {
        let none = if r.instinct {
            "nothing yet: an instinct ships with adage, with no evidence of its own"
        } else {
            "no evidence recorded"
        };
        v.extend(kv("Learned from", vec![Span::styled(none, t.ink2())]));
    } else {
        let x = r.support as f64 / r.opportunities as f64;
        let value = counted(r.support, format!(" of {} traces", r.opportunities), t);
        v.extend(m.row("Learned from", "", x, Tone::Accent, value));
        let value = counted(adage_app::evidence::num(r.lower_bound), " lower bound", t);
        v.extend(m.row("Confidence", "", r.lower_bound, Tone::Accent, value));
        if r.turns_saved > 0.0 {
            v.extend(kv("Saves", counted(format!("~{:.0}", r.turns_saved), " turns per trace", t)));
        }
    }
    // What the hooks recorded since it was installed, each count a chip.
    let l = &r.live;
    let stats =
        [(l.emitted, "fired"), (l.followed, "followed"), (l.ignored, "not observed"), (l.met, "finish line met")];
    if stats.iter().all(|(n, _)| *n == 0) {
        v.extend(kv("Since installed", vec![Span::styled("nothing yet: it has not fired here", t.dim())]));
    } else {
        let chips: Vec<Vec<Span<'static>>> = stats
            .iter()
            .map(|(n, what)| {
                let (number, words) = if *n == 0 { (t.dim(), t.dim()) } else { (t.bold(), t.ink2()) };
                vec![
                    Span::styled(n.to_string(), number),
                    Span::styled(format!("{NBSP}{}", what.replace(' ', NBSP)), words),
                ]
            })
            .collect();
        let lead = vec![Span::styled(t.pad("Since installed", LABEL + 2), t.dim())];
        v.extend(chip_rows(lead, chips, Span::raw("  "), width, t));
    }
    // Telling the agent against leaving it to itself.
    if let Some(u) = &r.more.uplift {
        let num = |x: f64| vec![Span::styled(adage_app::evidence::num(x), t.bold())];
        v.extend(m.row("Uplift", "when told", u.p_follow, Tone::Ok, num(u.p_follow)));
        v.extend(m.row("", "unaided", u.p_self, Tone::Muted, num(u.p_self)));
        let tone = if u.gated || u.lower <= 0.0 { Tone::Muted } else { Tone::Ok };
        let delta = vec![
            Span::styled(format!("uplift {}", signed(u.mean)), tone.style(t).patch(t.bold())),
            Span::styled(format!("{}lower bound {}", t.sep(), signed(u.lower)), t.dim()),
        ];
        v.extend(kv("", delta));
    }
    // The control arm: traces where the agent took the step, told and held back.
    if r.control.applied > 0 && r.with.applied > 0 {
        let arm = |a: crate::ui::routes::Arm| a.took as f64 / a.applied.max(1) as f64;
        let of = |a: crate::ui::routes::Arm| counted(a.took, format!("/{}", a.applied), t);
        v.extend(m.row("Took the step", "told", arm(r.with), Tone::Ok, of(r.with)));
        v.extend(m.row("", "held back", arm(r.control), Tone::Muted, of(r.control)));
    }
    // A finish line in a few words; the whole rule is in the details.
    if r.more.obligation.is_some() {
        v.extend(kv("Finish line", vec![Span::raw("adage asks for it once if the agent stops without running it")]));
    }
    v
}

/// What the agent sees: the advisory quoted under a rule, wrapped to the card with each line's
/// own indent kept. The wrapper's tags show only as delivered (`r`).
fn advisory(r: &RouteRow, raw: bool, t: &Theme, width: usize) -> Vec<Line<'static>> {
    let head = Section::new("What the agent sees").note("example values", Tone::Muted);
    let mut v = vec![section(head, Some(("r", if raw { "clean" } else { "raw" })), width, t)];
    if r.more.execution.is_some() {
        v.push(Line::from(Span::styled("Execution-only candidate; no advisory reminder is delivered.", t.dim())));
        return v;
    }
    if r.directive.trim().is_empty() {
        v.push(Line::from(Span::styled("No preview: the reflex has no directive text.", t.dim())));
        return v;
    }
    let bar = Span::styled(format!("{} ", t.ch("▎", "|")), t.accent());
    for l in r.directive.lines() {
        let trimmed = l.trim_start();
        let tag = trimmed.starts_with('<');
        if tag && !raw {
            continue;
        }
        let style = if tag {
            t.dim()
        } else if l.starts_with("  ") {
            t.command()
        } else if trimmed.starts_with("Learned by Adage") || trimmed.starts_with("Adage default") {
            t.ink2()
        } else {
            Style::default()
        };
        let lead = l.chars().take_while(|c| *c == ' ').count();
        for row in kit::wrap_spans(vec![Span::styled(t.text(l), style)], width.saturating_sub(2), lead) {
            let mut spans = vec![bar.clone()];
            spans.extend(row.spans);
            v.push(Line::from(spans));
        }
    }
    v
}

/// The review actions open to this reflex, as keys; the rest are in the keys overlay.
fn review(r: &RouteRow, s: &AppState, t: &Theme, width: usize) -> Vec<Line<'static>> {
    let mut v = vec![Section::new("Review").indent(0).line(width as u16, t)];
    let key = |k: &str, what: String| {
        vec![kit::keycap(k, t), Span::styled(format!("{NBSP}{}", what.replace(' ', NBSP)), t.ink2())]
    };
    let mut chips: Vec<Vec<Span<'static>>> = ACTIONS
        .iter()
        .filter(|(_, verb)| r.verbs.contains(verb))
        .map(|(k, verb)| key(&k.to_string(), words::verb(verb).to_string()))
        .collect();
    chips.push(key("d", "delegate".into()));
    chips.push(key("t", format!("traces ({})", traces(s, &r.id, &r.repo).len())));
    chips.push(key("o", "open in map".into()));
    v.extend(chip_rows(vec![], chips, Span::raw("   "), width, t));
    v
}

/// The details a command needs, closed until enter opens them: the id, the track and the
/// directory `adage review` takes, the mode's reason, the trigger and step as stored, the
/// statistics behind the meters, and the review command each action equals.
fn details(r: &RouteRow, open: bool, t: &Theme, width: usize) -> Vec<Line<'static>> {
    let title = format!("{} Details", if open { t.ch("▾", "v") } else { t.ch("▸", ">") });
    let mut v = vec![section(Section::new(title), Some(("v", if open { "close" } else { "open" })), width, t)];
    if !open {
        return v;
    }
    let mut kv = KeyValue::new()
        .row("reflex", Span::styled(r.id.clone(), t.bold()))
        .row("track", r.track.clone())
        .row("version", r.version.clone());
    if r.reference != r.track {
        kv = kv.row("review as", r.reference.clone());
    }
    if !r.repo.is_empty() {
        kv = kv.row("repository", scope::friendly(&r.repo));
    }
    kv = kv
        .row("mode", format!("{}{}{}", r.execution.mode, t.sep(), r.execution.reason))
        .row("trigger", format!("{}{}{}", r.trigger_kind, t.sep(), r.trigger))
        .row("step", format!("{}{}{}", r.step_kind, t.sep(), r.step));
    if r.opportunities > 0 {
        kv = kv.row(
            "evidence",
            format!(
                "{}/{} traces{sep}rate {}{sep}lower bound {}",
                r.support,
                r.opportunities,
                adage_app::evidence::num(r.rate),
                adage_app::evidence::num(r.lower_bound),
                sep = t.sep()
            ),
        );
    }
    if let Some(u) = &r.more.uplift {
        kv = kv.row("uplift", u.line().trim_start_matches("uplift ").to_string());
    }
    if let Some(value) = &r.more.value {
        kv = kv.row("value", value.trim_start_matches("value: ").to_string());
    }
    if let Some(o) = &r.more.obligation {
        kv = kv.row("finish line", o.trim_start_matches("finish line: ").to_string());
    }
    let mut limits = vec![
        format!(
            "ceiling {}",
            crate::ui::theme::parse_strength(&r.ceiling)
                .map_or_else(|| r.ceiling.clone(), |a| Theme::strength_word(a).to_string())
        ),
        format!("effect {}", r.effect),
    ];
    if r.authored {
        limits.push("pinned".into());
    }
    if !r.in_index {
        limits.push("not in the index".into());
    }
    kv = kv.row("limits", limits.join(&t.sep()));
    v.extend(kv.indent(2).lines(width, t));
    // The review as a command, on a line of its own so it copies whole. A nascent reflex (an
    // install step, or a quiet one) waits to be approved: that command first.
    let order = if r.nascent() {
        ["approve", "weaken", "strengthen", "extinguish"]
    } else {
        ["weaken", "strengthen", "extinguish", "approve"]
    };
    if let Some(verb) = order.into_iter().find(|v| r.verbs.contains(v)) {
        let key = ACTIONS.iter().find(|(_, x)| *x == verb).map_or(' ', |(k, _)| *k);
        v.push(Line::raw(""));
        v.push(Line::from(vec![
            Span::raw("  "),
            Span::styled(format!("{key}"), t.key()),
            Span::styled(format!(" {} is the same as, from any shell:", words::verb(verb)), t.dim()),
        ]));
        v.extend(kit::wrap_spans(
            vec![Span::raw("  "), Span::styled(words::review_command(verb, &r.reference, &r.id), t.command())],
            width,
            4,
        ));
    }
    v
}

/// A procedure card's contents, in the evidence's label column: its files in the order they were
/// first edited, the commands with their replay verdicts, what to avoid, the finish check, when a
/// prompt gets it, and what it never does.
fn card_lines(c: &adage_tracks::card::Card, threshold: Option<f32>, t: &Theme, width: usize) -> Vec<Line<'static>> {
    let mut v = vec![Section::new("Procedure card").indent(0).line(width as u16, t)];
    let of = |n: u32| format!("{n}/{}", c.sessions);
    let row = |label: &str, spans: Vec<Span<'static>>| {
        let mut all = vec![Span::styled(t.pad(label, LABEL + 2), t.dim())];
        all.extend(spans);
        kit::wrap_spans(all, width, LABEL + 2)
    };
    let label = |k: usize, name: &'static str| if k == 0 { name } else { "" };
    let path_w = c.files.iter().map(|f| f.path.chars().count()).max().unwrap_or(10).min(36);
    for (k, f) in c.files.iter().enumerate() {
        v.extend(row(
            label(k, "Files"),
            vec![
                Span::styled(format!("{} ", k + 1), t.dim()),
                Span::raw(col(&t.fit(&t.text(&f.path), path_w), path_w + 2)),
                Span::styled(format!("{} traces", of(f.sessions)), t.ink2()),
            ],
        ));
    }
    let cmd_w = c.commands.iter().chain(c.avoid.iter()).map(|x| x.command.chars().count()).max().unwrap_or(10).min(36);
    for (k, x) in c.commands.iter().enumerate() {
        // `adage train cards` replays them on a clean checkout: verified, failed there, or not
        // replayed (never run, or it needed the network or a cache: unverifiable is not stale).
        let (mark, style, verdict) = match x.verified {
            Some(true) => (t.glyph(Glyph::Ok), t.green(), "verified on a clean checkout"),
            Some(false) => (t.glyph(Glyph::Fail), t.red(), "failed on a clean checkout"),
            None => (t.glyph(Glyph::Off), t.dim(), "not replayed"),
        };
        v.extend(row(
            label(k, "Commands"),
            vec![
                Span::styled(format!("{mark} "), style),
                Span::styled(col(&t.fit(&t.text(&x.command), cmd_w), cmd_w + 2), t.command()),
                Span::styled(format!("{}{}{verdict}", of(x.sessions), t.sep()), t.ink2()),
            ],
        ));
    }
    for (k, x) in c.avoid.iter().enumerate() {
        v.extend(row(
            label(k, "Avoid"),
            vec![
                Span::styled(format!("{} ", t.glyph(Glyph::Fail)), t.red()),
                Span::raw(col(&t.fit(&t.text(&x.command), cmd_w), cmd_w + 2)),
                Span::styled(t.text(&x.error), t.ink2()),
            ],
        ));
    }
    if let Some(f) = &c.finish {
        v.extend(row("Finish with", vec![Span::styled(t.text(f), t.command())]));
    }
    if let Some(th) = threshold {
        let grey = (th - adage_tracks::card::GREY_MARGIN).max(0.0);
        v.extend(row("Gets it", vec![Span::raw(format!("a prompt at similarity {} {th:.2}, once", t.ch("≥", ">=")))]));
        v.extend(row(
            "",
            vec![Span::styled(
                format!("{grey:.2}{}{th:.2}: once one of these files is edited", t.ch("–", "-")),
                t.ink2(),
            )],
        ));
    }
    v.extend(row("Limits", vec![Span::styled("prime at most, never strengthened", t.dim())]));
    v.extend(row("", vec![Span::styled("first-hand only, never shared", t.dim())]));
    v
}

/// The card for a group: what it holds, in plain words, and its track's directory (what
/// `adage review` takes).
fn group_card(n: &Node, s: &AppState, t: &Theme, width: usize) -> Vec<Line<'static>> {
    let Node::Group { key, title, sub, count, .. } = n else { return vec![] };
    let what = match key.as_str() {
        "nascent" => "Reflexes conditioning formed that wait for a person. Approve one to put it into service.",
        "quiet" => "Quiet reflexes: telling the agent changes little (uplift below +0.10), because agents already take these steps unaided. They are not install steps; approve one to deliver it anyway.",
        "instincts" => "Instincts ship with adage: defaults for repositories like this one, never learned.",
        "extinguished" => "Reflexes extinguished by review or by conditioning; kept so they stay extinguished.",
        _ => "A track: the reflexes conditioned for one repository.",
    };
    let mut v =
        kit::wrap_spans(vec![Span::styled(title.clone(), t.bold()), Span::styled(sub.clone(), t.dim())], width, 2);
    // What it holds, by strength.
    let members: Vec<&RouteRow> =
        s.route_rows().iter().filter(|r| &group_of(r) == key && s.in_scope(r) && passes(&s.reflexes, r)).collect();
    let mut held = vec![counted(count, format!(" reflex{}", if *count == 1 { "" } else { "es" }), t)];
    for a in [Strength::Fire, Strength::Prime, Strength::Inhibit] {
        let n = members.iter().filter(|r| r.strength == a).count();
        if n > 0 {
            let mut chip = kit::strength(a, t);
            chip.push(Span::styled(format!(" {n}"), t.bold()));
            held.push(chip);
        }
    }
    v.extend(chip_rows(vec![], held, Span::raw("   "), width, t));
    v.push(Line::raw(""));
    v.extend(kit::wrap(what, width).into_iter().map(Line::raw));
    if let Some(dir) = key.strip_prefix("track:") {
        v.push(Line::raw(""));
        v.extend(KeyValue::new().row("track", Span::styled(dir.to_string(), t.ink2())).lines(width, t));
    }
    v.push(Line::raw(""));
    v.push(Line::from(vec![kit::keycap("enter", t), Span::styled(" opens or closes the group", t.dim())]));
    if key == "nascent" || key == "quiet" {
        v.push(Line::from(vec![
            Span::styled("same as ", t.dim()),
            Span::styled("adage review", t.command()),
            Span::styled(" from any shell", t.dim()),
        ]));
    }
    v
}

/// `lines` in `area` from `scroll` down. When they do not all fit, the last row says how many
/// more there are, and the key that shows them.
fn scrolled(f: &mut Frame, area: Rect, lines: Vec<Line<'static>>, scroll: u16, t: &Theme) {
    let h = area.height as usize;
    if lines.len() <= h || h < 3 {
        f.render_widget(Paragraph::new(lines), area);
        return;
    }
    let body = h - 1;
    let top = (scroll as usize).min(lines.len() - body);
    let below = lines.len() - top - body;
    let mut shown: Vec<Line<'static>> = lines.into_iter().skip(top).take(body).collect();
    let hint = if below > 0 {
        format!("{} {below} more line{} below{}PgDn", t.ch("▾", "v"), if below == 1 { "" } else { "s" }, t.sep())
    } else {
        format!("{} the top{}PgUp", t.ch("▴", "^"), t.sep())
    };
    shown.push(Line::from(Span::styled(t.text(&hint), t.dim())));
    f.render_widget(Paragraph::new(shown), area);
}

pub fn draw(f: &mut Frame, area: Rect, s: &AppState, ctx: &Ctx) {
    let t = &ctx.theme;
    let rows = match &s.routes {
        None => {
            f.render_widget(Paragraph::new(Span::styled(format!(" loading reflexes{}", t.more()), t.dim())), area);
            return;
        }
        Some(Err(e)) => {
            f.render_widget(
                Paragraph::new(Span::styled(format!(" could not load reflexes: {e}"), t.red()))
                    .wrap(Wrap { trim: false }),
                area,
            );
            return;
        }
        Some(Ok(r)) => &r.rows,
    };
    if rows.is_empty() {
        let lines = empty("No reflexes yet.", "Agents' traces arrive automatically; or run", "adage condition", t);
        f.render_widget(Paragraph::new(view::wrap_lines(lines, area.width)), area);
        return;
    }
    if !rows.iter().any(|r| s.in_scope(r)) {
        let name = s.scope_key().map_or_else(String::new, scope::friendly);
        let lines = vec![
            Line::raw(""),
            Line::from(Span::raw(format!(" No reflexes for {name} yet."))),
            Line::from(vec![
                Span::styled(" Agents' traces there condition them automatically; or run ", t.dim()),
                Span::styled("adage condition", t.command()),
            ]),
            Line::from(vec![
                Span::styled(" f", t.key()),
                Span::styled(" shows another repository's reflexes, or every repository's.", t.dim()),
            ]),
        ];
        f.render_widget(Paragraph::new(view::wrap_lines(lines, area.width)), area);
        return;
    }
    let nodes = tree(s);
    // Wide, the card stands beside the list; otherwise under it, with what the list can spare.
    let (list, card_a) = if area.width >= 120 {
        let left = (area.width / 2).clamp(56, 80);
        let [l, rule, d] =
            Layout::horizontal([Constraint::Length(left), Constraint::Length(3), Constraint::Min(20)]).areas(area);
        let bar: Vec<Line> =
            (0..rule.height).map(|_| Line::from(Span::styled(format!(" {}", t.ch("│", "|")), t.rule()))).collect();
        f.render_widget(Paragraph::new(bar), rule);
        (l, d)
    } else {
        let groups = nodes.iter().filter(|n| matches!(n, Node::Group { .. })).count();
        let needed = (3 + nodes.len() + groups.saturating_sub(1)).min(u16::MAX as usize) as u16;
        let list_height = needed.min(area.height.saturating_mul(2) / 5);
        let [l, rule, d] =
            Layout::vertical([Constraint::Length(list_height), Constraint::Length(1), Constraint::Min(5)]).areas(area);
        f.render_widget(Paragraph::new(view::rule("", None, area.width, t)), rule);
        // The same one-column margin as every other pane's text.
        (l, d.inner(Margin { horizontal: 1, vertical: 0 }))
    };
    let [filter_a, chips_a, _, tree_a] =
        Layout::vertical([Constraint::Length(1), Constraint::Length(1), Constraint::Length(1), Constraint::Min(1)])
            .areas(list);
    let st = &s.reflexes;
    let filter_line = if st.filtering || !st.filter.is_empty() {
        let shown = rows.iter().filter(|r| s.in_scope(r) && passes(st, r)).count();
        let mut v = vec![Span::styled(" / ", t.key()), Span::styled(st.filter.clone(), t.bold())];
        if st.filtering {
            v.push(Span::styled(t.ch("▌", "_"), t.accent()));
        }
        v.push(Span::styled(format!("   {shown} shown"), t.dim()));
        Line::from(v)
    } else {
        Line::from(vec![Span::styled(" / ", t.key()), Span::styled("filter reflexes, commands, files", t.dim())])
    };
    f.render_widget(Paragraph::new(filter_line), filter_a);
    f.render_widget(Paragraph::new(chips(s, t, list.width as usize)), chips_a);
    if nodes.is_empty() {
        f.render_widget(
            Paragraph::new(Span::styled(" Nothing matches. esc resets the search and filter chips.", t.dim()))
                .wrap(Wrap { trim: false }),
            tree_a,
        );
        return;
    }
    let sel = position(s, &nodes);
    let (lines, sel_line) = tree_lines(s, &nodes, sel, t, tree_a.width as usize);
    let h = tree_a.height as usize;
    let offset = sel_line.saturating_sub(h.saturating_sub(2)).min(lines.len().saturating_sub(h));
    f.render_widget(Paragraph::new(t.lines(lines)).scroll((offset as u16, 0)), tree_a);

    let width = card_a.width as usize;
    let d = match &nodes[sel] {
        Node::Reflex(i) => overview(&rows[*i], s, ctx, width, card_a.height >= 14, progress(st, t)),
        g => group_card(g, s, t, width),
    };
    let d = kit::wrap_lines(t.lines(d), width, 0);
    scrolled(f, card_a, d, st.detail_scroll, t);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(lines: &[Line]) -> Vec<String> {
        lines.iter().map(|l| l.spans.iter().map(|s| s.content.replace(NBSP, " ")).collect()).collect()
    }

    fn row(gist: &str, trigger: &str) -> RouteRow {
        RouteRow {
            reference: "acme/learned".into(),
            track: "acme/learned".into(),
            version: "1.0.0".into(),
            repo: "acme".into(),
            imported: false,
            in_index: true,
            instinct: false,
            id: "run-137805".into(),
            gist: gist.into(),
            strength: Strength::Fire,
            ceiling: "fire".into(),
            effect: "verify".into(),
            state: "firm".into(),
            authored: false,
            approved: false,
            trigger_kind: "after_edit".into(),
            trigger: trigger.into(),
            step_kind: "run".into(),
            step: "cargo build -p adage-cli".into(),
            executable: true,
            support: 22,
            opportunities: 35,
            rate: 0.63,
            lower_bound: 0.46,
            turns_saved: 0.0,
            live: Default::default(),
            with: Default::default(),
            control: Default::default(),
            directive: String::new(),
            review_preview: None,
            verbs: vec![],
            reasons: vec![],
            execution: Default::default(),
            more: Default::default(),
        }
    }

    #[test]
    fn rows_read_trigger_then_action() {
        let r = row("after editing crates/**/*.rs, run cargo fmt --all", "an edit touches crates/**/*.rs");
        assert_eq!(says(&r), ("after editing crates/**/*.rs".into(), "cargo fmt --all".into(), true));
        // A sequence's trigger keeps its own commas; the step is after the last `, run `.
        let r = row("after editing src/**, then make build, run make test", "");
        assert_eq!(says(&r), ("after editing src/**, then make build".into(), "make test".into(), true));
        let r = row("after bin/fetch-incident, read the files it names", "");
        assert_eq!(says(&r), ("after bin/fetch-incident".into(), "read the files it names".into(), false));
        // Advice is all action, after its trigger in plain words.
        let r = row("Cargo.lock: let cargo update it; don't edit it by hand", "an edit touches `Cargo.lock`");
        assert_eq!(
            says(&r),
            ("an edit touches Cargo.lock".into(), "let cargo update it; don't edit it by hand".into(), false)
        );
        let r = row("use pnpm here, not npm", "a prompt names a ticket");
        assert_eq!(says(&r).1, "use pnpm here, not npm");
        // No gist: the id, the last resort.
        assert_eq!(says(&row("", "a prompt")).1, "run-137805");
    }

    #[test]
    fn titles_are_the_gist_in_plain_words() {
        let r = row("after editing crates/**/*.rs, run cargo fmt --all", "");
        assert_eq!(title_of(&r), "After editing crates/**/*.rs, run cargo fmt --all");
        // A path keeps its case; an empty gist falls back to the id.
        assert_eq!(
            title_of(&row("crates/x/assets.rs: generated; regenerate it", "")),
            "crates/x/assets.rs: generated; regenerate it"
        );
        assert_eq!(title_of(&row("Cargo.lock: let cargo update it", "")), "Cargo.lock: let cargo update it");
        assert_eq!(title_of(&row("", "")), "run-137805");
    }

    #[test]
    fn rows_say_triggers_briefly_in_words() {
        assert_eq!(short_when("after editing crates/**/*.rs"), "edit crates/**/*.rs");
        assert_eq!(short_when("an edit touches Cargo.lock"), "edit Cargo.lock");
        assert_eq!(short_when("when npm test fails"), "npm test fails");
        assert_eq!(short_when("on a prompt that names a ticket"), "a prompt names a ticket");
        assert_eq!(short_when("at the start of a session"), "session start");
        assert_eq!(short_when("after bin/fetch-incident"), "after bin/fetch-incident");
    }

    #[test]
    fn a_long_trigger_keeps_its_paths_tail() {
        let t = Theme::with_color(false);
        assert_eq!(fit_when("edit crates/adage-cli/src/*.rs", 16, &t), "edit …/src/*.rs");
        assert_eq!(fit_when("edit crates/adage-cli/src/*.rs", 24, &t), "edit …adage-cli/src/*.rs", "already a part");
        assert_eq!(fit_when("after editing crates/adage-cli/src/*.rs", 26, &t), "after editing …/src/*.rs");
        assert_eq!(
            fit_when("after editing crates/adage-cli/src/*.rs", 80, &t),
            "after editing crates/adage-cli/src/*.rs"
        );
        // No path at the end, or too little room for one: cut at the right.
        assert_eq!(fit_when("on a prompt that names a ticket", 12, &t), "on a prompt…");
        assert_eq!(fit_when("after editing crates/adage-cli/src/*.rs", 16, &t), "after editing c…");
        assert_eq!(fit_when("after editing crates/adage-cli/src/*.rs", 26, &t.ascii()), "after editing .../src/*.rs");
    }

    #[test]
    fn list_meters_never_round_some_to_none_or_most_to_all() {
        assert_eq!(cells(0.0, 6), 0);
        assert_eq!(cells(f64::NAN, 6), 0);
        assert_eq!(cells(0.01, 6), 1, "any evidence shows");
        assert_eq!(cells(22.0 / 39.0, 6), 3);
        assert_eq!(cells(22.0 / 35.0, 6), 4);
        assert_eq!(cells(24.0 / 25.0, 6), 5, "only all of it fills every cell");
        assert_eq!(cells(1.0, 6), 6);
        assert_eq!(cells(3.0, 6), 6);
        let t = Theme::with_color(false);
        assert_eq!(text(&[Line::from(cell_meter(22.0 / 35.0, 6, t.ink2(), &t))]), ["▰▰▰▰▱▱"]);
        assert_eq!(text(&[Line::from(cell_meter(22.0 / 35.0, 6, t.ink2(), &t.ascii()))]), ["####.."]);
    }

    #[test]
    fn evidence_cells_say_what_the_share_is_of() {
        let mut r = row("", "");
        assert_eq!(ev(&r), Ev::Share(22.0 / 35.0, "22/35".into()));
        r.more.value = Some("value: saves ~1.25 calls".into());
        assert_eq!(ev(&r), Ev::Word("by value"), "a follow count would mislead");
        r.more.value = None;
        (r.opportunities, r.instinct) = (0, true);
        assert_eq!(ev(&r), Ev::Word("built in"));
        r.instinct = false;
        assert_eq!(ev(&r), Ev::Word("–"));
        // The lifecycle shows only while a reflex is not firm, and not under its own group.
        r.state = "forming".into();
        assert_eq!(badge(&r), Some("forming"));
        r.state = "nascent".into();
        assert_eq!(badge(&r), None, "the Waiting for review group says it");
        r.state = "firm".into();
        assert_eq!(badge(&r), None);
    }

    #[test]
    fn modes_read_as_what_the_reflex_may_do() {
        assert_eq!(mode_chip("Suggest"), ("Suggests", Tone::Muted, false));
        assert_eq!(mode_chip("Automatic"), ("Runs with your grant", Tone::Ok, false));
        assert_eq!(mode_chip("Needs recovery"), ("Needs recovery", Tone::Warn, true));
        assert!(mode_chip("Disabled").2, "what stops it is said beside it");
    }

    #[test]
    fn meters_fill_eased_and_settle() {
        assert_eq!(fill(0), 0.0);
        assert!(fill(50) > 0.25, "eased out: quick at first");
        assert!(fill(100) > fill(50) && fill(150) > fill(100) && fill(199) < 1.0);
        assert_eq!(fill(FILL_MS), 1.0);
        assert_eq!(fill(10_000), 1.0);
        let mut st = State::default();
        let now = Instant::now();
        assert!(!animating(&st, now));
        st.moved_at = Some(now);
        assert!(animating(&st, now));
        assert!(!animating(&st, now + Duration::from_millis(FILL_MS)));
        // Tests (and reduced motion) draw the settled frame.
        assert_eq!(progress(&st, &Theme::with_color(true)), 1.0);
    }

    #[test]
    fn chips_wrap_whole_under_their_label() {
        let t = Theme::with_color(false);
        let chips = ["3 fired", "2 followed", "1 not observed", "0 finish line met"]
            .iter()
            .map(|c| vec![Span::raw(c.replace(' ', NBSP))])
            .collect();
        let lines = chip_rows(vec![Span::raw("Since  ")], chips, Span::raw(" · "), 36, &t);
        assert_eq!(text(&lines), ["Since  3 fired · 2 followed", "       1 not observed", "       0 finish line met"]);
    }

    #[test]
    fn the_arc_is_boxes_when_there_is_room_and_a_line_when_not() {
        let t = Theme::with_color(false);
        let r = row("", "an edit touches crates/adage-cli/src/*.rs");
        let boxes = text(&arc(&r, &t, 72, true));
        assert_eq!(boxes.len(), 4, "{boxes:#?}");
        assert!(boxes[0].starts_with("╭─ WHEN ─") && boxes[0].contains("╭─ DO ─"), "{boxes:#?}");
        assert!(boxes[1].contains("│─── ◠ ──▶│ cargo build -p adage-cli │"), "{boxes:#?}");
        assert!(boxes[2].contains("│ crates/adage-cli/src/*.rs │"), "the path stays whole: {boxes:#?}");
        assert!(boxes.iter().all(|l| l.chars().count() <= 72), "{boxes:#?}");
        let line = text(&arc(&r, &t, 100, false));
        assert_eq!(line, ["when an edit touches crates/adage-cli/src/*.rs ──▶ ◠ ──▶ do cargo build -p adage-cli"]);
        // Short of room it breaks between its parts, never inside the command.
        let line = text(&arc(&r, &t, 72, false));
        assert_eq!(line, ["when an edit touches crates/adage-cli/src/*.rs ──▶ ◠ ──▶", "  do cargo build -p adage-cli"]);
        let narrow = text(&arc(&r, &t, 40, true));
        assert!(narrow.iter().all(|l| l.chars().count() <= 40) && narrow[1].starts_with("  "), "{narrow:#?}");
        let ascii = text(&arc(&r, &t.ascii(), 72, true));
        assert!(ascii.iter().all(|l| l.is_ascii()), "{ascii:#?}");
    }
}
