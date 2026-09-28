//! The Map tab: every live reflex as a reflex arc, laid out left to right in three layers,
//! stimulus → reflex → response, drawn with box characters. Stimuli and responses that several
//! reflexes share are drawn once, and their edges converge. `f` focuses on the selection (the
//! reflexes two hops away: those sharing its stimulus or response); `+`/`-` switch between boxes
//! and compact rows; `o` names the command that opens the same node in the web map.

use super::empty;
use crate::ui::app::{AppState, Cmd, Tab, WantKind};
use crate::ui::routes::RouteRow;
use crate::ui::theme::Theme;
use crate::ui::view::{self, Ctx};
use crate::ui::words::fit_with;
use adage_term::Glyph;
use ratatui::crossterm::event::KeyCode;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Wrap};
use ratatui::Frame;

#[derive(Debug, Default)]
pub struct State {
    /// The selected reflex (`reference\0id`); the first when unset.
    pub sel: Option<String>,
    /// Only the reflexes two hops from the selection.
    pub focus: bool,
    /// One row per reflex instead of boxes.
    pub compact: bool,
}

/// A stimulus or a response node.
#[derive(Debug, Clone, PartialEq)]
pub struct End {
    pub kind: String,
    pub title: &'static str,
    pub text: String,
    /// The layer row it is drawn at: its first reflex's.
    pub row: usize,
}

/// The three layers.
#[derive(Debug, Default)]
pub struct Graph {
    /// Row indices of the reflexes shown, top to bottom.
    pub reflexes: Vec<usize>,
    pub stimuli: Vec<End>,
    pub responses: Vec<End>,
    /// Per reflex (by layer row): its stimulus and response.
    pub stim_of: Vec<usize>,
    pub resp_of: Vec<usize>,
}

fn key_of(r: &RouteRow) -> String {
    format!("{}\u{0}{}", r.reference, r.id)
}

fn stimulus(r: &RouteRow) -> (&'static str, String) {
    let text = r.trigger.replace('`', "");
    match r.trigger_kind.as_str() {
        "prompt" => ("PROMPT", text.strip_prefix("a prompt ").unwrap_or(&text).to_string()),
        "after_command" => ("AFTER COMMAND", text),
        "after_failure" => ("AFTER FAILURE", text),
        "after_edit" => {
            ("AFTER EDIT", text.strip_prefix("an edit touches ").map_or(text.clone(), |p| format!("{p} changed")))
        }
        "session_start" => ("SESSION START", text),
        "intent" => ("INTENT", text.strip_prefix("a prompt means: ").unwrap_or(&text).to_string()),
        _ => ("STIMULUS", text),
    }
}

fn response(r: &RouteRow) -> (&'static str, String) {
    let title = match r.step_kind.as_str() {
        "run" => "RUN",
        "read_batch" => "READ TOGETHER",
        "avoid" => "RUN INSTEAD",
        "advise" => "ADVICE",
        _ => "RESPONSE",
    };
    (title, r.step.replace('`', ""))
}

/// Which reflexes the map shows: live ones (not nascent, not extinguished), learned before
/// instincts; with focus, only those sharing the selection's stimulus or response.
pub fn graph(s: &AppState) -> Graph {
    let rows = s.route_rows();
    let mut idx: Vec<usize> = (0..rows.len()).filter(|&i| !rows[i].nascent() && !rows[i].extinguished()).collect();
    idx.sort_by_key(|&i| rows[i].instinct);
    let sel = selected_index(s, &idx);
    if s.map.focus {
        if let Some(&c) = idx.get(sel) {
            let (st, rs) = (stimulus(&rows[c]), response(&rows[c]));
            idx.retain(|&i| stimulus(&rows[i]) == st || response(&rows[i]) == rs);
        }
    }
    let mut g = Graph { reflexes: idx.clone(), ..Default::default() };
    for (row, &i) in idx.iter().enumerate() {
        let (st, sx) = stimulus(&rows[i]);
        let k = rows[i].trigger_kind.clone();
        let si = g.stimuli.iter().position(|e| e.kind == k && e.text == sx).unwrap_or_else(|| {
            g.stimuli.push(End { kind: k, title: st, text: sx, row });
            g.stimuli.len() - 1
        });
        g.stim_of.push(si);
        let (rt, rx) = response(&rows[i]);
        let k = rows[i].step_kind.clone();
        let ri = g.responses.iter().position(|e| e.kind == k && e.text == rx).unwrap_or_else(|| {
            g.responses.push(End { kind: k, title: rt, text: rx, row });
            g.responses.len() - 1
        });
        g.resp_of.push(ri);
    }
    g
}

fn selected_index(s: &AppState, idx: &[usize]) -> usize {
    let rows = s.route_rows();
    s.map.sel.as_ref().and_then(|k| idx.iter().position(|&i| &key_of(&rows[i]) == k)).unwrap_or(0)
}

/// The selected reflex on the map.
pub fn selected(s: &AppState) -> Option<&RouteRow> {
    let g = graph(s);
    let i = selected_index(s, &g.reflexes);
    g.reflexes.get(i).map(|&r| &s.route_rows()[r])
}

pub fn key(s: &mut AppState, code: KeyCode) -> Option<Cmd> {
    let g = graph(s);
    let n = g.reflexes.len();
    let cur = selected_index(s, &g.reflexes);
    let go = |s: &mut AppState, i: usize| {
        if let Some(&r) = g.reflexes.get(i) {
            s.map.sel = Some(key_of(&s.route_rows()[r]));
        }
    };
    match code {
        KeyCode::Down | KeyCode::Char('j') => go(s, (cur + 1).min(n.saturating_sub(1))),
        KeyCode::Up | KeyCode::Char('k') => go(s, cur.saturating_sub(1)),
        KeyCode::Home => go(s, 0),
        KeyCode::End => go(s, n.saturating_sub(1)),
        KeyCode::Char('f') => {
            go(s, cur);
            s.map.focus = !s.map.focus;
        }
        // Two zoom levels; at either end the key says so rather than nothing.
        KeyCode::Char('+') | KeyCode::Char('=') if !s.map.compact => {
            s.toast("zoom: boxes is the largest; - shows compact rows", false)
        }
        KeyCode::Char('-') if s.map.compact => s.toast("zoom: rows is the smallest; + shows boxes", false),
        KeyCode::Char('+') | KeyCode::Char('=') => s.map.compact = false,
        KeyCode::Char('-') => s.map.compact = true,
        KeyCode::Char('R') => return Some(Cmd::Want(WantKind::Routes)),
        KeyCode::Enter => {
            let r = selected(s)?;
            let (reference, id) = (r.reference.clone(), r.id.clone());
            crate::ui::tabs::reflexes::select(s, &reference, &id);
            s.tab = Tab::Reflexes;
        }
        KeyCode::Char('o') => {
            let id = selected(s)?.id.clone();
            s.message(
                "Open in the map",
                vec![
                    "The same node in the web map, from any shell:".into(),
                    String::new(),
                    format!("  adage map --focus {id}"),
                ],
                false,
            );
        }
        _ => {}
    }
    None
}

// ---------------------------------------------------------------------------------------------
// Drawing

/// A character canvas: box text, then edges merged by the directions each cell connects.
struct Canvas {
    w: usize,
    cells: Vec<Vec<(String, Style)>>,
    /// Edge connections per cell: up 1, down 2, left 4, right 8.
    links: Vec<Vec<u8>>,
    hot: Vec<Vec<bool>>,
    /// The ellipsis for text cut to its box.
    more: &'static str,
}

const U: u8 = 1;
const D: u8 = 2;
const L: u8 = 4;
const R: u8 = 8;

impl Canvas {
    fn new(w: usize, h: usize, more: &'static str) -> Canvas {
        Canvas {
            w,
            more,
            cells: vec![vec![(" ".to_string(), Style::default()); w]; h],
            links: vec![vec![0; w]; h],
            hot: vec![vec![false; w]; h],
        }
    }

    fn put(&mut self, y: usize, x: usize, text: &str, style: Style) -> usize {
        let mut x = x;
        for c in text.chars() {
            if let Some(cell) = self.cells.get_mut(y).and_then(|r| r.get_mut(x)) {
                *cell = (c.to_string(), style);
            }
            x += 1;
        }
        x
    }

    fn spans(&mut self, y: usize, x: usize, spans: &[Span<'static>], room: usize) {
        let mut x0 = x;
        let end = x + room;
        for s in spans {
            let left = end.saturating_sub(x0);
            if left == 0 {
                break;
            }
            x0 = self.put(y, x0, &fit_with(&s.content, left, self.more), s.style);
        }
    }

    fn link(&mut self, y: usize, x: usize, bits: u8, hot: bool) {
        if y < self.links.len() && x < self.w {
            self.links[y][x] |= bits;
            self.hot[y][x] |= hot;
        }
    }

    fn hline(&mut self, y: usize, x0: usize, x1: usize, hot: bool) {
        for x in x0..=x1 {
            let bits = (if x > x0 { L } else { 0 }) | (if x < x1 { R } else { 0 });
            self.link(y, x, bits | if x0 == x1 { L | R } else { 0 }, hot);
        }
    }

    fn vline(&mut self, x: usize, y0: usize, y1: usize, hot: bool) {
        let (a, b) = (y0.min(y1), y0.max(y1));
        for y in a..=b {
            let bits = (if y > a { U } else { 0 }) | (if y < b { D } else { 0 });
            self.link(y, x, bits, hot);
        }
    }

    fn finish(mut self, t: &Theme, arrows: &[(usize, usize, bool)]) -> Vec<Line<'static>> {
        let unicode = t.ch("─", "-") == "─";
        for y in 0..self.links.len() {
            for x in 0..self.w {
                let b = self.links[y][x];
                if b == 0 {
                    continue;
                }
                let c = if !unicode {
                    match b {
                        _ if b & (U | D) != 0 && b & (L | R) != 0 => "+",
                        _ if b & (U | D) != 0 => "|",
                        _ => "-",
                    }
                } else {
                    match (b & U != 0, b & D != 0, b & L != 0, b & R != 0) {
                        (true, true, true, true) => "┼",
                        (true, true, false, true) => "├",
                        (true, true, true, false) => "┤",
                        (false, true, true, true) => "┬",
                        (true, false, true, true) => "┴",
                        (false, true, false, true) => "╭",
                        (false, true, true, false) => "╮",
                        (true, false, false, true) => "╰",
                        (true, false, true, false) => "╯",
                        (true, _, false, false) | (_, true, false, false) => "│",
                        _ => "─",
                    }
                };
                let style = if self.hot[y][x] { t.command().patch(t.bold()) } else { t.rule() };
                self.cells[y][x] = (c.to_string(), style);
            }
        }
        for &(y, x, hot) in arrows {
            if y < self.cells.len() && x < self.w {
                self.cells[y][x] =
                    (t.ch("▶", ">").to_string(), if hot { t.command().patch(t.bold()) } else { t.rule() });
            }
        }
        self.cells
            .into_iter()
            .map(|row| {
                let mut spans: Vec<Span<'static>> = Vec::new();
                for (c, st) in row {
                    match spans.last_mut() {
                        Some(last) if last.style == st => last.content.to_mut().push_str(&c),
                        _ => spans.push(Span::styled(c, st)),
                    }
                }
                Line::from(spans)
            })
            .collect()
    }
}

/// A canvas and the arrowheads drawn over it last.
struct Board {
    canvas: Canvas,
    arrows: Vec<(usize, usize, bool)>,
}

impl Board {
    /// An edge from the cell after a box's right side (`x0`, `y0`) to the cell before a box's
    /// left side (`x1`, `y1`): straight, or down (or up) a junction column near the source.
    fn edge(&mut self, x0: usize, y0: usize, x1: usize, y1: usize, hot: bool) {
        let c = &mut self.canvas;
        if y0 == y1 {
            c.hline(y0, x0, x1, hot);
        } else {
            let xj = (x0 + 3).min(x1.saturating_sub(2));
            c.hline(y0, x0, xj, hot);
            c.vline(xj, y0, y1, hot);
            c.hline(y1, xj, x1, hot);
        }
        self.arrows.push((y1, x1, hot));
    }

    fn boxed(&mut self, y: usize, x: usize, w: usize, lines: [Vec<Span<'static>>; 2], style: Style, t: &Theme) {
        let (tl, tr, bl, br, hz, vt) = if t.ch("╭", "+") == "╭" {
            ("╭", "╮", "╰", "╯", "─", "│")
        } else {
            ("+", "+", "+", "+", "-", "|")
        };
        let c = &mut self.canvas;
        c.put(y, x, &format!("{tl}{}{tr}", hz.repeat(w - 2)), style);
        for k in 1..=2 {
            c.put(y + k, x, vt, style);
            c.put(y + k, x + w - 1, vt, style);
        }
        c.put(y + 3, x, &format!("{bl}{}{br}", hz.repeat(w - 2)), style);
        for (k, l) in lines.iter().enumerate() {
            c.spans(y + 1 + k, x + 2, l, w - 4);
        }
    }
}

const STEP: usize = 5;

fn reflex_spans(r: &RouteRow, t: &Theme, chosen: bool) -> Vec<Span<'static>> {
    let lamp = if t.color {
        Span::styled(format!("{} ", t.glyph(Glyph::Lamp)), t.aspect(r.aspect))
    } else {
        Span::styled(format!("{} ", &Theme::strength_word(r.aspect)[..1].to_ascii_uppercase()), t.bold())
    };
    let glyph = if r.instinct { Glyph::Instinct } else { Glyph::Reflex };
    vec![
        lamp,
        Span::styled(format!("{} ", t.glyph(glyph)), t.accent()),
        Span::styled(r.id.clone(), if chosen { t.bold() } else { Style::default() }),
    ]
}

fn reflex_facts(r: &RouteRow, long: bool, t: &Theme) -> String {
    let mut v = vec![
        Theme::strength_word(r.aspect).to_string(),
        if r.instinct { "instinct".into() } else { r.lifecycle().to_string() },
        t.text(&r.count()),
    ];
    if long && r.opportunities > 0 {
        v.push(format!("lb {:.2}", r.lower_bound));
    }
    v.join(&t.sep())
}

fn end_text(e: &End, t: &Theme) -> Vec<Span<'static>> {
    let style = if e.kind == "run" || e.kind == "avoid" { t.command() } else { Style::default() };
    vec![Span::styled(t.text(&e.text), style)]
}

fn boxes(s: &AppState, g: &Graph, sel: usize, t: &Theme, width: usize) -> Option<(Vec<Line<'static>>, [usize; 3])> {
    let gap = ((width.saturating_sub(2)) / 12).clamp(6, 12);
    let bw = ((width.saturating_sub(2 + 2 * gap)) / 3).min(38);
    if bw < 20 {
        return None;
    }
    let cols = [1, 1 + bw + gap, 1 + 2 * (bw + gap)];
    let rows = s.route_rows();
    let n = g.reflexes.len();
    let mut b = Board { canvas: Canvas::new(width, n * STEP, t.more()), arrows: vec![] };
    let (sel_st, sel_rs) = (g.stim_of.get(sel).copied(), g.resp_of.get(sel).copied());
    for (k, e) in g.stimuli.iter().enumerate() {
        let style = if Some(k) == sel_st { t.accent() } else { t.rule() };
        b.boxed(
            e.row * STEP,
            cols[0],
            bw,
            [vec![Span::styled(e.title, t.dim().patch(t.bold()))], end_text(e, t)],
            style,
            t,
        );
    }
    for (k, &i) in g.reflexes.iter().enumerate() {
        let r = &rows[i];
        let chosen = k == sel;
        let style = if chosen { t.accent() } else { t.rule() };
        b.boxed(
            k * STEP,
            cols[1],
            bw,
            [reflex_spans(r, t, chosen), vec![Span::styled(reflex_facts(r, true, t), t.ink2())]],
            style,
            t,
        );
    }
    for (k, e) in g.responses.iter().enumerate() {
        let style = if Some(k) == sel_rs { t.accent() } else { t.rule() };
        b.boxed(
            e.row * STEP,
            cols[2],
            bw,
            [vec![Span::styled(e.title, t.dim().patch(t.bold()))], end_text(e, t)],
            style,
            t,
        );
    }
    for k in 0..n {
        let hot = k == sel;
        let st = &g.stimuli[g.stim_of[k]];
        b.edge(cols[0] + bw, st.row * STEP + 1, cols[1] - 1, k * STEP + 1, hot);
        let rs = &g.responses[g.resp_of[k]];
        b.edge(cols[1] + bw, k * STEP + 1, cols[2] - 1, rs.row * STEP + 1, hot);
    }
    let Board { canvas, arrows } = b;
    Some((canvas.finish(t, &arrows), cols))
}

fn compact(s: &AppState, g: &Graph, sel: usize, t: &Theme, width: usize) -> Vec<Line<'static>> {
    let rows = s.route_rows();
    let w = (width.saturating_sub(12)) / 3;
    let arrow = format!(" {}{} ", t.ch("─", "-"), t.ch("▶", ">"));
    g.reflexes
        .iter()
        .enumerate()
        .map(|(k, &i)| {
            let r = &rows[i];
            let chosen = k == sel;
            let st = &g.stimuli[g.stim_of[k]];
            let rs = &g.responses[g.resp_of[k]];
            let mut spans = vec![
                Span::styled(if chosen { t.ch("▌", ">") } else { " " }, t.accent()),
                Span::raw(" "),
                Span::raw(t.pad(&t.text(&st.text), w)),
                Span::styled(arrow.clone(), if chosen { t.command() } else { t.rule() }),
            ];
            let head = reflex_spans(r, t, chosen);
            let used: usize = head.iter().map(|s| s.width()).sum();
            spans.extend(head);
            spans.push(Span::raw(" ".repeat(w.saturating_sub(used))));
            spans.push(Span::styled(arrow.clone(), if chosen { t.command() } else { t.rule() }));
            spans.extend(end_text(rs, t).into_iter().map(|s| Span::styled(t.fit(&s.content, w), s.style)));
            Line::from(spans)
        })
        .collect()
}

pub fn draw(f: &mut Frame, area: Rect, s: &AppState, ctx: &Ctx) {
    let t = &ctx.theme;
    let routes = match &s.routes {
        None => {
            f.render_widget(Paragraph::new(Span::styled(format!(" loading the map{}", t.more()), t.dim())), area);
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
        Some(Ok(r)) => r,
    };
    let g = graph(s);
    if g.reflexes.is_empty() {
        let lines = empty("Nothing to map yet.", "Agents' traces arrive automatically; or run", "adage condition", t);
        f.render_widget(Paragraph::new(view::wrap_lines(lines, area.width)), area);
        return;
    }
    let sel = selected_index(s, &g.reflexes);
    let r = &s.route_rows()[g.reflexes[sel]];
    let [info, _, heads, canvas_a, strip] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(3),
        Constraint::Length(3),
    ])
    .areas(area);
    let instincts = g.reflexes.iter().filter(|&&i| s.route_rows()[i].instinct).count();
    let reflexes = g.reflexes.len() - instincts;
    let counts = format!(
        "{reflexes} reflex{}{sep}{instincts} instinct{}{sep}{} traces ",
        if reflexes == 1 { "" } else { "es" },
        if instincts == 1 { "" } else { "s" },
        routes.traces,
        sep = t.sep()
    );
    let focus = if s.map.focus { format!("2 hops around {}", r.id) } else { "off".into() };
    let drawn = if s.map.compact { None } else { boxes(s, &g, sel, t, canvas_a.width as usize) };
    let zoom = match (&drawn, s.map.compact) {
        (Some(_), _) => "boxes (max)",
        (None, true) => "rows (min)",
        (None, false) => "rows (too narrow for boxes)",
    };
    let mut left = vec![
        Span::styled(" focus ", t.dim()),
        Span::raw(focus),
        Span::styled("    zoom ", t.dim()),
        Span::raw(zoom),
        Span::styled("    layout ", t.dim()),
        Span::raw(format!("stimulus {} reflex {} response", t.glyph(Glyph::Arrow), t.glyph(Glyph::Arrow))),
    ];
    let lw: usize = left.iter().map(|s| s.width()).sum();
    if lw + counts.len() < area.width as usize {
        left.push(Span::raw(" ".repeat(area.width as usize - lw - counts.chars().count())));
        left.push(Span::styled(counts, t.ink2()));
    }
    f.render_widget(Paragraph::new(Line::from(left)), info);

    let (lines, top_of_sel, step) = match drawn {
        Some((lines, cols)) => {
            let mut h = vec![Span::raw(" ".repeat(cols[0] + 2)), Span::styled("STIMULUS", t.dim().patch(t.bold()))];
            h.push(Span::raw(" ".repeat(cols[1] - cols[0] - 8)));
            h.push(Span::styled("REFLEX", t.dim().patch(t.bold())));
            h.push(Span::raw(" ".repeat(cols[2] - cols[1] - 6)));
            h.push(Span::styled("RESPONSE", t.dim().patch(t.bold())));
            f.render_widget(Paragraph::new(Line::from(h)), heads);
            (lines, sel * STEP, STEP)
        }
        None => {
            let w = (canvas_a.width as usize).saturating_sub(12) / 3;
            let head = Line::from(vec![
                Span::raw("  "),
                Span::styled(t.pad("STIMULUS", w + 4), t.dim().patch(t.bold())),
                Span::styled(t.pad("REFLEX", w + 4), t.dim().patch(t.bold())),
                Span::styled("RESPONSE", t.dim().patch(t.bold())),
            ]);
            f.render_widget(Paragraph::new(head), heads);
            (compact(s, &g, sel, t, canvas_a.width as usize), sel, 1)
        }
    };
    let h = canvas_a.height as usize;
    let offset = (top_of_sel + step).saturating_sub(h).min(lines.len().saturating_sub(h));
    f.render_widget(Paragraph::new(lines).scroll((offset as u16, 0)), canvas_a);

    let mut facts = vec![Theme::strength_word(r.aspect).to_string(), r.lifecycle().to_string()];
    if r.opportunities > 0 {
        facts.push(format!("{}/{} traces", r.support, r.opportunities));
    }
    if r.turns_saved > 0.0 {
        facts.push(format!("{:.0} turns saved", r.turns_saved));
    }
    let strip_lines = vec![
        view::rule("", None, area.width, t),
        Line::from(vec![
            Span::styled(format!(" {} ", t.ch("▸", ">")), t.accent()),
            Span::styled(r.id.clone(), t.bold()),
            Span::styled(format!("  {}", facts.join(&t.sep())), t.ink2()),
        ]),
        Line::from(vec![
            Span::raw(" "),
            Span::styled("o", t.key()),
            Span::styled(" open in browser: ", t.dim()),
            Span::styled(format!("adage map --focus {}", r.id), t.command()),
            Span::styled("   enter", t.key()),
            Span::styled(" inspect in Reflexes", t.dim()),
        ]),
    ];
    f.render_widget(Paragraph::new(strip_lines), strip);
}
