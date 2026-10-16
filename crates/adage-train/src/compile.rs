//! Procedure graph → routes: what each step of a skill compiles to, in the pack route model
//! (docs/packs.md), and why a step does not.
//!
//! | Step | Route |
//! | --- | --- |
//! | a lookup whose slot the prompt carries (`scripts/wi-lookup {id}`) | `run` on a prompt span, the slot typed by its example |
//! | "read the linked files" after it | `read` after that command, of the paths its output lists |
//! | a command after an edit to a glob (`make gen` after `schema/*.json`) | `obligation` after that edit: checked at Stop |
//! | a verify step (`make test`) | `obligation` after an edit to the code the skill's edit step names |
//! | a verify step, then a "before you finish" verify step | a `sequence` after a code edit, which carries both |
//! | "before you finish, run `make ci`" | `obligation` after **any** edit (`*`) |
//! | a forbidden command with the failure signature | `avoid` at session start |
//! | "run the tests of the module you changed" (`tests/test_{module}.py`) | kept as text: requires runner-qualified target evidence absent from skill compilation |
//! | "do not commit" | the fixed advice `uncommitted` after any edit |
//!
//! Everything else stays text, with the reason: an edit or a decision (needs judgment), a slot the
//! agent chooses, a command with no stimulus Adage can see, an environment or history command
//! (never directed), a command that is not one plain argv, a script the repository does not have.
//! [`crate::verify`] then replays every candidate in the sandbox.

use crate::procedure::{Graph, Pre, Role, SlotFrom, Step};
use adage_core::{CommandTable, Effect};
use adage_pack::validate::{argv_token_ok, program, rank, Program};
use adage_pack::{
    AdviseSpec, AvoidSpec, ExitV2, Input, InputFrom, PromptSource, ReadSpec, RepoFile, ResultSource, RouteEvidence,
    RouteOrigin, RouteV2, RunSpec, ScriptRef, Shape, StepV2, Strength, TriggerV2, TypeDef,
};
use adage_tracks::io;
use once_cell::sync::Lazy;
use regex::Regex;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

/// What a replay must see for a candidate to be verified.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Check {
    /// Exit 0, with each of `expect` in the output. A regenerate step also reports whether it
    /// changed a clean checkout.
    Works { argv: Vec<String>, expect: Vec<String>, regenerate: bool },
    /// A forbidden runner: it must fail (the signature is what the skill says it fails with).
    Fails { argv: Vec<String>, signature: String },
    /// What a forbidden runner's `instead` does (reported, never a reason to drop the route).
    Instead { argv: Vec<String> },
}

/// Whether a candidate's replay verified it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "status", content = "why", rename_all = "snake_case")]
pub enum Verified {
    /// Not replayed (no sandbox here, no example value, nothing to run), and why.
    NotReplayed(String),
    /// Every check held on a clean checkout.
    Works,
    /// A check failed: the skill is stale here.
    Stale(String),
}

/// A route a step compiles to, before replay.
#[derive(Debug, Clone, Serialize)]
pub struct Candidate {
    /// The steps it compiles (`s4`, `s5` for a sequence).
    pub steps: Vec<String>,
    pub route: RouteV2,
    pub checks: Vec<Check>,
    /// What replaying each check did, one line each.
    pub replay: Vec<String>,
    pub verified: Verified,
}

/// A block of advice, and the route a fixed template made of it.
#[derive(Debug, Clone, Serialize)]
pub struct Advice {
    pub text: String,
    pub route: Option<String>,
}

/// A compiled skill before it is packed.
#[derive(Debug, Clone, Serialize)]
pub struct Compiled {
    pub graph: Graph,
    /// The route id prefix (the skill's name, as an id).
    pub prefix: String,
    /// The glob of the code the skill's edit step changes, if it names one.
    pub code_glob: Option<String>,
    pub candidates: Vec<Candidate>,
    /// Per step, why (part of) it stays text.
    pub text: BTreeMap<String, Vec<String>>,
    /// Per step, why a command was dropped by replay.
    pub stale: BTreeMap<String, Vec<String>>,
    /// The candidates a replay found stale, with their [`Verified::Stale`]: out of the pack, and
    /// kept so that `verified.json` names every compiled step and what its replay found.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub dropped: Vec<Candidate>,
    pub advice: Vec<Advice>,
    pub types: BTreeMap<String, TypeDef>,
    pub repo_files: BTreeMap<String, RepoFile>,
}

static READ_LINKED: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?i)\bread (the )?(linked|listed) files\b").unwrap());
static UNCOMMITTED: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i)\b(do not|don't|never)\s+commit\b|\bleave (your |the )?(changes|working tree)\b[^.]*\b(uncommitted|for review)\b")
        .unwrap()
});

/// A skill's name as a route id prefix: lowercase letters, digits and `-`, at most 32.
pub fn id_prefix(name: &str) -> String {
    let mut s: String = name
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    s.truncate(32);
    let s = s.trim_end_matches('-').to_string();
    if s.is_empty() {
        "skill".into()
    } else {
        s
    }
}

/// A route summary (for people, in review): the step's words without the characters a summary
/// may not hold (`io::safe_text`), at most 160 bytes.
fn summary(s: &str) -> String {
    let t: String = adage_pack::procedure::clean_text(s).chars().filter(|c| !"`<>\"\\$;|&{}".contains(*c)).collect();
    let mut t = t.split_whitespace().collect::<Vec<_>>().join(" ");
    if t.len() > 160 {
        let mut cut = 157;
        while !t.is_char_boundary(cut) {
            cut -= 1;
        }
        t.truncate(cut);
        t.push_str("...");
    }
    t
}

fn argv_of(template: &str) -> Vec<String> {
    template.split_whitespace().map(str::to_string).collect()
}

/// The glob of the code the skill's edit step changes (`acme/` → `acme/**`), else the first glob
/// a verify step runs after.
fn code_glob(g: &Graph) -> Option<String> {
    g.steps.iter().filter(|s| s.role == Role::Edit).flat_map(|s| s.edits.iter()).next().cloned().or_else(|| {
        g.steps.iter().find_map(|s| match (&s.pre, s.role) {
            (Pre::AfterEdit(glob), Role::Verify) => Some(glob.clone()),
            _ => None,
        })
    })
}

fn evidence(g: &Graph) -> RouteEvidence {
    RouteEvidence {
        provenance: format!("skill:{}", &g.skill.sha256[..12]),
        origin: Some(RouteOrigin::Authored),
        ..RouteEvidence::default()
    }
}

fn run_step(id: &str, argv: Vec<String>, effect: Effect, script: Option<ScriptRef>) -> StepV2 {
    StepV2 {
        id: id.into(),
        run: Some(RunSpec { argv }),
        read: None,
        avoid: None,
        advise: None,
        effect,
        script,
        outputs: BTreeMap::new(),
    }
}

/// The strength a shape takes at its effect's ceiling.
fn strength_for(effect: Effect, by_edit: bool) -> Option<Strength> {
    adage_tracks::ceiling(effect, by_edit)
}

struct Cx<'a> {
    g: &'a Graph,
    root: &'a Path,
    table: &'a CommandTable,
    out: Compiled,
}

/// Why a template cannot be a pack step, if it cannot; else its argv, script pin and effect.
struct Plain {
    argv: Vec<String>,
    script: Option<ScriptRef>,
    effect: Effect,
}

impl Cx<'_> {
    fn id(&self, step: &str, kind: &str) -> String {
        format!("{}-{step}-{kind}", self.out.prefix)
    }

    fn why(&mut self, step: &str, reason: impl Into<String>) {
        let v = self.out.text.entry(step.to_string()).or_default();
        let r = reason.into();
        if !v.contains(&r) {
            v.push(r);
        }
    }

    fn push(&mut self, steps: &[&str], route: RouteV2, checks: Vec<Check>) {
        if self.out.candidates.iter().any(|c| c.route.id == route.id) {
            return;
        }
        self.out.candidates.push(Candidate {
            steps: steps.iter().map(|s| s.to_string()).collect(),
            route,
            checks,
            replay: Vec::new(),
            verified: Verified::NotReplayed("not replayed yet".into()),
        });
    }

    /// The argv, script pin and effect of a template, or why it cannot be a step.
    fn plain(&mut self, template: &str) -> Result<Plain, String> {
        let argv = argv_of(template);
        if argv.is_empty() || argv.len() > 32 || !argv.iter().all(|t| argv_token_ok(t)) {
            return Err(format!(
                "`{template}` is not one plain command (shell syntax, quoting or a path outside the repository)"
            ));
        }
        let prog = program(&argv, self.table).map_err(|e| format!("`{template}`: {e:#}"))?;
        let script = match prog {
            Program::Tool(_) => None,
            Program::Env { path, .. } => {
                // The project's environment: bound where the checkout has it, never pinned.
                if !self.root.join(&path).exists() {
                    return Err(format!(
                        "`{template}` runs {path} from the project's environment, which this checkout does not have (set it up, then compile again)"
                    ));
                }
                None
            }
            Program::Script { path, .. } => {
                if std::fs::symlink_metadata(self.root.join(&path)).is_err() {
                    return Err(format!(
                        "`{template}` runs {path}, which is not in the repository: the agent writes it, so nothing can pin it"
                    ));
                }
                let sha = adage_pack::convert::hash_repo_file(self.root, &path)
                    .map_err(|e| format!("`{template}` runs {path}, which cannot be pinned ({e:#})"))?;
                self.out
                    .repo_files
                    .insert(path.clone(), RepoFile { path: path.clone(), sha256: sha.clone(), deps: Vec::new() });
                Some(ScriptRef { path, closure: adage_pack::ClosureLevel::Ref, sha256: sha, deps: Vec::new() })
            }
        };
        let effect = io::classify(template, self.table);
        Ok(Plain { argv, script, effect })
    }

    fn avoid(&mut self, s: &Step) {
        let instead = s.avoid[0].instead.clone();
        if instead.is_empty() {
            self.why(&s.id, "it forbids a command but names nothing to run instead");
            return;
        }
        let inst = match self.plain(&instead) {
            Ok(p) if p.script.is_none() && crate::procedure::slot_names(&instead).is_empty() => p,
            Ok(_) => {
                self.why(&s.id, format!("its instead, `{instead}`, is not a plain command with no slots"));
                return;
            }
            Err(e) => {
                self.why(&s.id, e);
                return;
            }
        };
        if rank(inst.effect) > rank(Effect::Regenerate) {
            self.why(&s.id, format!("its instead, `{instead}`, is not a verify or regenerate step"));
            return;
        }
        let mut runners = Vec::new();
        for a in &s.avoid {
            let argv = argv_of(&a.command);
            if argv.is_empty() || !argv.iter().all(|t| argv_token_ok(t) && !t.contains('{')) {
                self.why(&s.id, format!("the forbidden `{}` is not one plain command", a.command));
                continue;
            }
            runners.push(argv);
        }
        if runners.is_empty() {
            return;
        }
        let signature = s.avoid[0].signature.clone();
        let mut checks: Vec<Check> =
            runners.iter().map(|argv| Check::Fails { argv: argv.clone(), signature: signature.clone() }).collect();
        checks.push(Check::Instead { argv: inst.argv.clone() });
        let route = RouteV2 {
            id: self.id(&s.id, "avoid"),
            summary: summary(&format!("{}: not {}", s.title, s.avoid[0].command)),
            shape: Shape::Avoid,
            detector: None,
            trigger: TriggerV2::SessionStart,
            entry: true,
            inputs: BTreeMap::new(),
            steps: vec![StepV2 {
                id: "avoid".into(),
                run: None,
                read: None,
                avoid: Some(AvoidSpec { runners, instead: inst.argv, signature }),
                advise: None,
                effect: inst.effect,
                script: None,
                outputs: BTreeMap::new(),
            }],
            links: Vec::new(),
            strength: Strength::Inhibit,
            exit: vec![ExitV2::IntentChanges],
            exec: None,
            evidence: Some(evidence(self.g)),
        };
        self.push(&[&s.id], route, checks);
    }

    fn command(&mut self, s: &Step, c: &crate::procedure::Command) {
        let slots: Vec<&crate::procedure::SlotDef> = c.slots.iter().filter_map(|n| self.g.slots.get(n)).collect();
        if let Some(j) = slots.iter().find(|x| x.from == SlotFrom::Judgment) {
            self.why(
                &s.id,
                format!(
                    "`{}`: slot {{{}}} is the agent's choice (needs judgment: no stimulus binds it)",
                    c.template, j.name
                ),
            );
            return;
        }
        if slots.iter().any(|x| x.from == SlotFrom::Edit) {
            self.edit_tests(s, c);
            return;
        }
        let p = match self.plain(&c.template) {
            Ok(p) => p,
            Err(e) => {
                self.why(&s.id, e);
                return;
            }
        };
        if !slots.is_empty() {
            self.prompt_run(s, c, p);
            return;
        }
        let glob = match (&s.pre, s.role) {
            (Pre::AfterEdit(g), _) => Some(g.clone()),
            (Pre::BeforeFinish, _) => Some("*".to_string()),
            (Pre::Judgment, _) => {
                self.why(&s.id, format!("`{}` runs on a condition that needs judgment", c.template));
                return;
            }
            (Pre::Always, Role::Verify) => Some(self.out.code_glob.clone().unwrap_or_else(|| "*".into())),
            (Pre::Always, _) => None,
        };
        let Some(glob) = glob.filter(|g| adage_pack::validate::safe_glob(g)) else {
            let why = if p.effect == Effect::Observe {
                format!(
                    "`{}` has no stimulus Adage can see (an intent trigger could deliver it, with the embedding model)",
                    c.template
                )
            } else {
                format!("`{}` has no stimulus Adage can see: it follows the agent's own judgment", c.template)
            };
            self.why(&s.id, why);
            return;
        };
        let Some(ceiling) = strength_for(p.effect, true) else {
            self.why(
                &s.id,
                format!("`{}` is {}: Adage never directs it (effect ceiling)", c.template, p.effect.as_str()),
            );
            return;
        };
        let kind = if s.pre == Pre::BeforeFinish { "before-finish".to_string() } else { s.role.as_str().to_string() };
        let route = RouteV2 {
            id: self.id(&s.id, &kind),
            summary: summary(&s.title),
            shape: Shape::Obligation,
            detector: None,
            trigger: TriggerV2::AfterEdit { paths: glob },
            entry: true,
            inputs: BTreeMap::new(),
            steps: vec![run_step("run", p.argv.clone(), p.effect, p.script)],
            links: Vec::new(),
            strength: ceiling,
            exit: vec![ExitV2::ExploratoryChange],
            exec: None,
            evidence: Some(evidence(self.g)),
        };
        let check = Check::Works { argv: p.argv, expect: s.expect.clone(), regenerate: p.effect == Effect::Regenerate };
        self.push(&[&s.id], route, vec![check]);
    }

    /// A command whose slots the prompt carries (a work item id): a run on the prompt, and a read
    /// of the files its output lists when the step says to read them.
    fn prompt_run(&mut self, s: &Step, c: &crate::procedure::Command, p: Plain) {
        let slots: Vec<crate::procedure::SlotDef> =
            c.slots.iter().filter_map(|n| self.g.slots.get(n).cloned()).collect();
        let Some(ceiling) = strength_for(p.effect, false) else {
            self.why(
                &s.id,
                format!("`{}` is {}: Adage never directs it (effect ceiling)", c.template, p.effect.as_str()),
            );
            return;
        };
        let mut inputs = BTreeMap::new();
        for sl in &slots {
            let ty = sl.kind.clone();
            if adage_pack::types::is_builtin(&ty) || !adage_pack::validate::safe_id(&ty) {
                self.why(&s.id, format!("`{}`: slot {{{}}} has no pack type", c.template, sl.name));
                return;
            }
            self.out.types.entry(ty.clone()).or_insert_with(|| TypeDef::pattern(&ty, &sl.pattern));
            inputs.insert(
                sl.name.clone(),
                Input {
                    ty,
                    from: InputFrom {
                        prompt: Some(PromptSource {
                            span: adage_core::spans::SpanKind::Ticket,
                            vtype: None,
                            mentions: vec![],
                        }),
                        ..InputFrom::default()
                    },
                    values: None,
                    max: None,
                },
            );
        }
        let route = RouteV2 {
            id: self.id(&s.id, s.role.as_str()),
            summary: summary(&s.title),
            shape: Shape::Run,
            detector: None,
            trigger: TriggerV2::Prompt { span: adage_core::spans::SpanKind::Ticket },
            entry: true,
            inputs,
            steps: vec![run_step("run", p.argv.clone(), p.effect, p.script)],
            links: Vec::new(),
            strength: ceiling,
            exit: vec![ExitV2::IntentChanges],
            exec: None,
            evidence: Some(evidence(self.g)),
        };
        // Replayed with each slot's example, when the skill gives one.
        let bound: Option<Vec<String>> = p
            .argv
            .iter()
            .map(|t| {
                let mut t = t.clone();
                for sl in &slots {
                    let ph = format!("{{{}}}", sl.name);
                    if t.contains(&ph) {
                        t = t.replace(&ph, sl.example.as_deref()?);
                    }
                }
                Some(t)
            })
            .collect();
        let checks = match bound {
            Some(argv) => vec![Check::Works { argv, expect: s.expect.clone(), regenerate: false }],
            None => Vec::new(),
        };
        let key = adage_pack::closure::argv_key(&p.argv);
        self.push(&[&s.id], route, checks);
        if READ_LINKED.is_match(&s.text) && adage_pack::validate::command_key_ok(&key) {
            let route = RouteV2 {
                id: self.id(&s.id, "read-linked"),
                summary: "Read the files it lists, together".into(),
                shape: Shape::Read,
                detector: None,
                trigger: TriggerV2::AfterCommand { command: key },
                entry: true,
                inputs: BTreeMap::from([(
                    "files".to_string(),
                    Input {
                        ty: "repo-path-list".into(),
                        from: InputFrom {
                            result: Some(ResultSource { field: "*paths".into() }),
                            ..InputFrom::default()
                        },
                        values: None,
                        max: Some(5),
                    },
                )]),
                steps: vec![StepV2 {
                    id: "read".into(),
                    run: None,
                    read: Some(ReadSpec { from: "files".into(), max: 5 }),
                    avoid: None,
                    advise: None,
                    effect: Effect::Observe,
                    script: None,
                    outputs: BTreeMap::new(),
                }],
                links: Vec::new(),
                strength: Strength::Prime,
                exit: vec![ExitV2::IntentChanges],
                exec: None,
                evidence: Some(evidence(self.g)),
            };
            self.push(&[&s.id], route, Vec::new());
        }
    }

    /// A dynamic edited-file template needs target/runner evidence for every binding. Skill
    /// compilation has no trace/discovery input; replaying the first static pair cannot validate
    /// the rest of a name/co-change map. Preserve the instruction rather than emit that map.
    fn edit_tests(&mut self, s: &Step, c: &crate::procedure::Command) {
        self.why(
            &s.id,
            format!(
                "`{}`: runner-qualified target execution evidence is unavailable for the edited-file template; names and co-change alone do not establish selected tests; keep this instruction or use explicit checks",
                c.template
            ),
        );
    }

    /// A verify step followed by a "before you finish" verify step: one sequence after a code
    /// edit, which carries both finish lines (one edit, one directive).
    fn sequence(&mut self, a: &Step, b: &Step) {
        let Some(code) = self.out.code_glob.clone() else { return };
        let (Some(ca), Some(cb)) = (a.commands.first(), b.commands.first()) else { return };
        if !ca.slots.is_empty() || !cb.slots.is_empty() || ca.template == cb.template {
            return;
        }
        let (Ok(pa), Ok(pb)) = (self.plain(&ca.template), self.plain(&cb.template)) else { return };
        let effect = if rank(pa.effect) >= rank(pb.effect) { pa.effect } else { pb.effect };
        let Some(ceiling) = strength_for(effect, true) else { return };
        let route = RouteV2 {
            id: self.id(&format!("{}-{}", a.id, b.id), "sequence"),
            summary: summary(&format!("{}, then {}", a.title, b.title)),
            shape: Shape::Sequence,
            detector: None,
            trigger: TriggerV2::AfterEdit { paths: code },
            entry: true,
            inputs: BTreeMap::new(),
            steps: vec![
                run_step("first", pa.argv.clone(), pa.effect, pa.script),
                run_step("then", pb.argv.clone(), pb.effect, pb.script),
            ],
            links: Vec::new(),
            strength: ceiling,
            exit: vec![ExitV2::ExploratoryChange],
            exec: None,
            evidence: Some(evidence(self.g)),
        };
        let checks = vec![
            Check::Works { argv: pa.argv, expect: a.expect.clone(), regenerate: false },
            Check::Works { argv: pb.argv, expect: b.expect.clone(), regenerate: false },
        ];
        self.push(&[&a.id, &b.id], route, checks);
    }

    fn advice(&mut self, text: &str) {
        let mut route = None;
        if UNCOMMITTED.is_match(text) {
            let id = format!("{}-uncommitted", self.out.prefix);
            if !self.out.candidates.iter().any(|c| c.route.id == id) {
                let r = RouteV2 {
                    id: id.clone(),
                    summary: "Leave changes uncommitted".into(),
                    shape: Shape::Advise,
                    detector: None,
                    trigger: TriggerV2::AfterEdit { paths: "*".into() },
                    entry: true,
                    inputs: BTreeMap::new(),
                    steps: vec![StepV2 {
                        id: "advise".into(),
                        run: None,
                        read: None,
                        avoid: None,
                        advise: Some(AdviseSpec { template: "uncommitted".into(), slots: BTreeMap::new() }),
                        effect: Effect::Observe,
                        script: None,
                        outputs: BTreeMap::new(),
                    }],
                    links: Vec::new(),
                    strength: Strength::Prime,
                    exit: Vec::new(),
                    exec: None,
                    evidence: Some(evidence(self.g)),
                };
                self.push(&[], r, Vec::new());
            }
            route = Some(id);
        }
        self.out.advice.push(Advice { text: text.to_string(), route });
    }
}

/// Compile a procedure graph for the repository at `root` (scripts are pinned against it).
/// Nothing runs: [`crate::verify`] replays after.
pub fn compile(graph: Graph, root: &Path, table: &CommandTable) -> Compiled {
    let prefix = id_prefix(&graph.skill.name);
    let code_glob = code_glob(&graph).filter(|g| adage_pack::validate::safe_glob(g));
    let g = graph.clone();
    let mut cx = Cx {
        g: &g,
        root,
        table,
        out: Compiled {
            graph,
            prefix,
            code_glob,
            candidates: Vec::new(),
            text: BTreeMap::new(),
            stale: BTreeMap::new(),
            dropped: Vec::new(),
            advice: Vec::new(),
            types: BTreeMap::new(),
            repo_files: BTreeMap::new(),
        },
    };
    for (i, s) in g.steps.iter().enumerate() {
        if s.commands.is_empty() && s.avoid.is_empty() {
            let why = match s.role {
                Role::Edit => "needs judgment: the step is an edit the agent makes",
                _ => "needs judgment: the step directs no command",
            };
            cx.why(&s.id, why);
            continue;
        }
        if !s.avoid.is_empty() {
            cx.avoid(s);
        }
        for c in &s.commands {
            if matches!(c.role, Role::Install | Role::Vcs | Role::Remove) {
                cx.why(
                    &s.id,
                    format!("`{}` changes the environment or the history: Adage never directs it", c.template),
                );
                continue;
            }
            if matches!(c.role, Role::Edit) {
                cx.why(&s.id, format!("`{}` edits files: the agent's own change, never directed", c.template));
                continue;
            }
            cx.command(s, c);
        }
        if let Some(next) = g.steps.get(i + 1) {
            if s.role == Role::Verify && next.role == Role::Verify && next.pre == Pre::BeforeFinish {
                cx.sequence(s, next);
            }
        }
    }
    for a in &g.advice {
        cx.advice(a);
    }
    cx.out
}

impl Compiled {
    /// The route ids each step compiled to.
    pub fn routes_of(&self, step: &str) -> Vec<String> {
        self.candidates.iter().filter(|c| c.steps.iter().any(|s| s == step)).map(|c| c.route.id.clone()).collect()
    }

    /// Every program the candidates run (L-none tools and interpreters), for `requires.tools`.
    pub fn tools(&self, table: &CommandTable) -> BTreeSet<String> {
        let mut out = BTreeSet::new();
        for c in &self.candidates {
            for s in &c.route.steps {
                if let Some(r) = &s.run {
                    match program(&r.argv, table) {
                        Ok(Program::Tool(t)) => {
                            out.insert(t);
                        }
                        Ok(Program::Script { interpreter: Some(i), .. }) if !i.contains('/') => {
                            out.insert(i);
                        }
                        _ => {}
                    }
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::procedure;

    const ACME: &str = include_str!("../../../evals/lab/skills/skills/acme-work-item/SKILL.md");

    /// A checkout with the acme skill's script.
    fn acme_root() -> tempfile::TempDir {
        let t = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(t.path().join("scripts")).unwrap();
        std::fs::write(t.path().join("scripts/wi-lookup"), "#!/usr/bin/env python3\nprint('W')\n").unwrap();
        t
    }

    #[test]
    fn the_acme_skill_compiles_to_the_lab_s_routes() {
        let table = CommandTable::load();
        let root = acme_root();
        let g = procedure::parse(ACME.as_bytes(), "SKILL.md", "x", table);
        let c = compile(g, root.path(), table);
        let ids: Vec<&str> = c.candidates.iter().map(|c| c.route.id.as_str()).collect();
        assert_eq!(
            ids,
            [
                "acme-work-item-s1-lookup",
                "acme-work-item-s1-read-linked",
                "acme-work-item-s2-regenerate",
                "acme-work-item-s4-avoid",
                "acme-work-item-s4-verify",
                "acme-work-item-s4-s5-sequence",
                "acme-work-item-s5-before-finish",
                "acme-work-item-uncommitted",
            ]
        );
        let r = |id: &str| &c.candidates.iter().find(|x| x.route.id.ends_with(id)).unwrap().route;
        assert_eq!(r("s5-before-finish").trigger, TriggerV2::AfterEdit { paths: "*".into() });
        assert_eq!(r("s4-verify").trigger, TriggerV2::AfterEdit { paths: "acme/**".into() });
        assert_eq!(r("s2-regenerate").trigger, TriggerV2::AfterEdit { paths: "schema/*.json".into() });
        assert_eq!(r("s1-lookup").steps[0].script.as_ref().unwrap().path, "scripts/wi-lookup");
        assert!(c.types.contains_key("work-item-id"));
        assert_eq!(c.text["s3"], ["needs judgment: the step is an edit the agent makes"]);
        assert_eq!(c.advice.iter().filter(|a| a.route.is_some()).count(), 1);
        // The lookup is replayed with the skill's example id.
        assert_eq!(
            c.candidates[0].checks,
            [Check::Works {
                argv: vec!["scripts/wi-lookup".into(), "W-2042".into()],
                expect: vec![],
                regenerate: false
            }]
        );
    }

    #[test]
    fn a_missing_script_is_a_gap_not_a_route() {
        let table = CommandTable::load();
        let root = tempfile::tempdir().unwrap();
        let g = procedure::parse(ACME.as_bytes(), "SKILL.md", "x", table);
        let c = compile(g, root.path(), table);
        assert!(c.routes_of("s1").is_empty());
        assert!(c.text["s1"][0].contains("which is not in the repository"), "{:?}", c.text["s1"]);
    }
}
