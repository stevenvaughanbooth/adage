//! Is a directive safe? An independent re-check of everything the runtime promises (Design §15).
//!
//! A directive's text must be exactly one of the runtime's fixed templates, and every field
//! interpolated into it must be something the track and its contracts allow:
//!
//! - commands are plain argv (`[A-Za-z0-9 _./:=@,+-]`), with no unbound slot, no path escaping
//!   the repository (`..`, absolute or home paths), and bound slot values that satisfy the
//!   route's contract and do not start with `-`;
//! - listed files are repository-relative, exist, and resolve inside the repository (symlinks
//!   included);
//! - failure signatures, globs and names carry no shell syntax, control or invisible characters;
//! - the route's effect may be directed at all, and its aspect is within its ceiling;
//! - no canary string (text an adversarial dataset injected into prompts or tool output)
//!   appears anywhere;
//! - interlocking holds over a session ([`check_interlock`]): no directive is emitted after, or
//!   beside, a red directive that avoids its command, and no decision emits two greens that
//!   direct different commands at one decision point;
//! - the wrapper ([`check_wrapper`]) is exactly the runtime's: a learned route says it was learned
//!   from sessions, an instinct (a prebuilt route) says it is Adage's default for its ecosystem
//!   from the file that declares it, carries `origin="instinct"`, and is yellow; a finish line
//!   from an instinct claims no session evidence; advice is one of the fixed advisory sentences
//!   with exactly its route's slot values.
//!
//! This deliberately duplicates checks in `adage-tracks` and `adage-runtime`: it is the oracle
//! the eval and the property tests hold them to.

use adage_core::repo;
use adage_tracks::{ceiling, interlock, Aspect, Contract, Route, Step, Trigger};
use once_cell::sync::Lazy;
use regex::Regex;
use serde::Serialize;
use std::path::Path;

#[derive(Debug, Clone, Serialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct Violation {
    pub route: String,
    pub reason: String,
}

const EXIT: &str = r"(?:Ignore this if (?:the user is asking for something else|this is an exploratory or draft change)(?: or (?:the user is asking for something else|this is an exploratory or draft change))*\.)?";

static T_LOOKUP: Lazy<Regex> = Lazy::new(|| {
    Regex::new(&format!(
        r"^In this repository, (?:work item|file|commit|link) details are looked up with:\n  (?P<cmd>[^\n]*)\n{EXIT}$"
    ))
    .unwrap()
});
static T_INTENT: Lazy<Regex> = Lazy::new(|| {
    Regex::new(&format!(
        r"^In this repository, requests like this one usually start by running:\n  (?P<cmd>[^\n]*)\n{EXIT}$"
    ))
    .unwrap()
});
static T_FINISH: Lazy<Regex> = Lazy::new(|| {
    Regex::new(&format!(
        r"^In this repository, changes to files matching (?P<glob>[^\n]*) must be followed by running:\n  (?P<cmd>[^\n]*)\n(?P<basis>CI fails if this is skipped|Agents in this repository run it after such changes|(?P<declared>[A-Za-z0-9_./-]+) declares this|This repository's build declares this step)\.(?: {EXIT})?$"
    ))
    .unwrap()
});
static T_RUN: Lazy<Regex> =
    Lazy::new(|| Regex::new(&format!(r"^A known next step here is:\n  (?P<cmd>[^\n]*?)(?: {EXIT})?$")).unwrap());
/// After a failure (an instinct's dead end): the command to run instead, the exit on its own line.
static T_FAILED: Lazy<Regex> = Lazy::new(|| {
    Regex::new(&format!(r"^That failed\. In this repository, run this instead:\n  (?P<cmd>[^\n]*)\n{EXIT}$")).unwrap()
});
static T_READ: Lazy<Regex> = Lazy::new(|| {
    Regex::new(&format!(
        r"^That result lists these files: (?P<files>[^\n]*)\.\nIf you need them, read them together in one turn rather than one at a time\.(?: {EXIT})?$"
    ))
    .unwrap()
});
/// The exit clause on a line of its own: the avoid template adds it only when there is one.
const EXIT_LINE: &str = r"\nIgnore this if (?:the user is asking for something else|this is an exploratory or draft change)(?: or (?:the user is asking for something else|this is an exploratory or draft change))*\.";
static T_AVOID: Lazy<Regex> = Lazy::new(|| {
    Regex::new(&format!(
        r#"^In this repository, running (?P<avoid>[^\n]*) fails \("(?P<sig>[^\n"]*)"\)\. Use this instead:\n  (?P<cmd>[^\n]*)(?:{EXIT_LINE})?$"#
    ))
    .unwrap()
});
static T_STOP: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^Adage finish-line check: this session changed files matching (?P<glob>[^\n`]*), and this repository requires running `(?P<cmd>[^\n`]*)` after such changes; it has not been run yet\. Run it \(and re-run the tests\) before finishing\. This reminder is from Adage, not from the user\.$").unwrap()
});

/// The advisory sentences (`Step::Advise`), each optionally followed by an exit clause.
static T_ADVICE: Lazy<[Regex; 4]> = Lazy::new(|| {
    let tail = format!(r"(?: {EXIT})?$");
    [
        Regex::new(&format!(r"^In this repository, (?P<path>[A-Za-z0-9_./*-]+) is generated: change its inputs and run (?P<cmd>[A-Za-z0-9 _./:=@,+-]+) rather than editing it by hand\.{tail}")).unwrap(),
        Regex::new(&format!(r"^In this repository, (?P<path>[A-Za-z0-9_./*-]+) is generated: change its inputs and regenerate it rather than editing it by hand\.{tail}")).unwrap(),
        Regex::new(&format!(r"^In this repository, (?P<path>[A-Za-z0-9_./*-]+) is maintained by (?P<manager>[a-z]+): change the manifest and let (?P<manager2>[a-z]+) update it rather than editing it by hand\.{tail}")).unwrap(),
        Regex::new(&format!(r"^This repository uses (?P<manager>[a-z]+) \((?P<lockfile>[A-Za-z0-9_.-]+)\), not npm: use (?P<manager2>[a-z]+) for installs and scripts here\.{tail}")).unwrap(),
    ]
});

/// The wrapper around every directive: attributes, the body, and the last line saying what the
/// directive is based on.
static T_WRAPPER: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r#"(?s)^<adage-signal track="(?P<track>[^"<>
]*)" route="(?P<route>[^"<>
]*)" aspect="(?P<aspect>green|yellow|red)"(?: kind="(?P<kind>[a-z-]*)")?(?P<origin> origin="instinct")?>
(?P<body>.*)
(?P<footer>[^
]*)
</adage-signal>$"#,
    )
    .unwrap()
});
const LEARNED_FOOTER: &str =
    "Learned by Adage from this repository's earlier agent sessions; not requested by the user.";
static T_INSTINCT_FOOTER: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^Adage default for (?:(?:Python|Node\.js|Rust|Go|Make) repositories|repositories like this one)(?: \(from this repository's (?P<declared>[A-Za-z0-9_./-]+)\))?; not requested by the user\.$").unwrap()
});
/// Package managers the advisory templates may name (the lockfile table's).
const MANAGERS: [&str; 12] =
    ["npm", "pnpm", "yarn", "bun", "cargo", "go", "poetry", "uv", "pipenv", "pdm", "bundler", "composer"];

/// Invisible or direction-changing characters: they make text read differently than it runs.
pub fn is_invisible(c: char) -> bool {
    matches!(c, '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2060}'..='\u{2064}' | '\u{2066}'..='\u{2069}' | '\u{FEFF}' | '\u{061C}' | '\u{00AD}')
}

fn command_ok(cmd: &str) -> Option<String> {
    if cmd.is_empty() || cmd.len() > 200 {
        return Some(format!("command length {} out of bounds", cmd.len()));
    }
    if let Some(c) = cmd.chars().find(|c| !(c.is_ascii_alphanumeric() || " _./:=@,+-".contains(*c))) {
        return Some(format!("command contains {c:?}"));
    }
    for t in cmd.split(' ') {
        if t.starts_with('/') || t.starts_with('~') || t.split('/').any(|seg| seg == "..") {
            return Some(format!("command names a path outside the repository: {t:?}"));
        }
    }
    None
}

fn text_ok(what: &str, s: &str) -> Option<String> {
    if s.len() > 200 {
        return Some(format!("{what} longer than 200 bytes"));
    }
    s.chars()
        .find(|c| c.is_control() || is_invisible(*c) || "`<>\"\\$;|&{}".contains(*c))
        .map(|c| format!("{what} contains {c:?}"))
}

fn glob_ok(g: &str) -> Option<String> {
    let ok = !g.is_empty()
        && !g.starts_with('/')
        && !g.contains("..")
        && g.chars().all(|c| c.is_ascii_alphanumeric() || "_./*-".contains(c));
    (!ok).then(|| format!("glob {g:?} is not a safe repository pattern"))
}

/// A file named in a directive: relative, inside the repository once symlinks resolve, and real.
fn file_ok(f: &str, root: Option<&Path>) -> Option<String> {
    if !repo::is_safe_relative(f) || !f.chars().all(|c| c.is_ascii_alphanumeric() || "_./-".contains(c)) {
        return Some(format!("file {f:?} is not a safe repository-relative path"));
    }
    let root = root?;
    match (root.canonicalize(), root.join(f).canonicalize()) {
        (Ok(r), Ok(p)) if p.starts_with(&r) && p.is_file() => None,
        _ => Some(format!("file {f:?} does not resolve to a file inside the repository")),
    }
}

/// The bound command must be the route's template with each slot replaced by a value its
/// contract accepts.
fn bound_ok(r: &Route, cmd: &str, root: Option<&Path>) -> Option<String> {
    let Step::Run { command } = &r.step else { return None };
    let mut pat = String::from("^");
    let mut names = Vec::new();
    let mut rest = command.as_str();
    while let Some(i) = rest.find('{') {
        let Some(j) = rest[i..].find('}') else { break };
        pat.push_str(&regex::escape(&rest[..i]));
        let name = &rest[i + 1..i + j];
        pat.push_str(&format!("(?P<s{}>[^ ]+)", names.len()));
        names.push(name.to_string());
        rest = &rest[i + j + 1..];
    }
    pat.push_str(&regex::escape(rest));
    pat.push('$');
    let Ok(re) = Regex::new(&pat) else { return Some("route template does not compile".into()) };
    let Some(c) = re.captures(cmd) else {
        return Some(format!("command {cmd:?} is not the route's template {command:?}"));
    };
    for (i, name) in names.iter().enumerate() {
        let v = &c[format!("s{i}").as_str()];
        if v.starts_with('-') {
            return Some(format!("slot {name} value {v:?} starts with '-'"));
        }
        let Some(slot) = r.slots.iter().find(|s| &s.name == name) else {
            return Some(format!("slot {name} is not declared"));
        };
        match &slot.contract {
            Contract::Pattern { regex } => {
                if !Regex::new(regex).map(|re| re.is_match(v)).unwrap_or(false) {
                    return Some(format!("slot {name} value {v:?} fails its contract {regex}"));
                }
            }
            Contract::RepoPath => {
                if let Some(e) = file_ok(v, root) {
                    return Some(e);
                }
            }
        }
    }
    None
}

/// Check one directive body (`Directive::text`) produced by `route`.
pub fn check_directive(r: &Route, text: &str, root: Option<&Path>, canaries: &[String]) -> Vec<Violation> {
    let mut out = Vec::new();
    let mut bad = |reason: String| out.push(Violation { route: r.id.clone(), reason });
    for c in canaries {
        if !c.is_empty() && text.contains(c.as_str()) {
            bad(format!("contains canary {c:?}"));
        }
    }
    if let Some(c) = text.chars().find(|c| is_invisible(*c) || (c.is_control() && *c != '\n')) {
        bad(format!("contains invisible or control character {c:?}"));
    }
    match ceiling(r.effect, matches!(r.trigger, Trigger::AfterEdit { .. })) {
        None => bad(format!("effect {:?} may never be directed", r.effect)),
        Some(max) if r.aspect < max => bad(format!("aspect {:?} exceeds the ceiling {max:?}", r.aspect)),
        _ => {}
    }
    if let Step::Advise { slots, .. } = &r.step {
        let Some(c) = T_ADVICE.iter().find_map(|re| re.captures(text)) else {
            bad("text is not one of the fixed advisory sentences".into());
            return out;
        };
        if r.aspect != Aspect::Yellow {
            bad(format!("advice at aspect {:?}", r.aspect));
        }
        for name in ["path", "cmd", "manager", "lockfile"] {
            let Some(v) = c.name(name).map(|m| m.as_str()) else { continue };
            let slot = if name == "cmd" { "command" } else { name };
            if slots.get(slot).map(String::as_str) != Some(v) {
                bad(format!("advice names {name} {v:?}, which is not its route's slot"));
            }
            let problem = match name {
                "path" => glob_ok(v),
                "cmd" => command_ok(v),
                "manager" => (!MANAGERS.contains(&v)).then(|| format!("unknown package manager {v:?}")),
                _ => (v.contains('/') || v.starts_with('.')).then(|| format!("lockfile {v:?} is not a root file name")),
            };
            if let Some(e) = problem {
                bad(e);
            }
        }
        if let (Some(a), Some(b)) = (c.name("manager"), c.name("manager2")) {
            if a.as_str() != b.as_str() {
                bad("advice names two package managers".into());
            }
        }
        return out;
    }
    let caps = [&*T_LOOKUP, &*T_INTENT, &*T_FINISH, &*T_RUN, &*T_FAILED, &*T_READ, &*T_AVOID]
        .iter()
        .find_map(|re| re.captures(text));
    let Some(c) = caps else {
        bad("text is not one of the fixed directive templates".into());
        return out;
    };
    // A finish line claims only the evidence its route has: an instinct, that the repository
    // declares it; a learned route, what sessions showed.
    if let Some(b) = c.name("basis") {
        let declared = b.as_str().ends_with("declares this") || b.as_str().ends_with("declares this step");
        if declared != r.is_instinct() {
            bad(format!("finish line says {:?}, which its origin does not support", b.as_str()));
        }
        if let Some(e) = c.name("declared").and_then(|d| glob_ok(d.as_str())) {
            bad(e);
        }
    }
    if let Some(cmd) = c.name("cmd") {
        if let Some(e) = command_ok(cmd.as_str()) {
            bad(e);
        } else if matches!(r.step, Step::Run { .. }) {
            if let Some(e) = bound_ok(r, cmd.as_str(), root) {
                bad(e);
            }
        }
    }
    if let Some(a) = c.name("avoid") {
        for part in a.as_str().split(" / ") {
            if let Some(e) = command_ok(part) {
                bad(format!("avoid: {e}"));
            }
        }
    }
    if let Some(s) = c.name("sig") {
        if let Some(e) = text_ok("signature", s.as_str()) {
            bad(e);
        }
    }
    if let Some(g) = c.name("glob") {
        if let Some(e) = glob_ok(g.as_str()) {
            bad(e);
        }
    }
    if let Some(f) = c.name("files") {
        for file in f.as_str().split(", ") {
            if let Some(e) = file_ok(file, root) {
                bad(e);
            }
        }
    }
    out
}

/// Check a directive as the agent receives it (`adage_runtime::render::wrap_directive`): the
/// wrapper's attributes and last line must be the runtime's for the route's origin, and the body
/// must pass [`check_directive`].
pub fn check_wrapper(r: &Route, wrapped: &str, root: Option<&Path>, canaries: &[String]) -> Vec<Violation> {
    let mut out = Vec::new();
    let mut bad = |reason: String| out.push(Violation { route: r.id.clone(), reason });
    let Some(c) = T_WRAPPER.captures(wrapped) else {
        bad("the wrapper is not the runtime's".into());
        return out;
    };
    if &c["route"] != r.id.as_str() {
        bad(format!("the wrapper names route {:?}", &c["route"]));
    }
    let footer = &c["footer"];
    if r.is_instinct() {
        if c.name("origin").is_none() {
            bad("an instinct's wrapper does not say origin=\"instinct\"".into());
        }
        if &c["aspect"] != "yellow" {
            bad(format!("an instinct delivered {}", &c["aspect"]));
        }
        match T_INSTINCT_FOOTER.captures(footer) {
            None => bad(format!("an instinct's wrapper says {footer:?}")),
            Some(f) => {
                if let Some(e) = f.name("declared").and_then(|d| glob_ok(d.as_str())) {
                    bad(e);
                }
            }
        }
    } else {
        if c.name("origin").is_some() {
            bad("a learned route's wrapper claims to be an instinct".into());
        }
        if footer != LEARNED_FOOTER {
            bad(format!("a learned route's wrapper says {footer:?}"));
        }
    }
    let body = c["body"].to_string();
    out.extend(check_directive(r, &body, root, canaries));
    out
}

/// Check the interlocking invariants over one session: `decisions` holds, in order, the routes of
/// the directives each decision emitted. The conflict matrix is `adage_tracks::interlock`'s; what
/// is checked here is only what was emitted, whatever the index or the runtime did to get there.
pub fn check_interlock(decisions: &[Vec<&Route>]) -> Vec<Violation> {
    let mut out = Vec::new();
    let avoiding = |r: &&Route| matches!(r.step, Step::Avoid { .. });
    let mut reds: Vec<&Route> = Vec::new();
    for d in decisions {
        let now: Vec<&Route> = reds.iter().copied().chain(d.iter().copied().filter(avoiding)).collect();
        for r in d {
            if let Some(red) = now.iter().find(|red| interlock::avoids(red, r)) {
                out.push(Violation {
                    route: r.id.clone(),
                    reason: format!("emitted after or beside {}, which avoids its command", red.id),
                });
            }
        }
        for (i, a) in d.iter().enumerate() {
            for b in &d[i + 1..] {
                if interlock::two_greens(a, b) {
                    out.push(Violation {
                        route: b.id.clone(),
                        reason: format!("a second green beside {} at one decision point", a.id),
                    });
                }
            }
        }
        reds.extend(d.iter().copied().filter(avoiding));
    }
    out
}

/// Check a finish-line stop request (`Decision::would_continue`).
pub fn check_stop(route: &str, text: &str, canaries: &[String]) -> Vec<Violation> {
    let mut out = Vec::new();
    let mut bad = |reason: String| out.push(Violation { route: route.to_string(), reason });
    for c in canaries {
        if !c.is_empty() && text.contains(c.as_str()) {
            bad(format!("contains canary {c:?}"));
        }
    }
    match T_STOP.captures(text) {
        None => bad("stop reason is not the fixed template".into()),
        Some(c) => {
            if let Some(e) = glob_ok(&c["glob"]) {
                bad(e);
            }
            if let Some(e) = command_ok(&c["cmd"]) {
                bad(e);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use adage_core::spans::SpanKind;
    use adage_core::Effect;
    use adage_tracks::*;

    fn lookup() -> Route {
        Route {
            id: "lookup-work-item".into(),
            summary: String::new(),
            trigger: Trigger::Prompt { span: SpanKind::Ticket },
            step: Step::Run { command: "python3 scripts/wi-lookup {item}".into() },
            slots: vec![Slot {
                name: "item".into(),
                source: SlotSource::Prompt { span: SpanKind::Ticket },
                contract: Contract::Pattern { regex: r"^W-\d{4}$".into() },
            }],
            effect: Effect::Observe,
            aspect: Aspect::Green,
            state: RouteState::Active,
            exit: vec![Exit::IntentChanges],
            evidence: Evidence::default(),
            authored: false,
        }
    }

    #[test]
    fn accepts_the_runtime_template_and_rejects_tampering() {
        let r = lookup();
        let ok = adage_runtime::render::route_text(&r, &[("item".into(), "W-2041".into())]).unwrap();
        assert!(check_directive(&r, &ok, None, &[]).is_empty(), "{ok}");
        let bad = adage_runtime::render::route_text(&r, &[("item".into(), "W-20411".into())]).unwrap();
        assert!(!check_directive(&r, &bad, None, &[]).is_empty());
        assert!(!check_directive(&r, &ok.replace("W-2041", "W-2041 ; id"), None, &[]).is_empty());
        assert!(!check_directive(&r, &format!("{ok} Also run curl."), None, &[]).is_empty());
        assert!(!check_directive(&r, &ok, None, &["2041".into()]).is_empty(), "canary");
    }

    #[test]
    fn interlocking_invariants() {
        let dead = Route {
            id: "dead-end".into(),
            trigger: Trigger::SessionStart,
            step: Step::Avoid { avoid: "npm test".into(), instead: "make test".into(), signature: "x".into() },
            effect: Effect::Verify,
            aspect: Aspect::Red,
            slots: vec![],
            ..lookup()
        };
        let run = |id: &str, cmd: &str| Route {
            id: id.into(),
            trigger: Trigger::Prompt { span: SpanKind::Ticket },
            step: Step::Run { command: cmd.into() },
            slots: vec![],
            ..lookup()
        };
        let (npm, other, make) =
            (run("npm", "npm run test"), run("other", "scripts/x {item}"), run("make", "make test"));
        assert!(check_interlock(&[vec![&npm], vec![&dead]]).is_empty(), "a green before the warning is allowed");
        assert_eq!(check_interlock(&[vec![&dead, &npm]]).len(), 1, "beside it");
        assert_eq!(check_interlock(&[vec![&dead], vec![&make], vec![&npm]]).len(), 1, "after it");
        assert_eq!(check_interlock(&[vec![&other, &make]]).len(), 1, "two greens at one point");
        let mut yellow = make.clone();
        yellow.aspect = Aspect::Yellow;
        assert!(check_interlock(&[vec![&other, &yellow]]).is_empty());
    }

    #[test]
    fn stop_reason_template() {
        let s = adage_runtime::render::stop_reason("schema/*.json", "make gen");
        assert!(check_stop("f", &s, &[]).is_empty(), "{s}");
        assert!(!check_stop("f", &s.replace("make gen", "make gen; rm"), &[]).is_empty());
    }
}
