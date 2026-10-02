//! The wire format of Adage's own text inside a trace: the markers a Stop-hook continuation
//! carries, and what a rendered directive body names. The runtime writes this text
//! (`adage_runtime::render`); trace decoders and the hook read it back, so a session never counts
//! Adage's own direction as the agent's or the person's (Track G §3.2, steering by influence).
//! Writer and reader stay in step through the runtime's round-trip tests.

/// The first words of every Stop-hook continuation Adage writes. Where a continuation re-enters
/// as a user message (Codex turns a Stop `block` reason into the next user prompt), these words
/// are how the hook and the learners know it as Adage's own text, never the user's prompt.
pub const FINISH_LINE: &str = "Adage finish-line check:";
/// The provenance sentence that ends every Stop-hook continuation Adage writes.
pub const NOT_FROM_USER: &str = "This reminder is from Adage, not from the user.";

/// `body` as a Stop-hook continuation: [`FINISH_LINE`] first and [`NOT_FROM_USER`] last, each
/// added only when missing (so joining two continuations keeps one of each at the ends).
pub fn finish_line(body: &str) -> String {
    let body = body.trim();
    let mut s = if body.starts_with(FINISH_LINE) { body.to_string() } else { format!("{FINISH_LINE} {body}") };
    if !s.ends_with(NOT_FROM_USER) {
        s.push(' ');
        s.push_str(NOT_FROM_USER);
    }
    s
}

/// Whether `text` is a Stop-hook continuation Adage wrote: it begins with [`FINISH_LINE`], or a
/// harness quoted it inside framing of its own and both markers are there.
pub fn is_finish_line(text: &str) -> bool {
    let t = text.trim_start();
    t.starts_with(FINISH_LINE) || (t.contains(FINISH_LINE) && t.contains(NOT_FROM_USER))
}

/// What a rendered directive body names (Track G §3.2, steering by influence): the commands it
/// directs or recommends, the runners it says to avoid, and the files it lists. The inverse of
/// `adage_runtime::render::route_text` and `stop_reason` for every template: a command is an indented line (`  cmd`,
/// `  1. cmd`) or a backquoted span, avoided runners follow `running ` up to ` fails (`, and files
/// follow `lists these files: `. Nothing named (a body cut short, a template this does not know)
/// is an empty result, which a learner treats as naming everything.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Named {
    pub commands: Vec<String>,
    pub avoided: Vec<String>,
    pub files: Vec<String>,
}

impl Named {
    pub fn is_empty(&self) -> bool {
        self.commands.is_empty() && self.avoided.is_empty() && self.files.is_empty()
    }
}

/// See [`Named`].
pub fn named(body: &str) -> Named {
    let mut n = Named::default();
    for line in body.lines() {
        if let Some(rest) = line.strip_prefix("  ") {
            // The generic template ends its command line with the exit clause.
            let rest = rest.split(" Ignore this if ").next().unwrap_or(rest).trim();
            let rest = match rest.split_once(". ") {
                Some((k, cmd)) if !k.is_empty() && k.chars().all(|c| c.is_ascii_digit()) => cmd,
                _ => rest,
            };
            if !rest.is_empty() {
                n.commands.push(rest.to_string());
            }
        }
        if let Some((_, after)) = line.split_once("running ") {
            if let Some((avoid, _)) = after.split_once(" fails (") {
                n.avoided.extend(avoid.split(" / ").map(|x| x.trim().to_string()).filter(|x| !x.is_empty()));
            }
        }
        if let Some((_, after)) = line.split_once("lists these files: ") {
            let list = after.trim_end();
            let list = list.strip_suffix('.').unwrap_or(list);
            n.files.extend(list.split(", ").map(|x| x.trim().to_string()).filter(|x| !x.is_empty()));
        }
        let mut rest = line;
        while let Some((_, after)) = rest.split_once('`') {
            let Some((span, tail)) = after.split_once('`') else { break };
            if !span.trim().is_empty() {
                n.commands.push(span.trim().to_string());
            }
            rest = tail;
        }
    }
    n.commands.dedup();
    n
}
