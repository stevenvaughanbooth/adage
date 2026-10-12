//! The Live tab's main area: each agent session as a graph that grows while it works, newest at
//! the bottom. It is the Claude Code mod's Graph tab (`plugin/claude-code/hooks/view/graph.ts`) in
//! the terminal, drawn from the spool records the feed already reads.
//!
//! * A session opens with a header: its harness, and since when it is shown.
//! * Each turn (from a prompt, or the session's start, to the `Stop` that ends it) is a box, the
//!   root of a trunk. A turn that ended with nothing from Adage folds into one line.
//! * The agent's tool batches hang off the trunk, one line each (`Run ×3 · Edit`, a lone run with
//!   its command); three or more quiet ones in a row fold into `┊ 12 steps, nothing to say`.
//! * Each reflex Adage delivered branches off the step it came after: a `◆ adage · ● FIRE` box,
//!   an arrow, and its response, lit `✓ followed` once the agent took it. Whatever else Adage did
//!   there (a finish line, a rewrite, an answer from a receipt) hangs off the step as one line
//!   that says `adage`.
//!
//! From [`BOXES_FROM`] columns, with rows to spare, the boxes; narrower, one line per node. Pure:
//! the lines are a function of the items, the reflexes' gists and the motion state the caller
//! passes, so tests see final frames.

use super::feed::{self, Act, FeedItem};
use super::kit;
use super::theme::Theme;
use super::view::{self, Ctx, Fit, Pulse};
use adage_term::Glyph;
use adage_tracks::Strength;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use std::collections::{BTreeMap, HashMap, HashSet};

/// Quiet steps in a row this many or more fold into one line.
pub const QUIET_RUN: usize = 3;
/// The narrowest flow that draws boxes; narrower, one line per node.
pub const BOXES_FROM: u16 = 88;
/// The fewest rows the box tier is drawn in.
pub const BOX_ROWS: u16 = 14;
/// A delivery's box at its widest, the arrow between it and its response, and where the box
/// starts (two columns in, the trunk, the rail).
const MAX_BOX: usize = 56;
const ARROW_W: usize = 6;
const RAIL_LEAD: usize = 7;

// ---------------------------------------------------------------------------------------------
// What a step says

/// A call key without its marks (` !failed`, ` !refused`).
fn bare(key: &str) -> &str {
    key.trim_end_matches(" !failed").trim_end_matches(" !refused")
}

/// A call key (`exec cargo build -p adage-cli`, `read {path}`) as a step names it: the tool's
/// plain word, and for a run the command it ran, its placeholders as `…`.
///
/// ```ignore
/// assert_eq!(flow::call_word("exec rm -f {path}", t), ("Run".into(), Some("rm -f …".into())));
/// ```
pub fn call_word(key: &str, t: &Theme) -> (String, Option<String>) {
    let key = bare(key);
    let (verb, rest) = key.split_once(' ').unwrap_or((key, ""));
    let word = match verb {
        "exec" => "Run".to_string(),
        "read" => "Read".into(),
        "edit" => "Edit".into(),
        "write" => "Write".into(),
        "list" => "List".into(),
        "fetch" => "Fetch".into(),
        "search:filename" => "Find".into(),
        v if v.starts_with("search") => "Search".into(),
        v if v.starts_with("compound") => "Run".into(),
        "control" => match rest.split_whitespace().collect::<Vec<_>>().as_slice() {
            ["Task", ..] => "Agent".into(),
            ["mcp", name, ..] => name.rsplit("__").next().unwrap_or(name).to_string(),
            [other, ..] => (*other).to_string(),
            [] => "Tool".into(),
        },
        _ => "Tool".into(),
    };
    let command = (verb == "exec" && !rest.trim().is_empty()).then(|| {
        rest.split_whitespace()
            .map(|w| if w.starts_with('{') && w.ends_with('}') { t.more() } else { w })
            .collect::<Vec<_>>()
            .join(" ")
    });
    (word, command)
}

/// Each tool once with its count, the most used first (ties in the order first seen):
/// `[("Run", 3), ("Read", 1)]`.
pub fn tool_counts<'k>(keys: impl IntoIterator<Item = &'k String>, t: &Theme) -> Vec<(String, usize)> {
    let mut out: Vec<(String, usize)> = Vec::new();
    for k in keys {
        let (word, _) = call_word(k, t);
        match out.iter_mut().find(|(w, _)| *w == word) {
            Some((_, n)) => *n += 1,
            None => out.push((word, 1)),
        }
    }
    out.sort_by_key(|a| std::cmp::Reverse(a.1));
    out
}

/// Tool counts as spans, at most `max` of them (the rest as `+2`): `Run ×3 · Read · Edit`.
pub fn tools_spans(counts: &[(String, usize)], max: usize, style: Style, t: &Theme) -> Vec<Span<'static>> {
    let mut v = Vec::new();
    for (k, (word, n)) in counts.iter().take(max).enumerate() {
        if k > 0 {
            v.push(Span::styled(t.sep(), t.dim()));
        }
        v.push(Span::styled(word.clone(), style));
        if *n > 1 {
            v.push(Span::styled(format!(" {}{n}", t.ch("×", "x")), t.dim()));
        }
    }
    if counts.len() > max {
        v.push(Span::styled(format!("{}+{}", t.sep(), counts.len() - max), t.dim()));
    }
    v
}

/// The calls of an item that failed.
pub fn failed(i: &FeedItem) -> usize {
    i.calls.iter().filter(|k| k.ends_with(" !failed")).count()
}

/// Whether an item is a step on the trunk (a tool batch, a failure, a stop, the session's end); a
/// prompt heads a turn, the session's start is said on its header, and anything else only hangs
/// what Adage did there.
fn is_step(i: &FeedItem) -> bool {
    matches!(i.event.as_str(), "tools" | "failure" | "end" | "stop")
}

/// Whether a stop asked the agent to go on (a finish line), which does not end the turn.
fn asks_to_continue(i: &FeedItem) -> bool {
    i.acts.iter().any(|a| matches!(a, Act::StopRequest))
}

/// Whether an item ends its turn: a stop that let the agent finish, or the session's end.
fn closes(i: &FeedItem) -> bool {
    (i.event == "stop" && !asks_to_continue(i)) || i.event == "end"
}

/// Whether an item is drawn at all: a step, a prompt, or anything Adage did something at.
fn drawn(i: &FeedItem) -> bool {
    is_step(i) || i.event == "prompt" || !i.acts.is_empty()
}

/// The items grouped by session (each in its own order), the sessions in the order each was last
/// active.
pub fn by_session<'a>(items: &[&'a FeedItem]) -> Vec<&'a FeedItem> {
    let mut last: HashMap<&str, usize> = HashMap::new();
    for (n, i) in items.iter().enumerate() {
        last.insert(i.session.as_str(), n);
    }
    let mut out = items.to_vec();
    out.sort_by_key(|i| last[i.session.as_str()]);
    out
}

// ---------------------------------------------------------------------------------------------
// What became of each delivery

/// What the agent did with a delivered reflex.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// Not judged yet.
    Waiting,
    Followed,
    /// The finish line it set was met.
    Met,
    Ignored,
    /// The agent ran it, and it failed.
    Misfired,
}

/// Each delivery's outcome, and which judgements a delivery's box already shows.
#[derive(Debug, Default)]
pub struct Judged {
    /// `(item index, act index)` of a delivery → its outcome.
    pub outcome: HashMap<(usize, usize), Outcome>,
    /// Judgements drawn on their delivery's box, not as lines of their own.
    pub shown: HashSet<(usize, usize)>,
    /// Items at which the agent followed a delivery the graph shows: their step says so.
    pub followed: HashSet<usize>,
}

/// Match each delivery to the first later judgement of its reflex in its session, before the
/// same reflex is delivered again there.
pub fn judge(items: &[&FeedItem]) -> Judged {
    let mut j = Judged::default();
    let mut claimed: HashSet<(usize, usize)> = HashSet::new();
    for (n, i) in items.iter().enumerate() {
        for (k, a) in i.acts.iter().enumerate() {
            let Act::Directed { route, .. } = a else { continue };
            let mut outcome = Outcome::Waiting;
            'later: for (m, later) in items.iter().enumerate().skip(n + 1) {
                if later.session != i.session {
                    continue;
                }
                for (k2, b) in later.acts.iter().enumerate() {
                    let (r, o) = match b {
                        Act::Directed { route: r, .. } if r == route => break 'later,
                        Act::Followed(r) => (r, Outcome::Followed),
                        Act::Met(r) => (r, Outcome::Met),
                        Act::Ignored(r) => (r, Outcome::Ignored),
                        Act::Misfired(r) => (r, Outcome::Misfired),
                        _ => continue,
                    };
                    if r != route || !claimed.insert((m, k2)) {
                        continue;
                    }
                    outcome = o;
                    // A finish line met and a failed run keep their own lines: each says more.
                    match o {
                        Outcome::Followed => {
                            j.shown.insert((m, k2));
                            j.followed.insert(m);
                        }
                        Outcome::Ignored => {
                            j.shown.insert((m, k2));
                        }
                        _ => {}
                    }
                    break 'later;
                }
            }
            j.outcome.insert((n, k), outcome);
        }
    }
    j
}

// ---------------------------------------------------------------------------------------------
// Lines

/// One line of the graph: its edges, what the node says (cut to fit), and what sits flush right.
#[derive(Debug, Clone, Default)]
pub struct FlowLine {
    /// The item the line is about (`enter` inspects it).
    pub seq: u64,
    /// Lines of one block (a step and what hangs off it, a box) scroll off together.
    pub block: usize,
    pub lead: Vec<Span<'static>>,
    pub body: Vec<Span<'static>>,
    pub right: Vec<Span<'static>>,
    /// What the line stands for, counted where it scrolls off: sessions, turns begun, steps.
    pub sessions: usize,
    pub turns: usize,
    pub steps: usize,
    /// The newest node's lines (they glow, and slide in).
    pub newest: bool,
}

impl FlowLine {
    /// The line `width` columns wide, the body `shift` columns right of its place (the newest
    /// node sliding in). The right-hand part goes first when the body would be cut short.
    pub fn render(&self, width: usize, shift: usize, t: &Theme) -> Line<'static> {
        let lead_w = kit::spans_width(&self.lead);
        let right_w = kit::spans_width(&self.right);
        let body_w = kit::spans_width(&self.body);
        let free = width.saturating_sub(lead_w + shift);
        let keep_right = right_w > 0 && free >= right_w + 2 + body_w.min(16);
        let room = if keep_right { free - right_w - 2 } else { free };
        let mut spans = self.lead.clone();
        if shift > 0 {
            spans.push(Span::raw(" ".repeat(shift)));
        }
        let body = kit::clip_spans(self.body.clone(), room, t);
        let used = kit::spans_width(&body);
        spans.extend(body);
        if keep_right {
            spans.push(Span::raw(" ".repeat(free - used - right_w)));
            spans.extend(self.right.clone());
        }
        Line::from(kit::clip_spans(spans, width, t))
    }
}

/// How the graph is drawn.
#[derive(Debug, Clone, Copy)]
pub struct Opts<'a> {
    /// The columns it has.
    pub width: u16,
    /// Boxes (the wide tier), else one line per node.
    pub boxes: bool,
    /// Every repository is shown: a session's header names its repository.
    pub repo: bool,
    pub gists: &'a BTreeMap<String, String>,
    /// The newest firing's flash.
    pub pulse: Pulse,
    /// The newest node glows (its words in the accent, its time `now`).
    pub glow: bool,
}

/// The graph's own glyphs and their ASCII twins (one column each).
struct G<'t>(&'t Theme);

impl G<'_> {
    fn h(&self) -> &'static str {
        self.0.ch("─", "-")
    }
    fn v(&self) -> &'static str {
        self.0.ch("│", "|")
    }
    fn tee(&self) -> &'static str {
        self.0.ch("├", "+")
    }
    fn end(&self) -> &'static str {
        self.0.ch("╰", "`")
    }
    fn down(&self) -> &'static str {
        self.0.ch("┬", "+")
    }
    fn head(&self) -> &'static str {
        self.0.ch("▶", ">")
    }
    fn fold(&self) -> &'static str {
        self.0.ch("┊", ":")
    }
}

/// A turn's span of the items: the prompt that opened it (if one is in view), whether it begins
/// here (a prompt, the session's start, or the step after a turn ended), its other items, and
/// whether it ended.
#[derive(Debug, Clone, Default)]
struct Chunk {
    head: Option<usize>,
    starts: bool,
    /// The session's first turn in view that did not begin here.
    earliest: bool,
    body: Vec<usize>,
    closed: bool,
}

/// What hangs off a node: what Adage did there (one line each), then its deliveries.
#[derive(Debug, Default)]
struct Hang {
    did: Vec<Vec<Span<'static>>>,
    fires: Vec<(usize, usize)>,
}

impl Hang {
    fn len(&self) -> usize {
        self.did.len() + self.fires.len()
    }
}

struct Builder<'a, 'c> {
    t: &'c Theme,
    ctx: &'c Ctx,
    o: &'c Opts<'c>,
    items: &'c [&'a FeedItem],
    judged: Judged,
    /// What each reflex told each session, so later lines name it the same way.
    said: BTreeMap<(&'a str, &'a str), String>,
    newest: Option<usize>,
    /// The graph's columns, a delivery box's width (0: the list tier), and its right edge.
    w: usize,
    bw: usize,
    edge: usize,
    times: bool,
    out: Vec<FlowLine>,
    block: usize,
}

/// The graph's lines for `items` (oldest first), and its right edge (lines are no wider). Each
/// session is one section, the most recently active last, so the newest node is at the bottom
/// and sessions that ran side by side do not cut each other's turns apart.
pub fn lines(items: &[&FeedItem], ctx: &Ctx, o: &Opts) -> (Vec<FlowLine>, usize) {
    let t = &ctx.theme;
    let w = o.width as usize;
    let bw = if o.boxes && o.width >= BOXES_FROM {
        MAX_BOX.min(w.saturating_sub(RAIL_LEAD + ARROW_W + 1) / 2).max(14)
    } else {
        0
    };
    let edge = if bw > 0 { (RAIL_LEAD + 2 * bw + ARROW_W).min(w) } else { w };
    let items = &by_session(items);
    let mut said = BTreeMap::new();
    for i in items {
        for a in &i.acts {
            if let Act::Directed { route, detail, .. } = a {
                said.insert((i.session.as_str(), route.as_str()), detail.clone());
            }
        }
        for (route, text) in &i.texts {
            said.entry((i.session.as_str(), route.as_str())).or_insert_with(|| feed::detail_of(text));
        }
    }
    let mut b = Builder {
        t,
        ctx,
        o,
        items,
        judged: judge(items),
        said,
        newest: items.iter().rposition(|i| drawn(i)),
        w,
        bw,
        edge,
        times: w >= 56,
        out: Vec::new(),
        block: 0,
    };
    b.build();
    (b.out, edge)
}

impl<'a> Builder<'a, '_> {
    fn sp(n: usize) -> Span<'static> {
        Span::raw(" ".repeat(n))
    }

    fn dim(&self, s: impl Into<String>) -> Span<'static> {
        Span::styled(s.into(), self.t.dim())
    }

    fn push(&mut self, seq: u64, lead: Vec<Span<'static>>, body: Vec<Span<'static>>, right: Vec<Span<'static>>) {
        self.out.push(FlowLine { seq, block: self.block, lead, body, right, ..Default::default() });
    }

    fn next_block(&mut self) {
        self.block += 1;
    }

    /// Mark the lines of the current block as the newest node's.
    fn mark_newest(&mut self, n: usize) {
        if self.newest == Some(n) {
            let block = self.block;
            for l in self.out.iter_mut().rev().take_while(|l| l.block == block) {
                l.newest = true;
            }
        }
    }

    /// The reflex by what it told this session, else its gist, else its id.
    fn name(&self, session: &str, route: &str) -> String {
        self.said
            .get(&(session, route))
            .filter(|d| !d.is_empty())
            .or_else(|| self.o.gists.get(route))
            .map_or_else(|| route.to_string(), |d| d.to_string())
    }

    /// A node's time, flush right: `now` in the accent while the newest glows.
    fn time(&self, n: usize) -> Vec<Span<'static>> {
        if !self.times {
            return vec![];
        }
        if self.o.glow && self.newest == Some(n) {
            return vec![Span::styled("now", self.t.accent().patch(self.t.bold()))];
        }
        let secs = self.w >= 80;
        vec![self.dim(view::clock(self.items[n].ts, self.ctx.tz, secs))]
    }

    fn build(&mut self) {
        let n = self.items.len();
        let mut state: HashMap<&str, (bool, bool)> = HashMap::new(); // session → (seen, after a turn ended)
        let mut k = 0;
        let mut prev: Option<&str> = None;
        while k < n {
            let s = self.items[k].session.as_str();
            let end = k + self.items[k..].iter().take_while(|i| i.session == s).count();
            let (seen, after_stop) = state.get(s).copied().unwrap_or((false, false));
            if prev != Some(s) && self.items[k..end].iter().any(|i| drawn(i) || i.event == "start") {
                self.session_head(k, end, seen);
            }
            let mut chunks: Vec<Chunk> = Vec::new();
            let mut cur: Option<Chunk> = None;
            let mut after = after_stop;
            let mut first = !seen;
            for idx in k..end {
                let i = self.items[idx];
                if i.event == "start" {
                    // The session's start begins its first turn; it is drawn only where Adage
                    // did something at it.
                    chunks.extend(cur.take());
                    if !i.acts.is_empty() {
                        chunks.push(Chunk { starts: true, body: vec![idx], ..Default::default() });
                    }
                    (after, first) = (true, false);
                    continue;
                }
                if !drawn(i) {
                    continue;
                }
                if i.event == "prompt" {
                    chunks.extend(cur.take());
                    cur = Some(Chunk { head: Some(idx), starts: true, ..Default::default() });
                } else {
                    let c = cur.get_or_insert_with(|| Chunk { starts: after, earliest: first, ..Default::default() });
                    c.body.push(idx);
                    if closes(i) {
                        c.closed = true;
                        chunks.extend(cur.take());
                    }
                }
                first = false;
                // A turn ended here (or none is open): the next step begins one.
                after = cur.is_none();
            }
            chunks.extend(cur.take());
            state.insert(s, (true, after));
            let last = chunks.len().saturating_sub(1);
            for (n, c) in chunks.iter().enumerate() {
                self.chunk(c, n == last);
            }
            prev = Some(s);
            k = end;
        }
    }

    /// A session's header: its harness, since when it is shown, and what is special about it.
    fn session_head(&mut self, k: usize, end: usize, seen: bool) {
        let t = self.t;
        let i = self.items[k];
        let harness = adage_trace::Harness::parse(&i.harness).map_or(i.harness.as_str(), |h| h.display()).to_string();
        let mut facts = Vec::new();
        if self.o.repo && !i.repo.is_empty() {
            facts.push(super::scope::short(&i.repo));
        }
        let at = view::clock(i.ts, self.ctx.tz, false);
        facts.push(match (seen, self.items[k..end].iter().any(|j| j.event == "start")) {
            (true, _) => "session, continued".to_string(),
            (false, true) => format!("session started {at}"),
            (false, false) => format!("session, since {at}"),
        });
        if i.unrouted {
            facts.push("watching, no reflexes here yet".into());
        }
        let mut body = vec![Span::styled(harness, t.bold())];
        body.push(Span::styled(format!("  {}", facts.join(&t.sep())), t.ink2()));
        if i.control {
            body.push(Span::styled(format!("{}control: Adage holds back", t.sep()), t.control()));
        }
        let used = 1 + kit::spans_width(&body) + 1;
        let rule = t.ch("─", "-").repeat(self.edge.saturating_sub(used).max(2));
        body.push(Span::styled(format!(" {rule}"), t.rule()));
        if !self.out.is_empty() {
            self.push(i.seq, vec![], vec![], vec![]);
        }
        self.out.push(FlowLine {
            seq: i.seq,
            block: self.block,
            lead: vec![Span::raw(" ")],
            body,
            sessions: 1,
            ..Default::default()
        });
        self.next_block();
    }

    /// Whether a chunk folds into one line: its turn ended, with nothing from Adage in it.
    fn quiet_turn(&self, c: &Chunk) -> bool {
        let newest = self.newest.is_some_and(|n| c.head == Some(n) || c.body.contains(&n));
        c.closed
            && !newest
            && c.head.is_none_or(|h| self.items[h].acts.is_empty())
            && c.body.iter().all(|&b| self.items[b].acts.is_empty())
    }

    /// A turn: folded into one line, or its head and its trunk. `latest` is the session's last
    /// turn in view: drawn in the accent while it is still going.
    fn chunk(&mut self, c: &Chunk, latest: bool) {
        let steps: Vec<usize> =
            c.body.iter().copied().filter(|&b| matches!(self.items[b].event.as_str(), "tools" | "failure")).collect();
        if c.head.is_none() && c.body.iter().all(|&b| self.items[b].event == "start") {
            // What Adage did at the session's start, before any turn.
            for &b in &c.body {
                self.node(b, false, true);
            }
            return;
        }
        if self.quiet_turn(c) {
            return self.folded_turn(c, &steps);
        }
        self.turn_head(c, &steps, latest && !c.closed);
        // The pieces: steps (quiet runs folded), and what Adage did between them.
        let mut pieces: Vec<Result<usize, Vec<usize>>> = Vec::new();
        let mut run: Vec<usize> = Vec::new();
        let flush = |run: &mut Vec<usize>, pieces: &mut Vec<Result<usize, Vec<usize>>>| {
            if run.len() >= QUIET_RUN {
                pieces.push(Err(std::mem::take(run)));
            } else {
                pieces.extend(run.drain(..).map(Ok));
            }
        };
        for &b in &c.body {
            let i = self.items[b];
            if !drawn(i) {
                continue;
            }
            let quiet = i.event == "tools" && i.acts.is_empty() && failed(i) == 0 && self.newest != Some(b);
            if quiet {
                run.push(b);
            } else {
                flush(&mut run, &mut pieces);
                pieces.push(Ok(b));
            }
        }
        flush(&mut run, &mut pieces);
        let count = pieces.len();
        for (p, piece) in pieces.into_iter().enumerate() {
            let last = p + 1 == count;
            match piece {
                Err(run) => self.fold(&run),
                Ok(b) => {
                    // The trunk runs on beside what hangs here while the turn has more, or is going.
                    let more = !last || !c.closed;
                    self.node(b, last && c.closed, more);
                }
            }
        }
    }

    /// A turn that ended with nothing from Adage, in one line.
    fn folded_turn(&mut self, c: &Chunk, steps: &[usize]) {
        let t = self.t;
        let first = c.head.or(c.body.first().copied()).unwrap_or(0);
        let last = c.body.last().copied().unwrap_or(first);
        let (a, z) = (self.items[first].ts, self.items[last].ts);
        let lead = vec![Span::raw(" "), self.dim(format!("{} ", t.glyph(Glyph::Done)))];
        let (from, to) = (view::clock(a, self.ctx.tz, false), view::clock(z, self.ctx.tz, false));
        let span = if from == to { from } else { format!("{from}{}{to}", t.ch("–", "-")) };
        let mut body = vec![
            Span::styled(if c.starts { "turn" } else { "turn, from earlier" }, t.ink2()),
            self.dim(format!(" {span}")),
            self.dim(match steps.len() {
                0 => format!("{}no tool steps, nothing to say", t.sep()),
                n => format!("{}{}, nothing to say", t.sep(), crate::helpers::plural(n, "step", "steps")),
            }),
        ];
        let failures: usize = c.body.iter().map(|&b| failed(self.items[b])).sum();
        if failures > 0 {
            body.push(Span::styled(format!("{}{} {failures} failed", t.sep(), t.glyph(Glyph::Fail)), t.dim()));
        }
        let counts = tool_counts(steps.iter().flat_map(|&s| self.items[s].calls.iter()), t);
        if !counts.is_empty() {
            body.push(Span::raw("   "));
            body.extend(tools_spans(&counts, 3, t.dim(), t));
        }
        let seq = self.items[last].seq;
        self.out.push(FlowLine {
            seq,
            block: self.block,
            lead,
            body,
            turns: 1,
            steps: steps.len(),
            ..Default::default()
        });
        self.next_block();
    }

    /// What hangs off item `n`: what Adage did (each act but a delivery, and the judgements a
    /// delivery's box shows), then its deliveries.
    fn hang(&self, n: usize, skip_stop: bool) -> Hang {
        let t = self.t;
        let i = self.items[n];
        let mut h = Hang::default();
        for (k, a) in i.acts.iter().enumerate() {
            if matches!(a, Act::Directed { .. }) {
                h.fires.push((n, k));
                continue;
            }
            if self.judged.shown.contains(&(n, k)) || (skip_stop && matches!(a, Act::StopRequest)) {
                continue;
            }
            h.did.push(self.did(i, a, t));
        }
        h
    }

    /// One thing Adage did (or judged) as a line's words: `◆ adage ✓ finish line met: …`.
    fn did(&self, i: &FeedItem, a: &Act, t: &Theme) -> Vec<Span<'static>> {
        let tag = view::adage_did(a) || matches!(a, Act::Met(_)) || matches!(a, Act::Other(k, _) if k == "chose");
        let mut v = Vec::new();
        if tag {
            v.push(Span::styled(format!("{} adage ", t.glyph(Glyph::Active)), t.accent().patch(t.bold())));
        }
        match a {
            Act::Other(k, _) if k == "chose" => {
                v.push(Span::raw(format!("chose this step{}no model request", t.sep())));
            }
            Act::Other(k, _) if k == "would choose" => {
                v.push(Span::styled(
                    format!("{} would have chosen this step (withheld{}control session)", t.glyph(Glyph::Off), t.sep()),
                    t.control(),
                ));
            }
            _ => {
                let name = |r: &str| self.name(&i.session, r);
                let fit = if self.w < 60 { Fit::Narrow } else { Fit::Wide };
                v.extend(view::act_spans(a, t, fit, usize::MAX / 4, 0, &name));
            }
        }
        v
    }

    /// A turn's head: its box (or line), and what hangs off its prompt.
    fn turn_head(&mut self, c: &Chunk, steps: &[usize], going: bool) {
        let t = self.t;
        let g = G(t);
        let Some(anchor) = c.head.or(c.body.first().copied()) else { return };
        if !c.starts && c.head.is_none() {
            // A turn that began before what is shown (or before another session's events).
            let words = if c.earliest { "earlier in this turn" } else { "this turn, continued" };
            let lead = vec![Self::sp(if self.bw > 0 { 2 } else { 1 }), self.dim(g.down())];
            self.push(self.items[anchor].seq, lead, vec![self.dim(format!(" {words}"))], vec![]);
            self.next_block();
            return;
        }
        let fired: usize = c
            .head
            .iter()
            .chain(&c.body)
            .map(|&b| self.items[b].acts.iter().filter(|a| matches!(a, Act::Directed { .. })).count())
            .sum();
        let followed = c.head.iter().chain(&c.body).filter(|b| self.judged.followed.contains(b)).count();
        let counts = tool_counts(steps.iter().flat_map(|&s| self.items[s].calls.iter()), t);
        let calls: usize = steps.iter().map(|&s| self.items[s].observed).sum();
        let what = match (steps.len(), calls) {
            (0, _) => "no tool steps yet".to_string(),
            (n, c) if c > n => format!(
                "{}, {}",
                crate::helpers::plural(n, "step", "steps"),
                crate::helpers::plural(c, "call", "calls")
            ),
            (n, _) => crate::helpers::plural(n, "step", "steps"),
        };
        let mut facts: Vec<Span<'static>> =
            vec![Span::styled(what, if going { Style::default().add_modifier(Modifier::BOLD) } else { t.ink2() })];
        if !counts.is_empty() {
            facts.push(Span::styled(t.sep(), t.dim()));
            facts.extend(tools_spans(&counts, 4, t.ink2(), t));
        }
        if fired > 0 {
            facts.push(Span::styled(t.sep(), t.dim()));
            facts.push(Span::styled(format!("adage fired {fired}"), t.green()));
        }
        if followed > 0 {
            facts.push(Span::styled(t.sep(), t.dim()));
            facts.push(Span::styled(format!("{followed} followed"), t.green().patch(t.bold())));
        }
        let hot = c.head.is_some_and(|h| self.newest == Some(h));
        let prompt = c.head.is_some();
        let mark = if prompt { t.ch("❯", ">") } else { t.ch("▸", ">") };
        let title = if prompt { "PROMPT" } else { "TURN" };
        let seq = self.items[anchor].seq;
        let hang = match c.head {
            Some(h) => self.hang(h, false),
            None => Hang::default(),
        };
        let time = self.time(anchor);
        if self.bw > 0 {
            let w = self.edge;
            // The turn still going is drawn in the accent: it is where things happen.
            let border = if hot && self.o.glow || going { t.accent() } else { t.rule() };
            let h = g.h();
            let left = vec![
                Span::styled(format!("{}{h} ", t.ch("╭", "+")), border),
                Span::styled(format!("{mark} "), t.accent().patch(t.bold())),
                Span::styled(title, t.ink2().patch(t.bold())),
                Span::styled(" ", border),
            ];
            let mut right = Vec::new();
            if !time.is_empty() {
                right.push(Span::styled(" ", border));
                right.extend(time.clone());
                right.push(Span::styled(format!(" {h}{}", t.ch("╮", "+")), border));
            } else {
                right.push(Span::styled(format!("{h}{}", t.ch("╮", "+")), border));
            }
            let fill = w.saturating_sub(kit::spans_width(&left) + kit::spans_width(&right));
            let mut top = left;
            top.push(Span::styled(h.repeat(fill), border));
            top.extend(right);
            let inner = w.saturating_sub(4);
            let body = kit::clip_spans(facts, inner, t);
            let pad = inner.saturating_sub(kit::spans_width(&body));
            let mut mid = vec![Span::styled(format!("{} ", g.v()), border)];
            mid.extend(body);
            mid.push(Span::raw(" ".repeat(pad)));
            mid.push(Span::styled(format!(" {}", g.v()), border));
            let bottom: String = (0..w)
                .map(|x| match x {
                    0 => t.ch("╰", "+"),
                    _ if x == w - 1 => t.ch("╯", "+"),
                    2 => g.down(),
                    4 if hang.len() > 0 => g.down(),
                    _ => h,
                })
                .collect();
            self.out.push(FlowLine { seq, block: self.block, lead: top, turns: 1, ..Default::default() });
            self.push(seq, mid, vec![], vec![]);
            self.push(seq, vec![Span::styled(bottom, border)], vec![], vec![]);
        } else {
            let mut body = vec![Span::styled(title, t.ink2().patch(t.bold())), Span::styled(t.sep(), t.dim())];
            body.extend(facts);
            let lead = vec![Span::raw(" "), Span::styled(format!("{mark} "), t.accent().patch(t.bold()))];
            self.out.push(FlowLine { seq, block: self.block, lead, body, right: time, turns: 1, ..Default::default() });
        }
        let trunk = g.v();
        if let Some(h) = c.head {
            self.rail(h, hang, trunk);
            self.mark_newest(h);
        }
        self.next_block();
    }

    /// A step on the trunk, and what hangs off it. `last` closes the trunk; `more` keeps it going
    /// beside what hangs here.
    fn node(&mut self, n: usize, last: bool, more: bool) {
        let t = self.t;
        let g = G(t);
        let i = self.items[n];
        let seq = i.seq;
        let hot = self.newest == Some(n);
        let stop_ask = i.event == "stop" && asks_to_continue(i);
        let hang = self.hang(n, stop_ask);
        let label_style = |s: Style| {
            if hot && self.o.glow {
                s.patch(t.accent()).add_modifier(Modifier::BOLD)
            } else if hot {
                s.add_modifier(Modifier::BOLD)
            } else {
                s
            }
        };
        // The label: the step's tools, the session's start or end, a stop, or (for an event that
        // is no step) the first thing Adage did there.
        let mut hang = hang;
        let mut label: Vec<Span<'static>> = match i.event.as_str() {
            "tools" | "failure" => self.tools_label(i, &label_style),
            "start" => vec![Span::styled(format!("{} the session started", t.ch("▸", ">")), label_style(t.dim()))],
            "end" => vec![Span::styled(format!("{} session ended", t.ch("∎", ".")), label_style(t.dim()))],
            "stop" if stop_ask => vec![
                Span::styled(format!("{} adage ", t.glyph(Glyph::Active)), t.accent().patch(t.bold())),
                Span::styled(format!("{} ", t.ch("■", "#")), t.yellow().patch(t.bold())),
                Span::styled("finish line: asked the agent to go on before it stops", label_style(Style::default())),
            ],
            "stop" => vec![Span::styled(format!("{} done", t.glyph(Glyph::Done)), label_style(t.dim()))],
            _ if !hang.did.is_empty() => hang.did.remove(0),
            _ => vec![],
        };
        if label.is_empty() && !hang.fires.is_empty() {
            label = vec![self.dim(format!("{} {}", t.glyph(Glyph::Sep), i.event))];
        }
        let mut tags: Vec<Span<'static>> = Vec::new();
        let fails = failed(i);
        if fails > 0 || (i.event == "failure" && i.calls.is_empty()) {
            let n = fails.max(1);
            tags.push(Span::styled(format!("{} {n} failed", t.glyph(Glyph::Fail)), t.red()));
        }
        if self.judged.followed.contains(&n) {
            if !tags.is_empty() {
                tags.push(Span::raw("  "));
            }
            tags.push(Span::styled(format!("{} followed adage", t.glyph(Glyph::Ok)), t.green().patch(t.bold())));
        }
        if i.agent && i.event == "tools" && self.w >= 90 {
            if !tags.is_empty() {
                tags.push(Span::raw("  "));
            }
            tags.push(self.dim("subagent"));
        }
        let mut right = tags;
        let time = self.time(n);
        if !time.is_empty() {
            if !right.is_empty() {
                right.push(Span::raw("  "));
            }
            right.extend(time);
        }
        let branch = hang.len() > 0;
        let steps = usize::from(matches!(i.event.as_str(), "tools" | "failure"));
        let lead = if self.bw > 0 {
            let corner = if last { g.end() } else { g.tee() };
            vec![Self::sp(2), self.dim(format!("{corner}{}{} ", g.h(), if branch { g.down() } else { g.h() }))]
        } else {
            vec![Span::raw(" "), self.dim(format!("{} ", if last { g.end() } else { g.tee() }))]
        };
        self.out.push(FlowLine { seq, block: self.block, lead, body: label, right, steps, ..Default::default() });
        let trunk = if more { g.v() } else { " " };
        self.rail(n, hang, trunk);
        self.mark_newest(n);
        self.next_block();
    }

    /// A step's tools: `Run ×3 · Read`, a lone run with its command, or its count of calls.
    fn tools_label(&self, i: &FeedItem, style: &dyn Fn(Style) -> Style) -> Vec<Span<'static>> {
        let t = self.t;
        if i.calls.is_empty() {
            let what = match i.observed {
                0 => "a step".to_string(),
                1 => "1 call".to_string(),
                n => format!("{n} calls"),
            };
            return vec![Span::styled(what, style(t.ink2()))];
        }
        let counts = tool_counts(&i.calls, t);
        let mut v = tools_spans(&counts, 4, style(Style::default()), t);
        // One command (run once, or the same one again and again): what it ran.
        let commands: Vec<Option<String>> = i.calls.iter().map(|k| call_word(k, t).1).collect();
        if let Some(Some(cmd)) = commands.first().filter(|_| commands.windows(2).all(|w| w[0] == w[1])) {
            v.push(Span::raw("  "));
            v.push(Span::styled(cmd.clone(), style(t.command())));
        }
        v
    }

    /// What hangs off item `n` on its rail, under the node's line.
    fn rail(&mut self, n: usize, hang: Hang, trunk: &'static str) {
        let total = hang.len();
        let seq = self.items[n].seq;
        for (k, did) in hang.did.into_iter().enumerate() {
            let last = k + 1 == total;
            let lead = self.rail_lead(trunk, last);
            self.push(seq, lead, did, vec![]);
        }
        let done = total - hang.fires.len();
        for (k, (item, act)) in hang.fires.into_iter().enumerate() {
            let last = done + k + 1 == total;
            if self.bw > 0 {
                self.fire_boxes(item, act, last, trunk);
            } else {
                self.fire_lines(item, act, trunk);
            }
        }
    }

    fn rail_lead(&self, trunk: &'static str, last: bool) -> Vec<Span<'static>> {
        let g = G(self.t);
        if self.bw > 0 {
            vec![Self::sp(2), self.dim(format!("{trunk} {}{} ", if last { g.end() } else { g.tee() }, g.h()))]
        } else {
            vec![Span::raw(" "), self.dim(format!("{} ", g.v())), Self::sp(1)]
        }
    }

    /// A delivery's look: its strength, what it tells any agent, what it said here, and whether
    /// that is a command.
    fn look(&self, n: usize, k: usize) -> Look {
        let i = self.items[n];
        let Act::Directed { route, strength, detail, instinct } = &i.acts[k] else {
            return Look::default();
        };
        let card = detail == "card" || detail.starts_with("card · ");
        let text = i.texts.iter().find(|(r, _)| r == route).map(|(_, x)| x.as_str()).unwrap_or("");
        let command = !card && text.lines().any(|l| l.starts_with("  ") && !l.trim_start().starts_with('<'));
        let gist = self.o.gists.get(route).cloned().filter(|g| !g.is_empty());
        let say = match (&gist, card, *instinct) {
            (Some(g), _, _) => g.clone(),
            (None, true, _) => "a procedure card for this change".into(),
            (None, false, true) => "an instinct built into Adage".into(),
            (None, false, false) => "a reflex learned here".into(),
        };
        let response = if detail.is_empty() { self.name(&i.session, route) } else { detail.clone() };
        Look {
            strength: *strength,
            say,
            response,
            command,
            card,
            instinct: *instinct,
            outcome: self.judged.outcome.get(&(n, k)).copied().unwrap_or(Outcome::Waiting),
        }
    }

    /// The flash on a delivery that has just arrived: 3 (reversed), 2, 1, then 0.
    fn flash(&self, n: usize) -> u8 {
        if self.o.pulse.seq == self.items[n].seq && self.o.pulse.seq != 0 {
            self.o.pulse.level
        } else {
            0
        }
    }

    /// The outcome's words, on the response: `✓ followed`.
    fn outcome_spans(&self, l: &Look) -> Vec<Span<'static>> {
        let t = self.t;
        match l.outcome {
            Outcome::Followed => {
                let word = if l.strength == Strength::Inhibit { "headed off" } else { "followed" };
                vec![Span::styled(format!("{} {word}", t.glyph(Glyph::Ok)), t.green().patch(t.bold()))]
            }
            Outcome::Met => {
                vec![Span::styled(format!("{} finish line met", t.glyph(Glyph::Ok)), t.green().patch(t.bold()))]
            }
            Outcome::Ignored => vec![Span::styled(format!("{} not followed", t.glyph(Glyph::Fail)), t.red())],
            Outcome::Misfired => {
                vec![Span::styled(format!("{} ran, and it failed", t.glyph(Glyph::Fail)), t.red().patch(t.bold()))]
            }
            Outcome::Waiting => vec![],
        }
    }

    /// The `◆ adage · ● FIRE` mark of a delivery.
    fn adage_mark(&self, l: &Look, flash: u8, room: usize) -> Vec<Span<'static>> {
        let t = self.t;
        let st = t.strength(l.strength);
        let bold = st.patch(t.bold());
        let word = Theme::strength_word(l.strength).to_ascii_uppercase();
        let mut v = vec![Span::styled(format!("{} adage", t.glyph(Glyph::Active)), bold)];
        v.push(Span::styled(t.sep(), t.dim()));
        let lamp = if t.color { format!("{} ", t.glyph(Glyph::Lamp)) } else { String::new() };
        let mut strength = Span::styled(format!("{lamp}{word}"), bold);
        if flash >= 2 {
            strength.style = strength.style.add_modifier(Modifier::REVERSED);
        }
        v.push(strength);
        let kind = if l.card {
            Some(format!("{} card", t.glyph(Glyph::Card)))
        } else if l.instinct {
            Some(format!("{} instinct", t.glyph(Glyph::Instinct)))
        } else {
            None
        };
        if let Some(k) = kind {
            if kit::spans_width(&v) + 3 + kit::width(&k) <= room {
                v.push(Span::styled(t.sep(), t.dim()));
                v.push(Span::styled(k, t.accent()));
            }
        }
        v
    }

    /// Where a delivery came, as its box's caption says it.
    fn where_(&self, n: usize) -> &'static str {
        match self.items[n].event.as_str() {
            "prompt" => "at the prompt",
            "tools" => "after this step",
            "failure" => "after a failure",
            "stop" => "at the finish line",
            "start" => "at the session's start",
            _ => "here",
        }
    }

    /// One rounded box `w` wide: the title in the top border, `body` inside (one line or more,
    /// each cut to fit), the caption in the bottom border.
    fn boxed(
        &self,
        w: usize,
        title: Vec<Span<'static>>,
        body: &[String],
        style: Style,
        caption: Vec<Span<'static>>,
        border: Style,
    ) -> Vec<Vec<Span<'static>>> {
        let t = self.t;
        let g = G(t);
        let h = g.h();
        let edge = |s: String| Span::styled(s, border);
        let set = |l: &str, r: &str, inner: Vec<Span<'static>>| -> Vec<Span<'static>> {
            let kept = kit::clip_spans(inner, w.saturating_sub(5), t);
            if kept.is_empty() {
                return vec![edge(format!("{l}{}{r}", h.repeat(w.saturating_sub(2))))];
            }
            let kw = kit::spans_width(&kept);
            let mut v = vec![edge(format!("{l}{h} "))];
            v.extend(kept);
            v.push(edge(format!(" {}{r}", h.repeat(w.saturating_sub(5 + kw)))));
            v
        };
        let mut rows = vec![set(t.ch("╭", "+"), t.ch("╮", "+"), title)];
        for line in body {
            let text = t.fit(&t.text(line), w.saturating_sub(4));
            let tw = kit::width(&text);
            rows.push(vec![
                edge(format!("{} ", g.v())),
                Span::styled(text, style),
                Span::raw(" ".repeat(w.saturating_sub(4 + tw))),
                edge(format!(" {}", g.v())),
            ]);
        }
        rows.push(set(t.ch("╰", "+"), t.ch("╯", "+"), caption));
        rows
    }

    /// `text` in at most two lines `width` wide, the second cut with an ellipsis if it must be.
    fn two_lines(&self, text: &str, width: usize) -> Vec<String> {
        let mut lines = kit::wrap(text, width.max(4));
        if lines.len() > 2 {
            let rest = lines[1..].join(" ");
            lines.truncate(1);
            lines.push(self.t.fit(&rest, width));
        }
        lines
    }

    /// A delivery off the rail, as boxes: the `◆ adage` box in its strength (what the reflex
    /// tells any agent, in two lines where it needs them), the arrow, the response (what it said
    /// here), lit once the agent took it.
    fn fire_boxes(&mut self, n: usize, k: usize, last: bool, trunk: &'static str) {
        let t = self.t;
        let g = G(t);
        let l = self.look(n, k);
        let seq = self.items[n].seq;
        let flash = self.flash(n);
        let st = t.strength(l.strength);
        // The flash is in the title (reversed, then bold), never the border's weight: a bold box
        // line is a different glyph in some fonts.
        let border = st;
        let room = self.bw.saturating_sub(5);
        let title = self.adage_mark(&l, flash, room);
        let hot = self.newest == Some(n) && self.o.glow;
        let stamp = if hot { "now".to_string() } else { view::clock(self.items[n].ts, self.ctx.tz, true) };
        let said = format!("{}{}{stamp}", self.where_(n), t.sep());
        let cap_style = if hot { t.accent().patch(t.bold()) } else { t.dim() };
        let caption = vec![Span::styled(if kit::width(&said) <= room { said } else { stamp }, cap_style)];
        let say_style = if flash > 0 { Style::default().add_modifier(Modifier::BOLD) } else { Style::default() };
        let inner = self.bw.saturating_sub(4);
        let mut say = self.two_lines(&l.say, inner);
        let mut response = self.two_lines(&l.response, inner);
        let height = say.len().max(response.len());
        say.resize(height, String::new());
        response.resize(height, String::new());
        let a = self.boxed(self.bw, title, &say, say_style, caption, border);
        let lit = matches!(l.outcome, Outcome::Followed | Outcome::Met);
        let (rtitle, rstyle) = match (l.card, l.command) {
            (true, _) => ("CARD", t.ink2()),
            (_, true) => ("RUN", t.command()),
            _ => ("SAYS", Style::default()),
        };
        let r_border = match l.outcome {
            Outcome::Followed | Outcome::Met => t.green(),
            Outcome::Misfired => t.red(),
            _ => t.rule(),
        };
        let b = self.boxed(
            self.bw,
            vec![Span::styled(rtitle, t.dim().patch(t.bold()))],
            &response,
            rstyle,
            self.outcome_spans(&l),
            r_border,
        );
        let h = g.h();
        let arrow = Span::styled(
            format!(" {}{} ", h.repeat(3), g.head()),
            if lit { t.green().patch(t.bold()) } else { t.dim() },
        );
        let lead = vec![Self::sp(2), self.dim(trunk), Span::raw(" ")];
        for (row, (a_row, b_row)) in a.into_iter().zip(b).enumerate() {
            let mut r = lead.clone();
            match row {
                // The rail runs down to the box, and its branch points at the first line.
                0 => {
                    r.push(Span::styled(g.v(), st));
                    r.push(Self::sp(2));
                }
                1 => r.push(Span::styled(format!("{}{}{}", if last { g.end() } else { g.tee() }, h, g.head()), st)),
                _ => {
                    r.push(if last { Span::raw(" ") } else { self.dim(g.v()) });
                    r.push(Self::sp(2));
                }
            }
            r.extend(a_row);
            r.push(if row == 1 { arrow.clone() } else { Self::sp(ARROW_W) });
            r.extend(b_row);
            self.push(seq, r, vec![], vec![]);
        }
    }

    /// A delivery off the rail, in two lines: `◆ adage · ● FIRE · what it does`, then
    /// `──▶ what it said  ✓ followed`.
    fn fire_lines(&mut self, n: usize, k: usize, trunk: &'static str) {
        let t = self.t;
        let g = G(t);
        let l = self.look(n, k);
        let seq = self.items[n].seq;
        let flash = self.flash(n);
        let lead = vec![Span::raw(" "), self.dim(format!("{trunk} ")), Self::sp(1)];
        let mut first = self.adage_mark(&l, flash, 40);
        first.push(Span::styled(t.sep(), t.dim()));
        first.push(Span::styled(l.say.clone(), if flash > 0 { t.bold() } else { Style::default() }));
        self.push(seq, lead.clone(), first, vec![]);
        let lit = matches!(l.outcome, Outcome::Followed | Outcome::Met);
        let mut second = vec![
            Self::sp(2),
            Span::styled(
                format!("{}{} ", g.h().repeat(2), g.head()),
                if lit { t.green().patch(t.bold()) } else { t.dim() },
            ),
            Span::styled(l.response.clone(), if l.command { t.command() } else { Style::default() }),
        ];
        let out = self.outcome_spans(&l);
        if !out.is_empty() {
            second.push(Span::raw("  "));
            second.extend(out);
        }
        self.push(seq, lead, second, vec![]);
    }

    /// A run of quiet steps in one line: `┊   12 steps, nothing to say   Run ×8 · Read ×4`.
    fn fold(&mut self, run: &[usize]) {
        let t = self.t;
        let g = G(t);
        let last = self.items[run[run.len() - 1]];
        let unrouted = run.iter().all(|&s| self.items[s].unrouted);
        let quiet = if unrouted { "watching, no reflexes here yet" } else { "nothing to say" };
        let counts = tool_counts(run.iter().flat_map(|&s| self.items[s].calls.iter()), t);
        let calls: usize = run.iter().map(|&s| self.items[s].observed).sum();
        let lead = if self.bw > 0 {
            vec![Self::sp(2), self.dim(format!("{}   ", g.fold()))]
        } else {
            vec![Span::raw(" "), self.dim(format!("{} ", g.fold()))]
        };
        let mut body = vec![self.dim(format!("{} steps, {quiet}", run.len()))];
        if !counts.is_empty() {
            body.push(Span::raw("   "));
            body.extend(tools_spans(&counts, 3, t.dim(), t));
        } else if calls > 0 {
            body.push(self.dim(format!("{}{}", t.sep(), crate::helpers::plural(calls, "call", "calls"))));
        }
        let time = if self.times { vec![self.dim(view::clock(last.ts, self.ctx.tz, self.w >= 80))] } else { vec![] };
        self.out.push(FlowLine {
            seq: last.seq,
            block: self.block,
            lead,
            body,
            right: time,
            steps: run.len(),
            ..Default::default()
        });
        self.next_block();
    }
}

/// A delivery as drawn.
#[derive(Debug, Clone)]
struct Look {
    strength: Strength,
    say: String,
    response: String,
    command: bool,
    card: bool,
    instinct: bool,
    outcome: Outcome,
}

impl Default for Look {
    fn default() -> Look {
        Look {
            strength: Strength::Prime,
            say: String::new(),
            response: String::new(),
            command: false,
            card: false,
            instinct: false,
            outcome: Outcome::Waiting,
        }
    }
}

// ---------------------------------------------------------------------------------------------
// The window

/// The lines a pane `height` rows tall shows, `scroll` lines back from the newest, and what is
/// out of sight: whole blocks drop off the top, and a line says what they were, with the header
/// of the session the window starts in kept above it; scrolled back, a line at the bottom says
/// what is newer.
#[derive(Debug, Clone, Default)]
pub struct Window {
    /// The range of `lines` shown.
    pub from: usize,
    pub to: usize,
    /// Above it: sessions, turns and steps out of sight.
    pub earlier: Option<(usize, usize, usize)>,
    /// The header of the session the window starts in, when it is out of sight.
    pub sticky: Option<usize>,
    /// Below it (scrolled back): lines out of sight.
    pub newer: usize,
}

pub fn window(lines: &[FlowLine], height: usize, scroll: usize) -> Window {
    let total = lines.len();
    let height = height.max(1);
    // Scrolled all the way back, the first line shows (under the note that more is below).
    let scroll = scroll.min(total.saturating_sub(height.saturating_sub(1).max(1)));
    let to = total - scroll;
    let newer = total - to;
    let room = height.saturating_sub(usize::from(newer > 0));
    if to <= room {
        return Window { from: 0, to, newer, ..Default::default() };
    }
    // Never start in the middle of a block (a box, a step and what hangs off it).
    let start = |room: usize| {
        let from = to - room.min(to).max(1);
        let mut at = from;
        while at < to && at > 0 && lines[at].block == lines[at - 1].block {
            at += 1;
        }
        if at < to {
            at
        } else {
            from
        }
    };
    // Room for the note about what is above, and for the header of the session it starts in.
    let mut from = start(room.saturating_sub(1).max(1));
    let head = |from: usize| (0..from).rev().find(|&k| lines[k].sessions > 0);
    // A window that starts on a session's own header (its block) needs no other.
    let opens = |from: usize| lines[from..].iter().take_while(|l| l.block == lines[from].block).any(|l| l.sessions > 0);
    let mut sticky = head(from).filter(|_| !opens(from));
    if sticky.is_some() && room > 3 {
        from = start(room - 2);
        sticky = head(from).filter(|_| !opens(from));
    } else {
        sticky = None;
    }
    // The blank line before a session's header is not worth a row at the top.
    if from + 1 < to && lines[from].lead.is_empty() && lines[from].body.is_empty() {
        from += 1;
    }
    let gone = &lines[..from];
    let earlier = (
        gone.iter().map(|l| l.sessions).sum::<usize>() - usize::from(sticky.is_some()),
        gone.iter().map(|l| l.turns).sum(),
        gone.iter().map(|l| l.steps).sum(),
    );
    Window { from, to, earlier: Some(earlier), sticky, newer }
}

/// The note above the window: `┊ ↑ earlier: 1 session · 3 turns · 120 steps`.
pub fn earlier_line(earlier: (usize, usize, usize), boxes: bool, t: &Theme) -> Line<'static> {
    use crate::helpers::plural;
    let mut said = Vec::new();
    if earlier.0 > 0 {
        said.push(plural(earlier.0, "session", "sessions"));
    }
    if earlier.1 > 0 {
        said.push(plural(earlier.1, "turn", "turns"));
    }
    if earlier.2 > 0 {
        said.push(plural(earlier.2, "step", "steps"));
    }
    let said = if said.is_empty() { "more".to_string() } else { said.join(&t.sep()) };
    Line::from(vec![
        Span::raw(if boxes { "  " } else { " " }),
        Span::styled(format!("{} {} earlier: {said}", t.ch("┊", ":"), t.ch("↑", "^")), t.dim()),
    ])
}

/// The note below a window scrolled back: `┊ ↓ 24 newer lines · G back to the newest`.
pub fn newer_line(newer: usize, boxes: bool, t: &Theme) -> Line<'static> {
    Line::from(vec![
        Span::raw(if boxes { "  " } else { " " }),
        Span::styled(format!("{} {} {newer} newer lines{}", t.ch("┊", ":"), t.ch("↓", "v"), t.sep()), t.dim()),
        Span::styled("G", t.key()),
        Span::styled(" back to the newest", t.dim()),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use adage_store::Paths;
    use serde_json::{json, Value};
    use std::path::Path;

    const T0: u64 = 1_790_165_040_000;

    fn ctx(color: bool) -> Ctx {
        Ctx {
            paths: Paths::at(Path::new("/nonexistent")),
            root: None,
            label: "harbor-dispatch".into(),
            theme: Theme::with_color(color),
            tz: 0,
            demo: false,
        }
    }

    fn item(seq: u64, session: &str, event: &str, kind: &str, decision: Value) -> FeedItem {
        let mut i = feed::from_record(
            session,
            &json!({ "ts": T0 + seq * 1000, "event": event, "kind": kind, "arm": "treated", "repo": "r", "decision": decision }),
        );
        i.seq = seq;
        i
    }

    fn batch(seq: u64, calls: &[&str]) -> FeedItem {
        item(seq, "s1", "PostToolBatch", "batch", json!({ "observed": calls }))
    }

    fn text(lines: &[FlowLine], w: usize, t: &Theme) -> Vec<String> {
        lines.iter().map(|l| l.render(w, 0, t).to_string().trim_end().to_string()).collect()
    }

    fn draw(items: &[FeedItem], width: u16, boxes: bool) -> Vec<String> {
        let c = ctx(false);
        let refs: Vec<&FeedItem> = items.iter().collect();
        let gists = BTreeMap::new();
        let o = Opts { width, boxes, repo: false, gists: &gists, pulse: Pulse::default(), glow: false };
        let (lines, edge) = lines(&refs, &c, &o);
        text(&lines, edge, &c.theme)
    }

    const LOOKUP: &str = "A known next step here is:\n  bin/fetch-incident INC-2045. Ignore this if the user is asking for something else.";

    #[test]
    fn call_keys_read_as_tools_and_commands() {
        let t = Theme::with_color(false);
        assert_eq!(
            call_word("exec cargo build -p adage-cli", &t),
            ("Run".into(), Some("cargo build -p adage-cli".into()))
        );
        assert_eq!(call_word("exec rm -f {path} !failed", &t), ("Run".into(), Some("rm -f …".into())));
        assert_eq!(call_word("read {path} [range]", &t).0, "Read");
        assert_eq!(call_word("search:content {pattern}", &t).0, "Search");
        assert_eq!(call_word("search:filename {pattern}", &t).0, "Find");
        assert_eq!(call_word("control Task general-purpose", &t).0, "Agent");
        assert_eq!(call_word("control mcp browser__navigate", &t).0, "navigate");
        let keys: Vec<String> =
            ["exec ls", "read {path}", "exec cat", "edit {path}", "exec ls"].map(String::from).to_vec();
        let counts = tool_counts(&keys, &t);
        assert_eq!(counts, vec![("Run".to_string(), 3), ("Read".to_string(), 1), ("Edit".to_string(), 1)]);
        let line = Line::from(tools_spans(&counts, 4, Style::default(), &t)).to_string();
        assert_eq!(line, "Run ×3 · Read · Edit");
        assert_eq!(Line::from(tools_spans(&counts, 1, Style::default(), &t)).to_string(), "Run ×3 · +2");
        assert_eq!(
            Line::from(tools_spans(&counts, 4, Style::default(), &t.ascii())).to_string(),
            "Run x3 - Read - Edit"
        );
    }

    #[test]
    fn a_delivery_is_judged_by_the_next_judgement_of_its_reflex_in_its_session() {
        let fire = |seq, s: &str| {
            item(
                seq,
                s,
                "UserPromptSubmit",
                "prompt",
                json!({ "directives": [{ "route": "lookup", "strength": "fire", "text": LOOKUP }] }),
            )
        };
        let judged = |seq, s: &str, kind: &str| {
            item(seq, s, "PostToolBatch", "batch", json!({ "judgements": [{ "kind": kind, "route": "lookup" }] }))
        };
        let items = [
            fire(1, "a"),
            fire(2, "b"),
            judged(3, "b", "ignored"),
            judged(4, "a", "followed"),
            fire(5, "a"),
            judged(6, "a", "obligation_met"),
        ];
        let refs: Vec<&FeedItem> = items.iter().collect();
        let j = judge(&refs);
        assert_eq!(j.outcome[&(0, 0)], Outcome::Followed, "a's first: followed, not b's ignored");
        assert_eq!(j.outcome[&(1, 0)], Outcome::Ignored);
        assert_eq!(j.outcome[&(4, 0)], Outcome::Met, "the second delivery takes the later judgement");
        assert!(j.shown.contains(&(3, 0)) && j.shown.contains(&(2, 0)), "followed and ignored show on the box");
        assert!(!j.shown.contains(&(5, 0)), "a finish line met keeps its own line");
        assert!(j.followed.contains(&3) && j.followed.len() == 1);
        // Delivered again before any judgement: the first waits.
        let items = [fire(1, "a"), fire(2, "a"), judged(3, "a", "followed")];
        let refs: Vec<&FeedItem> = items.iter().collect();
        let j = judge(&refs);
        assert_eq!((j.outcome[&(0, 0)], j.outcome[&(1, 0)]), (Outcome::Waiting, Outcome::Followed));
    }

    #[test]
    fn quiet_steps_fold_and_quiet_turns_fold_into_one_line() {
        let stop = |seq| item(seq, "s1", "Stop", "stop", json!({}));
        let mut items = vec![];
        // A turn with nothing from Adage, ended: one line.
        items.extend((1..=4).map(|k| batch(k, &["exec ls"])));
        items.push(stop(5));
        // A turn still going: five quiet steps fold; the newest stands on its own.
        items.extend((6..=10).map(|k| batch(k, &["read {path}"])));
        items.push(batch(11, &["exec cargo build -p adage-cli"]));
        let rows = draw(&items, 100, false);
        assert!(
            rows.iter().any(|r| r.contains("◇ turn, from earlier 12:04")
                && r.contains("4 steps, nothing to say")
                && r.contains("Run ×4")),
            "{rows:#?}"
        );
        let fold = rows.iter().position(|r| r.contains("┊ 5 steps, nothing to say   Read ×5")).expect("the fold");
        assert!(
            rows[fold + 1].contains("╰ Run  cargo build -p adage-cli")
                || rows[fold + 1].contains("├ Run  cargo build -p adage-cli"),
            "{rows:#?}"
        );
        // Two quiet steps do not fold.
        let rows = draw(&[batch(1, &["exec ls"]), batch(2, &["exec ls"]), batch(3, &["edit {path}"])], 100, false);
        assert_eq!(rows.iter().filter(|r| r.contains("├ Run  ls")).count(), 2, "{rows:#?}");
    }

    #[test]
    fn a_delivery_branches_off_its_step_and_lights_when_followed() {
        let items = vec![
            item(1, "s1", "UserPromptSubmit", "prompt", json!({})),
            item(
                2,
                "s1",
                "PostToolBatch",
                "batch",
                json!({ "observed": ["exec ls"], "directives": [{ "route": "lookup", "strength": "fire", "text": LOOKUP }] }),
            ),
            item(
                3,
                "s1",
                "PostToolBatch",
                "batch",
                json!({ "observed": ["exec bin/fetch-incident INC-2045"], "judgements": [{ "kind": "followed", "route": "lookup" }] }),
            ),
            item(
                4,
                "s1",
                "PostToolBatch",
                "batch",
                json!({ "observed": ["edit {path}"], "judgements": [{ "kind": "obligation_met", "route": "codegen" }] }),
            ),
        ];
        let boxes = draw(&items, 110, true).join("\n");
        for want in [
            "╭─ ❯ PROMPT",
            "├─┬ Run  ls",
            "◆ adage · FIRE",
            "a reflex learned here",
            "─ RUN ─",
            "bin/fetch-incident INC-2045",
            "─ ✓ followed ─",
            "followed adage",
            "◆ adage ✓ finish line met: codegen",
        ] {
            assert!(boxes.contains(want), "the boxes lack {want:?}:\n{boxes}");
        }
        assert!(!boxes.contains("✓ followed bin/fetch"), "the judgement shows on the box, not again:\n{boxes}");
        let rows = draw(&items, 60, false).join("\n");
        for want in [
            "❯ PROMPT",
            "├ Run  ls",
            "◆ adage · FIRE · a reflex learned here",
            "──▶ bin/fetch-incident INC-2045  ✓ followed",
            "◆ adage ✓ finish line met: codegen",
        ] {
            assert!(rows.contains(want), "the list lacks {want:?}:\n{rows}");
        }
        for r in draw(&items, 110, true).iter().chain(&draw(&items, 60, false)) {
            assert!(kit::width(r) <= 110, "{r}");
        }
    }

    #[test]
    fn sessions_get_headers_and_the_window_keeps_whole_blocks() {
        let mut items: Vec<FeedItem> = (1..=30).map(|k| batch(k, &["exec ls"])).collect();
        let mut other = item(31, "s2", "SessionStart", "session_start", json!({}));
        other.harness = "codex".into();
        items.push(other);
        let rows = draw(&items, 100, false);
        assert!(rows[0].contains("Claude Code  session, since 12:04"), "{rows:#?}");
        assert!(rows.iter().any(|r| r.contains("Codex  session started 12:04")), "{rows:#?}");
        let c = ctx(false);
        let refs: Vec<&FeedItem> = items.iter().collect();
        let gists = BTreeMap::new();
        let o = Opts { width: 110, boxes: true, repo: false, gists: &gists, pulse: Pulse::default(), glow: false };
        let (lines, _) = lines(&refs, &c, &o);
        let w = window(&lines, 4, 0);
        assert_eq!(w.to, lines.len());
        assert!(w.to - w.from <= 3 && w.earlier.is_some(), "{w:?}");
        assert!(
            w.from == 0 || lines[w.from].block != lines[w.from - 1].block || lines[w.from].sessions > 0,
            "starts on a block, or a session's header"
        );
        assert!(!lines[w.from].lead.is_empty() || !lines[w.from].body.is_empty(), "never on a blank line");
        let back = window(&lines, 4, 2);
        assert_eq!((back.newer, back.to), (2, lines.len() - 2));
        assert_eq!(window(&lines, 100, 0).earlier, None);
    }
}
