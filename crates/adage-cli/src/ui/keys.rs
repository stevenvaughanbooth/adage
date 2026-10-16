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
    k(":", "palette", "open the full app command palette (also ctrl-k)", true),
    k("p", "pause", "pause or resume the feed", true),
    k("f", "scope", "this repository or all repositories", true),
    k("/", "filter", "filter the feed by trace, reflex or command", true),
    k("enter", "inspect", "what the agent saw at the newest event shown", true),
    k("m", "open map", "open the live web map from the terminal app", true),
    k("?", "keys", "this list", true),
    k("j/k ↑↓", "scroll", "scroll the feed", false),
    k("G", "newest", "back to the newest event", false),
    k("v", "quiet", "hide or show events where Adage said nothing", false),
    k("l", "all runs", "every conditioning run, and what each changed", false),
    k("esc", "clear", "clear the filter, close a popup; with neither, quit", false),
    k("q", "quit", "quit", false),
];

/// Every tab of `adage ui`.
pub const GLOBAL: &[Key] = &[
    k("?", "keys", "this list", true),
    k(":", "palette", "command palette (also ctrl-k): every action and reflex", true),
    k("tab", "next tab", "next tab (shift-tab: previous)", false),
    k("1-6", "tabs", "Live, Reflexes, Map, Health, Settings, Home", false),
    k("q", "quit", "quit", false),
];

pub const HOME: &[Key] = &[
    k("enter", "choose", "open the selected step", true),
    k("j/k", "move", "select a step", true),
    k(
        "u",
        "update",
        "update Adage, its hooks and the plugin (adage update; the first row says when one is out)",
        false,
    ),
];

pub const LIVE: &[Key] = &[
    k("p", "pause", "pause or resume the feed", true),
    k("f", "repository", "choose the repository Live, Reflexes and Map show, or all of them", true),
    k("/", "filter", "filter the feed", true),
    k("enter", "inspect", "what the agent saw at the newest event shown", true),
    k("j/k ↑↓", "scroll", "scroll the feed (G: back to the newest)", false),
    k("v", "quiet", "hide or show events where Adage said nothing", false),
    k("l", "all runs", "every conditioning run, and what each changed", false),
    k("m", "open map", "open the live web map in your browser (as adage watch does)", false),
];

pub const REFLEXES: &[Key] = &[
    k("/", "filter", "fuzzy filter reflexes by id, trigger, command or track", true),
    k("f", "repository", "choose the repository whose reflexes are shown, or all of them", true),
    k("j/k", "move", "move through the tree", true),
    k("enter", "expand", "open or close a group, or a reflex's details", true),
    k("tab", "next tab", "next tab", true),
    k("s", "strengthen", "strengthen: prime → fire (adage review strengthen)", false),
    k("w", "weaken", "weaken: fire → prime (adage review weaken)", false),
    k("x", "extinguish", "extinguish the reflex (adage review extinguish)", false),
    k("a", "approve", "approve it as it is (adage review approve)", false),
    k("n", "pin", "pin it: conditioning proposes instead of overwriting", false),
    k("m", "dismiss", "dismiss a pinned reflex's proposed change", false),
    k("d", "delegate", "review execution authority in a guided form", false),
    k("t", "traces", "the traces it fired in", false),
    k("o", "open in map", "the same reflex in the web map (adage map --focus)", false),
    k("r", "raw", "what the agent sees, exactly as delivered (tags and all)", false),
    k("F P I", "strengths", "show or hide fire, prime, inhibit", false),
    k("S", "states", "cycle the lifecycle filter", false),
    k("esc", "reset filters", "reset the search, strength and lifecycle filters", false),
    k("PgUp PgDn", "scroll", "scroll the card", false),
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
    k("enter", "fix", "fix the selected check, or read its full explanation and next steps", true),
    k("R", "recheck", "run the checks again", true),
    k("j/k", "move", "select a check, in the order shown", false),
    k("g/G", "first/last", "the first or last check", false),
    k("i", "install hooks", "install hooks for a harness", false),
    k("m", "fetch model", "fetch the intent model (adage models fetch)", false),
    k("c", "condition now", "condition this repository now (adage condition)", false),
    k("s", "settings", "open the settings", false),
    k("o", "fail-open log", "open the fail-open log", false),
];

pub const SETTINGS: &[Key] = &[
    k("enter", "edit", "edit (toggles on/off)", true),
    k("space", "toggle", "toggle on/off, or switch thresholds, notices and the update check", true),
    k("j/k ↑↓", "move", "select a setting", false),
    k("[ ]", "section", "the previous or next section (also page up and down)", false),
    k("g/G", "first/last", "the first or last setting", false),
    k("d", "default", "put the selected setting back to its default (asks first for a typed value)", false),
];

/// The conditioning result card (`ui::conditioned`); `enter` is drawn as the card's button.
pub const CONDITIONED: &[Key] = &[
    k("enter", "next step", "take the step the card names: review in Reflexes, or the history sources", false),
    k("v", "details", "show or hide the details: thresholds, the index, ids, and the notes as written", true),
    k("j/k ↑↓", "scroll", "scroll the card", true),
    k("esc", "close", "close the card", true),
];

pub const PALETTE: &[Key] = &[
    k("↑↓", "move", "move", true),
    k("enter", "run", "run", true),
    k("tab", "complete", "complete the query", true),
    k("esc", "close", "close", true),
];

/// The bar entries as spans, each key in a keycap (`kit::hints`: ` p  pause   f  scope`), as
/// many as fit in `width` columns.
pub fn bar(tables: &[&[Key]], t: &Theme, width: usize) -> Vec<Span<'static>> {
    let pairs: Vec<(&str, &str)> =
        tables.iter().flat_map(|k| k.iter()).filter(|k| k.bar).map(|k| (k.keys, k.label)).collect();
    super::kit::hints(&pairs, t, width)
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
