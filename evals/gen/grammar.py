"""The convention grammar: typed convention kinds, the spec format, and resolution of a spec plus a
seed into a concrete plan (which functions are bugged, which tasks exist, which IDs they get).

A spec says *what* a sandbox must contain; the seed picks every concrete detail. Resolution is a
pure function of (spec, seed): all randomness comes from one `random.Random(seed)`, consumed in a
fixed order, so the same inputs always give the same plan.
"""

import json
import random
from dataclasses import dataclass, field

import bugs
from skins import SKINS

# Every kind a conventions.json entry may have. `scope` and `env_sync` are reserved in the schema
# (Adage does not mine them yet) and are not generated.
KINDS = ("lookup", "linked_files", "dead_end", "finish_line", "scope", "env_sync", "decoy", "instinct_violation")
GENERATED_KINDS = ("lookup", "linked_files", "dead_end", "finish_line", "decoy", "instinct_violation")
DEAD_END_MODES = ("env_guard", "path_alias", "wrong_runner")
DECOY_MODES = ("unused_target", "lookalike_regen", "lookalike_verify", "legacy_script")
TASK_KINDS = ("bugfix", "registry", "control")
MIN_CONTROL_SHARE = 0.25

SPEC_KEYS = {"name", "stack", "skin", "conventions", "tasks", "id_digits", "description"}
CONVENTION_KEYS = {"lookup", "linked_files", "dead_end", "finish_line", "instinct_violation", "decoys"}
TASK_KEYS = {"bugfix", "registry", "control"}


class SpecError(ValueError):
    pass


@dataclass
class Conventions:
    lookup: bool
    linked_files: bool
    dead_end: object          # a mode from DEAD_END_MODES, or None
    finish_line: bool
    instinct_violation: bool
    decoys: list              # modes from DECOY_MODES


@dataclass
class Task:
    id: str
    kind: str                 # bugfix | registry | control
    split: str                # train | eval
    template: object = None   # bugfix/control: a bugs.Template id
    module: object = None     # bugfix/control: the module holding the function
    registry: object = None   # registry: a skins.RegistryTask


@dataclass
class Plan:
    spec: dict                # the normalised spec (every default filled in)
    seed: int
    name: str
    stack: str
    skin: object
    conv: Conventions
    layout: dict              # module -> [template ids], in file order
    bugged: list              # template ids whose function carries the seeded bug
    tasks: list
    decoy_items: list = field(default_factory=list)   # closed tracker items: (id, title, status, description, area, files)

    def task(self, tid):
        return next(t for t in self.tasks if t.id == tid)


def _bool(v, where):
    if not isinstance(v, bool):
        raise SpecError(f"{where}: expected true or false, got {v!r}")
    return v


def _int(v, where, lo=0, hi=None):
    if not isinstance(v, int) or isinstance(v, bool) or v < lo or (hi is not None and v > hi):
        raise SpecError(f"{where}: expected an integer in [{lo}, {hi if hi is not None else '...'}], got {v!r}")
    return v


def normalise(spec, stacks):
    """Validate a spec and fill in its defaults. `stacks` maps stack names to Stack classes."""
    if not isinstance(spec, dict):
        raise SpecError("spec: expected a JSON object")
    unknown = set(spec) - SPEC_KEYS
    if unknown:
        raise SpecError(f"spec: unknown keys {sorted(unknown)}")
    stack = spec.get("stack")
    if stack not in stacks:
        raise SpecError(f"spec.stack: one of {sorted(stacks)}, got {stack!r}")
    skin = spec.get("skin")
    if skin not in SKINS:
        raise SpecError(f"spec.skin: one of {sorted(SKINS)}, got {skin!r}")
    sk = SKINS[skin]
    c = dict(spec.get("conventions") or {})
    unknown = set(c) - CONVENTION_KEYS
    if unknown:
        raise SpecError(f"spec.conventions: unknown keys {sorted(unknown)} (generated kinds: {', '.join(GENERATED_KINDS)})")
    lookup = _bool(c.get("lookup", True), "conventions.lookup")
    linked = _bool(c.get("linked_files", lookup), "conventions.linked_files")
    if linked and not lookup:
        raise SpecError("conventions.linked_files needs conventions.lookup (the lookup output lists the files)")
    dead = c.get("dead_end", "auto")
    modes = stacks[stack].DEAD_END_MODES
    if dead is False or dead is None:
        dead = None
    elif dead is True:
        dead = "auto"
    elif isinstance(dead, dict):
        dead = dead.get("mode", "auto")
    if dead is not None and dead != "auto" and dead not in modes:
        raise SpecError(f"conventions.dead_end: stack {stack} supports modes {list(modes)} (or \"auto\"), got {dead!r}")
    finish = _bool(c.get("finish_line", True), "conventions.finish_line")
    instinct = _bool(c.get("instinct_violation", True), "conventions.instinct_violation")
    decoys = c.get("decoys", ["unused_target", "lookalike_regen"])
    if isinstance(decoys, int) and not isinstance(decoys, bool):
        decoys = list(DECOY_MODES[:_int(decoys, "conventions.decoys", 0, len(DECOY_MODES))])
    if not isinstance(decoys, list) or any(d not in DECOY_MODES for d in decoys) or len(set(decoys)) != len(decoys):
        raise SpecError(f"conventions.decoys: a count or a list of distinct modes from {list(DECOY_MODES)}")
    t = dict(spec.get("tasks") or {})
    unknown = set(t) - TASK_KEYS
    if unknown:
        raise SpecError(f"spec.tasks: unknown keys {sorted(unknown)}")
    n_bug = _int(t.get("bugfix", 4), "tasks.bugfix", 0, len(bugs.IDS))
    n_reg = _int(t.get("registry", 2 if finish else 0), "tasks.registry", 0, len(sk.registry_tasks))
    n_ctl = _int(t.get("control", 2), "tasks.control", 0, len(bugs.IDS))
    if n_bug + n_ctl > len(bugs.IDS):
        raise SpecError(f"tasks.bugfix + tasks.control: at most {len(bugs.IDS)} (one bug template each)")
    if n_reg and not finish:
        raise SpecError("tasks.registry needs conventions.finish_line")
    if (n_bug or n_reg) and not lookup:
        raise SpecError("tasks.bugfix and tasks.registry are named by tracker ID only, so they need conventions.lookup")
    total = n_bug + n_reg + n_ctl
    if total == 0:
        raise SpecError("spec.tasks: at least one task")
    if n_ctl < MIN_CONTROL_SHARE * total:
        raise SpecError(f"spec.tasks: control tasks must be at least {int(MIN_CONTROL_SHARE * 100)}% of all tasks "
                        f"({n_ctl} of {total})")
    digits = spec.get("id_digits", list(sk.id_digits))
    if (not isinstance(digits, list) or len(digits) != 2 or not all(isinstance(d, int) for d in digits)
            or not 1 <= digits[0] <= digits[1] <= 6):
        raise SpecError("spec.id_digits: [min, max] with 1 <= min <= max <= 6")
    name = spec.get("name") or f"{stack}-{skin}"
    if not isinstance(name, str) or not name.replace("-", "").replace("_", "").isalnum():
        raise SpecError("spec.name: letters, digits, '-' and '_' only")
    out = {
        "name": name, "stack": stack, "skin": skin,
        "conventions": {"lookup": lookup, "linked_files": linked, "dead_end": dead, "finish_line": finish,
                        "instinct_violation": instinct, "decoys": decoys},
        "tasks": {"bugfix": n_bug, "registry": n_reg, "control": n_ctl},
        "id_digits": digits,
    }
    if spec.get("description"):
        out["description"] = str(spec["description"])
    return out


def canonical(spec):
    """The spec as compact, canonical JSON (what live configs embed)."""
    return json.dumps(spec, sort_keys=True, separators=(",", ":"))


def _ids(rng, prefix, digits, n):
    lo_d, hi_d = digits
    out, taken = [], set()
    while len(out) < n:
        d = rng.randint(lo_d, hi_d)
        num = rng.randint(10 ** (d - 1) if d > 1 else 1, 10 ** d - 1)
        if num in taken:
            continue
        taken.add(num)
        out.append(f"{prefix}-{num}")
    return out


def resolve(spec, seed, stacks):
    """spec (raw or normalised) + seed -> Plan."""
    spec = normalise(spec, stacks)
    if not isinstance(seed, int) or isinstance(seed, bool) or seed < 0:
        raise SpecError(f"seed: a non-negative integer, got {seed!r}")
    sk = SKINS[spec["skin"]]
    rng = random.Random(f"adage-gen/{spec['stack']}/{spec['skin']}/{seed}")
    c = spec["conventions"]
    dead = c["dead_end"]
    if dead == "auto":
        dead = rng.choice(list(stacks[spec["stack"]].DEAD_END_MODES))
    conv = Conventions(c["lookup"], c["linked_files"], dead, c["finish_line"], c["instinct_violation"],
                       list(c["decoys"]))

    # Lay the twelve bug templates out over the three modules, four each.
    order = list(bugs.IDS)
    rng.shuffle(order)
    layout = {m: order[i * 4:(i + 1) * 4] for i, m in enumerate(sk.modules)}
    where = {tid: m for m, ts in layout.items() for tid in ts}

    n = spec["tasks"]
    picks = list(bugs.IDS)
    rng.shuffle(picks)
    bug_templates = picks[:n["bugfix"]]
    ctl_templates = picks[n["bugfix"]:n["bugfix"] + n["control"]]
    reg_pool = list(sk.registry_tasks)
    rng.shuffle(reg_pool)
    reg_tasks = reg_pool[:n["registry"]]

    ids = _ids(rng, sk.id_prefix, spec["id_digits"], n["bugfix"] + n["registry"] + len(sk.decoy_items))
    task_ids, decoy_ids = ids[:n["bugfix"] + n["registry"]], ids[n["bugfix"] + n["registry"]:]

    tasks = []
    k = 0
    for tid in bug_templates:
        tasks.append(Task(task_ids[k], "bugfix", "", template=tid, module=where[tid]))
        k += 1
    for rt in reg_tasks:
        tasks.append(Task(task_ids[k], "registry", "", registry=rt))
        k += 1
    for i, tid in enumerate(ctl_templates):
        tasks.append(Task(f"ctl-{i + 1}", "control", "", template=tid, module=where[tid]))
    # Splits: alternate within each kind, so both splits see every kind (train first).
    for kind in TASK_KINDS:
        for i, t in enumerate(x for x in tasks if x.kind == kind):
            t.split = "train" if i % 2 == 0 else "eval"

    decoy_items = []
    for did, (title, status, desc) in zip(decoy_ids, sk.decoy_items):
        area = rng.choice(list(sk.areas))
        decoy_items.append({"id": did, "title": title, "status": status, "description": desc, "area": area,
                            "reported": f"2026-{rng.randint(1, 6):02d}-{rng.randint(1, 28):02d}",
                            "reporter": rng.choice(list(sk.reporters)), "priority": rng.choice(["P3", "P4"])})
    return Plan(spec=spec, seed=seed, name=spec["name"], stack=spec["stack"], skin=sk, conv=conv, layout=layout,
                bugged=sorted(bug_templates + ctl_templates, key=bugs.IDS.index), tasks=tasks,
                decoy_items=decoy_items)
