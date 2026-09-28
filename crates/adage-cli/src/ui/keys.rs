//! The keymap: one table per screen. The status bar and the `?` overlay are both generated from
//! these tables, so they never drift apart.

use super::theme::Theme;
use ratatui::text::{Line, Span};

/// One key: what the status bar says (`label`), and what the overlay says (`about`).
#[derive(Debug, Clone, Copy)]
pub struct Key {
    pub keys: &'static str,
    pub label: &'static str,
    pub about: &'static str,
    /// Shown in the status bar (else only in the overlay).
    pub bar: bool,
}

const fn k(keys: &'static str, label: &'static str, about: &'static str, bar: bool) -> Key {
    Key { keys, label, about, bar }
}

/// `adage watch`.
pub const WATCH: &[Key] = &[
    k("p", "pause", "pause or resume the feed", true),
    k("f", "scope", "this repository or all repositories", true),
    k("/", "filter", "filter the feed by trace, reflex or command", true),
    k("enter", "inspect", "what the agent saw at the newest event shown", true),
    k("m", "open map", "how to open the map", true),
    k("?", "keys", "this list", true),
    k("j/k ↑↓", "scroll", "scroll the feed", false),
    k("G", "newest", "back to the newest event", false),
    k("v", "quiet", "hide or show events where Adage said nothing", false),
    k("esc", "clear", "clear the filter, close a popup; with neither, quit", false),
    k("q", "quit", "quit", false),
];

/// Every tab of `adage ui`.
pub const GLOBAL: &[Key] = &[
    k("?", "keys", "this list", true),
    k(":", "palette", "command palette (also ctrl-k): every action and reflex", true),
    k("tab", "next tab", "next tab (shift-tab: previous)", false),
    k("1-5", "tabs", "Live, Reflexes, Map, Health, Settings", false),
    k("q", "quit", "quit", false),
];

pub const LIVE: &[Key] = &[
    k("p", "pause", "pause or resume the feed", true),
    k("f", "scope", "this repository or all repositories", true),
    k("/", "filter", "filter the feed", true),
    k("enter", "inspect", "what the agent saw at the newest event shown", true),
    k("j/k ↑↓", "scroll", "scroll the feed (G: back to the newest)", false),
    k("v", "quiet", "hide or show events where Adage said nothing", false),
];

pub const REFLEXES: &[Key] = &[
    k("/", "filter", "fuzzy filter reflexes by id, trigger, command or track", true),
    k("j/k", "move", "move through the tree", true),
    k("enter", "expand", "open or close a group", true),
    k("tab", "next tab", "next tab", true),
    k("s", "strengthen", "strengthen: prime → fire (adage review strengthen)", false),
    k("w", "weaken", "weaken: fire → prime (adage review weaken)", false),
    k("x", "extinguish", "extinguish the reflex (adage review extinguish)", false),
    k("a", "approve", "approve it as it is (adage review approve)", false),
    k("n", "pin", "pin it: conditioning proposes instead of overwriting", false),
    k("m", "dismiss", "dismiss a pinned reflex's proposed change", false),
    k("d", "delegate", "let the reflex act (shows adage review delegate)", false),
    k("t", "traces", "the traces it fired in", false),
    k("o", "open in map", "the same reflex in the web map (adage map --focus)", false),
    k("F P I", "strengths", "show or hide fire, prime, inhibit", false),
    k("S", "states", "cycle the lifecycle filter", false),
    k("PgUp PgDn", "scroll", "scroll the detail", false),
    k("R", "reload", "reload the reflexes", false),
];

pub const MAP: &[Key] = &[
    k("enter", "inspect", "open the selected reflex in the Reflexes tab", true),
    k("f", "focus", "focus: only reflexes 2 hops from the selection", true),
    k("+/-", "zoom", "boxes or compact rows", true),
    k("j/k", "move", "select the next or previous reflex", true),
    k("o", "open in browser", "the same node in the web map (adage map --focus)", true),
];

pub const HEALTH: &[Key] = &[
    k("enter", "fix", "fix the selected check (⏎ marks fixable ones)", true),
    k("R", "recheck", "run the checks again", true),
    k("j/k", "move", "select a check", false),
    k("i", "install hooks", "install hooks for a harness", false),
    k("m", "fetch model", "fetch the intent model (adage models fetch)", false),
    k("c", "condition now", "condition this repository now (adage condition)", false),
    k("s", "settings", "open the settings", false),
    k("o", "fail-open log", "open the fail-open log", false),
];

pub const SETTINGS: &[Key] = &[
    k("enter", "edit", "edit (toggles on/off)", true),
    k("space", "toggle", "toggle on/off, or switch thresholds", true),
    k("j/k", "move", "select a setting", false),
];

pub const PALETTE: &[Key] = &[
    k("↑↓", "move", "move", true),
    k("enter", "run", "run", true),
    k("tab", "complete", "complete the query", true),
    k("esc", "close", "close", true),
];

/// The bar entries as spans (`p pause · f scope`), as many as fit in `width` columns.
pub fn bar(tables: &[&[Key]], t: &Theme, width: usize) -> Vec<Span<'static>> {
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut used = 0;
    for key in tables.iter().flat_map(|k| k.iter()).filter(|k| k.bar) {
        let sep = if spans.is_empty() { 0 } else { 3 };
        let keys = t.text(key.keys);
        let w = keys.chars().count() + 1 + key.label.chars().count();
        if used + sep + w > width {
            break;
        }
        if sep > 0 {
            spans.push(Span::styled(t.sep(), t.dim()));
        }
        spans.push(Span::styled(keys, t.key()));
        spans.push(Span::styled(format!(" {}", key.label), t.dim()));
        used += sep + w;
    }
    spans
}

/// The `?` overlay: each section's keys, aligned.
pub fn overlay(sections: &[(&str, &[Key])], t: &Theme) -> Vec<Line<'static>> {
    let w = sections.iter().flat_map(|(_, ks)| ks.iter()).map(|k| t.text(k.keys).chars().count()).max().unwrap_or(4);
    let mut out = Vec::new();
    for (i, (title, ks)) in sections.iter().enumerate() {
        if i > 0 {
            out.push(Line::raw(""));
        }
        out.push(Line::from(Span::styled(title.to_uppercase(), t.dim().patch(t.bold()))));
        for key in ks.iter() {
            // Two spans: a wrapped description continues under the second (`view::popup_keys`).
            out.push(Line::from(vec![
                Span::styled(format!("  {:>w$}  ", t.text(key.keys)), t.key()),
                Span::raw(t.text(key.about)),
            ]));
        }
    }
    out
}
