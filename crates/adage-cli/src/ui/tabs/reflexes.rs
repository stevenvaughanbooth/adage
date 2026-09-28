//! The Reflexes tab: a tree of every reflex by track, then the nascent ones waiting for review,
//! the instincts and the extinguished; filter chips for strength and lifecycle; a fuzzy `/`
//! filter; and a detail pane: what the agent sees, the evidence, with-vs-control when there are
//! control traces, the lifecycle, and the review actions with the `adage review` command each
//! one equals.

use super::{empty, head};
use crate::ui::app::{Action, AppState, Cmd, Modal, WantKind};
use crate::ui::feed::{self, Act};
use crate::ui::fuzzy;
use crate::ui::routes::{RouteRow, ACTIONS};
use crate::ui::theme::Theme;
use crate::ui::view::{self, Ctx};
use crate::ui::words;
use adage_term::Glyph;
use adage_tracks::Aspect;
use ratatui::crossterm::event::KeyCode;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Wrap};
use ratatui::Frame;
use std::collections::BTreeSet;

/// The lifecycle filter's choices, in the order `S` cycles them.
pub const STATES: [&str; 7] = ["all states", "firm", "forming", "nascent", "dormant", "weakened", "extinguished"];

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

fn strength_index(a: Aspect) -> usize {
    match a {
        Aspect::Green => 0,
        Aspect::Yellow => 1,
        Aspect::Red => 2,
    }
}

/// Whether a row passes the chips and the filter.
fn passes(st: &State, r: &RouteRow) -> bool {
    !st.hidden[strength_index(r.aspect)]
        && (st.states == 0 || r.lifecycle() == STATES[st.states])
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
        "instincts" => 2,
        "extinguished" => 3,
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
fn traces<'a>(s: &'a AppState, id: &str) -> Vec<&'a feed::FeedItem> {
    s.live.feed.iter().filter(|i| i.reflexes().any(|r| r == id)).collect()
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
        KeyCode::Esc if !s.reflexes.filter.is_empty() => s.reflexes.filter.clear(),
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
            let lines: Vec<String> = traces(s, &r.id)
                .iter()
                .map(|i| {
                    let what: Vec<String> = i
                        .acts
                        .iter()
                        .filter_map(|a| match a {
                            Act::Directed { route, aspect, .. } if *route == r.id => {
                                Some(format!("fired {}", Theme::strength_word(aspect.to_owned())))
                            }
                            Act::Withheld { route, .. } if *route == r.id => Some("withheld (control)".into()),
                            Act::Followed(x) if *x == r.id => Some("followed".into()),
                            Act::Derailed(x) if *x == r.id => Some("ignored".into()),
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
            let lines = if lines.is_empty() {
                vec![format!("{} has not fired in the traces this screen has seen.", r.id)]
            } else {
                lines
            };
            s.modal = Some(Modal::Viewer { title: format!("traces · {}", r.id), source: None, lines, scroll: 0 });
        }
        KeyCode::Char('o') => {
            let r = s.selected_route()?;
            let cmd = format!("adage map --focus {}", r.id);
            s.message(
                "Open in the map",
                vec!["The same reflex in the web map, from any shell:".into(), String::new(), format!("  {cmd}")],
                false,
            );
        }
        KeyCode::Char('d') => {
            let r = s.selected_route()?;
            s.message(
                "Delegate",
                vec![
                    format!("Delegating lets {} act on its own (prefetch, level e1).", r.id),
                    "It needs a level and the fields it may pass, so it is granted from a shell:".into(),
                    String::new(),
                    format!("  adage review delegate {} {} --level e1", r.reference, r.id),
                    String::new(),
                    format!("Take it back with: adage review undelegate {} {}", r.reference, r.id),
                ],
                false,
            );
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
            let lines = vec![
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
            s.confirm(&title, lines, Action::Review { verb, reference: r.reference.clone(), route: r.id.clone() });
        }
        _ => {}
    }
    None
}

// ---------------------------------------------------------------------------------------------
// Drawing

fn lamp_spans(r: &RouteRow, t: &Theme) -> Vec<Span<'static>> {
    let glyph = if r.instinct { Glyph::Instinct } else { Glyph::Reflex };
    let lamp = if r.nascent() || r.extinguished() {
        Span::styled(format!("{} ", t.glyph(Glyph::Off)), t.dim())
    } else if t.color {
        Span::styled(format!("{} ", t.glyph(Glyph::Lamp)), t.aspect(r.aspect))
    } else {
        Span::styled(format!("{} ", &Theme::strength_word(r.aspect)[..1].to_ascii_uppercase()), t.bold())
    };
    vec![lamp, Span::styled(format!("{} ", t.glyph(glyph)), t.accent())]
}

fn chips(s: &AppState, t: &Theme, width: usize) -> Line<'static> {
    let rows = s.route_rows();
    let mut spans = vec![Span::raw(" ")];
    for (k, a) in [Aspect::Green, Aspect::Yellow, Aspect::Red].into_iter().enumerate() {
        let n = rows.iter().filter(|r| r.aspect == a && !r.extinguished()).count();
        let on = !s.reflexes.hidden[k];
        let word = Theme::strength_word(a);
        if t.color {
            let lamp = if on { t.glyph(Glyph::Lamp) } else { t.glyph(Glyph::Off) };
            spans.push(Span::styled(format!("{lamp} "), if on { t.aspect(a) } else { t.dim() }));
            spans.push(Span::styled(format!("{word} {n}  "), if on { t.ink2() } else { t.dim() }));
        } else {
            let w = if on { word.to_ascii_uppercase() } else { format!("({word})") };
            spans.push(Span::styled(format!("{w} {n}  "), if on { t.bold() } else { t.dim() }));
        }
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
                spans.push(Span::styled(t.pad(&r.id, id_w), if chosen { t.bold() } else { Style::default() }));
                spans.push(Span::styled(t.pad(r.lifecycle(), 9), t.ink2()));
                spans.push(Span::styled(t.text(&r.count()), t.dim()));
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
    let glyph = if r.instinct { Glyph::Instinct } else { Glyph::Reflex };
    v.push(Line::from(vec![
        Span::styled(format!("{} ", t.glyph(glyph)), t.accent()),
        Span::styled(r.id.clone(), t.bold()),
    ]));
    let mut facts = vec![r.lifecycle().to_string(), format!("{}@{}", r.track, r.version)];
    if r.imported {
        facts.push("from a pack".into());
    }
    if let Some(ts) = last_fired(s, &r.id) {
        facts.push(format!("last fired {}", view::clock(ts, ctx.tz, true)));
    }
    let mut line = vec![];
    if t.color {
        line.push(Span::styled(format!("{} ", t.glyph(Glyph::Lamp)), t.aspect(r.aspect)));
        line.push(Span::styled(Theme::strength_word(r.aspect), t.aspect(r.aspect).patch(t.bold())));
    } else {
        line.push(Span::styled(Theme::strength_word(r.aspect).to_ascii_uppercase(), t.bold()));
    }
    line.push(Span::styled(format!("{}{}", t.sep(), facts.join(&t.sep())), t.ink2()));
    v.push(Line::from(line));
    for (k, d) in &r.reasons {
        v.push(Line::from(vec![
            Span::styled(format!("{} waits for review: ", t.glyph(Glyph::Off)), t.yellow()),
            Span::styled(format!("{k}{}", t.sep()), t.yellow().patch(t.bold())),
            Span::styled(t.text(d), t.ink2()),
        ]));
    }
    v.push(Line::raw(""));
    v.push(head("WHAT THE AGENT SEES", t));
    if r.directive.trim().is_empty() {
        v.push(Line::from(Span::styled("  (no preview: the reflex has no directive text)", t.dim())));
    } else {
        v.extend(view::directive_lines(&r.directive, t, ""));
    }
    v.push(Line::raw(""));
    // Two sources, each named: the traces conditioning read (history), and what the hooks have
    // recorded since the reflex was installed (live).
    v.push(head("EVIDENCE", t));
    if r.opportunities == 0 {
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
            "fired {}{sep}followed {}{sep}ignored {}{sep}finish line met {}",
            l.emitted,
            l.followed,
            l.derailed,
            l.met,
            sep = t.sep()
        ),
        t,
    ));
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
        let style = if now == "weakened" { t.yellow() } else { t.dim() };
        spans.push(Span::styled(format!("{} ", t.glyph(Glyph::Lamp)), style));
        spans.push(Span::styled(now.to_string(), t.bold()));
    }
    v.push(Line::from(spans));
    let mut notes = vec![format!(
        "ceiling {}",
        crate::ui::theme::parse_aspect(&r.ceiling)
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
    let n = traces(s, &r.id).len();
    v.push(Line::from(vec![
        Span::raw("  "),
        Span::styled("d", t.key()),
        Span::styled(" delegate   ", t.ink2()),
        Span::styled("t", t.key()),
        Span::styled(format!(" traces ({n})   "), t.ink2()),
        Span::styled("o", t.key()),
        Span::styled(" open in map", t.ink2()),
    ]));
    if let Some(verb) = ["demote", "promote", "reject", "approve"].into_iter().find(|v| r.verbs.contains(v)) {
        v.push(Line::from(vec![
            Span::styled("  same as ", t.dim()),
            Span::styled(words::review_command(verb, &r.reference, &r.id), t.command()),
            Span::styled(" from any shell", t.dim()),
        ]));
    }
    v
}

fn group_detail(n: &Node, t: &Theme) -> Vec<Line<'static>> {
    let Node::Group { key, title, sub, count, .. } = n else { return vec![] };
    let what = match key.as_str() {
        "nascent" => "Reflexes conditioning formed that wait for a person. Approve one to put it into service.",
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
    if key == "nascent" {
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
    let wide = area.width >= 90;
    let (list, detail_a) = if wide {
        let left = (area.width * 2 / 5).clamp(40, 56);
        let [l, rule, d] =
            Layout::horizontal([Constraint::Length(left), Constraint::Length(3), Constraint::Min(20)]).areas(area);
        let bar: Vec<Line> = (0..rule.height).map(|_| Line::from(Span::styled(t.ch("│", "|"), t.rule()))).collect();
        f.render_widget(Paragraph::new(bar), rule);
        (l, d)
    } else {
        let [l, rule, d] =
            Layout::vertical([Constraint::Percentage(45), Constraint::Length(1), Constraint::Min(5)]).areas(area);
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
    let nodes = tree(s);
    if nodes.is_empty() {
        f.render_widget(
            Paragraph::new(Span::styled(" Nothing matches. esc clears the filter; F P I S reset the chips.", t.dim()))
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
