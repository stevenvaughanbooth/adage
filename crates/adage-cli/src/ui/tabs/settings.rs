//! The Settings tab: `config.toml`, as `adage config` shows it, edited in place.
//!
//! Grouped into a few named sections, one line per setting: a plain label, its value said in
//! words (`● on`, `2 min`, `none`), and a mark only where it differs from the default or comes
//! from somewhere that wins over the file. The selected setting's card at the foot says the rest:
//! what it does, its key, what it accepts, its default, and where the value came from.

use crate::ui::app::{Action, AppState, Cmd, Modal, SettingRow};
use crate::ui::theme::Theme;
use crate::ui::view::{self, Ctx};
use adage_term::Glyph;
use ratatui::crossterm::event::KeyCode;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

/// A section of the tab, in the order they are shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Section {
    Learning,
    Sources,
    Instincts,
    Display,
    World,
    App,
    Advanced,
}

impl Section {
    fn title(self) -> &'static str {
        match self {
            Section::Learning => "Learning",
            Section::Sources => "History sources",
            Section::Instincts => "Instincts & packs",
            Section::Display => "In Claude Code",
            Section::World => "The world",
            Section::App => "In adage ui",
            Section::Advanced => "Advanced",
        }
    }
}

/// How the tab presents one key: its section, a plain label, what it does in a sentence, and
/// what it accepts.
struct Meta {
    key: &'static str,
    section: Section,
    label: &'static str,
    about: &'static str,
    accepts: &'static str,
}

const fn m(
    key: &'static str,
    section: Section,
    label: &'static str,
    about: &'static str,
    accepts: &'static str,
) -> Meta {
    Meta { key, section, label, about, accepts }
}

/// Every key the tab knows by name. A key missing here still shows, under Advanced, by its key.
const META: &[Meta] = &[
    m(
        "learn.auto",
        Section::Learning,
        "Learn automatically",
        "Learn from each agent session in the background, when the session stops or ends.",
        "on or off",
    ),
    m(
        "learn.min_interval_secs",
        Section::Learning,
        "Pause between runs",
        "Learn automatically at most once per repository in this window.",
        "0 to 86400 seconds",
    ),
    m(
        "learn.thresholds",
        Section::Learning,
        "Evidence needed",
        "How much evidence a habit needs before it becomes a reflex.",
        "default, or demo (lower bars, for trying Adage out)",
    ),
    m(
        "learn.repos",
        Section::Learning,
        "Repositories",
        "Which repositories Adage learns from.",
        "all, or repository keys as adage init lists them, comma-separated",
    ),
    m(
        "learn.parse_cache",
        Section::Learning,
        "Reuse parsed sessions",
        "Keep parsed sessions cached, so learning again only reads what changed.",
        "on or off",
    ),
    m(
        "learn.cards",
        Section::Learning,
        "Procedure cards",
        "Learn step-by-step cards for families of similar tasks.",
        "on or off",
    ),
    m(
        "learn.card_threshold",
        Section::Learning,
        "Card match strictness",
        "How closely a prompt must match a family of tasks to be given its card.",
        "0.5 to 0.99; higher is stricter",
    ),
    m(
        "learn.claude_projects",
        Section::Sources,
        "Claude Code history",
        "Where Adage finds Claude Code sessions to learn from.",
        "a folder (absolute, or starting ~/), or empty for ~/.claude/projects",
    ),
    m(
        "learn.codex_sessions",
        Section::Sources,
        "Codex history",
        "Where Adage finds Codex sessions to learn from.",
        "auto (once Codex hooks are registered), a folder, or empty for none",
    ),
    m(
        "learn.opencode_exports",
        Section::Sources,
        "OpenCode exports",
        "A folder of `opencode export` files to learn from.",
        "a folder, or empty for none",
    ),
    m(
        "instincts.enabled",
        Section::Instincts,
        "Built-in instincts",
        "Use Adage's built-in good habits, chosen from each repository's own files.",
        "on or off",
    ),
    m(
        "instincts.packs",
        Section::Instincts,
        "Instinct packs",
        "Which instinct packs to use.",
        "auto (default and added packs), plus optional pack names, comma-separated",
    ),
    m(
        "instincts.disabled",
        Section::Instincts,
        "Always leave out",
        "Instinct packs or single instincts never to use.",
        "pack names or ids, comma-separated, e.g. adage/lockfiles,U3 (see docs/instincts.md)",
    ),
    m(
        "ui.notices",
        Section::Display,
        "Session-start notice",
        "One line when a Claude Code session starts, and only when something is new.",
        "daily or off",
    ),
    m(
        "ui.spinner",
        Section::Display,
        "Spinner note",
        "Add an adage note to the spinner on turns where a reflex was delivered.",
        "on or off",
    ),
    m(
        "ui.turn_line",
        Section::Display,
        "Turn summary",
        "A one-line Adage summary beside the turn's duration, on turns with Adage activity.",
        "on or off",
    ),
    m(
        "ui.noticed",
        Section::Display,
        "Habit hints",
        "A line under a batch of tool calls when Adage notices a habit it is still counting.",
        "on or off",
    ),
    m(
        "world.enabled",
        Section::World,
        "Record what you do",
        "Keep a timeline of what you were doing (meetings, pull requests, tickets, files) and tell the agent which one a prompt means. A person's decision: adage world on.",
        "on or off",
    ),
    m(
        "world.desktop",
        Section::World,
        "Desktop titles",
        "Sample the active app and window title, titles only: never pixels, never keystrokes.",
        "on or off",
    ),
    m(
        "world.roots",
        Section::World,
        "Watched folders",
        "Folders whose recently edited files are things a prompt may refer to.",
        "absolute folders or ~/…, comma-separated; empty watches none",
    ),
    m(
        "world.transcripts",
        Section::World,
        "Transcript folders",
        "Folders where your meeting tools write transcripts.",
        "absolute folders or ~/…, comma-separated; empty watches none",
    ),
    m(
        "world.deny",
        Section::World,
        "Never recorded",
        "App names, window titles, addresses and paths that are never recorded, as patterns.",
        "glob patterns, comma-separated",
    ),
    m(
        "world.retention_days",
        Section::World,
        "Keep activity for",
        "How long the activity timeline is kept; the things it names keep only their titles.",
        "1 to 365 days",
    ),
    m(
        "ui.start_screen",
        Section::App,
        "Start screen",
        "Open on Adage's brain, turning, for about two seconds. Any key skips it.",
        "on or off",
    ),
    m(
        "runtime.rewrite",
        Section::Advanced,
        "Fix Python test runs",
        "Point Python test runs under the system Python at the project's verified environment before they run.",
        "on, or off to leave every command as the agent wrote it",
    ),
    m(
        "runtime.control",
        Section::Advanced,
        "Measure savings",
        "Hold out a share of sessions where Adage stays silent, so adage savings can compare them with the rest.",
        "0 (off: every session gets Adage) to 1; 0.1 measures with one session in ten",
    ),
    m(
        "runtime.routine_model",
        Section::Advanced,
        "Model for routine steps",
        "Send a step that only does what a delivered directive says to this model, at low effort.",
        "a model name or alias, e.g. haiku; empty for the session's own model",
    ),
    m(
        "runtime.predict_cap",
        Section::Advanced,
        "Steps Adage may take",
        "At most this many steps per session that Adage takes in the model's place, under a predict grant.",
        "0 to 50; 0 for none",
    ),
    m(
        "skills.invoke",
        Section::Advanced,
        "Invoke skills",
        "Run a skill at the step a skill reflex names, when it holds a predict grant and comes from a pack you accepted.",
        "on or off",
    ),
    m(
        "update.check",
        Section::Advanced,
        "Check for updates",
        "At most once a day, after background learning, ask whether a newer Adage is out. Nothing is sent but the request.",
        "daily or off",
    ),
];

fn meta(key: &str) -> Option<&'static Meta> {
    META.iter().find(|m| m.key == key)
}

/// The plain label a key is shown by: `Learn automatically` for `learn.auto`.
pub fn label(key: &str) -> &str {
    meta(key).map_or(key, |m| m.label)
}

fn section(key: &str) -> Section {
    meta(key).map_or(Section::Advanced, |m| m.section)
}

/// The settings' indices in the order they are shown: by section, then as [`META`] lists them.
fn order(s: &AppState) -> Vec<usize> {
    let rank = |k: &str| META.iter().position(|m| m.key == k).unwrap_or(usize::MAX);
    let mut ix: Vec<usize> = (0..s.settings.len()).collect();
    ix.sort_by_key(|&i| (section(s.settings[i].key), rank(s.settings[i].key), i));
    ix
}

/// The key `space` switches between two values, other than on and off.
fn switch(key: &str, value: &str) -> Option<&'static str> {
    match key {
        "learn.thresholds" => Some(if value == "demo" { "default" } else { "demo" }),
        "ui.notices" | "update.check" => Some(if value == "off" { "daily" } else { "off" }),
        _ => None,
    }
}

pub fn key(s: &mut AppState, code: KeyCode) -> Option<Cmd> {
    let order = order(s);
    let at = order.iter().position(|&i| i == s.setting_sel).unwrap_or(0);
    let go = |s: &mut AppState, to: usize| {
        if let Some(&i) = order.get(to.min(order.len().saturating_sub(1))) {
            s.setting_sel = i;
        }
    };
    match code {
        KeyCode::Up | KeyCode::Char('k') => go(s, at.saturating_sub(1)),
        KeyCode::Down | KeyCode::Char('j') => go(s, at + 1),
        KeyCode::Home | KeyCode::Char('g') => go(s, 0),
        KeyCode::End | KeyCode::Char('G') => go(s, usize::MAX),
        // The first setting of the previous or next section.
        KeyCode::Char('[') | KeyCode::Char(']') | KeyCode::PageUp | KeyCode::PageDown => {
            let here = section(s.settings.get(s.setting_sel)?.key);
            let forward = matches!(code, KeyCode::Char(']') | KeyCode::PageDown);
            let firsts: Vec<usize> = (0..order.len())
                .filter(|&p| p == 0 || section(s.settings[order[p]].key) != section(s.settings[order[p - 1]].key))
                .collect();
            let target = if forward {
                firsts.iter().copied().find(|&p| p > at && section(s.settings[order[p]].key) != here)
            } else {
                let start = firsts.iter().copied().rfind(|&p| p <= at).unwrap_or(0);
                if start < at {
                    Some(start)
                } else {
                    firsts.iter().copied().rfind(|&p| p < start)
                }
            };
            if let Some(p) = target {
                go(s, p);
            }
        }
        KeyCode::Char('d') => return reset(s),
        KeyCode::Enter | KeyCode::Char(' ') | KeyCode::Char('e') => {
            let row = s.settings.get(s.setting_sel)?;
            // Auto-condition starts or stops background work: it asks first, as Health's fix does.
            if row.key == "learn.auto" {
                let on = row.value != "true";
                s.confirm_auto_condition(on);
                return None;
            }
            if row.boolean() {
                let next = if row.value == "true" { "false" } else { "true" };
                return Some(Cmd::Do(Action::SetConfig { key: row.key, value: next.into() }));
            }
            if code == KeyCode::Char(' ') {
                if let Some(next) = switch(row.key, &row.value) {
                    return Some(Cmd::Do(Action::SetConfig { key: row.key, value: next.into() }));
                }
            }
            s.modal = Some(Modal::Edit { key: row.key, buffer: row.value.clone(), error: None });
        }
        _ => {}
    }
    None
}

/// `d`: the selected setting back to its default. A switch goes straight back, as space would
/// switch it; a value someone typed (a folder, a list, a number) is shown before it is replaced.
fn reset(s: &mut AppState) -> Option<Cmd> {
    let row = s.settings.get(s.setting_sel)?;
    let (key, default) = (row.key, row.default);
    if row.value == default {
        s.toast(format!("{} is already the default", label(key)), true);
        return None;
    }
    if key == "learn.auto" {
        s.confirm_auto_condition(default == "true");
        return None;
    }
    let action = Action::SetConfig { key, value: default.into() };
    if row.boolean() || switch(key, &row.value).is_some() {
        return Some(Cmd::Do(action));
    }
    let t = Theme::from_env();
    let (now, _) = shown(row, &t);
    let was = shown_default(row, &t);
    let lines = vec![
        Line::from(vec![Span::styled("now      ", t.dim()), Span::raw(now)]),
        Line::from(vec![Span::styled("default  ", t.dim()), Span::styled(was, t.bold())]),
        Line::raw(""),
        Line::from(vec![
            Span::styled("same as ", t.dim()),
            Span::styled(format!("adage config set {key} {}", quoted(default)), t.command()),
        ]),
        Line::from(vec![
            Span::styled("y", t.key()),
            Span::styled(format!(" reset{}", t.sep()), t.dim()),
            Span::styled("n", t.key()),
            Span::styled(" keep it", t.dim()),
        ]),
    ];
    s.confirm(&format!("Reset {} to its default", label(key)), lines, action);
    None
}

/// A value as a shell takes it: `""` for an empty one.
fn quoted(v: &str) -> String {
    if v.is_empty() {
        "\"\"".into()
    } else {
        v.into()
    }
}

/// A value in words, and its style: `● on`, `○ off`, `2 min`, `none`.
fn shown(row: &SettingRow, t: &Theme) -> (String, Style) {
    let v = row.value.as_str();
    if row.boolean() {
        return if v == "true" {
            (format!("{} on", t.glyph(Glyph::Lamp)), t.accent())
        } else {
            (format!("{} off", t.glyph(Glyph::Off)), t.dim())
        };
    }
    let plain = |s: String| (s, Style::default());
    let quiet = |s: &str| (s.to_string(), t.dim());
    let list = |v: &str| v.split(',').map(str::trim).filter(|x| !x.is_empty()).collect::<Vec<_>>().join(", ");
    match (row.key, v) {
        ("learn.min_interval_secs", _) => match v.parse::<u64>() {
            Ok(0) => quiet("no pause"),
            Ok(n) if n % 3600 == 0 => plain(format!("{} h", n / 3600)),
            Ok(n) if n % 60 == 0 => plain(format!("{} min", n / 60)),
            Ok(n) => plain(format!("{n} s")),
            Err(_) => plain(v.into()),
        },
        ("learn.thresholds", "default") => plain("standard".into()),
        ("learn.thresholds", "demo") => plain("lower (demo)".into()),
        ("learn.repos", "all") => plain("all repositories".into()),
        ("learn.claude_projects", "") => quiet("default (~/.claude/projects)"),
        ("learn.codex_sessions", "auto") => plain("auto, once Codex hooks are on".into()),
        ("instincts.packs", "auto") => plain("auto, defaults + added packs".into()),
        ("learn.repos" | "instincts.packs" | "instincts.disabled", _) if !v.is_empty() => plain(list(v)),
        ("ui.notices" | "update.check", "off") => quiet("off"),
        ("runtime.control", _) => match v.parse::<f64>() {
            Ok(x) if x <= 0.0 => quiet("off"),
            Ok(x) => plain(format!("{}% of sessions held out", (x * 1000.0).round() / 10.0)),
            Err(_) => plain(v.into()),
        },
        ("runtime.routine_model", "") => quiet("the session's own model"),
        ("runtime.predict_cap", "0") => quiet("none"),
        ("runtime.predict_cap", _) => plain(format!("{v} per session")),
        ("world.retention_days", _) => plain(format!("{v} days")),
        ("world.roots" | "world.transcripts", _) if !v.is_empty() => plain(list(v)),
        // The deny list is long by default: the first few, and how many more.
        ("world.deny", _) if !v.is_empty() => {
            let items: Vec<&str> = v.split(',').map(str::trim).filter(|x| !x.is_empty()).collect();
            if items.len() > 3 {
                plain(format!("{} +{} more", items[..3].join(", "), items.len() - 3))
            } else {
                plain(items.join(", "))
            }
        }
        (_, "") => quiet("none"),
        _ => plain(v.into()),
    }
}

/// The default, in the same words as a value.
fn shown_default(row: &SettingRow, t: &Theme) -> String {
    let d = SettingRow {
        key: row.key,
        value: row.default.to_string(),
        source: "default".into(),
        doc: row.doc,
        default: row.default,
    };
    shown(&d, t).0
}

/// The mark after a value: where it comes from, when that is not the default. An environment
/// variable or the older marker wins over the file, so it is always marked; a value the file
/// sets is marked only when it differs from the default.
fn mark(row: &SettingRow, t: &Theme) -> Option<Span<'static>> {
    if let Some(var) = row.source.strip_prefix("env ") {
        return Some(Span::styled(format!("env {var}"), t.yellow()));
    }
    if row.source.contains("marker") {
        return Some(Span::styled("off by marker", t.yellow()));
    }
    (row.value != row.default).then(|| Span::styled(format!("{} changed", t.ch("•", "*")), t.accent()))
}

/// Where the value came from, in a sentence's words.
fn source_words(row: &SettingRow) -> String {
    match row.source.as_str() {
        "default" => "the default".into(),
        "file" => "set in config.toml".into(),
        // The card's warning line says what that means for a change here.
        s if s.starts_with("env ") => format!("set by the environment ({})", &s[4..]),
        s => format!("set by the {s}, which wins over config.toml"),
    }
}

/// The selected setting's card: what it does, its key and range, its default and source, and
/// the keys that change it. As blocks with a rank (0 first), so a short pane keeps what matters
/// most: the name and key, what it does, how to change it.
fn card(row: &SettingRow, t: &Theme, width: u16, roomy: bool) -> Vec<(u8, Vec<Line<'static>>)> {
    let mt = meta(row.key);
    let about = mt.map_or(row.doc, |m| m.about);
    let note = Span::styled(row.key.to_string(), t.command());
    let mut d: Vec<(u8, Line<'static>)> = vec![(0, view::rule(label(row.key), Some(note), width, t))];
    d.push((1, Line::from(Span::raw(format!(" {about}")))));
    let sep = || Span::styled(t.sep(), t.dim());
    let now = Span::styled(source_words(row), if row.source == "default" { t.ink2() } else { t.accent() });
    if roomy {
        // Beside the list there is room for a fact a line, under a column of names.
        d.push((1, Line::raw("")));
        let fact =
            |name: &str, value: Span<'static>| Line::from(vec![Span::styled(format!(" {name:<9}"), t.dim()), value]);
        if let Some(m) = mt {
            d.push((4, fact("accepts", Span::styled(m.accepts, t.ink2()))));
        }
        d.push((4, fact("default", Span::styled(shown_default(row, t), t.ink2()))));
        d.push((4, fact("now", now)));
        d.push((4, Line::raw("")));
    } else {
        let mut facts = vec![Span::raw(" ")];
        if let Some(m) = mt {
            facts.extend([Span::styled("accepts ", t.dim()), Span::styled(m.accepts, t.ink2()), sep()]);
        }
        facts.push(Span::styled("default ", t.dim()));
        facts.push(Span::styled(shown_default(row, t), t.ink2()));
        facts.push(sep());
        facts.push(Span::styled("now ", t.dim()));
        facts.push(now);
        d.push((4, Line::from(facts)));
    }
    if row.key == "runtime.rewrite" {
        let (to, what) = if row.value == "true" { ("off", "on") } else { ("on", "off") };
        d.push((
            5,
            Line::from(vec![
                Span::styled(format!(" {} rewrites {what}{}same as ", t.glyph(Glyph::Rewrite), t.sep()), t.dim()),
                Span::styled(format!("adage delegate rewrites {to}"), t.command()),
                Span::styled(" to switch", t.dim()),
            ]),
        ));
    }
    let on_off = |next: &str| if next == "true" { "on" } else { "off" };
    let (keys, does, value): (&str, String, String) = if row.key == "learn.auto" {
        let next = if row.value == "true" { "false" } else { "true" };
        ("enter", format!(" turns it {} (asks first)", on_off(next)), next.into())
    } else if row.boolean() {
        let next = if row.value == "true" { "false" } else { "true" };
        ("space", format!(" turns it {}", on_off(next)), next.into())
    } else if let Some(next) = switch(row.key, &row.value) {
        ("space", format!(" switches to {next}{}enter types a value", t.sep()), next.into())
    } else {
        ("enter", " edits; checked before it is saved".into(), "<value>".into())
    };
    let same = Span::styled(format!("adage config set {} {value}", row.key), t.command());
    if roomy {
        d.push((
            2,
            Line::from(vec![
                Span::styled(format!(" {keys:<9}"), t.key()),
                Span::styled(does.trim().to_string(), t.ink2()),
            ]),
        ));
        d.push((2, Line::from(vec![Span::styled(format!(" {:<9}", "same as"), t.dim()), same])));
    } else {
        d.push((
            2,
            Line::from(vec![
                Span::styled(format!(" {keys}"), t.key()),
                Span::styled(does, t.ink2()),
                Span::styled(format!("{}same as ", t.sep()), t.dim()),
                same,
            ]),
        ));
    }
    // A typed value the file sets only: a switch's own key already puts it back, and a reset
    // would not undo an environment variable.
    let typed = !row.boolean() && switch(row.key, &row.value).is_none();
    if row.value != row.default && typed && !row.source.starts_with("env ") {
        let back = Span::styled(format!("back to the default, {}", shown_default(row, t)), t.ink2());
        d.push((
            6,
            if roomy {
                Line::from(vec![Span::styled(format!(" {:<9}", "d"), t.key()), back])
            } else {
                Line::from(vec![Span::styled(" d ", t.key()), back])
            },
        ));
    }
    if let Some(var) = row.source.strip_prefix("env ") {
        d.push((
            3,
            Line::from(Span::styled(
                format!(" ! {var} wins over config.toml: a change here takes effect once it is unset"),
                t.yellow(),
            )),
        ));
    }
    d.into_iter().map(|(rank, l)| (rank, view::wrap_hanging(t.lines(vec![l]), width, roomy))).collect()
}

/// The card's blocks that fit in `budget` rows, most important first, shown in their own order.
fn fit_card(blocks: Vec<(u8, Vec<Line<'static>>)>, budget: usize) -> Vec<Line<'static>> {
    let mut by_rank: Vec<usize> = (0..blocks.len()).collect();
    by_rank.sort_by_key(|&i| blocks[i].0);
    let mut used = 0;
    let mut keep = vec![false; blocks.len()];
    for i in by_rank {
        if used + blocks[i].1.len() <= budget {
            used += blocks[i].1.len();
            keep[i] = true;
        }
    }
    blocks.into_iter().zip(keep).filter(|(_, k)| *k).flat_map(|((_, b), _)| b).collect()
}

pub fn draw(f: &mut Frame, area: Rect, s: &AppState, ctx: &Ctx) {
    let t = &ctx.theme;
    let w = area.width as usize;

    // The head, one line: how many settings and how many changed, and the file they live in
    // (cut from the left, so its name survives).
    let changed = s.settings.iter().filter(|r| r.value != r.default).count();
    let env = s.settings.iter().filter(|r| r.source.starts_with("env ")).count();
    let mut spans = vec![
        Span::styled(format!(" {} settings", s.settings.len()), t.ink2().patch(t.bold())),
        Span::styled(t.sep(), t.dim()),
        Span::styled(format!("{changed} changed"), if changed > 0 { t.accent() } else { t.dim() }),
        Span::styled(t.sep(), t.dim()),
        Span::styled("same as ", t.dim()),
        Span::styled("adage config", t.command()),
        Span::styled(t.sep(), t.dim()),
    ];
    let used: usize = spans.iter().map(|s| s.width()).sum();
    let path = crate::detect::tilde(&s.config_path);
    spans.push(Span::styled(crate::ui::words::fit_left(&path, w.saturating_sub(used + 1), t.more()), t.ink2()));
    let mut head = vec![Line::from(spans)];
    if env > 0 {
        let what = if env == 1 { "setting is" } else { "settings are" };
        let note = format!(" ! {env} {what} set by environment variables, which win over the file");
        head.push(Line::from(Span::styled(t.fit(&note, w), t.yellow())));
    }
    for p in &s.settings_problems {
        head.push(Line::from(Span::styled(t.fit(&format!(" ! {p}"), w), t.yellow())));
    }
    let head = t.lines(head);

    let [head_a, _, body] =
        Layout::vertical([Constraint::Length(head.len() as u16), Constraint::Length(1), Constraint::Min(3)])
            .areas(area);
    f.render_widget(Paragraph::new(head), head_a);
    // Wide, the card stands beside the list with room for all of it; otherwise under the list,
    // with what the list can spare (about two fifths of the pane).
    let sel = s.settings.get(s.setting_sel);
    let (list_a, card_a, card) = if area.width >= 120 {
        // As wide as the list's longest line, so the card sits close by.
        let label_max = s.settings.iter().map(|r| label(r.key).chars().count()).max().unwrap_or(10);
        let value_max = s
            .settings
            .iter()
            .map(|r| shown(r, t).0.chars().count() + mark(r, t).map_or(0, |m| m.width() + 2))
            .max()
            .unwrap_or(30);
        let natural = 4 + label_max + 2 + value_max + 2;
        let left = (natural as u16).clamp(56, area.width * 3 / 5);
        let [l, _, r] =
            Layout::horizontal([Constraint::Length(left), Constraint::Length(2), Constraint::Min(20)]).areas(body);
        let card = sel.map(|row| fit_card(card(row, t, r.width, true), r.height as usize)).unwrap_or_default();
        (l, r, card)
    } else {
        let budget = (body.height as usize * 2 / 5).max(3);
        let card = sel.map(|row| fit_card(card(row, t, body.width, false), budget)).unwrap_or_default();
        let [l, c] = Layout::vertical([Constraint::Min(3), Constraint::Length(card.len() as u16)]).areas(body);
        (l, c, card)
    };
    let w = list_a.width as usize;

    // The list: a heading per section, then one line per setting.
    let order = order(s);
    let label_w = order.iter().map(|&i| label(s.settings[i].key).chars().count()).max().unwrap_or(10).min(w / 2);
    let mut lines: Vec<Line> = Vec::new();
    let mut sel_row = 0;
    let mut last = None;
    for &i in &order {
        let row = &s.settings[i];
        let sec = section(row.key);
        if last != Some(sec) {
            if last.is_some() {
                lines.push(Line::raw(""));
            }
            let rows = || order.iter().map(|&j| &s.settings[j]).filter(|r| section(r.key) == sec);
            let changed = rows().filter(|r| r.value != r.default).count();
            let mut heading = vec![
                Span::styled(format!("  {}", sec.title()), t.title()),
                Span::styled(format!("  {}", rows().count()), t.dim()),
            ];
            if changed > 0 {
                heading.push(Span::styled(t.sep(), t.dim()));
                heading.push(Span::styled(format!("{changed} changed"), t.accent()));
            }
            lines.push(Line::from(heading));
            last = Some(sec);
        }
        let selected = i == s.setting_sel;
        if selected {
            sel_row = lines.len();
        }
        let (value, style) = shown(row, t);
        let mark = mark(row, t);
        let mark_w = mark.as_ref().map_or(0, |m| m.width() + 2);
        let value_w = w.saturating_sub(4 + label_w + 2 + mark_w + 1).max(4);
        let mut spans = vec![
            Span::styled(if selected { t.ch("▌", ">") } else { " " }, t.accent()),
            Span::raw("   "),
            Span::styled(t.pad(label(row.key), label_w + 2), if selected { t.bold() } else { t.ink2() }),
            Span::styled(t.fit(&value, value_w), if selected { style.patch(t.bold()) } else { style }),
        ];
        if let Some(m) = mark {
            spans.push(Span::raw("  "));
            spans.push(m);
        }
        lines.push(Line::from(spans));
    }
    // Scrolled so the selected setting shows with a line of room below it, and its section's
    // heading too while it fits.
    let h = list_a.height as usize;
    let mut offset = (sel_row + 2).saturating_sub(h).min(sel_row.saturating_sub(1));
    // Never a blank gap at the top: start at the section heading below it instead.
    if offset > 0 && offset < sel_row && lines.get(offset).is_some_and(|l| l.width() == 0) {
        offset += 1;
    }
    f.render_widget(Paragraph::new(t.lines(lines)).scroll((offset as u16, 0)), list_a);
    f.render_widget(Paragraph::new(card), card_a);
}
