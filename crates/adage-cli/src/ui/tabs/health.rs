//! The Health tab: `adage doctor`'s checks as a dashboard, not a wall of text.
//!
//! * a summary at the top: a big lamp, `Healthy · 24 checks passed · 1 to look at`, and a meter;
//! * what needs a look pinned under it as cards: the problem in plain words, why, and the key that
//!   fixes it;
//! * every other check in a few named sections (`Hooks & delivery`, `Learning`, …), one short row
//!   each: a lamp, a plain label, the first clause of what it found;
//! * the selected check's card (beside the list; on a narrow screen it opens in place in the
//!   list): its whole text, wrapped, never cut, and its fix (`enter`).
//!
//! `check_sel` stays an index into doctor's checks; the keys move through them in the order shown.

use crate::doctor::{Check, Status};
use crate::ui::app::{AppState, Cmd, Modal, WantKind};
use crate::ui::health::{Fix, Health};
use crate::ui::kit::{self, Card, KeyValue, Section, Stat, Tone};
use crate::ui::theme::{Lamp, Theme};
use crate::ui::view::{self, Ctx};
use adage_term::Glyph;
use ratatui::crossterm::event::KeyCode;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

pub fn key(s: &mut AppState, code: KeyCode) -> Option<Cmd> {
    let fix_of = |s: &AppState, want: &dyn Fn(&Fix) -> bool| -> Option<Fix> {
        let h = s.health.as_ref()?;
        let sel = h.fixes.get(s.check_sel).cloned().flatten().filter(|f| want(f));
        sel.or_else(|| h.fixes.iter().flatten().find(|f| want(f)).cloned())
    };
    let fix = match code {
        KeyCode::Up | KeyCode::Char('k') => {
            step(s, -1);
            return None;
        }
        KeyCode::Down | KeyCode::Char('j') => {
            step(s, 1);
            return None;
        }
        KeyCode::Home | KeyCode::Char('g') => {
            step(s, isize::MIN);
            return None;
        }
        KeyCode::End | KeyCode::Char('G') => {
            step(s, isize::MAX);
            return None;
        }
        KeyCode::Char('R') => {
            s.toast("running the checks again", true);
            return Some(Cmd::Want(WantKind::Health));
        }
        KeyCode::Enter => {
            let h = s.health.as_ref()?;
            let found = h.fixes.get(s.check_sel).cloned().flatten();
            if found.is_none() {
                let c = h.checks.get(s.check_sel)?;
                let mut lines = vec![c.detail.clone()];
                if let Some(fix) = &c.fix {
                    lines.extend([String::new(), "Next steps".into(), fix.prose()]);
                } else if c.status == Status::Pass {
                    lines.push("This check passes. No action is needed.".into());
                }
                s.modal = Some(Modal::Viewer { title: label(&c.check), source: None, lines, scroll: 0 });
                return None;
            }
            found
        }
        KeyCode::Char('i') => fix_of(s, &|f| matches!(f, Fix::InstallHooks(_))),
        KeyCode::Char('m') => Some(Fix::FetchModel),
        KeyCode::Char('c') => Some(Fix::LearnNow),
        KeyCode::Char('s') => Some(Fix::OpenSettings),
        KeyCode::Char('o') => Some(Fix::OpenFailOpen),
        _ => None,
    }?;
    s.start_fix(fix)
}

/// Move the selection `by` places in the order shown (clamped at either end).
fn step(s: &mut AppState, by: isize) {
    let Some(h) = &s.health else { return };
    let order = order(h);
    let Some(last) = order.len().checked_sub(1) else { return };
    let at = order.iter().position(|&i| i == s.check_sel).unwrap_or(0);
    let to = at.saturating_add_signed(by).min(last);
    s.check_sel = order[to];
}

/// The check the tab selects first: the first problem, else the first check shown.
pub fn first(h: &Health) -> usize {
    order(h).first().copied().unwrap_or(0)
}

// ---------------------------------------------------------------------------------------------
// Grouping and words

/// A section of the tab, in the order shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Group {
    Hooks,
    Learning,
    Packs,
    Data,
    Install,
    Other,
}

impl Group {
    const ALL: [Group; 6] = [Group::Hooks, Group::Learning, Group::Packs, Group::Data, Group::Install, Group::Other];

    pub fn title(self) -> &'static str {
        match self {
            Group::Hooks => "Hooks & delivery",
            Group::Learning => "Learning",
            Group::Packs => "Packs & models",
            Group::Data => "Data & storage",
            Group::Install => "This install",
            Group::Other => "Other checks",
        }
    }
}

/// `hooks: codex (twice)` → (`hooks: codex`, `Some("twice")`).
fn split(check: &str) -> (&str, Option<&str>) {
    match check.split_once(" (") {
        Some((base, extra)) => (base, Some(extra.trim_end_matches(')'))),
        None => (check, None),
    }
}

/// The section a doctor check belongs to.
pub fn group(check: &str) -> Group {
    let (base, _) = split(check);
    if base.starts_with("hooks: ") || base.starts_with("hook activity: ") {
        return Group::Hooks;
    }
    if base.starts_with("auto-learn") || base.starts_with("history: ") {
        return Group::Learning;
    }
    match base {
        "claude-code plugin binary" | "hook latency" | "fail-open errors" | "kill switches" | "rewrites" => {
            Group::Hooks
        }
        "index" | "tracks" => Group::Learning,
        "instincts" | "packs" | "marketplace" | "embedding model" => Group::Packs,
        "data directory" | "config" | "session spools" | "cloud files" => Group::Data,
        "binary" | "versions" => Group::Install,
        _ => Group::Other,
    }
}

fn harness(h: &str) -> &str {
    match h {
        "claude-code" | "claude" => "Claude Code",
        "codex" => "Codex",
        "opencode" => "OpenCode",
        other => other,
    }
}

/// A check's name in plain words: `hook activity: codex` → `Codex activity`.
pub fn label(check: &str) -> String {
    let (base, extra) = split(check);
    let plain = if let Some(h) = base.strip_prefix("hooks: ") {
        format!("{} hooks", harness(h))
    } else if let Some(h) = base.strip_prefix("hook activity: ") {
        format!("{} activity", harness(h))
    } else if let Some(h) = base.strip_prefix("history: ") {
        format!("{} history", harness(h))
    } else {
        match base {
            "binary" => "Adage",
            "versions" => "Versions",
            "data directory" => "Data folder",
            "config" => "Settings file",
            "session spools" => "Trace recordings",
            "cloud files" => "Cloud-only files",
            "index" => "Reflex index",
            "tracks" => "Reflex collections",
            "auto-learn" => "Auto-condition",
            "auto-learn runs" => "Recent runs",
            "auto-learn lock" => "Learner lock",
            "auto-learn queue" => "Waiting to condition",
            "instincts" => "Instincts",
            "packs" => "Instinct packs",
            "marketplace" => "Marketplace",
            "embedding model" => "Embedding model",
            "claude-code plugin binary" => "Plugin's adage",
            "hook latency" => "Hook speed",
            "fail-open errors" => "Hook errors",
            "kill switches" => "Kill switches",
            "rewrites" => "Command rewrites",
            other => return sentence(other),
        }
        .to_string()
    };
    match extra {
        Some(e) => format!("{plain} ({e})"),
        None => plain,
    }
}

/// `s` with its first letter in capitals.
fn sentence(s: &str) -> String {
    let mut c = s.chars();
    c.next().map_or_else(String::new, |f| f.to_uppercase().chain(c).collect())
}

/// A row's short value: the first clause of what the check found, without the hashes and
/// sources a first read does not need (the whole text is the card's).
pub fn short(c: &Check) -> String {
    use std::sync::OnceLock;
    static NOISE: OnceLock<[regex::Regex; 3]> = OnceLock::new();
    let noise = NOISE.get_or_init(|| {
        [
            // `generation 8216e29f00, ` and `gen 8216e29f`
            regex::Regex::new(r"\bgen(eration)? [0-9a-f]{6,}(, )?").expect("static pattern"),
            // `potion-base-8m@f65d0f325faa`
            regex::Regex::new(r"@[0-9a-f]{8,}").expect("static pattern"),
            // where a setting came from: `(file)`, `(default)`, `(env ADAGE_X)`
            regex::Regex::new(r" \((file|default|env [^)]*)\)").expect("static pattern"),
        ]
    });
    let (base, _) = split(&c.check);
    let d = c.detail.as_str();
    let mut s = match base {
        // `adage (built into Adage 0.1.0): 8 packs, every one installed as pinned`
        "marketplace" => d.split_once(": ").map_or(d, |(_, rest)| rest).to_string(),
        _ if base.starts_with("hooks: ") && c.status == Status::Pass && d.contains(" runs ") => {
            // `~/x/.claude/settings.local.json runs ~/bin/adage (this binary)`: where, in a word.
            let file = d.split(' ').next().unwrap_or(d).rsplit('/').next().unwrap_or(d);
            format!("set up in {file}")
        }
        _ => d.to_string(),
    };
    for re in noise {
        s = re.replace_all(&s, "").into_owned();
    }
    kit::first_clause(&s).trim_end_matches('.').trim().to_string()
}

/// A check's lamp: a pass with nothing to check is off, not green.
fn lamp(c: &Check) -> Lamp {
    match c.status {
        Status::Pass if c.detail.contains("nothing to check") => Lamp::Off,
        s => view::lamp_of(s),
    }
}

fn problem(c: &Check) -> bool {
    c.status != Status::Pass
}

/// The checks in the order shown: problems first (failures, then warnings), then every other
/// check by section, in doctor's order within each.
pub fn order(h: &Health) -> Vec<usize> {
    let mut v: Vec<usize> = (0..h.checks.len()).filter(|&i| h.checks[i].status == Status::Fail).collect();
    v.extend((0..h.checks.len()).filter(|&i| h.checks[i].status == Status::Warn));
    for g in Group::ALL {
        v.extend((0..h.checks.len()).filter(|&i| !problem(&h.checks[i]) && group(&h.checks[i].check) == g));
    }
    v
}

/// The key that acts on a fix from anywhere in the tab, if one does.
fn fix_key(f: &Fix) -> Option<&'static str> {
    match f {
        Fix::InstallHooks(_) => Some("i"),
        Fix::FetchModel => Some("m"),
        Fix::LearnNow => Some("c"),
        Fix::OpenFailOpen => Some("o"),
        Fix::OpenSettings => Some("s"),
        Fix::EnableAutoLearn | Fix::History(_) => None,
    }
}

// ---------------------------------------------------------------------------------------------
// Drawing

pub fn draw(f: &mut Frame, area: Rect, s: &AppState, ctx: &Ctx) {
    let t = &ctx.theme;
    let Some(h) = &s.health else {
        let lines = vec![
            Line::raw(""),
            Line::from(vec![
                Span::raw("  "),
                Span::styled(format!("{} ", kit::spinner(0, t)), t.accent()),
                Span::styled(format!("Running the checks{}", t.more()), t.ink2()),
            ]),
            Line::from(Span::styled("    the same checks as adage doctor", t.dim())),
        ];
        f.render_widget(Paragraph::new(lines), area);
        return;
    };
    let hero = hero(h, t, area.width);
    let [hero_a, _, body] =
        Layout::vertical([Constraint::Length(hero.len() as u16), Constraint::Length(1), Constraint::Min(3)])
            .areas(area);
    f.render_widget(Paragraph::new(hero), hero_a);

    // Wide: the list beside the selected check's card. Narrow: the selected item opens in place.
    let wide = area.width >= 100;
    let list_a = if wide {
        let [l, _, r] =
            Layout::horizontal([Constraint::Percentage(56), Constraint::Length(2), Constraint::Min(30)]).areas(body);
        let (card, lines) = detail(h, s.check_sel, t, Card::inner_width(r));
        let rows = Card::height(lines.len()).min(r.height);
        card.render(f, Rect { height: rows, ..r }, lines, t);
        l
    } else {
        body
    };
    let (lines, (top, bottom)) = list(h, s.check_sel, !wide, &s.notices, t, list_a.width as usize);
    // Scrolled so the whole selected item shows, with a row of what follows where there is room.
    let height = list_a.height as usize;
    let offset = if bottom + 1 > height { (bottom + 2).min(lines.len()).saturating_sub(height) } else { 0 };
    f.render_widget(Paragraph::new(lines).scroll((offset.min(top) as u16, 0)), list_a);
}

/// The summary: a big lamp, the verdict and counts, a meter, and where the checks come from;
/// under them, what Adage measured here and today's notice, when there are.
fn hero(h: &Health, t: &Theme, width: u16) -> Vec<Line<'static>> {
    let (pass, warn, fail) = (h.count(Status::Pass), h.count(Status::Warn), h.count(Status::Fail));
    let total = h.checks.len();
    let lamp = if fail > 0 {
        Lamp::Fail
    } else if warn > 0 {
        Lamp::Warn
    } else {
        Lamp::Ok
    };
    let (verdict, tone) = match (fail, warn) {
        (0, 0) => ("Healthy", Tone::Ok),
        (0, _) => ("Healthy", Tone::Plain),
        _ => ("Needs a fix", Tone::Fail),
    };
    let mut stats = vec![Stat::new(pass, if pass == 1 { "check passed" } else { "checks passed" }, Tone::Ok)];
    if warn > 0 {
        stats.push(Stat::new(warn, "to look at", Tone::Warn));
    }
    if fail > 0 {
        stats.push(Stat::new(fail, "failing", Tone::Fail));
    }
    let verdict_style = if tone == Tone::Plain { t.bold() } else { tone.style(t).patch(t.bold()) };
    let mut row1 = vec![Span::styled(verdict.to_string(), verdict_style), Span::styled(t.sep(), t.dim())];
    row1.extend(kit::stat_chips(&stats, t));

    let room = (width as usize).saturating_sub(10);
    let meter_w = room.saturating_sub(16).clamp(8, 36);
    let mut row2 = kit::meter(if total == 0 { 0.0 } else { pass as f64 / total as f64 }, meter_w, Tone::Ok, t);
    row2.push(Span::styled(format!("  {pass} of {total} pass"), t.dim()));

    let row3 = vec![
        Span::styled("The same checks as ", t.dim()),
        Span::styled("adage doctor", t.command()),
        Span::styled(", rechecked live", t.dim()),
        Span::styled(t.sep(), t.dim()),
        Span::styled("R", t.key()),
        Span::styled(" checks now", t.dim()),
    ];
    let mut rows = vec![row1, row2, row3];
    if let Some(sv) = &h.savings {
        let mut l = view::savings_line(sv, t, room as u16).spans;
        if l.first().is_some_and(|s| s.content == " ") {
            l.remove(0);
        }
        rows.push(l);
    }
    if let Some(n) = &h.notice {
        rows.push(vec![
            Span::styled(format!("{} ", t.glyph(Glyph::Notice)), t.accent()),
            Span::styled("today ", t.ink2()),
            Span::raw(t.text(n)),
        ]);
    }
    let big = kit::big_lamp(lamp, t);
    rows.into_iter()
        .enumerate()
        .map(|(i, r)| {
            let mut v = vec![Span::raw("  ")];
            match big.get(i) {
                Some(b) => v.extend(b.spans.iter().cloned()),
                None => v.push(Span::raw("     ")),
            }
            v.push(Span::raw("   "));
            v.extend(kit::clip_spans(r, room, t));
            Line::from(v)
        })
        .collect()
}

/// The list: pinned problem cards, then each section's rows. With `open`, the selected item
/// opens in place into its card (the narrow layout). Returns the lines and the first and last
/// line of the selected item.
fn list(
    h: &Health,
    sel: usize,
    open: bool,
    notices: &[String],
    t: &Theme,
    width: usize,
) -> (Vec<Line<'static>>, (usize, usize)) {
    let mut out: Vec<Line<'static>> = Vec::new();
    let mut at = (0, 0);
    // A card one column in, as a block of lines.
    let indented = |lines: Vec<Line<'static>>| -> Vec<Line<'static>> {
        lines
            .into_iter()
            .map(|l| {
                let mut v = vec![Span::raw(" ")];
                v.extend(l.spans);
                Line::from(v)
            })
            .collect()
    };
    let card_w = width.saturating_sub(1);
    let opened = |i: usize| -> Vec<Line<'static>> {
        let (card, body) = detail(h, i, t, card_w.saturating_sub(4));
        indented(card.tone(Tone::Accent).lines(body, card_w, t))
    };
    let problems: Vec<usize> = order(h).into_iter().filter(|&i| problem(&h.checks[i])).collect();
    if !problems.is_empty() {
        let n = problems.len();
        out.push(
            Section::new("Needs a look")
                .count(n)
                .note(if n == 1 { "fix it here" } else { "fix them here" }, Tone::Warn)
                .line(width as u16, t),
        );
        for &i in &problems {
            let start = out.len();
            if open && i == sel {
                out.extend(opened(i));
            } else {
                out.extend(indented(problem_card(h, i, i == sel, t, card_w)));
            }
            if i == sel {
                at = (start, out.len() - 1);
            }
        }
    }
    let label_w = h.checks.iter().map(|c| kit::width(&label(&c.check))).max().unwrap_or(12).clamp(12, 24) + 2;
    for g in Group::ALL {
        let rows: Vec<usize> =
            (0..h.checks.len()).filter(|&i| !problem(&h.checks[i]) && group(&h.checks[i].check) == g).collect();
        if rows.is_empty() {
            continue;
        }
        if !out.is_empty() {
            out.push(Line::raw(""));
        }
        out.push(Section::new(g.title()).count(rows.len()).line(width as u16, t));
        for i in rows {
            let c = &h.checks[i];
            let start = out.len();
            if open && i == sel {
                out.extend(opened(i));
            } else {
                out.push(kit::row(lamp(c), &label(&c.check), &short(c), label_w, width, i == sel, t));
            }
            if i == sel {
                at = (start, out.len() - 1);
            }
        }
    }
    if !notices.is_empty() {
        out.push(Line::raw(""));
        out.push(Section::new("Messages").count(notices.len().min(3)).line(width as u16, t));
        for n in notices.iter().rev().take(3) {
            out.push(Line::from(vec![
                Span::raw("   "),
                Span::styled(kit::clip(&t.text(n), width.saturating_sub(4), t), t.yellow()),
            ]));
        }
    }
    (out, at)
}

/// A fix as spans: its command in the command colour, the words around it plain.
fn fix_spans(fx: &crate::doctor::Fix, t: &Theme) -> Vec<Span<'static>> {
    let prose = fx.prose();
    match fx.command() {
        Some(cmd) => {
            let quoted = format!("`{cmd}`");
            match prose.split_once(&quoted) {
                Some((before, after)) => vec![
                    Span::raw(before.to_string()),
                    Span::styled(cmd.to_string(), t.command()),
                    Span::raw(after.to_string()),
                ],
                None => vec![Span::styled(prose, t.command())],
            }
        }
        None => vec![Span::raw(prose.replace('`', ""))],
    }
}

/// A pinned problem: the problem in plain words, why, and the key that acts on it.
fn problem_card(h: &Health, i: usize, selected: bool, t: &Theme, width: usize) -> Vec<Line<'static>> {
    let c = &h.checks[i];
    let l = lamp(c);
    let fix = h.fixes.get(i).cloned().flatten();
    let detail = c.detail.trim_end_matches('.');
    let what = kit::first_clause(detail);
    let why = detail[what.len()..].trim_start_matches([';', ':', ',', '.', ' ']).trim();
    let inner = width.saturating_sub(4);
    let first = Line::from(kit::clip_spans(vec![Span::styled(sentence(what), t.bold())], inner, t));
    let second = if !why.is_empty() {
        Line::from(Span::styled(kit::clip(&sentence(why), inner, t), t.ink2()))
    } else if let Some(fx) = &c.fix {
        let mut v = vec![Span::styled("next  ", t.dim())];
        v.extend(fix_spans(fx, t));
        Line::from(kit::clip_spans(v, inner, t))
    } else {
        Line::raw("")
    };
    let hint: Vec<(&str, String)> = match (&fix, selected) {
        (Some(fx), true) => vec![("enter", fx.label())],
        (Some(fx), false) => fix_key(fx).map(|k| vec![(k, fx.label())]).unwrap_or_default(),
        (None, true) => vec![("enter", "read in full".to_string())],
        (None, false) => vec![],
    };
    let hint: Vec<(&str, &str)> = hint.iter().map(|(k, l)| (*k, l.as_str())).collect();
    let tone = if selected { Tone::Accent } else { Tone::of_lamp(l) };
    let title = format!("{} {}", t.lamp_mark(l), label(&c.check));
    Card::new(title).tone(tone).hints(&hint).lines(vec![first, second], width, t)
}

/// The selected check's card: its whole text, wrapped, its fix, and the check's name in
/// `adage doctor`.
fn detail(h: &Health, sel: usize, t: &Theme, inner: usize) -> (Card, Vec<Line<'static>>) {
    let Some(c) = h.checks.get(sel) else {
        return (Card::new("Check"), vec![Line::from(Span::styled("Nothing selected.", t.dim()))]);
    };
    let l = lamp(c);
    let state = match c.status {
        Status::Pass if l == Lamp::Off => "not in use here",
        Status::Pass => "passes",
        Status::Warn => "worth a look",
        Status::Fail => "failing",
    };
    let mut body = kit::wrap_spans(vec![Span::raw(t.text(&sentence(&c.detail)))], inner, 0);
    let mut kv = KeyValue::new();
    if let Some(fx) = &c.fix {
        // As doctor says it: a passing check's fix is a switch (or, for the binary, an update).
        let lead = match (c.status, c.check.as_str()) {
            (Status::Pass, "binary") => "update",
            (Status::Pass, _) => "switch",
            _ => "fix",
        };
        kv = kv.row(lead, Line::from(fix_spans(fx, t)));
    }
    kv = kv.row("doctor", Line::from(Span::styled(c.check.clone(), t.dim())));
    body.push(Line::raw(""));
    body.extend(kv.lines(inner, t));
    let fix = h.fixes.get(sel).cloned().flatten();
    let label_fix = fix.as_ref().map(Fix::label);
    let mut hint: Vec<(&str, &str)> = match &label_fix {
        Some(l) => vec![("enter", l.as_str())],
        None => vec![("enter", "read in full")],
    };
    hint.push(("R", "recheck"));
    let tone = if problem(c) { Tone::of_lamp(l) } else { Tone::Plain };
    let card = Card::new(format!("{} {}", t.lamp_mark(l), label(&c.check))).tone(tone).note(state);
    (card.hints(&hint), body)
}
