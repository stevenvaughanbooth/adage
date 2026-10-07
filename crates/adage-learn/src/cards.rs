//! Procedure cards for recurring work: a repository's traces grouped into **families** of similar
//! tasks, and for each family with a passing trace a **card** of what its traces had in common,
//! delivered once to the next task of the family (an `intent` trigger,
//! [`adage_tracks::Step::Card`]). The spec is the research's (evals/lab/research/
//! recurring-workflows, `confirm/README.md`, "Card compiler spec"; the reference is
//! `procedure.py`).
//!
//! - **Families.** Two traces are of one family when their first prompts are similar (cosine of
//!   the pinned static model's embeddings, the one the hook embeds with, at least
//!   [`CardParams::family_similarity`]) *and* the shapes of the files they edited overlap
//!   (Jaccard at least [`CardParams::file_overlap`]; a shape is directory, name token count, first
//!   token and extension, so `flag_url.go` and `flag_port.go` agree; a file a trace created also
//!   counts by its role, so `middleware/no_sniff.go` and `middleware/hsts.go` agree). Families are the connected
//!   components, in canonical order: every later trace that matched joins its family, and its
//!   prompt joins the family's prompt library.
//! - **Cards**: every file edited in at least half of the family's traces (all of them with one
//!   trace), in the median order of first edit; files edited in one trace each, grouped by shape
//!   and anti-unified (`flag_<x>.go`, `flag_<x>_test.go`), or, when each task created and named
//!   them whole (`middleware/no_sniff.go`, `middleware/hsts.go`), grouped by role: directory, test
//!   convention and extension (`middleware/<x>.go`, `middleware/<x>_test.go`); a file a command
//!   wrote by redirection counts as edited. For each, the longest text the most recent passing trace added to it (or
//!   to a file of the pattern's shape). The commands run in at least half of the traces
//!   (normalised to their head), kept when they mostly worked and listed to avoid when they mostly
//!   failed, with the first line of the error; and the check the passing traces finished with,
//!   anti-unified. Exemplars are clipped (14, 10, 6, 3 lines, then none) to fit 800 tokens.
//! - **Minimum traces.** No passing trace, no card, and no card from fewer than [`MIN_TRACES`]
//!   traces: one trace is any one-off bug fix. At most [`MAX_CARDS`] cards per repository, the
//!   best supported.
//! - **What a card never quotes** (the launch review, B3): a file that holds secrets by its kind
//!   (`.env*`, keys, credential stores) or that git ignores; a command that holds a secret or names
//!   such a file; a line, command or error that looks like a secret (`adage_core::secrets`, the
//!   rules `adage share` checks packs with). A secret-looking exemplar line becomes `…`.
//! - **What a card lists** (fresh-install B9): exemplars are whole hunks as the agent wrote them;
//!   commands are productive ones (no `git status`, `ls`, `cat`, `grep`, commits); a family's test
//!   runs are one command, by runner; a test run to avoid comes with the one that worked.
//! - **Triggering.** A new prompt's best cosine to the family's prompts, at the research's
//!   pre-registered threshold ([`THRESHOLD`], `learn.card_threshold`; lower for a family of one
//!   prompt). Within 0.10 below it, the card is said at the session's first edit of one of its
//!   files. Precise (no false fire on 53 same-repository negatives) but literal: paraphrases were
//!   not recognised (0 of 6). A card is prime, observe, and said once per session.
//! - **Verification.** `adage train cards` (the adage-train binary: `adage` spawns nothing) replays
//!   a card's commands on a clean checkout in adage-exec's sandbox and records the verdicts
//!   ([`VERDICTS_FILE`]); the next learn marks those that worked and drops those that did not from
//!   the commands that worked.
//!
//! Nothing here names a repository's files, commands or errors.

use crate::import::Session;
use adage_core::gitignore::Ignore;
use adage_core::secrets;
use adage_intent::model::StaticModel;
use adage_intent::normalize::normalize;
use adage_tracks::card::{
    name_tokens, Card, CardCommand, CardFile, Exemplar, MAX_EXEMPLAR_LINES, MAX_LINE, MAX_PROMPTS, VARYING,
};
use adage_tracks::why::Why;
use adage_tracks::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

/// Under `$ADAGE_HOME`: card id → command → whether its replay on a clean checkout worked.
pub const VERDICTS_FILE: &str = "train/cards.json";

pub type Verdicts = BTreeMap<String, BTreeMap<String, bool>>;

/// Read the verdicts `adage train cards` recorded (none when absent or unreadable).
pub fn load_verdicts(home: &Path) -> Verdicts {
    std::fs::read(home.join(VERDICTS_FILE)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

/// The research's pre-registered delivery threshold (confirm/README.md, "Detection"): a new
/// prompt's best cosine to the family's prompts; chosen on two families, precise on a third (0 of
/// 53 same-repository negatives), but it recognises only phrasing close to earlier prompts.
pub const THRESHOLD: f32 = 0.811;
/// ... and for a family of one prompt, how much lower its threshold is (0.753 at the default).
pub const SINGLE_PROMPT_DROP: f32 = 0.058;
/// A family needs this many traces for a card (review S8).
pub const MIN_TRACES: usize = 2;
/// At most this many cards per repository, the best supported (review S8): every card is in the
/// index every hook reads.
pub const MAX_CARDS: usize = 16;

/// How families form and when a card is delivered.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CardParams {
    /// A new prompt's best similarity to the family's prompts that delivers the card (a family of
    /// one prompt: [`SINGLE_PROMPT_DROP`] lower). Within `adage_tracks::card::GREY_MARGIN` below
    /// it, the card waits for the session's first edit of one of its files.
    pub threshold: f32,
    /// Two traces' first prompts must be at least this similar to be of one family.
    pub family_similarity: f32,
    /// ... and their edited files must overlap at least this much (Jaccard).
    pub file_overlap: f64,
    #[serde(default)]
    pub verdicts: Verdicts,
}

impl Default for CardParams {
    fn default() -> Self {
        CardParams { threshold: THRESHOLD, family_similarity: 0.6, file_overlap: 0.25, verdicts: Verdicts::new() }
    }
}

const EDIT_TOOLS: [&str; 4] = ["Edit", "Write", "MultiEdit", "NotebookEdit"];

struct EditView {
    k: usize,
    path: String,
    text: String,
    /// Written whole (`Write`): a file the task created, which recurring work names after the task.
    created: bool,
}

struct CmdView {
    k: usize,
    norm: String,
    failed: bool,
    out: String,
}

struct View<'a> {
    s: &'a Session,
    edits: Vec<EditView>,
    cmds: Vec<CmdView>,
    passed: bool,
}

/// The text an edit wrote, whole: a written file's content, or an edit's replacement as the agent
/// wrote it (fresh-install B9: lines picked out of it made fragments, an `if …:` without its
/// body). What to imitate is its format.
fn hunk(new: &str) -> String {
    new.trim_matches('\n').to_string()
}

/// A shell command's stable head: the non-`cd` segments of its first line, pipes and redirections
/// dropped, at most six tokens, a test id cut to its file.
pub fn norm_cmd(cmd: &str) -> String {
    let first = cmd.trim().lines().next().unwrap_or("");
    let first = first.split("<<").next().unwrap_or("");
    let segs: Vec<&str> =
        first.split("&&").flat_map(|s| s.split(';')).map(str::trim).filter(|s| !s.is_empty()).collect();
    let kept: Vec<&str> = segs.iter().copied().filter(|s| !s.starts_with("cd ")).collect();
    let segs = if kept.is_empty() { segs } else { kept };
    let joined = segs.iter().map(|s| s.split('|').next().unwrap_or("").trim()).collect::<Vec<_>>().join(" && ");
    let mut out: Vec<String> = Vec::new();
    for (t, quoted) in words(&joined) {
        if out.len() >= 6 {
            break;
        }
        // An inline script (`python -c "..."`) is this task's code, not a command to repeat.
        if quoted && t.chars().any(char::is_whitespace) {
            return String::new();
        }
        // Redirections (`2>&1`, `>/dev/null`, `> out.txt`) are not the command.
        if !quoted && (t.contains('>') || t.contains('<')) {
            break;
        }
        let t = t.split("::").next().unwrap_or(&t).to_string();
        if !t.is_empty() {
            out.push(t);
        }
    }
    out.join(" ")
}

/// Shell words, quotes removed, each with whether any of it was quoted. An unbalanced quote runs
/// to the end and counts as whitespace in its word.
fn words(s: &str) -> Vec<(String, bool)> {
    let mut out = Vec::new();
    let (mut cur, mut quoted, mut any) = (String::new(), None::<char>, false);
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        match (quoted, c) {
            (None, '\'' | '"') => {
                quoted = Some(c);
                any = true;
            }
            (Some(q), _) if c == q => quoted = None,
            (Some('"') | None, '\\') => {
                if let Some(n) = chars.next() {
                    cur.push(n);
                }
            }
            (None, _) if c.is_whitespace() => {
                if !cur.is_empty() || any {
                    out.push((std::mem::take(&mut cur), any));
                }
                any = false;
            }
            _ => cur.push(c),
        }
    }
    if quoted.is_some() {
        // A quote left open on the first line: a multi-line inline script.
        cur.push('\n');
    }
    if !cur.is_empty() || any {
        out.push((cur, any));
    }
    out
}

/// The relative paths a command line writes by redirection (`> f`, `>> f`, not `2>&1`, not
/// `/dev/null`, not an absolute path), normalised (`./a/b` → `a/b`).
fn redirect_targets(line: &str) -> Vec<String> {
    let b = line.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] != b'>' || (i > 0 && matches!(b[i - 1], b'<' | b'>' | b'&' | b'0'..=b'9')) {
            i += 1;
            continue;
        }
        let mut j = i + 1;
        if j < b.len() && b[j] == b'>' {
            j += 1;
        }
        while j < b.len() && b[j] == b' ' {
            j += 1;
        }
        let start = j;
        while j < b.len() && !b" \t;&|<>".contains(&b[j]) {
            j += 1;
        }
        let t = &line[start..j];
        let t = t.trim_start_matches("./");
        if !t.is_empty()
            && !t.starts_with('/')
            && !t.starts_with('&')
            && !t.starts_with('$')
            && !t.split('/').any(|s| s.is_empty() || s == "..")
            && !out.iter().any(|x: &String| x == t)
        {
            out.push(t.to_string());
        }
        i = j.max(i + 1);
    }
    out
}

/// A file's role for grouping names that vary: its directory, the number of tokens in its name,
/// its first token and its extension (`flag_url.go` and `flag_port.go` share one; `flag_url_test.go`
/// has another).
fn shape(path: &str) -> (String, usize, String, String) {
    let (dir, name) = path.rsplit_once('/').unwrap_or(("", path));
    let t = name_tokens(name);
    (dir.to_string(), t.len(), t.first().unwrap_or(&"").to_string(), t.last().unwrap_or(&"").to_string())
}

/// Names of one shape aligned token by token: tokens that agree are kept, the others become
/// `<x>` (`flag_url.go`, `flag_port.go` → `flag_<x>.go`).
fn anti_unify(paths: &[&str]) -> String {
    let (dir, _) = paths[0].rsplit_once('/').unwrap_or(("", ""));
    let toks: Vec<Vec<&str>> = paths.iter().map(|p| name_tokens(p.rsplit_once('/').map_or(*p, |x| x.1))).collect();
    let name: String = toks[0]
        .iter()
        .enumerate()
        .map(|(i, t)| if toks.iter().all(|x| x.get(i) == Some(t)) { *t } else { VARYING })
        .collect();
    if dir.is_empty() {
        name
    } else {
        format!("{dir}/{name}")
    }
}

/// A created file's role when its whole name is the task's (`middleware/no_sniff.go`,
/// `middleware/hsts.go` → `middleware/<x>.go`): its directory, whether it is a test sibling and of
/// which convention (`<x>_test`, `test_<x>`, `<x>.test`, `<x>.spec`), and its extension. Coarser
/// than [`shape`], so only files a task wrote whole are grouped this way.
fn role(path: &str) -> String {
    let (dir, name) = path.rsplit_once('/').unwrap_or(("", path));
    let (stem, ext) = match name.rsplit_once('.') {
        Some((s, e)) if !s.is_empty() => (s, format!(".{e}")),
        _ => (name, String::new()),
    };
    let template = if stem.ends_with("_test") {
        format!("{VARYING}_test")
    } else if stem.starts_with("test_") {
        format!("test_{VARYING}")
    } else if let Some(k) = [".test", ".spec"].into_iter().find(|k| stem.ends_with(k)) {
        format!("{VARYING}{k}")
    } else {
        VARYING.to_string()
    };
    if dir.is_empty() {
        format!("{template}{ext}")
    } else {
        format!("{dir}/{template}{ext}")
    }
}

/// A path is one a card file stands for: itself, a pattern's shape with its fixed tokens, or a
/// role pattern's directory, test convention and extension.
fn stands_for(file: &str, path: &str) -> bool {
    if file == path {
        return true;
    }
    if file.contains(VARYING) && role(path) == file {
        return true;
    }
    if !file.contains(VARYING) || shape(file).0 != shape(path).0 {
        return false;
    }
    let (f, p) = (file.rsplit_once('/').map_or(file, |x| x.1), path.rsplit_once('/').map_or(path, |x| x.1));
    // `<x>` is one token of the pattern (`flag`, `_`, `<x>`, `.`, `go`): the same shape, token for
    // token, so `flag_<x>.go` does not stand for `flag_url_test.go`.
    let (ft, pt) = (name_tokens(f), name_tokens(p));
    ft.len() == pt.len() && ft.iter().zip(&pt).all(|(a, b)| *a == VARYING || a == b)
}

/// Whether a card may quote a file (review B3): not one that holds secrets by its kind (`.env*`,
/// keys, credential stores), and not one git ignores in the trace's checkout.
fn quotable(path: &str, ignore: &Ignore) -> bool {
    !secrets::sensitive_path(path) && !ignore.ignored(path)
}

/// Whether a command line names a file a card may not quote, or holds a secret itself.
fn names_secret(cmd: &str, ignore: &Ignore) -> bool {
    secrets::has_secret(cmd)
        || cmd.split(|c: char| c.is_whitespace() || "'\"<>|&;=".contains(c)).any(|w| {
            let w = w.trim_start_matches("./");
            !w.is_empty()
                && !w.starts_with('-')
                && (secrets::sensitive_path(w) || (!w.starts_with('/') && ignore.ignored(w)))
        })
}

fn view<'a>(s: &'a Session, ignore: &Ignore) -> View<'a> {
    let root = s.root.to_string_lossy().to_string();
    let mut edits = Vec::new();
    let mut cmds = Vec::new();
    for (k, st) in s.steps.iter().enumerate() {
        let c = &st.call;
        if EDIT_TOOLS.contains(&c.tool.as_str()) {
            let Some(path) = st.rel_path.clone() else { continue };
            if c.failed || !quotable(&path, ignore) {
                continue;
            }
            let i = &c.input;
            let s_ = |v: &serde_json::Value, k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string();
            let text = if c.tool == "MultiEdit" {
                i.get("edits")
                    .and_then(|e| e.as_array())
                    .map(|es| es.iter().map(|e| hunk(&s_(e, "new_string"))).collect::<Vec<_>>().join("\n…\n"))
                    .unwrap_or_default()
            } else {
                hunk(&if c.tool == "Write" { s_(i, "content") } else { s_(i, "new_string") })
            };
            edits.push(EditView { k, path, text, created: c.tool == "Write" });
        } else if c.tool == "Bash" {
            let Some(cmd) = c.input.get("command").and_then(|x| x.as_str()) else { continue };
            let cmd = if root.is_empty() {
                cmd.to_string()
            } else {
                cmd.replace(&format!("{root}/"), "").replace(&root, ".")
            };
            // A command that holds a secret, or reads or writes a file a card may not quote
            // (`echo "KEY=…" > .env`, `cat .env.local`), is left out whole.
            if names_secret(&cmd, ignore) {
                continue;
            }
            let norm = norm_cmd(&cmd);
            // Cards currently carry a command string without a working directory. Only
            // quote an execution known to run at the session root: dropping `cd subdir`
            // would turn a valid generator into advice to run it in the wrong directory.
            let at_root = matches!(&st.norm.action,
                adage_core::CanonicalAction::Exec { cwd, .. } if Path::new(cwd) == s.root);
            if at_root && !norm.is_empty() {
                cmds.push(CmdView { k, norm, failed: c.failed, out: c.output.clone() });
            }
            // A file the command wrote by redirection (`go doc -all . > godoc-current.txt`) is
            // an edit of it, with the command as its text.
            if !c.failed {
                let first = cmd.trim().lines().next().unwrap_or("");
                for path in redirect_targets(first) {
                    edits.push(EditView { k, path, text: first.to_string(), created: false });
                }
            }
        }
    }
    let passed = s.outcome.or_else(|| s.proxy_outcome()) == Some(true);
    View { s, edits, cmds, passed }
}

/// The shapes of the files a trace edited, and the roles of those it created: traces of one family
/// overlap here even when each names its new files after its own task, in part (`flag_url.go`,
/// `flag_port.go`: one shape) or whole (`no_sniff.go`, `hsts.go`: one role).
fn files_of(v: &View) -> BTreeSet<String> {
    let mut out: BTreeSet<String> = v
        .edits
        .iter()
        .map(|e| {
            let (d, n, f, x) = shape(&e.path);
            format!("shape:{d}\u{1f}{n}\u{1f}{f}\u{1f}{x}")
        })
        .collect();
    out.extend(v.edits.iter().filter(|e| e.created).map(|e| format!("role:{}", role(&e.path))));
    out
}

fn jaccard<T: Ord>(a: &BTreeSet<T>, b: &BTreeSet<T>) -> f64 {
    let u = a.union(b).count();
    if u == 0 {
        0.0
    } else {
        a.intersection(b).count() as f64 / u as f64
    }
}

fn median(v: &mut [f64]) -> f64 {
    v.sort_by(|a, b| a.total_cmp(b));
    if v.is_empty() {
        0.0
    } else {
        v[v.len() / 2]
    }
}

/// Text safe for a card's error line: printable, no quotes or shell syntax, bounded.
fn error_line(out: &str) -> String {
    let l = out.lines().map(str::trim).find(|l| !l.is_empty() && !l.starts_with("Exit code")).unwrap_or("");
    let l: String = l.chars().filter(|c| !"`<>\"\\$;|&{}".contains(*c) && !c.is_control()).take(150).collect();
    let l = l.trim().to_string();
    if adage_tracks::io::safe_text(&l) && !secrets::has_secret(&l) {
        l
    } else {
        String::new()
    }
}

/// A command a card may name: plain, no absolute path, no placeholder, nothing secret-looking.
fn plain(c: &str) -> bool {
    adage_tracks::io::safe_command(c) && !c.contains('{') && !secrets::has_secret(c)
}

/// A command worth listing (fresh-install B9): not one that only looks around or reads version
/// control (`git status`, `ls -la`, `cat`, `grep`), nor a commit; what does the work (tests,
/// builds, linters, generators, the project's scripts) is.
fn productive(c: &str) -> bool {
    use adage_core::action::Role;
    let argv: Vec<String> = c.split_whitespace().map(str::to_string).collect();
    !adage_core::pattern::is_housekeeping(c)
        && !matches!(
            adage_core::action::classify_argv(&argv).role,
            Role::Read | Role::Search | Role::Trivial | Role::VcsRead | Role::VcsWrite
        )
}

/// Whether a command runs tests.
fn is_test_run(c: &str) -> bool {
    let argv: Vec<String> = c.split_whitespace().map(str::to_string).collect();
    adage_core::action::classify_argv(&argv).role == adage_core::action::Role::TestRun
}

/// The runner a test command starts with, before its targets and options
/// (`.venv/bin/python -m pytest tests/a.py -x` → `.venv/bin/python -m pytest`): test runs of one
/// family name different tests, and are one command.
fn runner_key(c: &str) -> String {
    let toks: Vec<&str> = c.split_whitespace().collect();
    let mut out: Vec<&str> = toks.iter().take(1).copied().collect();
    let mut i = 1;
    while i < toks.len() {
        let t = toks[i];
        if t == "-m" && i + 1 < toks.len() {
            out.extend([t, toks[i + 1]]);
            i += 2;
            continue;
        }
        if t.starts_with('-') || t.contains(['/', '.', ':', '=']) {
            break;
        }
        out.push(t);
        i += 1;
    }
    out.join(" ")
}

/// The tokens every command of a group begins with (at least its runner).
fn common_prefix(cs: &[String]) -> String {
    let toks: Vec<Vec<&str>> = cs.iter().map(|c| c.split_whitespace().collect()).collect();
    let Some(first) = toks.first() else { return String::new() };
    let n = (0..first.len()).take_while(|&i| toks.iter().all(|t| t.get(i) == Some(&first[i]))).count();
    first[..n].join(" ")
}

/// Learn one family's card (views in chronological order). `None` without a passing trace or a
/// file edited by enough of them.
fn learn(views: &[&View], verdicts: Option<&BTreeMap<String, bool>>) -> Option<Card> {
    let n = views.len();
    let passed = views.iter().filter(|v| v.passed).count();
    if passed == 0 {
        return None;
    }
    // At least two traces (review S8): one trace is any one-off bug fix, and says neither which
    // part of a file name varies nor what the task has in common with the next one (the
    // research's Go family: a one-trace card saved 9%, a four-trace one 34%).
    if n < MIN_TRACES {
        return None;
    }
    let need = n.div_ceil(2);
    // 1. Files, in the median order of their first edit; files named differently each time are
    //    grouped by shape and anti-unified (`flag_<x>.go`).
    let mut per_file: BTreeMap<String, BTreeSet<usize>> = BTreeMap::new();
    let mut first_pos: BTreeMap<String, Vec<f64>> = BTreeMap::new();
    for (i, v) in views.iter().enumerate() {
        let mut seen: Vec<&str> = Vec::new();
        for e in &v.edits {
            if !seen.contains(&e.path.as_str()) {
                seen.push(&e.path);
            }
        }
        for (j, p) in seen.iter().enumerate() {
            per_file.entry(p.to_string()).or_default().insert(i);
            first_pos.entry(p.to_string()).or_default().push(j as f64 / seen.len().max(1) as f64);
        }
    }
    let mut files: Vec<(String, usize, f64)> = per_file
        .iter()
        .filter(|(_, s)| s.len() >= need)
        .map(|(p, s)| (p.clone(), s.len(), median(&mut first_pos[p].clone())))
        .collect();
    if n > 1 {
        let mut groups: BTreeMap<(String, usize, String, String), Vec<&str>> = BTreeMap::new();
        for (p, s) in &per_file {
            if s.len() == 1 {
                groups.entry(shape(p)).or_default().push(p);
            }
        }
        let mut grouped: BTreeSet<&str> = BTreeSet::new();
        for ps in groups.values() {
            let sessions: BTreeSet<usize> = ps.iter().flat_map(|p| per_file[*p].iter().copied()).collect();
            let pat = anti_unify(ps);
            if sessions.len() >= need && pat.contains(VARYING) {
                let mut pos: Vec<f64> = ps.iter().filter_map(|p| first_pos[*p].first().copied()).collect();
                files.push((pat, sessions.len(), median(&mut pos)));
                grouped.extend(ps.iter().copied());
            }
        }
        // Files each task created and named whole after itself (`middleware/no_sniff.go`,
        // `middleware/hsts.go`) share no name shape: grouped by role instead (`middleware/<x>.go`),
        // and only files written whole, so edits to a package's existing files never merge.
        let created: BTreeSet<&str> =
            views.iter().flat_map(|v| v.edits.iter().filter(|e| e.created).map(|e| e.path.as_str())).collect();
        let mut roles: BTreeMap<String, Vec<&str>> = BTreeMap::new();
        for (p, s) in &per_file {
            if s.len() == 1 && created.contains(p.as_str()) && !grouped.contains(p.as_str()) {
                roles.entry(role(p)).or_default().push(p);
            }
        }
        for (pat, ps) in &roles {
            let sessions: BTreeSet<usize> = ps.iter().flat_map(|p| per_file[*p].iter().copied()).collect();
            if ps.len() >= 2 && sessions.len() >= need {
                let mut pos: Vec<f64> = ps.iter().filter_map(|p| first_pos[*p].first().copied()).collect();
                files.push((pat.clone(), sessions.len(), median(&mut pos)));
            }
        }
    }
    files.sort_by(|a, b| a.2.total_cmp(&b.2).then_with(|| a.0.cmp(&b.0)));
    files.truncate(adage_tracks::card::MAX_FILES);
    if files.is_empty() {
        return None;
    }
    // 2. Exemplars, from the most recent passing trace (then the rest, most recent first): the
    //    longest text it added to the file, or to a file of the pattern's shape.
    let order: Vec<&&View> =
        views.iter().rev().filter(|v| v.passed).chain(views.iter().rev().filter(|v| !v.passed)).collect();
    let mut exemplars = Vec::new();
    for (p, _, _) in &files {
        for v in &order {
            let hunks: Vec<&str> = v.edits.iter().filter(|e| stands_for(p, &e.path)).map(|e| e.text.as_str()).collect();
            let Some(h) = hunks.iter().max_by_key(|h| h.lines().count()) else { continue };
            // A line that cannot be quoted (markup, too long) or looks like a secret is elided:
            // one `…` for a run of them.
            let mut lines: Vec<String> = Vec::new();
            for l in h.trim_matches('\n').lines() {
                let l = l.replace('\t', "    ");
                let l: String = l.chars().take(MAX_LINE).collect();
                let l = if adage_tracks::card::line_ok(&l) && !secrets::has_secret(&l) { l } else { "…".to_string() };
                if !(l == "…" && lines.last().is_some_and(|p| p == "…")) {
                    lines.push(l);
                }
            }
            if !lines.is_empty() && !lines.iter().all(|l| l.trim().is_empty() || l == "…") {
                exemplars.push(Exemplar { path: p.clone(), lines });
            }
            break;
        }
    }
    // 3. Commands: those run in at least half of the traces; mostly working, or to avoid.
    #[derive(Default)]
    struct Agg {
        sessions: BTreeSet<usize>,
        runs: u32,
        fails: u32,
        err: String,
    }
    // Only productive commands (fresh-install B9); test runs grouped by their runner, so the
    // runs of one family that name different tests are one command.
    let mut per_cmd: BTreeMap<String, Agg> = BTreeMap::new();
    let mut shown: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (i, v) in views.iter().enumerate() {
        for c in &v.cmds {
            if c.norm.is_empty() || !productive(&c.norm) {
                continue;
            }
            let key = if is_test_run(&c.norm) { runner_key(&c.norm) } else { c.norm.clone() };
            shown.entry(key.clone()).or_default().push(c.norm.clone());
            let a = per_cmd.entry(key).or_default();
            a.sessions.insert(i);
            a.runs += 1;
            if c.failed {
                a.fails += 1;
                if a.err.is_empty() {
                    a.err = error_line(&c.out);
                }
            }
        }
    }
    let verdict = |c: &str| verdicts.and_then(|m| m.get(c).copied());
    let entry = |k: &String, a: &Agg| {
        let command = common_prefix(&shown[k]);
        let verified = verdict(&command);
        CardCommand {
            command,
            sessions: a.sessions.len() as u32,
            runs: a.runs,
            failed: a.fails,
            error: String::new(),
            verified,
        }
    };
    let mut commands: Vec<CardCommand> = Vec::new();
    let mut avoid: Vec<CardCommand> = Vec::new();
    for (k, a) in &per_cmd {
        let c = entry(k, a);
        if a.sessions.len() < need || !plain(&c.command) {
            continue;
        }
        if a.fails * 2 < a.runs {
            commands.push(c);
        } else {
            avoid.push(CardCommand { error: a.err.clone(), verified: None, ..c });
        }
    }
    let by_use = |a: &CardCommand, b: &CardCommand| b.sessions.cmp(&a.sessions).then_with(|| a.command.cmp(&b.command));
    commands.sort_by(by_use);
    avoid.sort_by(by_use);
    commands.truncate(adage_tracks::card::MAX_COMMANDS);
    avoid.truncate(adage_tracks::card::MAX_AVOID);
    // A test run to avoid comes with the one that worked instead (fresh-install B9), even when
    // fewer than half of the traces ran it: the test run the passing traces used most.
    if avoid.iter().any(|x| is_test_run(&x.command)) && !commands.iter().any(|x| is_test_run(&x.command)) {
        let worked = per_cmd
            .iter()
            .filter(|(k, a)| is_test_run(k) && a.fails * 2 < a.runs)
            .map(|(k, a)| entry(k, a))
            .filter(|c| plain(&c.command) && c.verified != Some(false))
            .max_by(|a, b| by_use(b, a));
        if let Some(c) = worked {
            commands.truncate(adage_tracks::card::MAX_COMMANDS - 1);
            commands.insert(0, c);
        }
    }
    // 4. The finishing check: the command passing traces ran successfully after their last edit,
    //    of a head they also ran more than once before it (a verify loop).
    let head2 = |c: &str| c.split_whitespace().take(2).collect::<Vec<_>>().join(" ");
    let mut loops: BTreeSet<String> = BTreeSet::new();
    for v in views {
        let last_edit = v.edits.iter().map(|e| e.k).max().unwrap_or(0);
        let heads: Vec<String> =
            v.cmds.iter().filter(|c| c.k < last_edit && productive(&c.norm)).map(|c| head2(&c.norm)).collect();
        loops.extend(heads.iter().filter(|h| heads.iter().filter(|x| x == h).count() >= 2).cloned());
    }
    let mut finals: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for v in views.iter().filter(|v| v.passed) {
        let last_edit = v.edits.iter().map(|e| e.k).max().unwrap_or(0);
        let mut seen = BTreeSet::new();
        for c in v
            .cmds
            .iter()
            .filter(|c| c.k > last_edit && !c.failed && productive(&c.norm) && loops.contains(&head2(&c.norm)))
        {
            if seen.insert(head2(&c.norm)) {
                finals.entry(head2(&c.norm)).or_default().push(c.norm.clone());
            }
        }
    }
    let finish = finals
        .iter()
        .max_by(|a, b| a.1.len().cmp(&b.1.len()).then_with(|| b.0.cmp(a.0)))
        .and_then(|(_, cs)| {
            // Anti-unified token by token: tokens every passing trace agreed on, `<…>` for the
            // others; nothing when half of it or more differs.
            let toks: Vec<Vec<&str>> = cs.iter().map(|c| c.split_whitespace().collect()).collect();
            let m = toks.iter().map(|t| t.len()).min().unwrap_or(0);
            let f: Vec<&str> =
                (0..m).map(|i| if toks.iter().all(|t| t[i] == toks[0][i]) { toks[0][i] } else { "<…>" }).collect();
            let varying = f.iter().filter(|t| **t == "<…>").count();
            (!f.is_empty() && varying * 2 < f.len()).then(|| f.join(" "))
        })
        .filter(|f| plain(&f.replace("<…>", "x")) && verdict(f) != Some(false));
    // A command the replay found not to work on a clean checkout is not listed as working.
    let base = Card {
        sessions: n as u32,
        passed: passed as u32,
        files: files.iter().map(|(p, k, _)| CardFile { path: p.clone(), sessions: *k as u32 }).collect(),
        exemplars: vec![],
        commands,
        avoid,
        finish,
        prompts: vec![],
    };
    // The budget (800 tokens): exemplars clipped to 14 lines, then 10, 6, 3, then none.
    for k in [MAX_EXEMPLAR_LINES, 10, 6, 3, 0] {
        let clipped = exemplars
            .iter()
            .filter(|_| k > 0)
            .map(|e| {
                let mut lines = e.lines.clone();
                if lines.len() > k {
                    lines.truncate(k - 1);
                    lines.push("…".into());
                }
                Exemplar { path: e.path.clone(), lines }
            })
            .collect();
        let card = Card { exemplars: clipped, ..base.clone() };
        if adage_tracks::card::check(&card).is_ok() {
            return Some(card);
        }
    }
    None
}

/// A card's id: stable across relearns as the family grows (from its earliest trace).
fn card_id(first: &str) -> String {
    let h = Sha256::digest(first.as_bytes());
    format!("card-{}", h.iter().take(5).map(|b| format!("{b:02x}")).collect::<String>())
}

/// Families and their cards for one repository's sessions (see the module documentation).
/// Returns the card routes, each with why, and the number of families found.
pub fn compile(ss: &[&Session], model: &StaticModel, p: &CardParams, provenance: &str) -> (Vec<(Route, Why)>, usize) {
    // Each checkout's `.gitignore` rules, read once.
    let mut ignores: BTreeMap<&Path, Ignore> = BTreeMap::new();
    for s in ss {
        ignores.entry(s.root.as_path()).or_insert_with(|| Ignore::new(&s.root));
    }
    let mut items: Vec<(View, Vec<f32>)> = ss
        .iter()
        .filter_map(|s| {
            let v = view(s, &ignores[s.root.as_path()]);
            if v.edits.is_empty() {
                return None;
            }
            let e = model.embed(&normalize(&s.prompt))?;
            Some((v, e))
        })
        .collect();
    // Chronological, then by id: the order exemplars and ids are read in.
    items.sort_by(|a, b| (&a.0.s.day, &a.0.s.id).cmp(&(&b.0.s.day, &b.0.s.id)));
    let n = items.len();
    let files: Vec<_> = items.iter().map(|(v, _)| files_of(v)).collect();
    let mut parent: Vec<usize> = (0..n).collect();
    fn find(p: &mut [usize], i: usize) -> usize {
        let mut r = i;
        while p[r] != r {
            r = p[r];
        }
        let mut i = i;
        while p[i] != r {
            let next = p[i];
            p[i] = r;
            i = next;
        }
        r
    }
    for i in 0..n {
        for j in i + 1..n {
            let sim = adage_intent::cosine(&items[i].1, &items[j].1);
            if sim >= p.family_similarity && jaccard(&files[i], &files[j]) >= p.file_overlap {
                let (a, b) = (find(&mut parent, i), find(&mut parent, j));
                if a != b {
                    parent[a.max(b)] = a.min(b);
                }
            }
        }
    }
    let mut groups: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for i in 0..n {
        let r = find(&mut parent, i);
        groups.entry(r).or_default().push(i);
    }
    let mut out = Vec::new();
    for members in groups.values() {
        let views: Vec<&View> = members.iter().map(|&i| &items[i].0).collect();
        let id = card_id(&views[0].s.id);
        let Some(mut card) = learn(&views, p.verdicts.get(&id)) else { continue };
        // The family's prompts, most recent first: every trace that joined the family adds its
        // own phrasing, so the library grows with the sessions that matched.
        card.prompts = members.iter().rev().take(MAX_PROMPTS).map(|&i| adage_intent::quantize(&items[i].1)).collect();
        card.prompts.dedup();
        let t = if members.len() == 1 { p.threshold - SINGLE_PROMPT_DROP } else { p.threshold };
        let threshold = t.clamp(INTENT_MIN_THRESHOLD, INTENT_MAX_THRESHOLD);
        let dim = items[members[0]].1.len();
        let mut mean = vec![0f32; dim];
        for &i in members {
            for (m, x) in mean.iter_mut().zip(&items[i].1) {
                *m += x;
            }
        }
        let centroid = adage_intent::quantize(&adage_intent::unit(mean));
        let passed = card.passed;
        let route = Route {
            id: id.clone(),
            summary: "What earlier sessions of this family of tasks had in common".into(),
            trigger: Trigger::Intent { family: id.clone(), model: model.id.clone(), centroid, threshold },
            step: Step::Card { card },
            slots: vec![],
            effect: adage_core::Effect::Observe,
            strength: Strength::Prime,
            state: RouteState::Forming,
            exit: vec![Exit::IntentChanges],
            evidence: Evidence {
                opportunities: members.len() as u32,
                support: passed,
                days: views.iter().map(|v| v.s.day.as_str()).collect::<BTreeSet<_>>().len() as u32,
                rate: passed as f64 / members.len() as f64,
                provenance: provenance.to_string(),
                ..Default::default()
            },
            authored: false,
        };
        let probe = Track {
            manifest: Manifest {
                track: TrackInfo {
                    name: "probe/learned".into(),
                    version: "0.0.0".into(),
                    description: String::new(),
                    provenance: String::new(),
                },
                scope: Scope { repo: String::new() },
                gauge: Gauge::default(),
            },
            routes: vec![route.clone()],
        };
        if io::validate(&probe).is_err() {
            continue;
        }
        let why = Why {
            shape: "card".into(),
            pattern: format!("[prompt ~ {id} (τ {threshold:.2})] → [card of {} trace(s)]", members.len()),
            backoff: 1,
            gain: 0.0,
            lift: 0.0,
            conf: (route.evidence.rate * 1e4).round() / 1e4,
            conf_lb: 0.0,
            support: passed,
            days: route.evidence.days,
            members: vec![],
            examples: views.iter().take(3).map(|v| v.s.id.clone()).collect(),
            prior: None,
        };
        out.push((route, why));
    }
    // The best supported cards, then by id: deterministic.
    out.sort_by(|a, b| {
        (b.0.evidence.support, b.0.evidence.opportunities)
            .cmp(&(a.0.evidence.support, a.0.evidence.opportunities))
            .then_with(|| a.0.id.cmp(&b.0.id))
    });
    out.truncate(MAX_CARDS);
    let families = groups.len();
    (out, families)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commands_are_normalised_to_their_head() {
        assert_eq!(
            norm_cmd("cd /r && python -m pytest tests/test_v.py::test_x -xvs 2>&1 | tail -20"),
            "python -m pytest tests/test_v.py -xvs"
        );
        assert_eq!(norm_cmd("make test > out.txt"), "make test");
        assert_eq!(norm_cmd("cat <<EOF > x\nhello\nEOF"), "cat");
        // Quotes are words: an inline script is no command to repeat, a quoted path is.
        assert_eq!(norm_cmd("python -c \"import sys; print(1)\""), "");
        assert_eq!(norm_cmd("python -c 'import sys'"), "");
        assert_eq!(norm_cmd("python -c \"\nimport sys\nprint(1)\""), "");
        assert_eq!(norm_cmd("grep -n 'class ' src/v.py"), "");
        assert_eq!(norm_cmd("wc -l \"tests/test_v.py\""), "wc -l tests/test_v.py");
        assert_eq!(norm_cmd("echo 'a>b'"), "echo a>b");
    }

    #[test]
    fn redirects_are_writes_and_names_anti_unify_by_shape() {
        assert_eq!(redirect_targets("go doc -all . > godoc-current.txt"), ["godoc-current.txt"]);
        assert_eq!(redirect_targets("make gen >> ./out/api.txt 2>&1"), ["out/api.txt"]);
        assert!(redirect_targets("x 2>&1 >/dev/null").is_empty());
        assert!(redirect_targets("x > /tmp/a && y > ../b && z > $F").is_empty());
        assert!(redirect_targets("cat <<EOF").is_empty());
        assert_eq!(anti_unify(&["flag_url.go", "flag_port.go"]), "flag_<x>.go");
        assert_eq!(anti_unify(&["tests/test_hex.py", "tests/test_port.py"]), "tests/test_<x>.py");
        assert_ne!(shape("flag_url.go"), shape("flag_url_test.go"), "a test sibling is its own role");
        assert!(stands_for("flag_<x>.go", "flag_ip.go") && !stands_for("flag_<x>.go", "flag_ip_test.go"));
        assert!(stands_for("a/b.py", "a/b.py") && !stands_for("a/b.py", "a/c.py"));
    }

    /// Files each task names whole after itself (chi's `middleware/no_sniff.go`, `hsts.go`) share
    /// no shape; their role (directory, test convention, extension) is what recurs.
    #[test]
    fn a_created_file_named_after_its_task_has_a_role() {
        assert_ne!(shape("middleware/no_sniff.go"), shape("middleware/hsts.go"));
        assert_eq!(role("middleware/no_sniff.go"), "middleware/<x>.go");
        assert_eq!(role("middleware/hsts.go"), "middleware/<x>.go");
        assert_eq!(role("middleware/hsts_test.go"), "middleware/<x>_test.go");
        assert_eq!(role("tests/test_port.py"), "tests/test_<x>.py");
        assert_eq!(role("src/utils.test.ts"), "src/<x>.test.ts");
        assert_eq!(role("Makefile"), "<x>");
        assert!(stands_for("middleware/<x>.go", "middleware/real_ip.go"));
        assert!(!stands_for("middleware/<x>.go", "middleware/real_ip_test.go"));
        assert!(!stands_for("middleware/<x>.go", "other/real_ip.go"));
    }

    #[test]
    fn an_edit_is_quoted_as_a_whole_hunk() {
        // Fresh-install B9: picking the added lines out made `if …:` without its body.
        assert_eq!(hunk("\nif x:\n    return 1\n"), "if x:\n    return 1");
    }

    /// Fresh-install B9: listed commands do the work; a family's test runs are one command.
    #[test]
    fn commands_are_productive_and_test_runs_group_by_runner() {
        for c in ["git status", "ls -la", "cat src/a.py", "grep -n x src", "git log --oneline -5", "git commit -m x"] {
            assert!(!productive(c), "{c}");
        }
        for c in ["make check", ".venv/bin/python -m pytest tests", "go test ./...", "npm run build"] {
            assert!(productive(c), "{c}");
        }
        assert_eq!(runner_key(".venv/bin/python -m pytest tests/test_a.py -xvs"), ".venv/bin/python -m pytest");
        assert_eq!(runner_key("go test ./..."), "go test");
        assert_eq!(runner_key("make check"), "make check");
        let cs = [".venv/bin/python -m pytest tests/a.py -q", ".venv/bin/python -m pytest tests/b.py -q"];
        assert_eq!(common_prefix(&cs.map(String::from)), ".venv/bin/python -m pytest");
        assert_eq!(common_prefix(&["go test ./...".to_string(), "go test ./...".to_string()]), "go test ./...");
    }

    /// Review B3: nothing a card may not quote reaches it.
    #[test]
    fn secrets_and_secret_files_are_not_quotable() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join(".gitignore"), "local_settings.py\n/build/\n").unwrap();
        let ig = Ignore::new(d.path());
        for p in [".env", "config/.env.local", "certs/key.pem", "local_settings.py", "build/out.js"] {
            assert!(!quotable(p, &ig), "{p}");
        }
        assert!(quotable("src/settings.py", &ig));
        let token = format!("ghp_{}", "A1b2C3d4E5".repeat(4));
        for c in [
            format!("GITHUB_TOKEN={token} gh pr create"),
            "echo \"API_KEY=abcdefgh12345678\" > .env".to_string(),
            "cat .env.local".to_string(),
            "cp local_settings.py /tmp/x".to_string(),
        ] {
            assert!(names_secret(&c, &ig), "{c}");
        }
        assert!(!names_secret("python -m pytest tests -q", &ig));
        assert!(error_line(&format!("error: bad credentials {token}")).is_empty());
        assert!(!plain(&format!("curl -H token:{token} x")));
    }
}
