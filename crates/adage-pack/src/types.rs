//! Contract types: the built-in registry, strictness of user types, and value checks.
//!
//! A type is a contract that a value must pass before it may reach an argv, an exit predicate or
//! a typed output. The built-ins are `repo-path`, `repo-path-list`, `git-rev`, `int`, `bool` and
//! `enum` (values inline, at most 32). A user type (`types/<id>.yaml`) reduces to one of the bases
//! `string`, `int`, `enum` or `list`, and a string pattern must be anchored, strict (it refuses
//! every hostile probe) and bounded (no value longer than [`MAX_VALUE_LEN`] bytes can match).

use crate::model::{Base, TypeDef};
use crate::validate::{safe_path, safe_token};
use anyhow::{bail, Result};
use regex::Regex;
use std::collections::BTreeMap;

/// The built-in types.
pub const BUILTINS: [&str; 6] = ["repo-path", "repo-path-list", "git-rev", "int", "bool", "enum"];
/// No contract value may be longer than this (as `adage_core::vtype::MAX_LEN`).
pub const MAX_VALUE_LEN: usize = adage_core::vtype::MAX_LEN;
/// At most this many values in an enum.
pub const MAX_ENUM: usize = 32;
/// At most this many items in a list type.
pub const MAX_LIST: u32 = 64;
/// `git-rev`'s pattern.
pub const GIT_REV: &str = "^[0-9a-f]{7,40}$";

pub fn is_builtin(id: &str) -> bool {
    BUILTINS.contains(&id)
}

/// Values a strict contract must refuse: shell syntax, whitespace, traversal, flags, quoting,
/// the empty string, and characters that are invisible or change direction (as
/// `adage_tracks::io::contract_is_strict`, which has the first fourteen).
pub const PROBES: [&str; 22] = [
    "a b",
    "a;b",
    "a|b",
    "$(x)",
    "`x`",
    "a&b",
    "a\nb",
    "../x",
    "-x",
    "a>b",
    "a<b",
    "a'b",
    "a\"b",
    "",
    "a\tb",
    "a\rb",
    "a\u{0}b",
    "a\\b",
    "a\u{200B}b",
    "a\u{202E}b",
    "a\u{2066}b",
    "a\u{FEFF}b",
];

/// Why a pattern is not an acceptable contract, if it is not.
pub fn pattern_problem(pattern: &str) -> Option<String> {
    if pattern.len() > 256 {
        return Some("longer than 256 bytes".into());
    }
    if !pattern.starts_with('^') || !pattern.ends_with('$') || pattern.ends_with("\\$") {
        return Some("not anchored with ^…$".into());
    }
    let re = match regex::RegexBuilder::new(pattern).size_limit(1 << 16).build() {
        Ok(r) => r,
        Err(e) => return Some(format!("does not compile: {e}")),
    };
    if let Some(p) = PROBES.iter().find(|p| re.is_match(p)) {
        return Some(format!("accepts the hostile probe {p:?}"));
    }
    // Anchors inside alternations (`^a$|b`) leave a branch unanchored; parse and check the whole
    // expression is one anchored concatenation.
    let hir = match regex_syntax::Parser::new().parse(pattern) {
        Ok(h) => h,
        Err(e) => return Some(format!("does not parse: {e}")),
    };
    let props = hir.properties();
    let anchored = props.look_set_prefix().contains(regex_syntax::hir::Look::Start)
        && props.look_set_suffix().contains(regex_syntax::hir::Look::End);
    if !anchored {
        return Some("not anchored at both ends in every branch".into());
    }
    match props.maximum_len() {
        None => return Some("unbounded (a value of any length could match)".into()),
        Some(n) if n > MAX_VALUE_LEN => {
            return Some(format!("bounded at {n} bytes; at most {MAX_VALUE_LEN} are allowed"))
        }
        _ => {}
    }
    if props.minimum_len().unwrap_or(0) == 0 {
        return Some("accepts the empty string".into());
    }
    None
}

/// Whether a pattern is an acceptable contract.
pub fn pattern_is_strict(pattern: &str) -> bool {
    pattern_problem(pattern).is_none()
}

/// An enum value: a short token (`Active`, `closed`, `v2`).
pub fn enum_value_ok(v: &str) -> bool {
    safe_token(v, 64)
}

/// Check a user type on its own (references to other types are checked by [`resolve`]).
pub fn check_typedef(t: &TypeDef) -> Result<()> {
    let only = |pattern: bool, range: bool, values: bool, item: bool, max: bool| {
        (t.pattern.is_some() <= pattern)
            && (t.range.is_some() <= range)
            && (t.values.is_some() <= values)
            && (t.item.is_some() <= item)
            && (t.max.is_some() <= max)
    };
    match t.base {
        Base::String => {
            if !only(true, false, false, false, false) {
                bail!("type {}: a string type has only `pattern`", t.id);
            }
            let Some(p) = &t.pattern else { bail!("type {}: a string type needs a pattern", t.id) };
            if let Some(why) = pattern_problem(p) {
                bail!("type {}: pattern {p:?} is not a strict contract: {why}", t.id);
            }
        }
        Base::Int => {
            if !only(false, true, false, false, false) {
                bail!("type {}: an int type has only `range`", t.id);
            }
            if let Some([lo, hi]) = t.range {
                const LIM: i64 = 1_000_000_000_000_000;
                if lo > hi || lo < -LIM || hi > LIM {
                    bail!("type {}: range [{lo}, {hi}] is not a bounded interval", t.id);
                }
            }
        }
        Base::Enum => {
            if !only(false, false, true, false, false) {
                bail!("type {}: an enum type has only `values`", t.id);
            }
            check_enum(&t.id, t.values.as_deref())?;
        }
        Base::List => {
            if !only(false, false, false, true, true) {
                bail!("type {}: a list type has only `item` and `max`", t.id);
            }
            if t.item.is_none() {
                bail!("type {}: a list type needs an item type", t.id);
            }
            match t.max {
                Some(m) if (1..=MAX_LIST).contains(&m) => {}
                _ => bail!("type {}: a list type needs max in 1..={MAX_LIST}", t.id),
            }
        }
    }
    Ok(())
}

pub fn check_enum(ctx: &str, values: Option<&[String]>) -> Result<()> {
    let Some(v) = values else { bail!("{ctx}: an enum needs values") };
    if v.is_empty() || v.len() > MAX_ENUM {
        bail!("{ctx}: an enum has 1..={MAX_ENUM} values");
    }
    let mut seen = std::collections::BTreeSet::new();
    for x in v {
        if !enum_value_ok(x) {
            bail!("{ctx}: enum value {x:?} is not a plain token");
        }
        if !seen.insert(x) {
            bail!("{ctx}: enum value {x:?} is repeated");
        }
    }
    Ok(())
}

/// A type reference, resolved: what a value of it must be.
#[derive(Debug, Clone, PartialEq)]
pub enum Resolved {
    RepoPath,
    Pattern(String),
    Int(Option<[i64; 2]>),
    Bool,
    Enum(Vec<String>),
    List(Box<Resolved>, u32),
}

impl Resolved {
    /// A list of repository paths (what a `read` step reads).
    pub fn is_repo_path_list(&self) -> bool {
        matches!(self, Resolved::List(item, _) if **item == Resolved::RepoPath)
    }

    pub fn is_list(&self) -> bool {
        matches!(self, Resolved::List(..))
    }

    /// Whether a scalar value passes this type (a list never takes a scalar).
    pub fn accepts(&self, v: &str) -> bool {
        if v.is_empty() || v.len() > MAX_VALUE_LEN {
            return false;
        }
        match self {
            Resolved::RepoPath => safe_path(v),
            Resolved::Pattern(p) => Regex::new(p).is_ok_and(|re| re.is_match(v)),
            Resolved::Int(range) => {
                let ok_syntax =
                    v.strip_prefix('-').unwrap_or(v).bytes().all(|c| c.is_ascii_digit()) && v.len() <= 19 && v != "-";
                match (ok_syntax, v.parse::<i64>()) {
                    (true, Ok(n)) => range.is_none_or(|[lo, hi]| lo <= n && n <= hi),
                    _ => false,
                }
            }
            Resolved::Bool => v == "true" || v == "false",
            Resolved::Enum(vals) => vals.iter().any(|x| x == v),
            Resolved::List(..) => false,
        }
    }
}

/// Resolve a type reference in a route (`values` and `max` are the input's or output's inline
/// fields). Refuses an unknown type, a list of lists, and inline fields the type does not take.
pub fn resolve(
    ty: &str,
    values: Option<&[String]>,
    max: Option<u32>,
    user: &BTreeMap<String, TypeDef>,
    ctx: &str,
) -> Result<Resolved> {
    let r = resolve_bare(ty, user, 0).map_err(|e| anyhow::anyhow!("{ctx}: {e}"))?;
    if ty == "enum" {
        check_enum(ctx, values)?;
    } else if values.is_some() {
        bail!("{ctx}: `values` is only for type enum");
    }
    let r = match (r, ty == "enum") {
        (_, true) => Resolved::Enum(values.unwrap_or_default().to_vec()),
        (r, false) => r,
    };
    match (r, max) {
        (Resolved::List(item, cap), Some(m)) => {
            if m == 0 || m > cap {
                bail!("{ctx}: max {m} is outside 1..={cap}");
            }
            Ok(Resolved::List(item, m))
        }
        (_, Some(_)) => bail!("{ctx}: `max` is only for list types"),
        (r, None) => Ok(r),
    }
}

fn resolve_bare(ty: &str, user: &BTreeMap<String, TypeDef>, depth: u8) -> Result<Resolved> {
    Ok(match ty {
        "repo-path" => Resolved::RepoPath,
        "repo-path-list" => Resolved::List(Box::new(Resolved::RepoPath), MAX_LIST),
        "git-rev" => Resolved::Pattern(GIT_REV.into()),
        "int" => Resolved::Int(None),
        "bool" => Resolved::Bool,
        "enum" => Resolved::Enum(Vec::new()),
        _ => {
            let Some(t) = user.get(ty) else { bail!("type {ty:?} is neither built in nor in types/ (dangling)") };
            match t.base {
                Base::String => Resolved::Pattern(t.pattern.clone().unwrap_or_default()),
                Base::Int => Resolved::Int(t.range),
                Base::Enum => Resolved::Enum(t.values.clone().unwrap_or_default()),
                Base::List => {
                    let item = t.item.as_deref().unwrap_or_default();
                    if depth > 0 || item == "enum" {
                        bail!("type {ty}: list items must be a scalar type other than bare enum");
                    }
                    let inner = resolve_bare(item, user, depth + 1)?;
                    if inner.is_list() {
                        bail!("type {ty}: a list of lists is not allowed");
                    }
                    Resolved::List(Box::new(inner), t.max.unwrap_or(MAX_LIST))
                }
            }
        }
    })
}

/// The user types a reference reaches (itself, and a list's item type).
pub fn user_refs(ty: &str, user: &BTreeMap<String, TypeDef>) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(t) = user.get(ty) {
        out.push(ty.to_string());
        if let Some(i) = &t.item {
            if user.contains_key(i) {
                out.push(i.clone());
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strict_patterns() {
        for ok in ["^W-\\d{4,9}$", "^[0-9a-f]{7,40}$", "^(?:BIL\\-[0-9]{3,4})$", "^[A-Z]{2,5}-[0-9]{1,6}$"] {
            assert!(pattern_is_strict(ok), "{ok}: {:?}", pattern_problem(ok));
        }
        for bad in [
            "W-\\d{4}",
            "^.*$",
            "^.+$",
            "^[A-Z]+-\\d+$",
            "^\\S{1,10}$",
            "^[^;]{1,5}$",
            "^a$|^.*",
            "^(a|b)*$",
            "^[a-z\\-]{1,20}$",
            "^[a-z]{1,200}$",
            "^x{0,3}$",
            "^[[:word:]\u{200B}]{1,9}$",
        ] {
            assert!(!pattern_is_strict(bad), "{bad} should be refused");
        }
    }

    #[test]
    fn values() {
        let u = BTreeMap::new();
        let r = resolve("int", None, None, &u, "t").unwrap();
        assert!(r.accepts("42") && r.accepts("-3") && !r.accepts("4 2") && !r.accepts("-") && !r.accepts("+1"));
        let e = resolve("enum", Some(&["Active".into(), "Closed".into()]), None, &u, "t").unwrap();
        assert!(e.accepts("Closed") && !e.accepts("closed"));
        assert!(resolve("repo-path-list", None, Some(65), &u, "t").is_err());
        assert!(resolve("repo-path", None, Some(5), &u, "t").is_err());
        assert!(resolve("wi-state", None, None, &u, "t").is_err());
        let g = resolve("git-rev", None, None, &u, "t").unwrap();
        assert!(g.accepts("1a2b3c4") && !g.accepts("1A2B3C4"));
    }
}
