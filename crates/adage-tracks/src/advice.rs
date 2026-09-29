//! Advisory steps ([`crate::Step::Advise`]): a closed set of fixed templates, owned here, whose
//! only variable parts are slot values that pass each slot's contract.
//!
//! A track names a template by id and supplies slot values; it never supplies text. Validation
//! ([`check`], called by [`crate::io::validate`]) refuses an unknown template, a missing or unknown
//! slot, and any value that fails its contract. The runtime renders with [`text`], and the eval's
//! safety oracle accepts exactly these sentences.

use adage_core::{CommandTable, Effect};
use std::collections::BTreeMap;

/// `(template id, required slots, optional slots)`.
pub const TEMPLATES: [(&str, &[&str], &[&str]); 5] = [
    ("generated-file", &["path"], &["command"]),
    ("lockfile", &["path", "manager"], &[]),
    ("package-manager", &["manager", "lockfile"], &[]),
    ("test-command", &["command"], &[]),
    // A skill's prohibition on committing ("do not commit; leave the working tree for review"),
    // compiled by `adage train`. No slots: the sentence is the whole advice.
    ("uncommitted", &[], &[]),
];

/// Root lockfiles and the tool that maintains each: the only values the `lockfile` and
/// `manager` slots accept (the instinct catalogue's `[lockfiles]` table is the same list).
pub const LOCKFILES: [(&str, &str); 14] = [
    ("package-lock.json", "npm"),
    ("npm-shrinkwrap.json", "npm"),
    ("pnpm-lock.yaml", "pnpm"),
    ("yarn.lock", "yarn"),
    ("bun.lock", "bun"),
    ("bun.lockb", "bun"),
    ("Cargo.lock", "cargo"),
    ("go.sum", "go"),
    ("poetry.lock", "poetry"),
    ("uv.lock", "uv"),
    ("Pipfile.lock", "pipenv"),
    ("pdm.lock", "pdm"),
    ("Gemfile.lock", "bundler"),
    ("composer.lock", "composer"),
];

/// The fixed sentence of a template, by id and whether the optional `command` slot is bound.
pub fn template(id: &str, with_command: bool) -> Option<&'static str> {
    Some(match (id, with_command) {
        ("generated-file", true) => {
            "In this repository, {path} is generated: change its inputs and run {command} rather than editing it by hand."
        }
        ("generated-file", false) => {
            "In this repository, {path} is generated: change its inputs and regenerate it rather than editing it by hand."
        }
        ("lockfile", _) => {
            "In this repository, {path} is maintained by {manager}: change the manifest and let {manager} update it rather than editing it by hand."
        }
        ("package-manager", _) => {
            "This repository uses {manager} ({lockfile}), not npm: use {manager} for installs and scripts here."
        }
        ("test-command", _) => "In this repository, tests run with {command}, followed by the test files or names to run.",
        ("uncommitted", _) => {
            "In this repository, leave your changes uncommitted: do not commit them, the working tree is reviewed as it is."
        }
        _ => return None,
    })
}

/// A repository-relative glob: no absolute path, no `..`, nothing but `[A-Za-z0-9_./*-]`.
pub fn safe_glob(g: &str) -> bool {
    !g.is_empty()
        && g.len() <= 120
        && !g.starts_with('/')
        && !g.contains("..")
        && g.chars().all(|c| c.is_ascii_alphanumeric() || "_./*-".contains(c))
}

/// Plain argv Adage may name: no shell syntax, no path outside the repository, no slot.
pub fn plain_command(c: &str) -> bool {
    let inside = |part: &str| !part.starts_with('/') && !part.split('/').any(|seg| seg == "..");
    !c.is_empty()
        && c.len() <= 200
        && c.split(' ').all(|t| !t.is_empty())
        && c.chars().all(|ch| ch.is_ascii_alphanumeric() || " _./:=@,+-".contains(ch))
        && c.split(' ').all(|t| t.split(['=', ',']).all(inside))
}

fn rank(e: Effect) -> u8 {
    match e {
        Effect::Observe => 0,
        Effect::Verify => 1,
        Effect::Regenerate => 2,
        Effect::Environment => 3,
        Effect::Author => 4,
        Effect::Publish => 5,
    }
}

/// Whether `value` passes slot `name`'s contract.
pub fn slot_ok(name: &str, value: &str, table: &CommandTable) -> bool {
    match name {
        "path" => safe_glob(value),
        "lockfile" => LOCKFILES.iter().any(|(l, _)| *l == value),
        "manager" => LOCKFILES.iter().any(|(_, m)| *m == value),
        "command" => plain_command(value) && rank(crate::io::classify(value, table)) <= rank(Effect::Regenerate),
        _ => false,
    }
}

/// Why an advisory step is refused, if it is.
pub fn check(template_id: &str, slots: &BTreeMap<String, String>, table: &CommandTable) -> Result<(), String> {
    let Some((_, required, optional)) = TEMPLATES.iter().find(|(id, ..)| *id == template_id) else {
        return Err(format!("unknown advisory template {template_id:?}"));
    };
    for k in *required {
        if !slots.contains_key(*k) {
            return Err(format!("advisory slot {k} is missing"));
        }
    }
    for (k, v) in slots {
        if !required.contains(&k.as_str()) && !optional.contains(&k.as_str()) {
            return Err(format!("slot {k} is not in advisory template {template_id}"));
        }
        if !slot_ok(k, v, table) {
            return Err(format!("advisory slot {k} value {v:?} fails its contract"));
        }
    }
    Ok(())
}

/// The sentence the agent receives: the fixed template with the slot values in place. `None` if
/// the template is unknown or a required slot is missing. (Values are not re-checked here; the
/// track was validated when it was read, and the runtime sanitises every interpolated value.)
pub fn text(template_id: &str, slots: &BTreeMap<String, String>) -> Option<String> {
    let (_, required, optional) = TEMPLATES.iter().find(|(id, ..)| *id == template_id)?;
    let mut s = template(template_id, slots.contains_key("command"))?.to_string();
    for k in required.iter().chain(optional.iter()) {
        match slots.get(*k) {
            Some(v) => s = s.replace(&format!("{{{k}}}"), v),
            None if required.contains(k) => return None,
            None => {}
        }
    }
    (!s.contains('{')).then_some(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slots(kv: &[(&str, &str)]) -> BTreeMap<String, String> {
        kv.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    #[test]
    fn templates_render_and_contracts_hold() {
        let t = CommandTable::load();
        let s = slots(&[("path", "acme/generated/models.py"), ("command", "make gen")]);
        assert!(check("generated-file", &s, &t).is_ok());
        assert_eq!(
            text("generated-file", &s).unwrap(),
            "In this repository, acme/generated/models.py is generated: change its inputs and run make gen rather than editing it by hand."
        );
        assert!(check("lockfile", &slots(&[("path", "uv.lock"), ("manager", "uv")]), &t).is_ok());
        assert!(check("package-manager", &slots(&[("manager", "pnpm"), ("lockfile", "pnpm-lock.yaml")]), &t).is_ok());
        let s = slots(&[("command", ".venv/bin/python -m pytest")]);
        assert!(check("test-command", &s, &t).is_ok());
        assert_eq!(
            text("test-command", &s).unwrap(),
            "In this repository, tests run with .venv/bin/python -m pytest, followed by the test files or names to run."
        );
        assert!(check("uncommitted", &slots(&[]), &t).is_ok());
        assert!(check("uncommitted", &slots(&[("command", "git commit")]), &t).is_err());
        assert!(text("uncommitted", &slots(&[])).unwrap().starts_with("In this repository, leave your changes uncommitted"));
        // Refused: unknown template, unknown slot, missing slot, hostile values.
        assert!(check("free-text", &slots(&[("path", "x")]), &t).is_err());
        assert!(check("lockfile", &slots(&[("path", "uv.lock")]), &t).is_err());
        assert!(check("lockfile", &slots(&[("path", "uv.lock"), ("manager", "uv"), ("note", "x")]), &t).is_err());
        assert!(check("lockfile", &slots(&[("path", "../uv.lock"), ("manager", "uv")]), &t).is_err());
        assert!(check("lockfile", &slots(&[("path", "uv.lock"), ("manager", "curl")]), &t).is_err());
        for bad in ["make gen; rm -rf x", "make gen && x", "npm publish", "../x", "make {x}"] {
            let s = slots(&[("path", "a.py"), ("command", bad)]);
            assert!(check("generated-file", &s, &t).is_err(), "{bad}");
        }
    }
}
