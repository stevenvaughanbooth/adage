//! Seeded task generation: prompts that give each claim a chance to fire, and controls that give
//! none a chance.
//!
//! - **Trigger tasks** per exercisable claim: `regenerate_after` asks for a change to a real file
//!   matching the glob; `verify` and `avoid` ask to check or fix the build; `lookup` carries the
//!   example id.
//! - **Phrasing variety**: three templates × two verbosities per kind. About 70% of trigger tasks
//!   are *implicit* (they never name the skill or the command); about 30% are *explicit*.
//! - **Controls**: at least 25% of tasks, outside every claim (files no claim's glob matches, and
//!   questions that need no build).
//!
//! The same claims, files and seed always produce the same tasks.

use crate::claims::{Claim, ClaimKind};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// splitmix64: small, seedable, the same on every platform.
#[derive(Debug, Clone)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Rng {
        Rng(seed ^ 0x5eed_a3a6_e000_0001)
    }
    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }
    pub fn below(&mut self, n: usize) -> usize {
        if n == 0 {
            0
        } else {
            (self.next_u64() % n as u64) as usize
        }
    }
    pub fn shuffle<T>(&mut self, xs: &mut [T]) {
        for i in (1..xs.len()).rev() {
            let j = self.below(i + 1);
            xs.swap(i, j);
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Verbosity {
    Terse,
    Verbose,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Task {
    /// `t01`, `t02`, … (`v01`, … for held-out validation tasks).
    pub id: String,
    /// The claim it exercises; `None` for a control.
    pub claim: Option<String>,
    pub control: bool,
    /// Names the skill or the command.
    pub explicit: bool,
    pub verbosity: Verbosity,
    pub template: usize,
    pub prompt: String,
    /// The repository file the prompt asks to change or explain, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
}

const WORDS: &[&str] = &["notes", "currency", "priority", "region", "archived", "tags", "due_date", "owner"];
/// Lead-ins, for variety that says nothing about the claim.
const LEADS: &[&str] = &["", "Quick one. ", "When you have a moment: ", "Small request. ", "Next up: "];

/// Whether generation can give this claim a chance to fire in this repository.
pub fn exercisable(c: &Claim, files: &[String]) -> Result<(), String> {
    if let Some(u) = &c.r#unsafe {
        return Err(format!("unsafe: {u}"));
    }
    match &c.kind {
        ClaimKind::RegenerateAfter { glob: None, .. } => Err("no input glob is named, so nothing triggers it".into()),
        ClaimKind::RegenerateAfter { glob: Some(g), .. } if matching(g, files).is_empty() => {
            Err(format!("no file in the repository matches `{g}`"))
        }
        ClaimKind::Lookup { id_example: None, .. } => Err("no example id to put in a prompt".into()),
        ClaimKind::Environment { .. } => Err("environment claims are never exercised".into()),
        _ => Ok(()),
    }
}

/// Repository files matching a glob, sorted.
pub fn matching(glob: &str, files: &[String]) -> Vec<String> {
    let Ok(p) = glob::Pattern::new(glob) else { return vec![] };
    let opts = glob::MatchOptions { require_literal_separator: true, ..Default::default() };
    files.iter().filter(|f| p.matches_with(f, opts)).cloned().collect()
}

fn skill_ref(skill: &str) -> String {
    format!("the `{skill}` skill")
}

fn trigger_prompt(
    kind: &ClaimKind,
    template: usize,
    v: Verbosity,
    explicit: bool,
    skill: &str,
    target: Option<&str>,
    word: &str,
) -> String {
    let verbose = v == Verbosity::Verbose;
    let tail = match (verbose, kind) {
        (false, _) => "",
        (true, ClaimKind::RegenerateAfter { .. }) => {
            " Keep the change small, leave the repository consistent, and tell me what you did."
        }
        (true, _) => " Tell me what you ran and what you found.",
    };
    match kind {
        ClaimKind::RegenerateAfter { command, .. } => {
            let f = target.unwrap_or("the file");
            let base = match template {
                0 => format!("Add an optional `{word}` field to {f}."),
                1 => format!("Update {f} so that it also has a `{word}` entry."),
                _ => format!("Extend {f} with a new `{word}` property."),
            };
            let base = if verbose { format!("We need to record one more thing. {base}") } else { base };
            if explicit {
                format!("{base} Follow {} and run `{command}` afterwards.{tail}", skill_ref(skill))
            } else {
                format!("{base}{tail}")
            }
        }
        ClaimKind::Verify { command } | ClaimKind::Avoid { instead: command, .. } => {
            let base = match template {
                0 => "Check whether this repository's tests pass.".to_string(),
                1 => "Make sure the test suite is green, and fix anything that is broken.".to_string(),
                _ => "Is anything failing in the checks? Find out.".to_string(),
            };
            let base = if verbose { format!("Before I merge today: {}", lower_first(&base)) } else { base };
            if explicit {
                format!("{base} Use `{command}`, as {} says.{tail}", skill_ref(skill))
            } else {
                format!("{base}{tail}")
            }
        }
        ClaimKind::Lookup { tool, id_example, .. } => {
            let id = id_example.as_deref().unwrap_or("the ticket");
            let base = match template {
                0 => format!("What does {id} ask for?"),
                1 => format!("Start on {id}: work out what it needs."),
                _ => format!("Summarise {id} for me."),
            };
            let base = if verbose { format!("I have been assigned {id}. {base}") } else { base };
            if explicit {
                format!("{base} Look it up with `{tool}`, as {} describes.{tail}", skill_ref(skill))
            } else {
                format!("{base}{tail}")
            }
        }
        ClaimKind::Environment { .. } => String::new(),
    }
}

fn lower_first(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_lowercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

fn control_prompt(template: usize, v: Verbosity, target: Option<&str>) -> String {
    let verbose = v == Verbosity::Verbose;
    match (template, target) {
        (0, Some(f)) => {
            if verbose {
                format!("I am new to this codebase. Explain what {f} is for and how it fits into the rest of the repository.")
            } else {
                format!("Explain what {f} does.")
            }
        }
        (1, Some(f)) => {
            if verbose {
                format!("Read {f} and suggest one improvement to its naming or comments. Do not change any files.")
            } else {
                format!("Suggest one naming improvement in {f}; do not edit anything.")
            }
        }
        _ => {
            if verbose {
                "Give me a tour of this repository: list its top-level directories and what each one is for. Do not change anything.".into()
            } else {
                "List the top-level directories and what each is for.".into()
            }
        }
    }
}

/// Generate `n` tasks. `files` are the repository's files (sorted); `exclude` are prompts that
/// must not recur (the exploration tasks, when generating held-out validation tasks); `prefix`
/// names the ids (`t`, `v`).
pub fn generate(
    claims: &[Claim],
    files: &[String],
    skill: &str,
    seed: u64,
    n: usize,
    exclude: &BTreeSet<String>,
    prefix: &str,
) -> Vec<Task> {
    let mut rng = Rng::new(seed);
    let active: Vec<&Claim> = claims.iter().filter(|c| exercisable(c, files).is_ok()).collect();
    let claim_globs: Vec<String> = claims
        .iter()
        .filter_map(|c| match &c.kind {
            ClaimKind::RegenerateAfter { glob: Some(g), .. } => Some(g.clone()),
            _ => None,
        })
        .collect();
    let control_files: Vec<String> = files
        .iter()
        .filter(|f| !claim_globs.iter().any(|g| !matching(g, std::slice::from_ref(f)).is_empty()))
        .filter(|f| !f.starts_with(".github/"))
        .cloned()
        .collect();
    let controls = if active.is_empty() { n } else { n.div_ceil(4).max(usize::from(n > 0)) };
    let triggers = n - controls;
    let explicit_n = (triggers * 3 + 5) / 10;
    let mut explicit: Vec<bool> = (0..triggers).map(|i| i < explicit_n).collect();
    rng.shuffle(&mut explicit);

    let mut out: Vec<Task> = Vec::new();
    let mut seen: BTreeSet<String> = exclude.clone();
    let mut push = |out: &mut Vec<Task>, mut t: Task, rng: &mut Rng, regen: &mut dyn FnMut(&mut Rng) -> Task| {
        // A prompt already used (in this set, or the excluded one) is drawn again, a few times.
        for _ in 0..8 {
            if seen.insert(t.prompt.clone()) {
                break;
            }
            t = regen(rng);
        }
        out.push(t);
    };
    for i in 0..triggers {
        let c = active[i % active.len()];
        let ex = explicit[i];
        let pick_target = |rng: &mut Rng| match &c.kind {
            ClaimKind::RegenerateAfter { glob: Some(g), .. } => {
                let m = matching(g, files);
                Some(m[rng.below(m.len())].clone())
            }
            _ => None,
        };
        let mut make = |rng: &mut Rng| {
            let template = rng.below(3);
            let verbosity = if rng.below(2) == 0 { Verbosity::Terse } else { Verbosity::Verbose };
            let target = pick_target(rng);
            let word = WORDS[rng.below(WORDS.len())];
            let lead = LEADS[rng.below(LEADS.len())];
            Task {
                id: String::new(),
                claim: Some(c.id.clone()),
                control: false,
                explicit: ex,
                verbosity,
                template,
                prompt: format!(
                    "{lead}{}",
                    trigger_prompt(&c.kind, template, verbosity, ex, skill, target.as_deref(), word)
                ),
                target,
            }
        };
        let t = make(&mut rng);
        push(&mut out, t, &mut rng, &mut make);
    }
    for _ in 0..controls {
        let mut make = |rng: &mut Rng| {
            let template = rng.below(3);
            let verbosity = if rng.below(2) == 0 { Verbosity::Terse } else { Verbosity::Verbose };
            let target = if template < 2 && !control_files.is_empty() {
                Some(control_files[rng.below(control_files.len())].clone())
            } else {
                None
            };
            Task {
                id: String::new(),
                claim: None,
                control: true,
                explicit: false,
                verbosity,
                template,
                prompt: format!(
                    "{}{}",
                    LEADS[rng.below(LEADS.len())],
                    control_prompt(template, verbosity, target.as_deref())
                ),
                target,
            }
        };
        let t = make(&mut rng);
        push(&mut out, t, &mut rng, &mut make);
    }
    rng.shuffle(&mut out);
    for (i, t) in out.iter_mut().enumerate() {
        t.id = format!("{prefix}{:02}", i + 1);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use adage_core::Effect;

    fn claim(id: &str, kind: ClaimKind) -> Claim {
        Claim { id: id.into(), kind, effect: Effect::Verify, r#unsafe: None, at: "x".into(), text: String::new() }
    }

    fn fixture() -> (Vec<Claim>, Vec<String>) {
        let claims = vec![
            claim("c1", ClaimKind::Verify { command: "python3 -m unittest".into() }),
            claim("c2", ClaimKind::RegenerateAfter { glob: Some("schema/*.json".into()), command: "make gen".into() }),
            claim(
                "c3",
                ClaimKind::Lookup {
                    tool: "scripts/wi-lookup".into(),
                    command: "scripts/wi-lookup W-2042".into(),
                    id_example: Some("W-2042".into()),
                },
            ),
            claim("c4", ClaimKind::RegenerateAfter { glob: Some("proto/*.proto".into()), command: "make gen".into() }),
        ];
        let files = ["Makefile", "README.md", "acme/billing.py", "schema/customer.json", "schema/invoice.json"];
        (claims, files.iter().map(|s| s.to_string()).collect())
    }

    #[test]
    fn generation_is_seeded_and_balanced() {
        let (claims, files) = fixture();
        let a = generate(&claims, &files, "acme", 7, 40, &BTreeSet::new(), "t");
        assert_eq!(a, generate(&claims, &files, "acme", 7, 40, &BTreeSet::new(), "t"));
        assert_ne!(a, generate(&claims, &files, "acme", 8, 40, &BTreeSet::new(), "t"));
        let controls = a.iter().filter(|t| t.control).count();
        assert!(controls * 4 >= a.len(), "{controls}");
        let triggers: Vec<&Task> = a.iter().filter(|t| !t.control).collect();
        let explicit = triggers.iter().filter(|t| t.explicit).count();
        assert_eq!(explicit, (triggers.len() * 3 + 5) / 10);
        // The claim with no matching file gets no tasks.
        assert!(!a.iter().any(|t| t.claim.as_deref() == Some("c4")));
        // Implicit prompts never name the skill or the command.
        for t in triggers.iter().filter(|t| !t.explicit) {
            for w in ["acme", "make gen", "unittest", "wi-lookup"] {
                assert!(!t.prompt.contains(w), "{}", t.prompt);
            }
        }
        // Regenerate prompts name a real matching file; controls never do.
        for t in &a {
            match (t.claim.as_deref(), &t.target) {
                (Some("c2"), Some(f)) => assert!(f.starts_with("schema/")),
                (None, Some(f)) => assert!(!f.starts_with("schema/"), "{f}"),
                _ => {}
            }
        }
        // Every template and verbosity is used.
        let shapes: BTreeSet<(usize, bool)> =
            triggers.iter().map(|t| (t.template, t.verbosity == Verbosity::Verbose)).collect();
        assert_eq!(shapes.len(), 6);
    }

    #[test]
    fn held_out_tasks_do_not_repeat_prompts() {
        let (claims, files) = fixture();
        let a = generate(&claims, &files, "acme", 1, 24, &BTreeSet::new(), "t");
        let seen: BTreeSet<String> = a.iter().map(|t| t.prompt.clone()).collect();
        let b = generate(&claims, &files, "acme", 2, 12, &seen, "v");
        assert!(b.iter().all(|t| !seen.contains(&t.prompt)), "{b:#?}");
        assert!(b.iter().all(|t| t.id.starts_with('v')));
    }
}
