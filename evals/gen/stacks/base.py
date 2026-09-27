"""What every stack shares: the tracker, the task texts, conventions.json, tasks.json, the reference
solutions and the live-runner configs. A stack subclass supplies only what differs between
languages and toolchains: file layout, naming, commands, and the text of each file.

A stack subclass must define:
  NAME, DEAD_END_MODES, LANG ("py" | "js" — which bugs.py variants it uses)
  naming:    fn(snake), module_path(module), reader_path(), generated_path()
  commands:  verify_cmd, regen_cmd, instinct_cmd, dead_end_runner(), lookup_cmd(), decoy_command(mode)
  texts:     dead_end_signature(), instinct_signature(), stale_signature()
  files:     repo_files() -> {rel: (text, executable)}, generated_registry(tables) -> text,
             hidden_test(task) -> (rel name, text), pristine_files() -> {rel: text}, scoring() -> dict
  rehearsal: obvious-runner variants in dead_end_variants()
"""

import json
import random
import re
import textwrap
import zlib

import bugs

# adage-core's ticket and path span patterns (crates/adage-core/src/spans.rs).
TICKET = re.compile(r"\b[A-Z][A-Z0-9]{0,9}-\d{1,7}\b")
PATH_SPAN = re.compile(r"(?:^|[\s`'(])((?:\.{0,2}/)?[A-Za-z0-9_.-]+(?:/[A-Za-z0-9_.-]+)*\.[A-Za-z0-9]{1,6})\b", re.M)

CONTROL_PROMPTS = (
    "In {path}, {problem}. Please fix it.",
    "There is a bug in {path}: {problem}. Can you fix it?",
    "Fix this in {path}, please: {problem}.",
)
BRIEF_PROMPTS = (
    "The change to make is described in BRIEF.md. Please make it.",
    "Please read BRIEF.md and do what it asks.",
    "BRIEF.md describes a fix this repository needs. Can you take care of it?",
)


def slug(task_id):
    return task_id.lower().replace("-", "_")


class Stack:
    NAME = ""
    LANG = ""
    DEAD_END_MODES = ()

    def __init__(self, plan):
        self.plan = plan
        self.skin = plan.skin
        self.conv = plan.conv

    # ------------------------------------------------------------------ naming and vocabulary

    def fn(self, snake):
        return snake

    def fname(self, template_id):
        return self.fn(self.skin.functions[template_id][0])

    def fill(self, text, template_id):
        """Fill a bugs.py text or code template with this skin's words, in this stack's style."""
        sk = self.skin
        name, phrase = sk.functions[template_id]
        return bugs.fill(
            text, fn=self.fn(name), name=self.fn(name), phrase=phrase, Phrase=phrase[:1].upper() + phrase[1:],
            field=sk.fields.get(template_id, "value"), const=sk.lookup_const, table=self.const_literal(),
            noun=sk.noun, lc=sk.lookup_lc, uc=sk.lookup_lc.upper())

    def const_literal(self):
        return "{" + ", ".join(f'"{k}": {v}' for k, v in sorted(self.skin.lookup_table.items())) + "}"

    def lookup_cmd(self):
        return f"./{self.skin.tool} {{item}}"

    def table(self, name):
        return next(t for t in self.skin.tables if t.name == name)

    # ------------------------------------------------------------------ code, with and without bugs

    def function_code(self, template_id, bugged):
        t = bugs.TEMPLATES[template_id]
        code = getattr(t, f"{self.LANG}_{'bad' if bugged else 'good'}")
        return self.fill(code, template_id)

    def test_body(self, template_id):
        return self.fill(getattr(bugs.TEMPLATES[template_id], f"{self.LANG}_test"), template_id)

    def csv_text(self, table):
        return "\n".join([",".join(table.columns)] + [",".join(r) for r in table.rows]) + "\n"

    def stable_row(self, table):
        """A row no registry task in the skin's pool changes: safe for visible tests."""
        changed = {rt.code for rt in self.skin.registry_tasks if rt.table == table.name and rt.op == "change"}
        return next(r for r in table.rows if r[0] not in changed)

    @staticmethod
    def typed(v):
        return int(v) if v.isdigit() else v

    # ------------------------------------------------------------------ the tracker

    def items(self):
        """Every tracker item: the task items plus closed decoys, in a seeded order."""
        rng = random.Random(f"adage-gen/tracker/{self.plan.stack}/{self.skin.name}/{self.plan.seed}")
        out = []
        for t in self.plan.tasks:
            if t.kind == "control":
                continue
            item = {"id": t.id, "status": "open", "priority": rng.choice(["P1", "P2", "P3"]),
                    "reported": f"2026-{rng.randint(7, 9):02d}-{rng.randint(1, 28):02d}",
                    "reporter": rng.choice(list(self.skin.reporters))}
            if t.kind == "bugfix":
                tpl = bugs.TEMPLATES[t.template]
                item.update(title=self.fill(tpl.title, t.template), description=self.fill(tpl.description, t.template),
                            acceptance=self.fill(tpl.acceptance, t.template), area=t.module,
                            files=[self.module_path(t.module)] if self.conv.linked_files else [])
            else:
                rt = t.registry
                item.update(title=rt.title, description=rt.description, acceptance=self.registry_acceptance(rt),
                            area="registry",
                            files=[f"registry/{rt.table}.csv", self.reader_path()] if self.conv.linked_files else [])
            out.append(item)
        for d in self.plan.decoy_items:
            out.append({"id": d["id"], "status": d["status"], "priority": d["priority"], "reported": d["reported"],
                        "reporter": d["reporter"], "title": d["title"], "description": d["description"],
                        "acceptance": "None recorded.", "area": d["area"], "files": []})
        rng.shuffle(out)
        return out

    def registry_acceptance(self, rt):
        tab = self.table(rt.table)
        reader = self.fn(tab.noun)
        vals = " and ".join(f"{k} {v}" for k, v in rt.values.items())
        return f"Looking up {rt.code} with {reader} gives {vals}, also when the code is typed in lower case."

    def store_bytes(self):
        body = json.dumps(self.items(), sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode()
        return self.skin.magic.encode() + zlib.compress(body, 9)

    @staticmethod
    def render_item(item):
        """The lookup tool's plain-text output (both tool implementations print exactly this)."""
        wrap = lambda s: textwrap.wrap(s, 88) or [""]  # noqa: E731
        out = [f"{item['id']}  {item['title']}",
               f"status: {item['status']}   priority: {item['priority']}   area: {item['area']}   "
               f"reported: {item['reported']} by {item['reporter']}", ""]
        out += wrap(item["description"])
        out += ["", "Acceptance:"] + [f"  {x}" for x in wrap(item["acceptance"])]
        out += ["", "Files:"] + ([f"  {f}" for f in item["files"]] or ["  (none)"])
        return "\n".join(out) + "\n"

    # ------------------------------------------------------------------ tasks and solutions

    def control_problem(self, t):
        return self.fill(bugs.TEMPLATES[t.template].control, t.template)

    def prompts(self, t):
        if t.kind == "control":
            return [p.format(path=self.module_path(t.module), problem=self.control_problem(t)) for p in CONTROL_PROMPTS]
        return [p.format(id=t.id) for p in self.skin.prompts]

    def solution(self, t):
        """The reference solution: exact text edits, then commands to run."""
        if t.kind in ("bugfix", "control"):
            path = self.module_path(t.module)
            return {"edits": [{"file": path, "old": self.function_code(t.template, True),
                               "new": self.function_code(t.template, False)}], "run": []}
        rt = t.registry
        tab = self.table(rt.table)
        path = f"registry/{tab.name}.csv"
        header = ",".join(tab.columns)
        if rt.op == "add":
            row = ",".join([rt.code] + [rt.values[c] for c in tab.columns[1:]])
            edit = {"file": path, "old": header + "\n", "new": header + "\n" + row + "\n"}
        else:
            old = next(r for r in tab.rows if r[0] == rt.code)
            new = [rt.values.get(c, v) for c, v in zip(tab.columns, old)]
            edit = {"file": path, "old": ",".join(old) + "\n", "new": ",".join(new) + "\n"}
        return {"edits": [edit], "run": [self.regen_cmd]}

    def exercises(self, t):
        if t.kind == "control":
            return []
        out = ["lookup"] + (["linked-files"] if self.conv.linked_files else [])
        if t.kind == "registry":
            out.append("finish-line")
        return out

    def task_files(self, t):
        if t.kind == "registry":
            return [f"registry/{t.registry.table}.csv"]
        return [self.module_path(t.module)]

    def hidden_count(self, text):
        raise NotImplementedError

    # ------------------------------------------------------------------ ground truth

    def conventions(self):
        c, sk = self.conv, self.skin
        out = []
        ids = [t.id for t in self.plan.tasks if t.kind != "control"]
        if c.lookup:
            digits = sorted(len(i.split("-")[1]) for i in ids) or list(self.plan.spec["id_digits"])
            lo, hi = digits[0], digits[-1]
            rep = f"{{{lo}}}" if lo == hi else f"{{{lo},{hi}}}"
            out.append({"id": "lookup", "kind": "lookup", "command": self.lookup_cmd(),
                        "id_pattern": f"^{re.escape(sk.id_prefix)}-\\d{rep}$",
                        "notes": f"Task prompts name only an item ID. Items live in {sk.tracker_phrase}: an opaque "
                                 f"store ({sk.store}) that only {sk.tool} reads. The README mentions only "
                                 f"{sk.tracker_phrase}."})
        if c.linked_files:
            out.append({"id": "linked-files", "kind": "linked_files", "command": self.lookup_cmd(),
                        "notes": "The lookup output lists the files to change under 'Files:' (the `files` field "
                                 "with --json). Registry items list the CSV and the module that reads it, never "
                                 "the generated file."})
        if c.dead_end:
            out.append({"id": "dead-end", "kind": "dead_end", "mode": c.dead_end, "command": self.verify_cmd,
                        "avoided": self.dead_end_runner(), "signature": self.dead_end_signature(),
                        "notes": self.dead_end_notes()})
        if c.instinct_violation:
            out.append({"id": "instinct-violation", "kind": "instinct_violation", "command": self.verify_cmd,
                        "avoided": self.instinct_cmd, "signature": self.instinct_signature(),
                        "notes": f"The universal default ({self.instinct_cmd}) is retired here and fails; the suite "
                                 f"runs with {self.verify_cmd}, declared only in .ci/pipeline.yml."})
        if c.finish_line:
            out.append({"id": "finish-line", "kind": "finish_line", "command": self.regen_cmd, "glob": "registry/*.csv",
                        "signature": self.stale_signature(),
                        "notes": f"Lookups read {self.generated_path()}, generated from registry/*.csv. Editing a CSV "
                                 f"requires {self.regen_cmd}; {self.verify_cmd} and the hidden checks fail on stale "
                                 f"output."})
        for mode in c.decoys:
            out.append({"id": f"decoy-{mode.replace('_', '-')}", "kind": "decoy", "mode": mode,
                        "command": self.decoy_command(mode), "notes": self.decoy_notes(mode)})
        return out

    def decoy_notes(self, mode):
        return {
            "unused_target": "A plausible target nothing depends on. Running it changes nothing.",
            "lookalike_regen": "Looks like a regeneration step but generates nothing tracked; it is never required "
                               "after any edit.",
            "lookalike_verify": "A lint step that always passes; it is not the test suite and not a dead end.",
            "legacy_script": "A leftover script that does nothing; no task needs it.",
        }[mode]

    def session_conventions(self):
        return [x for x in ("dead-end", "instinct-violation")
                if (x == "dead-end" and self.conv.dead_end) or (x == "instinct-violation" and self.conv.instinct_violation)]

    def tasks_json(self, hidden_names):
        out = []
        for t in self.plan.tasks:
            e = {"id": t.id, "kind": t.kind, "split": t.split, "prompts": self.prompts(t),
                 "exercises": self.exercises(t), "files": self.task_files(t), "hidden": f"hidden/{hidden_names[t.id]}"}
            if t.kind == "control":
                e["summary"] = self.control_problem(t)
            elif t.kind == "bugfix":
                e["summary"] = self.fill(bugs.TEMPLATES[t.template].title, t.template)
            else:
                e["summary"] = t.registry.title
            out.append(e)
        return {"name": self.plan.name, "stack": self.NAME, "skin": self.skin.name, "seed": self.plan.seed,
                "repo": self.skin.repo, "verify": self.verify_cmd,
                "session_conventions": self.session_conventions(), "tasks": out}

    # ------------------------------------------------------------------ live runner configs

    def classify(self):
        raise NotImplementedError

    def live_configs(self, spec_json):
        seed = str(self.plan.seed)
        # The runner str.format()s every argv element ({repo}, {dir}, {task}): escape the JSON's braces.
        spec_json = spec_json.replace("{", "{{").replace("}", "}}")
        gen =["python3", "{repo}/evals/gen/gen.py", "--spec-json", spec_json, "--seed", seed,
               "--repo-only", "{dir}", "--task", "{task}"]
        score = ["python3", "{repo}/evals/gen/score.py", "--spec-json", spec_json, "--seed", seed, "{dir}", "{task}"]
        base = {
            "repo_key": self.skin.repo, "workdir": self.skin.repo, "sandbox": gen, "score": score,
            "max_turns": 30, "concurrency": 3, "learn_args": ["--demo"], "classify": self.classify(),
            "labels": {"lookup": f"{self.skin.tracker_phrase} lookup", "regen": self.regen_cmd},
        }
        if self.conv.finish_line:
            base["obligation"] = {"paths": "registry/*.csv"}
        split = lambda kinds, s: [t.id for t in self.plan.tasks if t.kind in kinds and t.split == s]  # noqa: E731
        out = {}
        if split(("bugfix", "registry"), "train") and split(("bugfix", "registry"), "eval"):
            out["config.json"] = {
                "name": f"{self.plan.name}-s{seed}",
                "description": f"Generated sandbox {self.skin.repo} ({self.NAME}, skin {self.skin.name}, seed {seed}): "
                               f"tracker tasks",
                **base, "prompts": list(self.skin.prompts),
                "train": {"tasks": split(("bugfix", "registry"), "train"), "repeats": 2},
                "eval": {"tasks": split(("bugfix", "registry"), "eval"), "repeats": 3, "arms": ["holdout", "adage"]},
                "budget_usd": 20.0,
            }
        if split(("control",), "train") and split(("control",), "eval"):
            out["config-control.json"] = {
                "name": f"{self.plan.name}-s{seed}-control",
                "description": f"Generated sandbox {self.skin.repo} ({self.NAME}, seed {seed}): control tasks, which "
                               f"exercise no task convention (the brief is written to BRIEF.md)",
                **base, "prompts": list(BRIEF_PROMPTS),
                "train": {"tasks": split(("control",), "train"), "repeats": 2},
                "eval": {"tasks": split(("control",), "eval"), "repeats": 3, "arms": ["holdout", "adage"]},
                "budget_usd": 10.0,
            }
        return out

    def brief(self, task_id):
        t = next((x for x in self.plan.tasks if x.id == task_id), None)
        if t is None or t.kind != "control":
            return None
        return f"# Brief\n\n{self.prompts(t)[0]}\n"
