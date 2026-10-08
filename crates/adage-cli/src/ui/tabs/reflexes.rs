//! The Reflexes tab: a tree of every reflex by track, then the nascent ones waiting for review
//! (install steps), the quiet ones (uplift below the bar: agents already do this, so they wait for
//! a person), the instincts and the extinguished; filter chips for strength and lifecycle; a
//! fuzzy `/` filter; and a detail pane: what the agent sees, the evidence (a value estimate when
//! that is what the strength rests on, the uplift, a finish line's obligation), a procedure
//! card's contents, with-vs-control when there are control traces, the lifecycle, and the review
//! actions with the `adage review` command each one equals.

use super::{empty, head};
use crate::ui::app::{Action, AppState, Cmd, Modal, WantKind};
use crate::ui::feed::{self, Act};
use crate::ui::fuzzy;
use crate::ui::routes::{RouteRow, ACTIONS};
use crate::ui::theme::Theme;
use crate::ui::view::{self, Ctx};
use crate::ui::words;
use adage_term::Glyph;
use adage_tracks::Strength;
use ratatui::crossterm::event::KeyCode;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Wrap};
use ratatui::Frame;
use std::collections::BTreeSet;

/// The lifecycle filter's choices, in the order `S` cycles them.
pub const STATES: [&str; 7] = ["all states", "firm", "forming", "nascent", "quiet", "dormant", "extinguished"];

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
    for r in rows {
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
        let members: Vec<usize> =
            (0..rows.len()).filter(|&i| group_of(&rows[i]) == g && passes(st, &rows[i])).collect();
        if members.is_empty() {
            continue;
        }
        let first = &rows[members[0]];
        let (title, sub) = match g.as_str() {
            "nascent" => ("nascent".to_string(), " · waits for review".to_string()),
            "quiet" => ("quiet".to_string(), " · agents already do this; approve to deliver".to_string()),
            "instincts" => {
                let tracks: BTreeSet<String> =
                    members.iter().map(|&i| format!("{}@{}", rows[i].track, rows[i].version)).collect();
                let sub = if tracks.len() == 1 {
                    format!(" · {}", tracks.iter().next().cloned().unwrap_or_default())
                } else {
                    String::new()
                };
                ("instincts".to_string(), sub)
            }
            "extinguished" => ("extinguished".to_string(), format!(" · {}", members.len())),
            _ => {
                (first.reference.clone(), format!("@{}{}", first.version, if first.imported { " · pack" } else { "" }))
            }
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

/// Select a reflex (from the palette or the map), opening its group.
pub fn select(s: &mut AppState, reference: &str, id: &str) {
    let Some(r) = s.route_rows().iter().find(|r| r.reference == reference && r.id == id).cloned() else { return };
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
    st.detail_scroll = 0;
}

fn move_by(s: &mut AppState, delta: isize) {
    let nodes = tree(s);
    if nodes.is_empty() {
        return;
    }
    let i = position(s, &nodes) as isize + delta;
    let i = i.clamp(0, nodes.len() as isize - 1) as usize;
    s.reflexes.sel = Some(node_key(s, &nodes[i]));
    s.reflexes.detail_scroll = 0;
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
        KeyCode::Char('F') => s.reflexes.hidden[0] = !s.reflexes.hidden[0],
        KeyCode::Char('P') => s.reflexes.hidden[1] = !s.reflexes.hidden[1],
        KeyCode::Char('I') => s.reflexes.hidden[2] = !s.reflexes.hidden[2],
        KeyCode::Char('S') => s.reflexes.states = (s.reflexes.states + 1) % STATES.len(),
        KeyCode::Char('R') => return Some(Cmd::Want(WantKind::Routes)),
        KeyCode::Enter | KeyCode::Left | KeyCode::Right | KeyCode::Char('h') | KeyCode::Char('l') => {
            let nodes = tree(s);
            let n = nodes.get(position(s, &nodes)).cloned()?;
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
                s.reflexes.sel = Some(format!("group:{g}"));
            }
        }
        KeyCode::Char('t') => {
            let r = s.selected_route()?.clone();
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
            s.modal =
                Some(Modal::Viewer { title: format!("recent traces · {}", r.id), source: None, lines, scroll: 0 });
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
            let word = words::verb(verb);
            if !r.verbs.contains(&verb) {
                let open: Vec<&str> = r.verbs.iter().map(|v| words::verb(v)).collect();
                s.message(
                    "Not available",
                    vec![
                        format!("{word} does not apply to {} as it stands.", r.id),
                        format!("Available: {}", open.join(", ")),
                    ],
                    true,
                );
                return None;
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
                return None;
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
            lines.push(Line::raw(
                "Confirmation applies only to this exact reviewed version. Enter/y confirms; Esc/n cancels.",
            ));
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
        _ => {}
    }
    None
}

// ---------------------------------------------------------------------------------------------
// Drawing

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

/// The evidence column of a row: `23/33`; `value` when the strength rests on a value estimate
/// (a follow count would mislead); a card's traces.
fn evidence_cell(r: &RouteRow) -> String {
    match &r.more.card {
        Some(c) => format!("{} trace{}", c.sessions, if c.sessions == 1 { "" } else { "s" }),
        None if r.more.value.is_some() => "value".into(),
        None => r.count(),
    }
}

fn lamp_spans(r: &RouteRow, t: &Theme) -> Vec<Span<'static>> {
    let glyph = kind_glyph(r);
    let lamp = if r.quiet() {
        Span::styled(format!("{} ", t.glyph(Glyph::Quiet)), t.dim())
    } else if r.nascent() || r.extinguished() {
        Span::styled(format!("{} ", t.glyph(Glyph::Off)), t.dim())
    } else if t.color {
        Span::styled(format!("{} ", t.glyph(Glyph::Lamp)), t.strength(r.strength))
    } else {
        Span::styled(format!("{} ", Theme::strength_word(r.strength)[..1].to_ascii_uppercase()), t.bold())
    };
    vec![lamp, Span::styled(format!("{} ", t.glyph(glyph)), t.accent())]
}

fn chips(s: &AppState, t: &Theme, width: usize) -> Line<'static> {
    let rows = s.route_rows();
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

fn tree_lines(s: &AppState, nodes: &[Node], sel: usize, t: &Theme, width: usize) -> (Vec<Line<'static>>, usize) {
    let rows = s.route_rows();
    let id_w = width.saturating_sub(3 + 4 + 9 + 7).clamp(10, 24);
    // One evidence width for the tree, so the stage column lines up row to row.
    let ev_w = rows.iter().map(|r| evidence_cell(r).chars().count()).max().unwrap_or(5).min(12);
    let mut out = Vec::new();
    let mut sel_line = 0;
    for (k, n) in nodes.iter().enumerate() {
        let chosen = k == sel;
        let bar = if chosen { Span::styled(t.ch("▌", ">"), t.accent()) } else { Span::raw(" ") };
        match n {
            Node::Group { title, sub, open, count, key } => {
                if k > 0 {
                    out.push(Line::raw(""));
                }
                if chosen {
                    sel_line = out.len();
                }
                let arrow = if *open { t.ch("▾", "v") } else { t.ch("▸", ">") };
                let sub = if !open && key.starts_with("track:") { format!("{sub} · {count}") } else { sub.clone() };
                let sub = t.text(&sub);
                let title_style = if key == "extinguished" {
                    t.dim()
                } else if chosen {
                    t.bold()
                } else {
                    Style::default()
                };
                out.push(Line::from(vec![
                    bar,
                    Span::styled(format!("{arrow} "), t.dim()),
                    Span::styled(t.fit(&t.text(title), width.saturating_sub(4)), title_style),
                    Span::styled(t.fit(&sub, width.saturating_sub(4 + title.chars().count())), t.dim()),
                ]));
            }
            Node::Reflex(i) => {
                let r = &rows[*i];
                if chosen {
                    sel_line = out.len();
                }
                let mut spans = vec![bar, Span::raw("  ")];
                spans.extend(lamp_spans(r, t));
                // What it tells the agent, never its id (docs/design/everyday-ux.md, rule 4): the
                // sentence takes the room the stage and the evidence leave; the id heads the
                // detail pane, for commands.
                let evidence = t.text(&evidence_cell(r));
                let used: usize = spans.iter().map(|s| s.width()).sum();
                let say = if r.gist.is_empty() { r.id.clone() } else { words::list_label(&r.gist, &t.sep()) };
                let room = width.saturating_sub(used + 9 + ev_w + 3).max(id_w);
                spans.push(Span::styled(
                    t.pad(&t.fit(&t.text(&say), room), room + 2),
                    if chosen { t.bold() } else { Style::default() },
                ));
                spans.push(Span::styled(t.pad(r.stage(), 9), t.ink2()));
                spans.push(Span::styled(evidence, t.dim()));
                out.push(Line::from(spans));
            }
        }
    }
    (out, sel_line)
}

/// A line of evidence that says where it comes from: `from conditioning: 24/25 traces took it`.
fn sourced(source: &str, text: String, t: &Theme) -> Line<'static> {
    Line::from(vec![Span::raw("  "), Span::styled(format!("{source}: "), t.dim()), Span::raw(text)])
}

fn bar(took: u64, applied: u64, style: Style, t: &Theme) -> Span<'static> {
    let n = if applied == 0 { 0 } else { ((took as f64 / applied as f64) * 20.0).round() as usize };
    let n = if took > 0 { n.max(1) } else { 0 };
    Span::styled(format!("{}{}", t.ch("█", "#").repeat(n), " ".repeat(20 - n.min(20))), style)
}

/// The detail pane for one reflex.
pub fn detail(r: &RouteRow, s: &AppState, ctx: &Ctx) -> Vec<Line<'static>> {
    let t = &ctx.theme;
    let mut v = Vec::new();
    v.push(Line::from(vec![
        Span::styled(format!("{} ", t.glyph(kind_glyph(r))), t.accent()),
        Span::styled(r.id.clone(), t.bold()),
    ]));
    // Execution authority is separate from learned strength. Keep the action and any
    // uncertain outcome above the evidence, including in the stacked narrow layout.
    let fact = |key: &str, value: String| {
        Line::from(vec![Span::styled(format!("{key:<9}"), t.dim()), Span::raw(t.text(&value))])
    };
    v.push(fact("mode", format!("{}{}{}", r.execution.mode, t.sep(), r.execution.reason)));
    v.push(fact("when", r.trigger.clone()));
    v.push(fact("do", r.step.clone()));
    if let Some(result) = &r.execution.last_result {
        v.push(fact("last run", result.clone()));
    }
    let mut facts = vec![r.stage().to_string(), format!("{}@{}", r.track, r.version)];
    if r.is_card() {
        facts.push("procedure card".into());
    }
    if r.more.skill {
        facts.push("from a skill".into());
    } else if r.imported {
        facts.push("from a pack".into());
    }
    if let Some(ts) = last_fired(s, &r.id) {
        facts.push(format!("last fired {}", view::clock(ts, ctx.tz, true)));
    }
    let mut line = vec![];
    if t.color {
        line.push(Span::styled(format!("{} ", t.glyph(Glyph::Lamp)), t.strength(r.strength)));
        line.push(Span::styled(Theme::strength_word(r.strength), t.strength(r.strength).patch(t.bold())));
    } else {
        line.push(Span::styled(Theme::strength_word(r.strength).to_ascii_uppercase(), t.bold()));
    }
    line.push(Span::styled(format!("{}{}", t.sep(), facts.join(&t.sep())), t.ink2()));
    v.push(Line::from(line));
    // Quiet is not an install step: its own mark, and why, in one line.
    if let Some(q) = r.more.quiet_line.as_ref().filter(|_| r.quiet()) {
        v.push(Line::from(vec![
            Span::styled(format!("{} ", t.glyph(Glyph::Quiet)), t.dim()),
            Span::styled(t.text(q), t.ink2()),
        ]));
    }
    for (k, d) in r.reasons.iter().filter(|(k, _)| !(k == "quiet" && r.more.quiet_line.is_some())) {
        v.push(Line::from(vec![
            Span::styled(format!("{} waits for review: ", t.glyph(Glyph::Off)), t.yellow()),
            Span::styled(format!("{k}{}", t.sep()), t.yellow().patch(t.bold())),
            Span::styled(t.text(d), t.ink2()),
        ]));
    }
    v.push(Line::raw(""));
    v.push(head("ADVISORY PREVIEW (EXAMPLE VALUES)", t));
    if r.more.execution.is_some() {
        v.push(Line::from(Span::styled("  Execution-only candidate; no advisory reminder is delivered.", t.dim())));
    } else if r.directive.trim().is_empty() {
        v.push(Line::from(Span::styled("  (no preview: the reflex has no directive text)", t.dim())));
    } else {
        v.extend(view::directive_lines(&r.directive, t, ""));
    }
    v.push(Line::raw(""));
    // Two sources, each named: the traces conditioning read (history), and what the hooks have
    // recorded since the reflex was installed (live).
    v.push(head("EVIDENCE", t));
    if let Some(e) = &r.more.execution {
        v.push(sourced(
            "from conditioning",
            format!(
                "{}/{} matching situations with supported continuations across {} traces",
                e.successful, e.opportunities, e.sessions
            ),
            t,
        ));
        v.push(Line::from(Span::styled(
            "  Observed routine; avoided agent work has not been established by these counts.",
            t.dim(),
        )));
    } else if let Some(value) = &r.more.value {
        // Decided on the calls it saves, not on how often traces took it.
        v.push(sourced("from conditioning", value.clone(), t));
    } else if let Some(c) = &r.more.card {
        v.push(sourced(
            "from conditioning",
            format!(
                "a family of {} similar trace{}, {} passed the project's checks",
                c.sessions,
                if c.sessions == 1 { "" } else { "s" },
                c.passed
            ),
            t,
        ));
    } else if r.opportunities == 0 {
        v.push(sourced(
            "from conditioning",
            if r.instinct {
                "none, an instinct ships with Adage and has no evidence of its own".into()
            } else {
                "no evidence recorded".into()
            },
            t,
        ));
    } else {
        let mut facts = vec![
            format!("{}/{} traces took it", r.support, r.opportunities),
            format!("lower bound {:.2}", r.lower_bound),
        ];
        if r.turns_saved > 0.0 {
            facts.push(format!("{:.0} turns saved per trace", r.turns_saved));
        }
        v.push(sourced("from conditioning", facts.join(&t.sep()), t));
    }
    let l = &r.live;
    v.push(sourced(
        "live since installed",
        format!(
            "fired {}{sep}followed {}{sep}not observed within 3 batches {}{sep}finish line met {}",
            l.emitted,
            l.followed,
            l.ignored,
            l.met,
            sep = t.sep()
        ),
        t,
    ));
    if let Some(u) = &r.more.uplift {
        v.push(sourced("uplift", u.trim_start_matches("uplift ").to_string(), t));
    }
    if let Some(o) = &r.more.obligation {
        v.push(sourced("enforced", o.trim_start_matches("finish line: ").to_string(), t));
    }
    if let Some(c) = &r.more.card {
        v.push(Line::raw(""));
        v.extend(card_lines(c, r.more.card_threshold, t));
    }
    if r.control.applied > 0 && r.with.applied > 0 {
        v.push(Line::raw(""));
        v.push(head("WITH THE REFLEX vs CONTROL", t));
        v.push(sourced("live since installed", "traces where the agent took the step".into(), t));
        v.push(Line::from(vec![
            Span::raw("  "),
            Span::styled(t.pad("with the reflex", 17), t.ink2()),
            bar(r.with.took, r.with.applied, t.accent(), t),
            Span::raw(format!("  {}/{}", r.with.took, r.with.applied)),
        ]));
        v.push(Line::from(vec![
            Span::raw("  "),
            Span::styled(t.pad("control", 17), t.ink2()),
            bar(r.control.took, r.control.applied, t.dim(), t),
            Span::styled(format!("  {}/{}", r.control.took, r.control.applied), t.ink2()),
        ]));
    }
    v.push(Line::raw(""));
    v.push(head("LIFECYCLE", t));
    let now = r.lifecycle();
    let rank = words::lifecycle_rank(now);
    let dash = format!(" {} ", t.ch("────", "----"));
    let mut spans = vec![Span::raw("  ")];
    for (k, stage) in ["nascent", "forming", "firm"].into_iter().enumerate() {
        if k > 0 {
            spans.push(Span::styled(dash.clone(), t.rule()));
        }
        let reached = k <= rank;
        let (mark, style) = match (reached, stage) {
            (false, _) => (t.glyph(Glyph::Off), t.dim()),
            (true, "firm") => (t.glyph(Glyph::Lamp), t.green()),
            (true, "forming") => (t.glyph(Glyph::Lamp), t.accent()),
            (true, _) => (t.glyph(Glyph::Off), t.dim()),
        };
        spans.push(Span::styled(format!("{mark} "), style));
        spans.push(Span::styled(stage, if stage == now { t.bold() } else { t.ink2() }));
    }
    if rank == 2 && now != "firm" {
        spans.push(Span::styled(dash.clone(), t.rule()));
        spans.push(Span::styled(format!("{} ", t.glyph(Glyph::Lamp)), t.dim()));
        spans.push(Span::styled(now.to_string(), t.bold()));
    }
    v.push(Line::from(spans));
    let mut notes = vec![format!(
        "ceiling {}",
        crate::ui::theme::parse_strength(&r.ceiling)
            .map_or_else(|| r.ceiling.clone(), |a| Theme::strength_word(a).to_string())
    )];
    notes.push(format!("effect {}", r.effect));
    if r.authored {
        notes.push("pinned".into());
    }
    if !r.in_index {
        notes.push("not in the index".into());
    }
    v.push(Line::from(Span::styled(format!("  {}", notes.join(&t.sep())), t.dim())));
    v.push(Line::raw(""));
    v.push(head("ACTIONS", t));
    let mut acts = vec![Span::raw("  ")];
    for (k, verb) in ACTIONS {
        let open = r.verbs.contains(&verb);
        acts.push(Span::styled(k.to_string(), if open { t.key() } else { t.dim() }));
        acts.push(Span::styled(format!(" {}   ", words::verb(verb)), if open { t.ink2() } else { t.dim() }));
    }
    v.push(Line::from(acts));
    let n = traces(s, &r.id, &r.repo).len();
    v.push(Line::from(vec![
        Span::raw("  "),
        Span::styled("d", t.key()),
        Span::styled(" delegate   ", t.ink2()),
        Span::styled("t", t.key()),
        Span::styled(format!(" traces ({n})   "), t.ink2()),
        Span::styled("o", t.key()),
        Span::styled(" open in map", t.ink2()),
    ]));
    // A nascent reflex (an install step, or a quiet one) waits to be approved: that command first.
    let order = if r.nascent() {
        ["approve", "weaken", "strengthen", "extinguish"]
    } else {
        ["weaken", "strengthen", "extinguish", "approve"]
    };
    if let Some(verb) = order.into_iter().find(|v| r.verbs.contains(v)) {
        v.push(Line::from(vec![
            Span::styled("  same as ", t.dim()),
            Span::styled(words::review_command(verb, &r.reference, &r.id), t.command()),
            Span::styled(" from any shell", t.dim()),
        ]));
    }
    v
}

/// A procedure card's contents: its files in order, the commands with their replay verdicts,
/// what to avoid, the finish check, when a prompt gets it, and what it never does.
fn card_lines(c: &adage_tracks::card::Card, threshold: Option<f32>, t: &Theme) -> Vec<Line<'static>> {
    let mut v = vec![head("PROCEDURE CARD", t)];
    let of = |n: u32| format!("{n}/{}", c.sessions);
    let path_w = c
        .files
        .iter()
        .map(|f| f.path.chars().count())
        .chain(c.commands.iter().chain(c.avoid.iter()).map(|x| x.command.chars().count()))
        .max()
        .unwrap_or(10)
        .clamp(10, 36);
    if !c.files.is_empty() {
        v.push(Line::from(Span::styled("  files, in the order they were first edited", t.dim())));
        for (k, f) in c.files.iter().enumerate() {
            v.push(Line::from(vec![
                Span::styled(format!("   {:>2} ", k + 1), t.dim()),
                Span::raw(t.pad(&t.text(&f.path), path_w + 2)),
                Span::styled(format!("{} traces", of(f.sessions)), t.ink2()),
            ]));
        }
    }
    if !c.commands.is_empty() {
        v.push(Line::from(Span::styled("  commands", t.dim())));
        for x in &c.commands {
            // `adage train cards` replays them on a clean checkout: verified, failed there, or not
            // replayed (never run, or it needed the network or a cache: unverifiable is not stale).
            let (mark, style, verdict) = match x.verified {
                Some(true) => (t.glyph(Glyph::Ok), t.green(), "verified on a clean checkout"),
                Some(false) => (t.glyph(Glyph::Fail), t.red(), "failed on a clean checkout"),
                None => (t.glyph(Glyph::Off), t.dim(), "not replayed"),
            };
            v.push(Line::from(vec![
                Span::styled(format!("    {mark} "), style),
                Span::styled(t.pad(&t.text(&x.command), path_w + 2), t.command()),
                Span::styled(format!("{} traces{}{verdict}", of(x.sessions), t.sep()), t.ink2()),
            ]));
        }
    }
    if !c.avoid.is_empty() {
        v.push(Line::from(Span::styled("  avoid", t.dim())));
        for x in &c.avoid {
            v.push(Line::from(vec![
                Span::styled(format!("    {} ", t.glyph(Glyph::Fail)), t.red()),
                Span::raw(t.pad(&t.text(&x.command), path_w + 2)),
                Span::styled(t.text(&x.error), t.ink2()),
            ]));
        }
    }
    if let Some(f) = &c.finish {
        v.push(Line::from(vec![Span::styled("  finish with  ", t.dim()), Span::styled(t.text(f), t.command())]));
    }
    if let Some(th) = threshold {
        let grey = (th - adage_tracks::card::GREY_MARGIN).max(0.0);
        v.push(Line::from(vec![
            Span::styled("  match  ", t.dim()),
            Span::raw(format!("a prompt at similarity {} {th:.2} gets it once", t.ch("≥", ">="))),
        ]));
        v.push(Line::from(vec![
            Span::styled("  grey zone  ", t.dim()),
            Span::raw(format!("{grey:.2}{}{th:.2}: at the first edit of one of these files instead", t.ch("–", "-"))),
        ]));
    }
    v.push(Line::from(Span::styled(
        format!("  prime at most, never strengthened{}first-hand only, never shared", t.sep()),
        t.dim(),
    )));
    v
}

fn group_detail(n: &Node, t: &Theme) -> Vec<Line<'static>> {
    let Node::Group { key, title, sub, count, .. } = n else { return vec![] };
    let what = match key.as_str() {
        "nascent" => "Reflexes conditioning formed that wait for a person. Approve one to put it into service.",
        "quiet" => "Quiet reflexes: telling the agent changes little (uplift below +0.10), because agents already take these steps unaided. They are not install steps; approve one to deliver it anyway.",
        "instincts" => "Instincts ship with Adage: defaults for repositories like this one, never learned.",
        "extinguished" => "Reflexes extinguished by review or by conditioning; kept so they stay extinguished.",
        _ => "A track: the reflexes conditioned for one repository.",
    };
    let mut v = vec![
        Line::from(vec![Span::styled(title.clone(), t.bold()), Span::styled(sub.clone(), t.dim())]),
        Line::from(Span::styled(format!("{count} reflex{}", if *count == 1 { "" } else { "es" }), t.ink2())),
        Line::raw(""),
        Line::raw(what),
        Line::raw(""),
        Line::from(vec![Span::styled("enter", t.key()), Span::styled(" opens or closes the group", t.dim())]),
    ];
    if key == "nascent" || key == "quiet" {
        v.push(Line::from(vec![
            Span::styled("same as ", t.dim()),
            Span::styled("adage review", t.command()),
            Span::styled(" from any shell", t.dim()),
        ]));
    }
    v
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
    let nodes = tree(s);
    let wide = area.width >= 90;
    let (list, detail_a) = if wide {
        let left = (area.width * 2 / 5).clamp(40, 56);
        let [l, rule, d] =
            Layout::horizontal([Constraint::Length(left), Constraint::Length(3), Constraint::Min(20)]).areas(area);
        let bar: Vec<Line> = (0..rule.height).map(|_| Line::from(Span::styled(t.ch("│", "|"), t.rule()))).collect();
        f.render_widget(Paragraph::new(bar), rule);
        (l, d)
    } else {
        let groups = nodes.iter().filter(|n| matches!(n, Node::Group { .. })).count();
        let needed = (3 + nodes.len() + groups.saturating_sub(1)).min(u16::MAX as usize) as u16;
        let list_height = needed.min(area.height.saturating_mul(45) / 100);
        let [l, rule, d] =
            Layout::vertical([Constraint::Length(list_height), Constraint::Length(1), Constraint::Min(5)]).areas(area);
        f.render_widget(Paragraph::new(view::rule("", None, area.width, t)), rule);
        // The same one-column margin as every other pane's text.
        (l, d.inner(ratatui::layout::Margin { horizontal: 1, vertical: 0 }))
    };
    let [filter_a, chips_a, _, tree_a] =
        Layout::vertical([Constraint::Length(1), Constraint::Length(1), Constraint::Length(1), Constraint::Min(1)])
            .areas(list);
    let st = &s.reflexes;
    let filter_line = if st.filtering || !st.filter.is_empty() {
        let shown = rows.iter().filter(|r| passes(st, r)).count();
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
    f.render_widget(Paragraph::new(lines).scroll((offset as u16, 0)), tree_a);

    let d = match &nodes[sel] {
        Node::Reflex(i) => detail(&rows[*i], s, ctx),
        g => group_detail(g, t),
    };
    let d = view::wrap_lines(t.lines(d), detail_a.width);
    f.render_widget(Paragraph::new(d).scroll((s.reflexes.detail_scroll, 0)), detail_a);
}
