"""The convention grammar: typed convention kinds, the spec format, and resolution of a spec plus a
seed into a concrete plan (which functions are bugged, which tasks exist, which IDs they get).

A spec says *what* a sandbox must contain; the seed picks every concrete detail. Resolution is a
pure function of (spec, seed): all randomness comes from one `random.Random(seed)`, consumed in a
fixed order, so the same inputs always give the same plan.
"""

import json
import random
import re
from dataclasses import dataclass, field

import bugs
from skins import SKINS

# Every kind a conventions.json entry may have. `scope`, `env_sync` and `sequence` (Track G G3) are
# generated on the python-make stack only, and only when a spec asks for them.
KINDS = ("lookup", "linked_files", "dead_end", "finish_line", "scope", "env_sync", "sequence", "decoy",
         "instinct_violation")
GENERATED_KINDS = ("lookup", "linked_files", "dead_end", "finish_line", "scope", "env_sync", "sequence", "decoy",
                   "instinct_violation")
# The kinds only python-make generates (Track G G3).
NEW_KINDS = ("scope", "env_sync", "sequence")
DEAD_END_MODES = ("env_guard", "path_alias", "wrong_runner")
DECOY_MODES = ("unused_target", "lookalike_regen", "lookalike_verify", "legacy_script")
TASK_KINDS = ("bugfix", "registry", "control", "scoped", "deps")
# Tasks named by a tracker id (the others name their files in the prompt).
TRACKER_KINDS = ("bugfix", "registry")
MIN_CONTROL_SHARE = 0.25

SPEC_KEYS = {"name", "stack", "skin", "conventions", "tasks", "id_digits", "id_shape", "description"}

# conventions.json is `adage-conventions/1`, the schema `adage eval routes` reads (docs/evals.md):
# `conventions` holds what a learner should find, `decoys` the planted commands no route may
# direct. An instinct violation is a dead end with `mode: instinct_violation`.
CONVENTIONS_SCHEMA = "adage-conventions/1"
# Ids no tracker ever issues: a learned lookup contract must reject them.
COUNTER_SAMPLES = ("X-1", "ABCDEFG-123456789", "12", "not-an-id")


def entries(doc):
    """Every entry of a conventions.json document: the conventions, then the decoys."""
    return list(doc["conventions"]) + list(doc.get("decoys", []))


def kind_of(entry):
    """The generator's kind of an entry: `instinct_violation` for the retired universal default
    (a `dead_end` with that mode), else its kind (`decoy` for a decoy)."""
    return "instinct_violation" if entry.get("mode") == "instinct_violation" else entry["kind"]

# Tracker id shapes (Track G §4.4). The id is always the form the lookup tool takes (the value a
# learned type must accept); prompts name it by its mention (`#482` for a bare number).
#   prefix_dash  BIL-1041 (the default; the only shape before Track G, and what adage's hard-coded
#                ticket detector knows)
#   hash_num     #482 in prompts, 482 on the command line
#   uuid         3fa85f64-5717-4562-b3fc-2c963f66afa6 (lowercase, version 4 layout)
#   path_num     bil/412 (the skin's prefix in lower case, a slash, a number)
#   branch       fix/<word>-<n>, the word from BRANCH_WORDS
#   mixed        two trackers in one repository: BIL-1041 and #482 alternate, one lookup tool
ID_SHAPES = ("prefix_dash", "hash_num", "uuid", "path_num", "branch", "mixed")
BRANCH_WORDS = ("audit", "batch", "cache", "float", "index", "label", "limit", "login", "merge", "parse", "query",
                "queue", "retry", "round", "scope", "token")
CONVENTION_KEYS = {"lookup", "linked_files", "dead_end", "finish_line", "instinct_violation", "decoys", "scope",
                   "env_sync", "sequence"}
TASK_KEYS = {"bugfix", "registry", "control", "scoped", "deps"}


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
    scope: bool = False       # packages/<name>/ with their own suites (`make -C packages/<name> check`)
    env_sync: bool = False    # requirements.txt, and `make deps` after changing it
    sequence: bool = False    # after a registry edit: `make gen`, then `make check` (replaces finish_line)


@dataclass
class Task:
    id: str
    kind: str                 # bugfix | registry | control
    split: str                # train | eval
    template: object = None   # bugfix/control/scoped: a bugs.Template id
    module: object = None     # bugfix/control: the module holding the function
    registry: object = None   # registry: a skins.RegistryTask
    package: object = None    # scoped: the package under packages/
    dep: object = None        # deps: the (library, version) pinned


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
    new = {k: _bool(c.get(k, False), f"conventions.{k}") for k in NEW_KINDS}
    if any(new.values()) and stack != "python-make":
        raise SpecError(f"conventions.{', '.join(k for k, v in new.items() if v)}: only the python-make stack "
                        f"generates {', '.join(NEW_KINDS)}")
    if new["sequence"] and not finish:
        raise SpecError("conventions.sequence needs conventions.finish_line (the registry and its generator)")
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
    n_scoped = _int(t.get("scoped", 3 if new["scope"] else 0), "tasks.scoped", 0, 3)
    n_deps = _int(t.get("deps", 3 if new["env_sync"] else 0), "tasks.deps", 0, 3)
    if n_scoped and not new["scope"]:
        raise SpecError("tasks.scoped needs conventions.scope")
    if n_deps and not new["env_sync"]:
        raise SpecError("tasks.deps needs conventions.env_sync")
    if n_bug + n_ctl > len(bugs.IDS):
        raise SpecError(f"tasks.bugfix + tasks.control: at most {len(bugs.IDS)} (one bug template each)")
    if n_reg and not finish:
        raise SpecError("tasks.registry needs conventions.finish_line")
    if (n_bug or n_reg) and not lookup:
        raise SpecError("tasks.bugfix and tasks.registry are named by tracker ID only, so they need conventions.lookup")
    total = n_bug + n_reg + n_ctl + n_scoped + n_deps
    if total == 0:
        raise SpecError("spec.tasks: at least one task")
    if n_ctl < MIN_CONTROL_SHARE * total:
        raise SpecError(f"spec.tasks: control tasks must be at least {int(MIN_CONTROL_SHARE * 100)}% of all tasks "
                        f"({n_ctl} of {total})")
    digits = spec.get("id_digits", list(sk.id_digits))
    if (not isinstance(digits, list) or len(digits) != 2 or not all(isinstance(d, int) for d in digits)
            or not 1 <= digits[0] <= digits[1] <= 6):
        raise SpecError("spec.id_digits: [min, max] with 1 <= min <= max <= 6")
    shape = spec.get("id_shape", "prefix_dash")
    if shape not in ID_SHAPES:
        raise SpecError(f"spec.id_shape: one of {list(ID_SHAPES)}, got {shape!r}")
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
    # The Track G G3 kinds are left out when off, so existing specs keep their bytes.
    for k, v in new.items():
        if v:
            out["conventions"][k] = True
    if n_scoped:
        out["tasks"]["scoped"] = n_scoped
    if n_deps:
        out["tasks"]["deps"] = n_deps
    if shape != "prefix_dash":  # the default is left out, so existing specs keep their bytes
        out["id_shape"] = shape
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


def _number(rng, digits):
    d = rng.randint(digits[0], digits[1])
    return rng.randint(10 ** (d - 1) if d > 1 else 1, 10 ** d - 1)


def make_id(rng, shape, prefix, digits, k):
    """The k-th id of a shape (any shape but prefix_dash, whose ids `_ids` draws as it always did)."""
    if shape == "hash_num" or (shape == "mixed" and k % 2 == 1):
        return str(_number(rng, digits))
    if shape == "mixed":
        return f"{prefix}-{_number(rng, digits)}"
    if shape == "uuid":
        return (f"{rng.getrandbits(32):08x}-{rng.getrandbits(16):04x}-4{rng.getrandbits(12):03x}-"
                f"{rng.choice('89ab')}{rng.getrandbits(12):03x}-{rng.getrandbits(48):012x}")
    if shape == "path_num":
        return f"{prefix.lower()}/{_number(rng, digits)}"
    if shape == "branch":
        return f"fix/{rng.choice(BRANCH_WORDS)}-{_number(rng, digits)}"
    raise ValueError(shape)


def _shaped_ids(rng, shape, prefix, digits, n):
    if shape == "prefix_dash":
        return _ids(rng, prefix, digits, n)
    out = []
    while len(out) < n:
        i = make_id(rng, shape, prefix, digits, len(out))
        if i not in out:
            out.append(i)
    return out


def mention(item_id):
    """How a prompt names an item: `#482` for a bare number, else the id itself."""
    return f"#{item_id}" if item_id.isdigit() else item_id


def number_digits(ids):
    """The digit counts of the trailing numbers of ids, sorted."""
    return sorted(len(re.search(r"(\d+)$", i).group(1)) for i in ids if re.search(r"(\d+)$", i))


def _rep(ds, default):
    lo, hi = (ds[0], ds[-1]) if ds else tuple(default)
    return f"{{{lo}}}" if lo == hi else f"{{{lo},{hi}}}"


UUID_RE = "[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}"


def id_pattern(shape, prefix, ids, default_digits):
    """The anchored ground-truth pattern of a lookup's ids (the observed digit range, as today)."""
    if shape == "prefix_dash":
        return f"^{re.escape(prefix)}-\\d{_rep(number_digits(ids), default_digits)}$"
    if shape == "hash_num":
        return f"^\\d{_rep(number_digits(ids), default_digits)}$"
    if shape == "uuid":
        return f"^{UUID_RE}$"
    if shape == "path_num":
        return f"^{prefix.lower()}/\\d{_rep(number_digits(ids), default_digits)}$"
    if shape == "branch":
        return f"^fix/(?:{'|'.join(BRANCH_WORDS)})-\\d{_rep(number_digits(ids), default_digits)}$"
    if shape == "mixed":
        a = number_digits([i for i in ids if not i.isdigit()])
        b = number_digits([i for i in ids if i.isdigit()])
        return f"^(?:{re.escape(prefix)}-\\d{_rep(a, default_digits)}|\\d{_rep(b, default_digits)})$"
    raise ValueError(shape)


def mention_regex(shape, prefix):
    """What an id looks like inside a prompt, for checking that prompts name only their item."""
    ticket = r"\b[A-Z][A-Z0-9]{0,9}-\d{1,7}\b"  # adage-core's ticket span (stacks/base.py TICKET)
    return re.compile({
        "prefix_dash": ticket,
        "hash_num": r"#\d+\b",
        "uuid": r"\b" + UUID_RE + r"\b",
        "path_num": r"\b" + re.escape(prefix.lower()) + r"/\d+\b",
        "branch": r"\bfix/[a-z]+-\d+\b",
        "mixed": ticket + r"|#\d+\b",
    }[shape])


def held_out(spec, task_ids, seed, n=200):
    """Fresh ids and near misses for `adage eval types` (Track G §4.4): `n` fresh ids from the
    generator's own id function, with the digit range the bundle's `id_pattern` declares, none of
    them a task's id; and `n` near misses of them: a wrong prefix, one character too few or too
    many, a leading `-`, shell metacharacters. Every near miss fails `id_pattern`; every fresh id
    matches it. A pure function of (spec, task ids, seed)."""
    sk = SKINS[spec["skin"]]
    shape = spec.get("id_shape", "prefix_dash")
    pattern = re.compile(id_pattern(shape, sk.id_prefix, task_ids, spec["id_digits"]))
    rng = random.Random(f"adage-gen/holdout/{canonical(spec)}/{seed}")

    def digits_of(ids):
        ds = number_digits(ids)
        return (ds[0], ds[-1]) if ds else tuple(spec["id_digits"])

    fams = {"a": digits_of([i for i in task_ids if not i.isdigit()]), "d": digits_of([i for i in task_ids if i.isdigit()])}
    fresh, taken = [], set(task_ids)
    guard = 0
    while len(fresh) < n and guard < 100 * n:
        guard += 1
        k = len(fresh)
        fam = "d" if shape == "hash_num" or (shape == "mixed" and k % 2 == 1) else "a"
        if shape == "prefix_dash":
            i = f"{sk.id_prefix}-{_number(rng, fams['a'])}"
        else:
            i = make_id(rng, shape, sk.id_prefix, fams[fam], k)
        if i not in taken and pattern.fullmatch(i):
            taken.add(i)
            fresh.append(i)

    def wrong_prefix(v):
        if v.isdigit():
            return rng.choice("xqz") + v
        if shape == "uuid":
            return rng.choice("ghjk") + v[1:]
        head = v.split("/")[0].split("-")[0]
        pos = rng.randrange(len(head))
        alphabet = "ABCDEFGHIJKLMNOPQRSTUVWXYZ" if head[pos].isupper() else "abcdefghijklmnopqrstuvwxyz"
        c = rng.choice([x for x in alphabet if x != head[pos]])
        return v[:pos] + c + v[pos + 1:]

    def length(v, delta):
        if shape == "uuid":
            groups = v.split("-")
            g = rng.randrange(len(groups))
            groups[g] = groups[g][:-1] if delta < 0 else groups[g] + rng.choice("0123456789abcdef")
            return "-".join(groups)
        m = re.search(r"(\d+)$", v)
        body, num = v[:m.start()], m.group(1)
        return body + (num[:-1] if delta < 0 else num + str(rng.randint(0, 9)))

    shell = (";rm -rf ~", "$(id)", "`id`", "|sh", "&&id", " x", ">out", "\nid")
    kinds = ("wrong_prefix", "shorter", "longer", "leading_dash", "shell")
    near, seen = [], set()
    k = 0
    while len(near) < n and k < 100 * n:
        v = fresh[k % len(fresh)]
        kind = kinds[k % len(kinds)]
        k += 1
        if kind == "wrong_prefix":
            m = wrong_prefix(v)
        elif kind in ("shorter", "longer"):
            m = length(v, -1 if kind == "shorter" else 1)
        elif kind == "leading_dash":
            m = "-" + v
        else:
            s = shell[k % len(shell)]
            m = f"$({v})" if s == "$(id)" and k % 2 else v + s
        if m and m not in seen and not pattern.fullmatch(m):
            seen.add(m)
            near.append({"value": m, "kind": kind})
    return {"id_shape": shape, "id_pattern": pattern.pattern, "fresh": fresh, "near_miss": near}


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
                       list(c["decoys"]), c.get("scope", False), c.get("env_sync", False), c.get("sequence", False))

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

    ids = _shaped_ids(rng, spec.get("id_shape", "prefix_dash"), sk.id_prefix, spec["id_digits"],
                      n["bugfix"] + n["registry"] + len(sk.decoy_items))
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
    # Track G G3 tasks, drawn after everything else so existing specs draw exactly what they did.
    if n.get("scoped"):
        pk = rng.sample(list(sk.packages), n["scoped"])
        tpl = rng.sample(list(bugs.IDS), n["scoped"])
        for i, (p, tid) in enumerate(zip(pk, tpl)):
            tasks.append(Task(f"pkg-{i + 1}", "scoped", "", template=tid, package=p))
    if n.get("deps"):
        for i, d in enumerate(rng.sample(list(sk.deps[2:]), n["deps"])):
            tasks.append(Task(f"dep-{i + 1}", "deps", "", dep=d))
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
