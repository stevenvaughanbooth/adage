//! What a failing pytest test's own source says about it: where its function is, which
//! parametrized case its node id names (`test_x[bad12]` is `value = [45, 90]`), and its check when
//! it has exactly one. A failed run's receipt gives these beside the failing test's name, instead
//! of pytest's failure messages: those are the run's output, which Adage never passes to the
//! agent (docs/execution.md).
//!
//! Pure text; nothing runs. A small reader of Python's lexical structure (strings, comments,
//! brackets, logical lines) finds the test's `def`, its decorators and its body. The literal
//! values of its one `@pytest.mark.parametrize` are read, and pytest's own id rule is followed
//! only where it is certain: strings of printable ASCII, numbers, booleans and `None` by value,
//! containers by `argname<index>`, and explicit ids. Anything else names no case: never a guess.
//! Quoted source is data, checked as the orientation card's lines are ([`crate::orient::shown`]).

use std::collections::{BTreeMap, BTreeSet};

/// A shown parameter value is cut to this many characters.
const VALUE: usize = 100;
/// A test body longer than this many statements is not read for its check.
const BODY: usize = 400;
/// A parametrize list with more entries than this names no case.
const ENTRIES: usize = 10_000;
/// Values nested deeper than this are not read.
const NESTING: usize = 32;

/// Facts about one test from its source ([`facts`]). Lines count from 1.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CaseFacts {
    /// The line of the test function's `def`.
    pub def_line: usize,
    /// The parametrized case the node id names: the line its entry starts on and, when it can be
    /// shown, `argname = value` (several argnames: `a = 1, b = 'x'`).
    pub case: Option<(usize, Option<String>)>,
    /// The line of the test's `parametrize` decorator, when the id is parametrized but its case
    /// could not be identified.
    pub cases_line: Option<usize>,
    /// The test's only check (one `assert`, `pytest.raises`/`warns`/`fail` or `self.assert…` in its
    /// body): its line and the line's text.
    pub check: Option<(usize, String)>,
}

/// The facts `source` (a test file's text) gives about the test `nodeid` names
/// (`path.py::[Class::]name[params]`), or `None` when its function cannot be found.
pub fn facts(source: &str, nodeid: &str) -> Option<CaseFacts> {
    TestSource::new(source)?.facts(nodeid)
}

/// A test file's text, read once for the facts about any number of its tests.
pub struct TestSource<'a> {
    text: &'a str,
    lines: Vec<Logical>,
}

impl<'a> TestSource<'a> {
    /// `None` when the text is not one this reader can follow (an unterminated string).
    pub fn new(text: &'a str) -> Option<TestSource<'a>> {
        Some(TestSource { text, lines: lex(text)? })
    }

    /// See [`facts`].
    pub fn facts(&self, nodeid: &str) -> Option<CaseFacts> {
        test_facts(self.text, &self.lines, nodeid)
    }
}

fn test_facts(source: &str, lines: &[Logical], nodeid: &str) -> Option<CaseFacts> {
    let (_, rest) = nodeid.split_once("::")?;
    let (path, params) = match rest.find('[') {
        Some(i) if rest.ends_with(']') => (&rest[..i], Some(&rest[i + 1..rest.len() - 1])),
        Some(_) => return None,
        None => (rest, None),
    };
    let mut scopes: Vec<&str> = path.split("::").collect();
    let func = scopes.pop()?;
    if !ident(func) || !scopes.iter().all(|c| ident(c)) {
        return None;
    }
    let mut scope: &[Logical] = lines;
    let mut indent = 0;
    let mut class_marked = false;
    for class in &scopes {
        let k = last_def(scope, indent, "class", class)?;
        class_marked |= decorators(scope, k).iter().any(|d| parametrize(d).is_some());
        scope = body(scope, k);
        indent = scope.first()?.indent;
    }
    let k = last_def(scope, indent, "def", func)?;
    let mut out = CaseFacts { def_line: scope[k].line(), ..Default::default() };
    out.check = check(body(scope, k), source);
    let Some(p) = params else { return Some(out) };
    let marks: Vec<&Logical> = decorators(scope, k).into_iter().filter(|d| parametrize(d).is_some()).collect();
    // A `pytestmark` parametrization, at module or class level, adds to every id.
    let module_marked =
        lines.iter().any(|l| l.toks[0].is_name("pytestmark") && l.toks.iter().any(|t| t.is_name("parametrize")));
    match marks.as_slice() {
        [one] if !class_marked && !module_marked => match case(one, p, lines, source) {
            Some(c) => out.case = Some(c),
            None => out.cases_line = Some(one.line()),
        },
        [first, ..] => out.cases_line = Some(first.line()),
        [] => {}
    }
    Some(out)
}

fn ident(s: &str) -> bool {
    let mut cs = s.chars();
    cs.next().is_some_and(|c| c.is_alphabetic() || c == '_') && cs.all(|c| c.is_alphanumeric() || c == '_')
}

// ---------------------------------------------------------------------------------------------
// Lexical structure

#[derive(Debug, Clone, PartialEq)]
enum Kind {
    Name,
    Number,
    /// A string literal: its prefix (lowercased) and its body between the quotes, unescaped.
    Str {
        prefix: String,
        body: String,
    },
    Op,
}

#[derive(Debug, Clone)]
struct Tok {
    kind: Kind,
    text: String,
    line: usize,
    /// Byte offsets of the token in the source.
    start: usize,
    end: usize,
}

impl Tok {
    fn is_name(&self, n: &str) -> bool {
        self.kind == Kind::Name && self.text == n
    }
    fn is_op(&self, o: &str) -> bool {
        self.kind == Kind::Op && self.text == o
    }
}

/// A logical line: its tokens (never empty) and the indentation of the physical line it starts on.
#[derive(Debug, Clone)]
struct Logical {
    toks: Vec<Tok>,
    indent: usize,
}

impl Logical {
    fn line(&self) -> usize {
        self.toks[0].line
    }
}

const TWO_CHAR_OPS: &[&str] =
    &["==", "!=", "<=", ">=", "->", "**", "//", ":=", "<<", ">>", "+=", "-=", "*=", "/=", "%=", "&=", "|=", "^=", "@="];

/// `source` as logical lines, or `None` for what this reader cannot follow (an unterminated
/// string).
fn lex(source: &str) -> Option<Vec<Logical>> {
    let cs: Vec<(usize, char)> = source.char_indices().collect();
    let at = |i: usize| cs.get(i).map(|x| x.1);
    let off = |i: usize| cs.get(i).map_or(source.len(), |x| x.0);
    let starts: Vec<&str> = source.split('\n').collect();
    let indent = |line: usize| {
        let mut w = 0;
        for c in starts.get(line - 1).copied().unwrap_or_default().chars() {
            match c {
                ' ' => w += 1,
                '\t' => w = (w / 8 + 1) * 8,
                '\x0c' => w = 0,
                _ => break,
            }
        }
        w
    };
    let (mut i, mut line, mut depth) = (0usize, 1usize, 0usize);
    let mut out: Vec<Logical> = Vec::new();
    let mut cur: Vec<Tok> = Vec::new();
    let push = |cur: &mut Vec<Tok>, out: &mut Vec<Logical>| {
        if let Some(first) = cur.first() {
            let indent = indent(first.line);
            out.push(Logical { toks: std::mem::take(cur), indent });
        }
    };
    while let Some(c) = at(i) {
        let tok_line = line;
        match c {
            '\n' => {
                if depth == 0 {
                    push(&mut cur, &mut out);
                }
                line += 1;
                i += 1;
            }
            '#' => {
                while at(i).is_some_and(|c| c != '\n') {
                    i += 1;
                }
            }
            '\\' if at(i + 1) == Some('\n') => {
                line += 1;
                i += 2;
            }
            '\\' if at(i + 1) == Some('\r') && at(i + 2) == Some('\n') => {
                line += 1;
                i += 3;
            }
            c if c.is_whitespace() => i += 1,
            '\'' | '"' => {
                let (tok, next, lines) = string(source, &cs, i, i)?;
                cur.push(Tok { line: tok_line, ..tok });
                line += lines;
                i = next;
            }
            c if c.is_alphabetic() || c == '_' => {
                let mut j = i;
                while at(j).is_some_and(|c| c.is_alphanumeric() || c == '_') {
                    j += 1;
                }
                let word = &source[off(i)..off(j)];
                let prefix =
                    matches!(word.to_ascii_lowercase().as_str(), "r" | "u" | "b" | "f" | "br" | "rb" | "fr" | "rf");
                if prefix && matches!(at(j), Some('\'' | '"')) {
                    let (tok, next, lines) = string(source, &cs, i, j)?;
                    cur.push(Tok { line: tok_line, ..tok });
                    line += lines;
                    i = next;
                } else {
                    cur.push(Tok { kind: Kind::Name, text: word.into(), line, start: off(i), end: off(j) });
                    i = j;
                }
            }
            c if c.is_ascii_digit() || (c == '.' && at(i + 1).is_some_and(|d| d.is_ascii_digit())) => {
                let mut j = i;
                let hex = c == '0' && matches!(at(i + 1), Some('x' | 'X'));
                while let Some(d) = at(j) {
                    let sign = matches!(d, '+' | '-') && !hex && j > i && matches!(at(j - 1), Some('e' | 'E'));
                    if d.is_ascii_alphanumeric() || d == '_' || d == '.' || sign {
                        j += 1;
                    } else {
                        break;
                    }
                }
                cur.push(Tok {
                    kind: Kind::Number,
                    text: source[off(i)..off(j)].into(),
                    line,
                    start: off(i),
                    end: off(j),
                });
                i = j;
            }
            _ => {
                let two: String = [Some(c), at(i + 1)].into_iter().flatten().collect();
                let n = if TWO_CHAR_OPS.contains(&two.as_str()) { 2 } else { 1 };
                match c {
                    '(' | '[' | '{' => depth += 1,
                    ')' | ']' | '}' => depth = depth.saturating_sub(1),
                    _ => {}
                }
                cur.push(Tok {
                    kind: Kind::Op,
                    text: source[off(i)..off(i + n)].into(),
                    line,
                    start: off(i),
                    end: off(i + n),
                });
                i += n;
            }
        }
    }
    push(&mut cur, &mut out);
    Some(out)
}

/// The string literal whose prefix starts at `from` and whose quote is at `q`: the token, the index
/// after it and the newlines it spans; `None` when it is not terminated.
fn string(source: &str, cs: &[(usize, char)], from: usize, q: usize) -> Option<(Tok, usize, usize)> {
    let at = |i: usize| cs.get(i).map(|x| x.1);
    let off = |i: usize| cs.get(i).map_or(source.len(), |x| x.0);
    let quote = at(q)?;
    let triple = at(q + 1) == Some(quote) && at(q + 2) == Some(quote);
    let open = if triple { q + 3 } else { q + 1 };
    let (mut j, mut lines) = (open, 0usize);
    loop {
        match at(j)? {
            '\\' => {
                if at(j + 1) == Some('\n') {
                    lines += 1;
                }
                j += 2;
            }
            '\n' if !triple => return None,
            '\n' => {
                lines += 1;
                j += 1;
            }
            c if c == quote && (!triple || (at(j + 1) == Some(quote) && at(j + 2) == Some(quote))) => break,
            _ => j += 1,
        }
    }
    let close = if triple { j + 3 } else { j + 1 };
    let tok = Tok {
        kind: Kind::Str {
            prefix: source[off(from)..off(q)].to_ascii_lowercase(),
            body: source[off(open)..off(j)].into(),
        },
        text: source[off(from)..off(close)].into(),
        line: 0,
        start: off(from),
        end: off(close),
    };
    Some((tok, close, lines))
}

/// The index of the bracket closing the one at `open`.
fn close_of(toks: &[Tok], open: usize) -> Option<usize> {
    let mut depth = 0usize;
    for (i, t) in toks.iter().enumerate().skip(open) {
        if t.kind != Kind::Op {
            continue;
        }
        match t.text.as_str() {
            "(" | "[" | "{" => depth += 1,
            ")" | "]" | "}" => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
    }
    None
}

/// `toks` split at `sep` outside brackets. A trailing separator leaves a last empty part.
fn split_top<'a>(toks: &'a [Tok], sep: &str) -> Vec<&'a [Tok]> {
    let mut parts = Vec::new();
    let (mut depth, mut from) = (0usize, 0usize);
    for (i, t) in toks.iter().enumerate() {
        if t.kind != Kind::Op {
            continue;
        }
        match t.text.as_str() {
            "(" | "[" | "{" => depth += 1,
            ")" | "]" | "}" => depth = depth.saturating_sub(1),
            s if s == sep && depth == 0 => {
                parts.push(&toks[from..i]);
                from = i + 1;
            }
            _ => {}
        }
    }
    parts.push(&toks[from..]);
    parts
}

// ---------------------------------------------------------------------------------------------
// Definitions

/// The last `kw name` (`class` or `def`, `async def` too) at `indent` in `scope`.
fn last_def(scope: &[Logical], indent: usize, kw: &str, name: &str) -> Option<usize> {
    scope.iter().rposition(|l| {
        let t = &l.toks;
        let t = if kw == "def" && t[0].is_name("async") { &t[1..] } else { &t[..] };
        l.indent == indent && t.len() > 1 && t[0].is_name(kw) && t[1].is_name(name)
    })
}

/// The logical lines of `scope[k]`'s body (indented under it).
fn body(scope: &[Logical], k: usize) -> &[Logical] {
    let indent = scope[k].indent;
    let end = scope[k + 1..].iter().position(|l| l.indent <= indent).map_or(scope.len(), |p| k + 1 + p);
    &scope[k + 1..end]
}

/// The decorators directly above `scope[k]`.
fn decorators(scope: &[Logical], k: usize) -> Vec<&Logical> {
    let indent = scope[k].indent;
    let mut out: Vec<&Logical> =
        scope[..k].iter().rev().take_while(|l| l.indent == indent && l.toks[0].is_op("@")).collect();
    out.reverse();
    out
}

/// A `@pytest.mark.parametrize(…)` (or `@mark.parametrize(…)`) decorator's arguments.
fn parametrize(d: &Logical) -> Option<&[Tok]> {
    let t = &d.toks;
    let open = t.iter().position(|x| x.is_op("("))?;
    let dotted: Vec<&str> = t[1..open].iter().map(|x| x.text.as_str()).collect();
    let named = matches!(dotted.as_slice(), ["pytest", ".", "mark", ".", "parametrize"] | ["mark", ".", "parametrize"]);
    (named && close_of(t, open) == Some(t.len() - 1)).then(|| &t[open + 1..t.len() - 1])
}

/// The body's one check, quoted, or `None` when it has none or several.
fn check(body: &[Logical], source: &str) -> Option<(usize, String)> {
    if body.len() > BODY {
        return None;
    }
    let mut found: Vec<&Logical> = Vec::new();
    for l in body {
        let t = &l.toks;
        let mut n = usize::from(t[0].is_name("assert"));
        n += t
            .windows(4)
            .filter(|w| {
                w[0].is_name("pytest")
                    && w[1].is_op(".")
                    && ["raises", "warns", "fail"].iter().any(|n| w[2].is_name(n))
                    && w[3].is_op("(")
            })
            .count();
        n += t
            .windows(3)
            .filter(|w| {
                w[0].is_op(".") && w[1].kind == Kind::Name && w[1].text.starts_with("assert") && w[2].is_op("(")
            })
            .count();
        found.extend(std::iter::repeat_n(l, n));
        if found.len() > 1 {
            return None;
        }
    }
    let [l] = found[..] else { return None };
    // The whole statement, on one line.
    let (a, b) = (l.toks.first()?, l.toks.last()?);
    let text = source[a.start..b.end].split_whitespace().collect::<Vec<_>>().join(" ");
    Some((l.line(), quotable(&text)?))
}

/// A source line as a receipt may quote it: the orientation card's checks
/// ([`crate::orient::shown`]), no text Adage's own markers use, and nothing [`crate::render::sanitise`]
/// would remove (double quotes become single quotes when the line has none).
fn quotable(text: &str) -> Option<String> {
    let text = if text.contains('\'') { text.to_string() } else { text.replace('"', "'") };
    let shown = crate::orient::shown(&text)?;
    safe(&shown).then_some(shown)
}

/// Text that reads the same after [`crate::render::sanitise`] and carries none of Adage's own
/// markers.
fn safe(s: &str) -> bool {
    let reserved = ["adage-", crate::render::FINISH_LINE, crate::render::NOT_FROM_USER];
    plain(s) && !reserved.iter().any(|r| s.contains(r))
}

/// Whether [`crate::render::sanitise`] leaves `s`'s characters as they are.
fn plain(s: &str) -> bool {
    s.chars().all(|c| {
        !matches!(c, '`' | '<' | '>' | '"' | '\\' | '\u{85}' | '\u{2028}' | '\u{2029}')
            && !c.is_control()
            && !adage_tracks::io::invisible(c)
    })
}

// ---------------------------------------------------------------------------------------------
// Values and pytest ids

/// A literal value as far as it is read: what pytest's id rule and the receipt need.
#[derive(Debug, Clone, PartialEq)]
enum Val {
    Str(String),
    Int(String),
    Float(f64),
    Bool(bool),
    None,
    /// A tuple (`(`), list (`[`) or set (`{`) display.
    Seq(char, Vec<Val>),
    Dict(Vec<(Val, Val)>),
    /// `pytest.param(…)`: its values and its `id`.
    Param(Vec<Val>, IdSpec),
    /// Any other expression, by its source text.
    Other(String),
}

#[derive(Debug, Clone, PartialEq)]
enum IdSpec {
    /// pytest generates it.
    Generated,
    Given(String),
    /// Given, but not as a literal this reader can read.
    Unknown,
}

fn value(toks: &[Tok], source: &str) -> Val {
    nested(toks, source, 0)
}

/// [`value`] at a nesting depth: deeper than [`NESTING`], any expression is read as its text.
fn nested(toks: &[Tok], source: &str, depth: usize) -> Val {
    let value = |toks: &[Tok], source: &str| nested(toks, source, depth + 1);
    let other = || match (toks.first(), toks.last()) {
        (Some(a), Some(b)) => Val::Other(source[a.start..b.end].to_string()),
        _ => Val::Other(String::new()),
    };
    let Some(first) = toks.first() else { return other() };
    if depth > NESTING {
        return other();
    }
    if toks.iter().all(|t| matches!(t.kind, Kind::Str { .. })) {
        // Adjacent literals concatenate. Escapes, f-strings and bytes are not read.
        let mut s = String::new();
        for t in toks {
            let Kind::Str { prefix, body } = &t.kind else { return other() };
            if body.contains('\\') || !matches!(prefix.as_str(), "" | "u" | "r") {
                return other();
            }
            s.push_str(body);
        }
        return Val::Str(s);
    }
    match toks {
        [t] if t.is_name("None") => return Val::None,
        [t] if t.is_name("True") => return Val::Bool(true),
        [t] if t.is_name("False") => return Val::Bool(false),
        [t] if t.kind == Kind::Number => return number(&t.text, false).unwrap_or_else(other),
        [s, t] if (s.is_op("-") || s.is_op("+")) && t.kind == Kind::Number => {
            return number(&t.text, s.is_op("-")).unwrap_or_else(other)
        }
        _ => {}
    }
    if first.kind == Kind::Op
        && matches!(first.text.as_str(), "(" | "[" | "{")
        && close_of(toks, 0) == Some(toks.len() - 1)
    {
        let open = first.text.chars().next().unwrap_or('(');
        let Some(mut parts) = items(&toks[1..toks.len() - 1]) else { return other() };
        let trailing = parts.last().is_some_and(|p| p.is_empty());
        if trailing {
            parts.pop();
        }
        if open == '(' && parts.len() == 1 && !trailing {
            return value(parts[0], source);
        }
        if open == '{' && parts.first().is_some_and(|p| split_top(p, ":").len() == 2) {
            let mut pairs = Vec::new();
            for p in &parts {
                let kv = split_top(p, ":");
                let [k, v] = kv.as_slice() else { return other() };
                pairs.push((value(k, source), value(v, source)));
            }
            return Val::Dict(pairs);
        }
        if open == '{' && parts.is_empty() {
            return Val::Dict(Vec::new());
        }
        return Val::Seq(open, parts.iter().map(|p| value(p, source)).collect());
    }
    // `pytest.param(…)` or `param(…)`.
    let call = match toks {
        [a, d, b, o, ..] if a.is_name("pytest") && d.is_op(".") && b.is_name("param") && o.is_op("(") => Some(3),
        [b, o, ..] if b.is_name("param") && o.is_op("(") => Some(1),
        _ => None,
    };
    if let Some(open) = call.filter(|o| close_of(toks, *o) == Some(toks.len() - 1)) {
        let Some(mut parts) = items(&toks[open + 1..toks.len() - 1]) else { return other() };
        if parts.last().is_some_and(|p| p.is_empty()) {
            parts.pop();
        }
        let mut values = Vec::new();
        let mut id = IdSpec::Generated;
        for p in parts {
            match keyword(p) {
                Some(("id", v)) => {
                    id = match value(v, source) {
                        Val::None => IdSpec::Generated,
                        Val::Str(s) => IdSpec::Given(s),
                        _ => IdSpec::Unknown,
                    }
                }
                Some(_) => {}
                None => values.push(value(p, source)),
            }
        }
        return Val::Param(values, id);
    }
    other()
}

/// A display's or call's comma-separated items; `None` for a comprehension or an unpacked item.
fn items(inner: &[Tok]) -> Option<Vec<&[Tok]>> {
    let parts = split_top(inner, ",");
    if parts.len() == 1 && parts[0].is_empty() {
        return Some(Vec::new());
    }
    let comprehension = parts.iter().any(|p| depth0(p).any(|t| t.is_name("for")));
    let unpacked = parts.iter().any(|p| p.first().is_some_and(|t| t.is_op("*") || t.is_op("**")));
    (!comprehension && !unpacked).then_some(parts)
}

/// The tokens of `toks` outside any bracket.
fn depth0(toks: &[Tok]) -> impl Iterator<Item = &Tok> {
    let mut depth = 0usize;
    toks.iter().filter(move |t| {
        let top = depth == 0;
        if t.kind == Kind::Op {
            match t.text.as_str() {
                "(" | "[" | "{" => depth += 1,
                ")" | "]" | "}" => depth = depth.saturating_sub(1),
                _ => {}
            }
        }
        top && !(t.kind == Kind::Op && matches!(t.text.as_str(), "(" | "[" | "{"))
    })
}

/// `name=value` in a call's arguments.
fn keyword(p: &[Tok]) -> Option<(&str, &[Tok])> {
    match p {
        [n, eq, rest @ ..] if n.kind == Kind::Name && eq.is_op("=") => Some((n.text.as_str(), rest)),
        _ => None,
    }
}

/// A number literal, signed: a decimal integer or a float. `None` for others (hex, complex, …).
fn number(text: &str, negative: bool) -> Option<Val> {
    let t = text.replace('_', "");
    if !t.is_empty() && t.chars().all(|c| c.is_ascii_digit()) {
        let digits = t.trim_start_matches('0');
        let digits = if digits.is_empty() { "0" } else { digits };
        if t.len() > 1 && t.starts_with('0') && digits != "0" {
            return None; // not Python
        }
        return Some(Val::Int(if negative && digits != "0" { format!("-{digits}") } else { digits.to_string() }));
    }
    if t.chars().all(|c| c.is_ascii_digit() || matches!(c, '.' | 'e' | 'E' | '+' | '-')) {
        let f: f64 = t.parse().ok()?;
        return Some(Val::Float(if negative { -f } else { f }));
    }
    None
}

/// Python's `repr` of a float (its shortest round-trip digits, as Rust's `{:?}` gives them, with
/// Python's exponent form: `1e+16`, `1e-05`).
fn float_repr(f: f64) -> Option<String> {
    if !f.is_finite() {
        return None;
    }
    let s = format!("{f:?}");
    match s.split_once('e') {
        None => Some(s),
        Some((m, e)) => {
            let e: i32 = e.parse().ok()?;
            Some(format!("{m}e{}{:02}", if e < 0 { '-' } else { '+' }, e.unsigned_abs()))
        }
    }
}

/// pytest's id for one value (`_idval`), where it is certain.
fn idval(v: &Val, argname: &str, index: usize) -> Option<String> {
    match v {
        Val::Str(s) => s.chars().all(|c| (' '..='~').contains(&c) && c != '\\').then(|| s.clone()),
        Val::Int(s) => Some(s.clone()),
        Val::Float(f) => float_repr(*f),
        Val::Bool(b) => Some(if *b { "True" } else { "False" }.into()),
        Val::None => Some("None".into()),
        Val::Seq(..) | Val::Dict(_) => Some(format!("{argname}{index}")),
        Val::Param(..) | Val::Other(_) => None,
    }
}

/// A value as the receipt shows it (Python's literal form), or `None` when it cannot be shown
/// faithfully. Its nesting is bounded by [`nested`].
fn shown_value(v: &Val) -> Option<String> {
    let s = match v {
        Val::Str(s) if !s.contains('\'') => format!("'{s}'"),
        Val::Str(_) => return None,
        Val::Int(s) => s.clone(),
        Val::Float(f) => float_repr(*f)?,
        Val::Bool(b) => if *b { "True" } else { "False" }.into(),
        Val::None => "None".into(),
        Val::Seq(open, items) => {
            let inner: Vec<String> = items.iter().map(shown_value).collect::<Option<_>>()?;
            match open {
                '(' if inner.len() == 1 => format!("({},)", inner[0]),
                '(' => format!("({})", inner.join(", ")),
                '[' => format!("[{}]", inner.join(", ")),
                _ => format!("{{{}}}", inner.join(", ")),
            }
        }
        Val::Dict(pairs) => {
            let inner: Vec<String> = pairs
                .iter()
                .map(|(k, v)| Some(format!("{}: {}", shown_value(k)?, shown_value(v)?)))
                .collect::<Option<_>>()?;
            format!("{{{}}}", inner.join(", "))
        }
        Val::Param(..) => return None,
        Val::Other(text) => {
            let one: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
            if one.contains('\'') {
                one
            } else {
                one.replace('"', "'")
            }
        }
    };
    (plain(&s) && s.len() <= 64 * VALUE).then_some(s)
}

/// `s` cut to [`VALUE`] characters.
fn cut(s: &str) -> String {
    if s.chars().count() <= VALUE {
        return s.to_string();
    }
    let mut c: String = s.chars().take(VALUE).collect();
    c.push('…');
    c
}

/// The case of the parametrize decorator `d` whose id is `p`: its entry's line and, when it can be
/// shown, `argname = value`.
fn case(d: &Logical, p: &str, lines: &[Logical], source: &str) -> Option<(usize, Option<String>)> {
    let args = parametrize(d)?;
    let mut parts = split_top(args, ",");
    if parts.last().is_some_and(|x| x.is_empty()) {
        parts.pop();
    }
    let (mut positional, mut kw): (Vec<&[Tok]>, BTreeMap<&str, &[Tok]>) = (Vec::new(), BTreeMap::new());
    for part in parts {
        match keyword(part) {
            Some((k, v)) => {
                kw.insert(k, v);
            }
            None => positional.push(part),
        }
    }
    let names = kw.get("argnames").copied().or_else(|| positional.first().copied())?;
    // pytest's `force_tuple`: one name given in a string takes each entry as its value; names
    // given as a sequence (even one) take each entry as a sequence of values.
    let names = value(names, source);
    let single = matches!(&names, Val::Str(s) if !s.contains(','));
    let names: Vec<String> = match names {
        Val::Str(s) => s.split(',').map(|n| n.trim().to_string()).filter(|n| !n.is_empty()).collect(),
        Val::Seq(_, items) => items
            .into_iter()
            .map(|v| match v {
                Val::Str(s) => Some(s.trim().to_string()),
                _ => None,
            })
            .collect::<Option<_>>()?,
        _ => return None,
    };
    if names.is_empty() || !names.iter().all(|n| ident(n)) {
        return None;
    }
    let values = kw.get("argvalues").copied().or_else(|| positional.get(1).copied())?;
    let values = match values {
        // A list named at module level: `CASES = [...]`, its last assignment above the decorator.
        [n] if n.kind == Kind::Name => {
            let a = lines.iter().rev().find(|l| {
                l.indent == 0
                    && l.line() < d.line()
                    && l.toks.len() > 2
                    && l.toks[0].is_name(&n.text)
                    && l.toks[1].is_op("=")
            })?;
            &a.toks[2..]
        }
        v => v,
    };
    let open = values.first()?;
    if !(open.is_op("[") || open.is_op("(")) || close_of(values, 0) != Some(values.len() - 1) {
        return None;
    }
    let mut entries = items(&values[1..values.len() - 1])?;
    let trailing = entries.last().is_some_and(|e| e.is_empty());
    if trailing {
        entries.pop();
    }
    if entries.is_empty() || entries.len() > ENTRIES || (open.is_op("(") && entries.len() == 1 && !trailing) {
        return None;
    }
    // Explicit ids: a literal list of strings (or `None`, generated); anything else is unknown.
    let given: Option<Vec<IdSpec>> = match kw.get("ids").copied().or_else(|| positional.get(3).copied()) {
        None => None,
        Some(ids) => match value(ids, source) {
            Val::None => None,
            Val::Seq(_, items) if items.len() == entries.len() => Some(
                items
                    .into_iter()
                    .map(|v| match v {
                        Val::Str(s) => IdSpec::Given(s),
                        Val::None => IdSpec::Generated,
                        _ => IdSpec::Unknown,
                    })
                    .collect(),
            ),
            _ => Some(vec![IdSpec::Unknown; entries.len()]),
        },
    };
    let mut cases: Vec<Option<Vec<Val>>> = Vec::new();
    let mut ids: Vec<Option<String>> = Vec::new();
    for (i, e) in entries.iter().enumerate() {
        let (vals, spec) = match value(e, source) {
            Val::Param(vals, spec) => (Some(vals).filter(|v| v.len() == names.len()), spec),
            v if single => (Some(vec![v]), IdSpec::Generated),
            Val::Seq('(' | '[', items) if items.len() == names.len() => (Some(items), IdSpec::Generated),
            _ => (None, IdSpec::Generated),
        };
        let spec = match (spec, given.as_ref().map(|g| &g[i])) {
            (IdSpec::Generated, Some(g)) => g.clone(),
            (s, _) => s,
        };
        let id = match &spec {
            IdSpec::Given(s) => idval(&Val::Str(s.clone()), "", i),
            IdSpec::Unknown => None,
            IdSpec::Generated => vals.as_ref().and_then(|vals| {
                let parts: Option<Vec<String>> = names.iter().zip(vals).map(|(n, v)| idval(v, n, i)).collect();
                parts.map(|p| p.join("-"))
            }),
        };
        ids.push(id);
        cases.push(vals);
    }
    let i = find(&ids, p)?;
    let line = entries[i].first()?.line;
    let shown = cases[i].as_ref().and_then(|vals| {
        let parts: Option<Vec<String>> =
            names.iter().zip(vals).map(|(n, v)| shown_value(v).map(|s| format!("{n} = {s}"))).collect();
        let full = parts?.join(", ");
        if adage_core::secrets::has_secret(&full) {
            return None;
        }
        let text = cut(&full);
        (crate::orient::shown(&text).as_deref() == Some(text.as_str()) && safe(&text)).then_some(text)
    });
    Some((line, shown))
}

/// The entry whose final id is `p`, when exactly one can be. pytest makes duplicate ids unique by
/// suffixing an index, with an underscore before it when the id ends in a digit in newer versions
/// and without in older ones: an entry found under either counts, if it is the only one.
fn find(ids: &[Option<String>], p: &str) -> Option<usize> {
    let hits =
        |ids: &[String]| -> Vec<usize> { ids.iter().enumerate().filter(|(_, x)| *x == p).map(|(i, _)| i).collect() };
    let Some(raw) = ids.iter().cloned().collect::<Option<Vec<String>>>() else {
        // Some ids are unknown: only an exact id no other known entry shares.
        let known: Vec<usize> =
            ids.iter().enumerate().filter(|(_, x)| x.as_deref() == Some(p)).map(|(i, _)| i).collect();
        return match known[..] {
            [i] => Some(i),
            _ => None,
        };
    };
    let mut found: BTreeSet<usize> = BTreeSet::new();
    for newer in [true, false] {
        found.extend(hits(&unique(&raw, newer)));
    }
    match found.into_iter().collect::<Vec<_>>()[..] {
        [i] => Some(i),
        _ => None,
    }
}

/// pytest's unique ids for `raw` (`make_unique_parameterset_ids`): `newer` is the form with an
/// underscore after a trailing digit and a check against ids already present.
fn unique(raw: &[String], newer: bool) -> Vec<String> {
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    for r in raw {
        *counts.entry(r).or_default() += 1;
    }
    let mut out: Vec<String> = raw.to_vec();
    // The ids as they stand, counted (pytest checks each new id against them).
    let mut current: BTreeMap<String, usize> = BTreeMap::new();
    for r in raw {
        *current.entry(r.clone()).or_default() += 1;
    }
    let mut next: BTreeMap<String, usize> = BTreeMap::new();
    for (i, r) in raw.iter().enumerate() {
        if counts[r.as_str()] < 2 {
            continue;
        }
        let suffix = if newer && r.ends_with(|c: char| c.is_ascii_digit()) { "_" } else { "" };
        let n = next.entry(r.clone()).or_default();
        let mut id = format!("{r}{suffix}{n}");
        while newer && current.get(&id).is_some_and(|c| *c > 0) {
            *n += 1;
            id = format!("{r}{suffix}{n}");
        }
        *n += 1;
        if let Some(c) = current.get_mut(r) {
            *c -= 1;
        }
        *current.entry(id.clone()).or_default() += 1;
        out[i] = id;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The held-out b08 shape: the agent's own parametrized test declares a valid pair invalid.
    const B08: &str = r#"import pytest

from marshmallow import validate
from marshmallow.exceptions import ValidationError


def test_and():
    validator = validate.And(validate.Range(min=0))
    assert validator(2)


@pytest.mark.parametrize(
    "valid_latlong",
    [
        (0, 0),
        (45, 90),
        (-45, -90),
    ],
)
def test_latlong_valid(valid_latlong):
    validator = validate.LatLong()
    assert validator(valid_latlong) == valid_latlong


@pytest.mark.parametrize(
    "invalid_latlong",
    [
        (91, 0),
        (-91, 0),
        (0, 181),
        (0, -181),
        (90.1, 0),
        (-90.1, 0),
        (0, 180.1),
        (0, -180.1),
        (100, 200),
        (45,),
        (45, 90, 45),
        [],
        [45, 90],
        "45, 90",
        45,
        None,
        (None, 0),
        (0, None),
        ("invalid", 0),
        (0, "invalid"),
    ],
)
def test_latlong_invalid(invalid_latlong):
    validator = validate.LatLong()
    with pytest.raises(ValidationError):
        validator(invalid_latlong)
"#;

    fn line_of(src: &str, needle: &str) -> usize {
        src.lines().position(|l| l.contains(needle)).unwrap() + 1
    }

    #[test]
    fn the_b08_case_is_named_with_its_value_and_its_check() {
        let f = facts(B08, "tests/test_validate.py::test_latlong_invalid[invalid_latlong12]").unwrap();
        assert_eq!(f.def_line, line_of(B08, "def test_latlong_invalid"));
        assert_eq!(f.case, Some((line_of(B08, "        [45, 90],"), Some("invalid_latlong = [45, 90]".into()))));
        assert_eq!(f.check, Some((line_of(B08, "pytest.raises"), "with pytest.raises(ValidationError):".into())));
        assert_eq!(f.cases_line, None);
        // Other ids of the same list: by value for scalars, by index for containers.
        let case = |id: &str| facts(B08, &format!("tests/test_validate.py::test_latlong_invalid[{id}]")).unwrap().case;
        assert_eq!(case("invalid_latlong0").unwrap().1.as_deref(), Some("invalid_latlong = (91, 0)"));
        assert_eq!(case("invalid_latlong9").unwrap().1.as_deref(), Some("invalid_latlong = (45,)"));
        assert_eq!(case("45").unwrap().1.as_deref(), Some("invalid_latlong = 45"));
        assert_eq!(case("None").unwrap().1.as_deref(), Some("invalid_latlong = None"));
        assert_eq!(case("45, 90").unwrap().1.as_deref(), Some("invalid_latlong = '45, 90'"));
        assert_eq!(case("invalid_latlong18").unwrap().1.as_deref(), Some("invalid_latlong = ('invalid', 0)"));
        assert!(case("invalid_latlong99").is_none(), "no such entry");
        // The valid test: one assert.
        let v = facts(B08, "tests/test_validate.py::test_latlong_valid[valid_latlong1]").unwrap();
        assert_eq!(v.case.unwrap().1.as_deref(), Some("valid_latlong = (45, 90)"));
        assert_eq!(v.check.unwrap().1, "assert validator(valid_latlong) == valid_latlong");
        // Not parametrized: the def and its check.
        let a = facts(B08, "tests/test_validate.py::test_and").unwrap();
        assert_eq!((a.def_line, a.case, a.cases_line), (line_of(B08, "def test_and"), None, None));
        assert_eq!(a.check.unwrap().1, "assert validator(2)");
        assert!(facts(B08, "tests/test_validate.py::test_missing").is_none());
    }

    #[test]
    fn ids_follow_pytest_only_where_certain() {
        let src = r#"
import pytest

CASES = [(1, 2), (3, 4)]

@pytest.mark.parametrize("a, b", CASES)
def test_named(a, b):
    assert a < b

@pytest.mark.parametrize(("x", "y"), [(1, "p"), pytest.param(2.5, "q", id="second"), (0.00001, True)])
def test_multi(x, y):
    assert x

@pytest.mark.parametrize(
    "v",
    [
        1,
        1,
        2,
    ],
    ids=None,
)
def test_dup(v):
    assert v

@pytest.mark.parametrize("v", [make(1), 7])
def test_unknown(v):
    assert v
    assert v

@pytest.mark.parametrize("v", [1, 2], ids=str)
def test_ids_fn(v):
    pass

@pytest.mark.parametrize("v", [x for x in range(3)])
def test_comprehension(v):
    pass

@pytest.mark.parametrize("v", [1])
@pytest.mark.parametrize("w", [2])
def test_stacked(v, w):
    pass

@pytest.mark.parametrize(("w",), [(1,), (2,)])
def test_tuple_names(w):
    pass

@pytest.mark.parametrize("w", [(1,), (2,)])
def test_string_name(w):
    pass

class TestThing:
    @pytest.mark.parametrize("v", ["a", 'it"s', "b\\n"])
    def test_method(self, v):
        self.assertTrue(v)
"#;
        let c = |id: &str| facts(src, &format!("t.py::{id}")).unwrap();
        assert!(c("test_named[a0-b0]").case.is_none(), "tuples of scalars are named by their values");
        assert_eq!(c("test_named[1-2]").case.unwrap().1.as_deref(), Some("a = 1, b = 2"));
        assert_eq!(c("test_multi[1-p]").case.unwrap().1.as_deref(), Some("x = 1, y = 'p'"));
        assert_eq!(c("test_multi[second]").case.unwrap().1.as_deref(), Some("x = 2.5, y = 'q'"));
        assert_eq!(c("test_multi[1e-05-True]").case.unwrap().1.as_deref(), Some("x = 1e-05, y = True"));
        // Duplicates: 1, 1 become 1_0, 1_1 (newer pytest) or 10, 11 (older); 2 stays.
        let dup = |id: &str| c(&format!("test_dup[{id}]")).case.map(|x| x.0);
        assert!(dup("1_0").is_some() && dup("1_1").is_some() && dup("10").is_some() && dup("2").is_some());
        assert_ne!(dup("1_0"), dup("1_1"));
        assert!(dup("1").is_none(), "no test has the bare duplicate id");
        // An unknown id beside a known one: the known one is found exactly; the unknown is not guessed.
        assert!(c("test_unknown[7]").case.is_some());
        let u = c("test_unknown[v0]");
        assert!(u.case.is_none() && u.cases_line.is_some(), "{u:?}");
        assert!(u.check.is_none(), "two checks: none is quoted");
        for id in ["test_ids_fn[1]", "test_comprehension[0]", "test_stacked[2-1]"] {
            let f = c(id);
            assert!(f.case.is_none() && f.cases_line.is_some(), "{id}: {f:?}");
        }
        // One name in a sequence takes each entry as a sequence of values; in a string, as the value.
        assert_eq!(c("test_tuple_names[2]").case.unwrap().1.as_deref(), Some("w = 2"));
        assert!(c("test_tuple_names[w1]").case.is_none());
        assert_eq!(c("test_string_name[w1]").case.unwrap().1.as_deref(), Some("w = (2,)"));
        let m = c("TestThing::test_method[a]");
        assert_eq!(m.case.unwrap().1.as_deref(), Some("v = 'a'"));
        assert_eq!(m.check.unwrap().1, "self.assertTrue(v)");
        // A string with a double quote is not shown (the receipt strips double quotes), nor one with
        // an escape (not read); their ids are not pytest's verbatim either.
        assert!(c("TestThing::test_method[v1]").case.is_none());
        assert!(facts(src, "t.py::test_method[a]").is_none(), "a method is not a module-level test");
    }

    /// pytest's duplicate suffixes, newer form: an id already present is skipped.
    #[test]
    fn duplicate_ids_are_made_unique_as_pytest_makes_them() {
        let ids = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(unique(&ids(&["a", "a", "a0"]), true), ids(&["a1", "a2", "a0"]));
        assert_eq!(unique(&ids(&["2024", "2024", "x"]), true), ids(&["2024_0", "2024_1", "x"]));
        assert_eq!(unique(&ids(&["2024", "2024", "x"]), false), ids(&["20240", "20241", "x"]));
        let many: Vec<String> = (0..ENTRIES).map(|_| "same".to_string()).collect();
        assert_eq!(unique(&many, true).last().map(String::as_str), Some("same9999"));
    }

    #[test]
    fn floats_print_as_python_prints_them() {
        for (f, py) in [
            (0.5, "0.5"),
            (90.1, "90.1"),
            (1e16, "1e+16"),
            (1e15, "1000000000000000.0"),
            (0.0001, "0.0001"),
            (0.00001, "1e-05"),
            (1.5e-7, "1.5e-07"),
            (-0.0, "-0.0"),
            (1e300, "1e+300"),
            (0.1 + 0.2, "0.30000000000000004"),
        ] {
            assert_eq!(float_repr(f).as_deref(), Some(py), "{f}");
        }
        assert_eq!(number("1_000", true), Some(Val::Int("-1000".into())));
        assert_eq!(number("0x10", false), None);
        assert_eq!(number("012", false), None);
    }

    #[test]
    fn quoted_source_is_data_and_bounded() {
        let token = format!("ghp_{}", "A1b2C3d4E5".repeat(4));
        let long = "x".repeat(400);
        let src = format!(
            r#"import pytest

@pytest.mark.parametrize("v", [["{long}"], "{token}", "adage-reflex"])
def test_v(v):
    assert v != "{token}"

def test_marker():
    assert "Adage finish-line check: run it" == x

def test_quotes():
    assert x == "a"
"#
        );
        let f = facts(&src, "t.py::test_v[v0]").unwrap();
        let (_, shown) = f.case.unwrap();
        let shown = shown.unwrap();
        assert!(shown.ends_with('…') && shown.chars().count() <= VALUE + 10, "{shown}");
        assert!(f.check.is_none(), "a line with a secret is withheld");
        let secret = facts(&src, &format!("t.py::test_v[{token}]")).unwrap().case.unwrap();
        assert_eq!(secret.1, None, "the case is found, its value withheld");
        let marker = facts(&src, "t.py::test_v[adage-reflex]").unwrap().case.unwrap();
        assert_eq!(marker.1, None, "Adage's own markers are never quoted");
        assert_eq!(facts(&src, "t.py::test_marker").unwrap().check, None, "Adage's own markers are never quoted");
        assert_eq!(facts(&src, "t.py::test_quotes").unwrap().check.unwrap().1, "assert x == 'a'");
        // Deep nesting is read as text, not recursed into without bound.
        let deep = format!(
            "import pytest\n\n@pytest.mark.parametrize('v', [{}1{}, 2])\ndef test_deep(v):\n    pass\n",
            "[".repeat(5000),
            "]".repeat(5000)
        );
        assert_eq!(facts(&deep, "t.py::test_deep[v0]").unwrap().case.unwrap().1, None);
        assert!(facts(&deep, "t.py::test_deep[2]").unwrap().case.is_some());
        // Unterminated strings and odd ids read nothing.
        assert!(facts("def test_a():\n    x = 'oops\n", "t.py::test_a").is_none());
        assert!(facts(&src, "t.py::test_v[v0").is_none());
        assert!(facts(&src, "t.py::not-a-name").is_none());
    }
}
