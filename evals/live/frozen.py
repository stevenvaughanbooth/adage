#!/usr/bin/env python3
"""Prepare, execute and report one frozen Claude Code experiment.

Preparation runs only sandbox generators and version probes. `run` is a read-only plan unless
--execute is explicit. Paid execution reuses runner.Live's isolated homes, confinement and ledger.
"""
from __future__ import annotations

import argparse
import copy
import hashlib
import json
import os
import subprocess
import sys
import tempfile
from decimal import Decimal
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import evidence
import runner
import native_runtime
from budget import Budget, cap_charge_valid, execution_lock, known_cost

ADAPTER = "claude-code-frozen/v2"
RUNTIME_PROFILE = "fresh-home-bundled-defaults/v1"
MAX_JSON = 64 * 1024 * 1024


def encoded(value):
    return json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=False, allow_nan=False).encode()


def digest(body):
    return hashlib.sha256(body).hexdigest()


def file_digest(path):
    sha = hashlib.sha256()
    with Path(path).open("rb") as handle:
        for block in iter(lambda: handle.read(1024 * 1024), b""):
            sha.update(block)
    return sha.hexdigest()


def read_json(path):
    with Path(path).open("rb") as handle:
        body = handle.read(MAX_JSON + 1)
    if len(body) > MAX_JSON:
        raise ValueError("experiment JSON exceeds 64 MiB")
    return json.loads(body)


def publish(path, value, *, replace=False):
    """Atomic private publication. Return False for identical redelivery; never rewrite evidence."""
    return publish_bytes(path, encoded(value), replace=replace)


def publish_bytes(path, body, *, replace=False):
    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.NamedTemporaryFile(dir=path.parent, delete=False) as handle:
        temp = Path(handle.name)
        try:
            handle.write(body)
            handle.flush()
            os.fsync(handle.fileno())
            if replace:
                os.replace(temp, path)
            else:
                try:
                    os.link(temp, path)
                except FileExistsError:
                    if path.read_bytes() != body:
                        raise ValueError(f"refusing to replace experiment evidence: {path}") from None
                    return False
            directory = os.open(path.parent, os.O_RDONLY)
            try:
                os.fsync(directory)
            finally:
                os.close(directory)
            return True
        finally:
            temp.unlink(missing_ok=True)


class Objects:
    def __init__(self, root):
        self.root = Path(root).resolve()

    def body(self, sha):
        if not isinstance(sha, str) or len(sha) != 64 or any(c not in "0123456789abcdef" for c in sha):
            raise ValueError("invalid experiment object digest")
        path = self.root / f"{sha}.json"
        with path.open("rb") as handle:
            body = handle.read(MAX_JSON + 1)
        if len(body) > MAX_JSON:
            raise ValueError("experiment object exceeds 64 MiB")
        if digest(body) != sha:
            raise ValueError(f"experiment object checksum mismatch: {sha}")
        return body

    def get(self, sha):
        return json.loads(self.body(sha))

    def put(self, value):
        body = encoded(value)
        sha = digest(body)
        if len(body) > MAX_JSON:
            raise ValueError("experiment object exceeds 64 MiB")
        publish_bytes(self.root / f"{sha}.json", body)
        return sha


def adage_json(adage, *args):
    result = subprocess.run([str(adage), "eval", "experiment", *map(str, args)],
                            capture_output=True, text=True, timeout=120)
    if result.returncode:
        raise ValueError(result.stderr.strip() or "experiment validation failed")
    return json.loads(result.stdout)


def artifact(adage, index, repo, checkout=None):
    args = ["artifact", index, "--repo", repo]
    if checkout is not None:
        args.extend(["--checkout", checkout])
    result = adage_json(adage, *args)
    if not result["directive_reflexes"]:
        raise ValueError("the frozen Claude Code adapter requires at least one live directive reflex for this repository")
    # This collector copies only the index, not an installed model or custom home settings.
    # A syntactically valid intent/card route can otherwise remain inert for every paid run.
    if not isinstance(result.get("directive_embeddings"), list) or "bundled_embedding" not in result:
        raise ValueError("Adage build cannot inspect frozen embedding requirements; rebuild the pinned binary")
    required = result["directive_embeddings"]
    bundled = result["bundled_embedding"]
    if any(requirement != bundled for requirement in required):
        raise ValueError("frozen intent/card directives require the pinned binary's bundled embedding model; "
                         "installed model assets are not supported by this collector")
    return result


def check_runtime_home(home, *, initial=False, intervention=False):
    """Frozen v2 evaluates binary defaults in a fresh home; external policy/model assets are absent."""
    home = Path(home)
    if home.is_symlink():
        raise ValueError("frozen runtime home must not be a symlink")
    forbidden = ("config.toml", "models", "tracks", "exec")
    if any((home / name).exists() or (home / name).is_symlink() for name in forbidden):
        raise ValueError("frozen runtime home contains unsupported settings, model or learned assets")
    allowed = {"index.json"} if intervention else set()
    if initial and home.exists() and any(p.name not in allowed or p.is_symlink() for p in home.iterdir()):
        raise ValueError("frozen runtime home must be fresh before dispatch")


def group_key(repo, identity):
    """Canonical adage-trace v1 declared group identity; Rust validates the complete schedule."""
    kind = "group" if identity.get("group_id", "").strip() else "task"
    value = identity.get(f"{kind}_id", "").strip()
    if not value:
        raise ValueError("frozen execution needs declared task identities")
    sha = hashlib.sha256()
    for part in ("adage-evidence-group-v1", repo, kind, value):
        raw = part.encode()
        sha.update(len(raw).to_bytes(8, "little"))
        sha.update(raw)
    return "group-v1:" + sha.hexdigest()


def checkout_digest(root):
    """Exact initial files and executable bits, without Git object timestamps or external links."""
    root = Path(root)
    files = []
    for directory, names, entries in os.walk(root, followlinks=False):
        names[:] = sorted(n for n in names if n != ".git")
        for name in sorted(names + entries):
            path = Path(directory) / name
            if path.is_symlink():
                raise ValueError("frozen sandbox inputs must not contain symlinks")
            if path.is_file() and path.name != ".git":
                files.append((path.relative_to(root).as_posix(), bool(path.stat().st_mode & 0o111), file_digest(path)))
    return digest(encoded(sorted(files)))


def task_revision(identity, prompt, checkout):
    return digest(encoded({"identity": identity, "prompt": prompt, "checkout": checkout}))


def environment_revision(spec):
    value = {k: spec[k] for k in ("adapter", "runtime_profile", "inputs", "private_inputs", "harness_version", "adage_sha256")}
    if spec["runtime_profile"] == native_runtime.PROFILE:
        value["native_inputs"] = spec["native_inputs"]
    return digest(encoded(value))


def evaluator_revision(spec):
    return digest(encoded({"command": spec["config"]["score"], "inputs": spec["inputs"]}))


def check_inputs(spec, adage):
    if (spec.get("adapter") != ADAPTER or spec.get("runtime_profile") not in (RUNTIME_PROFILE, native_runtime.PROFILE)
            or file_digest(adage) != spec["adage_sha256"]):
        raise ValueError("runner or Adage build differs from the frozen experiment")
    if spec["runtime_profile"] == native_runtime.PROFILE:
        native_runtime.check_version(spec["harness_version"])
        if native_runtime.static_inputs(spec["config"], adage) != spec.get("native_inputs"):
            raise ValueError("native plugin binaries, settings or managed policy changed")
    for path, sha in spec["inputs"].items():
        if file_digest(path) != sha:
            raise ValueError(f"frozen input changed: {path}")


def prepared_spec(cfg, adage, input_paths, version, runtime_profile=RUNTIME_PROFILE):
    # Pin the collector itself, its shared execution/accounting code and the supplied grader/data
    # closure. Environment/container lockfiles can be declared as --input too.
    paths = {HERE / name for name in ("frozen.py", "runner.py", "evidence.py", "budget.py")}
    paths.add(runner.REPO / "demo" / "report.py")
    # Both profiles import these modules; pin their source even when native dispatch is unused.
    paths.update(HERE / name for name in ("native_assets.py", "native_runtime.py"))
    paths.add(runner.REPO / "demo" / "adage_metrics.py")
    private = sorted(str(Path(p).resolve()) for p in input_paths)
    if any(not runner.under(p, runner.CONFINE_HIDE) for p in private):
        raise ValueError("grader/data inputs must live under the standard confinement's hidden directories")
    paths.update(Path(p) for p in private)
    for key in ("sandbox", "score"):
        for arg in runner.expand(cfg[key], dir="__future_checkout__", task="__future_task__", arm="off"):
            if Path(arg).is_file():
                paths.add(Path(arg).resolve())
    spec = {"adapter": ADAPTER, "runtime_profile": runtime_profile, "config": cfg, "harness_version": version,
            "adage_sha256": file_digest(adage), "private_inputs": private,
            "inputs": {str(p): file_digest(p) for p in sorted(paths)}}
    if runtime_profile == native_runtime.PROFILE:
        spec["native_inputs"] = native_runtime.static_inputs(cfg, adage)
    return spec


def require_e0_config(cfg):
    if set(cfg["eval"].get("arms", [])) & set(runner.PLUGIN_ARMS):
        raise ValueError("the frozen v2 collector supports E0 advice only; it cannot freeze Claude mod "
                         "plugin assets, settings or operator grants")


def require_profile_config(cfg, runtime_profile):
    if runtime_profile == RUNTIME_PROFILE:
        require_e0_config(cfg)
    elif runtime_profile == native_runtime.PROFILE:
        if set(cfg["eval"].get("arms", [])) != {"off", "mod"} or len(cfg["eval"]["arms"]) != 2:
            raise ValueError("native frozen E0 profile requires exactly off/mod arms")
        if cfg.get("capture_debug") is not True:
            raise ValueError("native frozen E0 profile requires debug observations")
        if cfg.get("forward_env"):
            raise ValueError("native frozen E0 profile does not support custom forwarded settings")
    else:
        raise ValueError("unsupported frozen runtime profile")


def prepare(config, audit_path, index_path, objects, out, adage, inputs, *, timeout=600,
            learner_revision, policy=None, identities=None, intervention_model=None,
            runtime_profile=RUNTIME_PROFILE):
    if sys.platform != "linux":
        raise ValueError("frozen live preparation requires the Linux confinement profile")
    cfg = runner.load_config(config, {})
    require_profile_config(cfg, runtime_profile)
    if not cfg.get("confine") or cfg.get("confine_hide") or cfg.get("confine_expose"):
        raise ValueError("frozen studies require confine=true with the standard hidden directories")
    if not inputs:
        raise ValueError("declare the independent grader/data dependency closure with --input")
    if not isinstance(learner_revision, str) or not learner_revision.strip():
        raise ValueError("record the learner build/parameters that actually produced the reflex set")
    if type(timeout) is not int or timeout <= 0:
        raise ValueError("timeout must be a positive integer")
    artifact(adage, index_path, cfg["repo_key"])
    version = runner.harness_version()
    if not version:
        raise ValueError("cannot pin the installed Claude Code version")
    if runtime_profile == native_runtime.PROFILE:
        native_runtime.check_version(version)
        cfg["harness_binary"] = native_runtime.harness_identity()["resolved_path"]
        if intervention_model and intervention_model != cfg["model"]:
            raise ValueError("native frozen E0 profile requires the same model in both arms")
    if type(cfg["max_turns"]) is not int or cfg["max_turns"] <= 0:
        raise ValueError("max_turns must be a positive integer")
    micros = Decimal(str(cfg["session_budget_usd"])) * 1_000_000
    if micros != micros.to_integral_value():
        raise ValueError("session budget must be an exact number of microdollars")
    tasks = runner.tasks_for_round(cfg, 1)
    if len(set(tasks)) != len(tasks):
        raise ValueError("repeated evaluation task id")
    if Decimal(str(cfg["budget_usd"])) * 1_000_000 < micros * len(tasks) * 2:
        raise ValueError("budget_usd cannot reserve the complete paired cohort")
    policy = policy or {"min_groups": 30, "max_quality_loss": 0.02, "min_savings": 0.2, "min_baseline_success": 0.8}
    identities = identities or {}
    cfg.update(rounds=1, concurrency=1, capture_usage=True, timeout_seconds=timeout, setting_sources="")
    cfg["execution_cwd"] = str(Path(cfg.get("execution_cwd") or Path.cwd()).resolve())
    for key in ("sandbox", "score"):
        for i, arg in enumerate(cfg[key]):
            expanded = runner.expand([arg], dir="__future_checkout__", task="__future_task__", arm="off")[0]
            candidate = Path(expanded)
            if not candidate.is_absolute():
                candidate = Path(cfg["execution_cwd"]) / candidate
            if candidate.is_file():
                cfg[key][i] = str(candidate.resolve())
            elif i == 0 and runner.shutil.which(expanded):
                cfg[key][i] = str(Path(runner.shutil.which(expanded)).resolve())
    treatment = "mod" if runtime_profile == native_runtime.PROFILE else "adage"
    cfg["eval"] = {"tasks": tasks, "repeats": 1, "arms": ["off", treatment]}
    spec = prepared_spec(cfg, adage, inputs, version, runtime_profile)
    test_tasks, prompts = [], {}
    with tempfile.TemporaryDirectory(prefix="adage-frozen-prepare-") as scratch:
        for i, task in enumerate(tasks):
            identity = identities.get(task, {"task_id": task})
            if identity.get("task_id") != task:
                raise ValueError("execution task id must equal its canonical task_id")
            prompt = runner.task_prompt(cfg, task, cfg["prompts"][i % len(cfg["prompts"])]).format(id=task)
            prompts[task] = prompt
            workdir = Path(scratch) / str(i) / cfg["workdir"]
            workdir.parent.mkdir(parents=True)
            subprocess.run(runner.expand(cfg["sandbox"], dir=str(workdir), task=task, arm="off"),
                           check=True, capture_output=True, timeout=timeout, cwd=cfg["execution_cwd"])
            artifact(adage, index_path, cfg["repo_key"], workdir)
            if runtime_profile == native_runtime.PROFILE:
                native_runtime.check_checkout(workdir)
            test_tasks.append({"identity": identity, "task_revision": task_revision(identity, prompt, checkout_digest(workdir)),
                               "environment_revision": environment_revision(spec), "evaluator_revision": evaluator_revision(spec)})
    # The effective prompts, rather than rotating template positions, are the frozen inputs.
    spec["config"]["task_prompts"] = prompts
    other = copy.deepcopy(spec)
    other["config"]["model"] = intervention_model or cfg["model"]
    check_inputs(spec, adage)
    budget = {"max_model_calls": cfg["max_turns"], "max_cost_microusd": int(micros), "timeout_seconds": timeout}
    protocol = {"schema": 1, "experiment": cfg["name"], "repo": cfg["repo_key"], "seed": cfg.get("seed") or 0,
                "kind": "same_model" if cfg["model"] == other["config"]["model"] else "model_transfer",
                "training_audit_sha256": adage_json(adage, "object", audit_path, "--objects", objects.root)["sha256"],
                "learner_revision": learner_revision,
                "policy": policy, "budget": budget,
                "baseline": {"model": cfg["model"], "configuration_sha256": objects.put(spec), "reflex_set_sha256": None},
                "intervention": {"model": other["config"]["model"], "configuration_sha256": objects.put(other),
                                 "reflex_set_sha256": None}, "tasks": test_tasks}
    # Preserve the exact compiled index bytes; reformatting changes the runtime artifact digest.
    # Rust's object command preserves input bytes and private atomic publication.
    stored = adage_json(adage, "object", index_path, "--objects", objects.root)
    protocol["intervention"]["reflex_set_sha256"] = stored["sha256"]
    with tempfile.TemporaryDirectory() as temp:
        path = Path(temp) / "protocol.json"
        publish(path, protocol)
        frozen = adage_json(adage, "freeze", path, "--objects", objects.root)
    publish(out, frozen)
    return frozen


class Observer:
    def __init__(self, frozen, job, spec, objects, adage):
        self.frozen, self.job, self.spec, self.objects, self.adage = frozen, job, spec, objects, adage
        p = frozen["protocol"]
        self.task = next(t for t in p["tasks"] if group_key(p["repo"], t["identity"]) == job["group"])
        self.observed, self.grade, self.grade_record, self.version = None, None, None, None
        self.native = spec["runtime_profile"] == native_runtime.PROFILE
        self.init, self.workdir, self.config_dir = None, None, None

    def prepared(self, live, run_id, task, arm, workdir, settings, env):
        check_inputs(self.spec, self.adage)
        intervention = self.job["side"] == "intervention"
        if self.native:
            native_runtime.check_home(live.home, initial=True, intervention=intervention)
            native_runtime.check_checkout(workdir)
            self.workdir, self.config_dir = Path(workdir), Path(env["CLAUDE_CONFIG_DIR"])
            if any(path.is_symlink() for path in (self.config_dir, *self.config_dir.parents)):
                raise ValueError("native session configuration must not use linked directories")
            if self.config_dir.exists() and any(self.config_dir.iterdir()):
                raise ValueError("native session configuration and plugin data must be fresh")
        else:
            check_runtime_home(live.home, initial=True, intervention=intervention)
        if live.cfg != self.spec["config"] or not live.isolate:
            raise ValueError("actual execution configuration changed")
        if exposed_private_inputs(self.spec, live.root.resolve(), self.objects, env):
            raise ValueError("a mounted tool/auth directory would expose independent grader/data or study evidence")
        if live.state["harness_version"] != self.spec["harness_version"]:
            raise ValueError("harness version changed")
        treatment = "mod" if self.native else "adage"
        if arm not in ("off", treatment):
            raise ValueError("actual execution arm differs from the frozen runtime profile")
        expected_side = "baseline" if arm == "off" else "intervention"
        if expected_side != self.job["side"] or run_id != self.job["id"] or task != self.task["identity"]["task_id"]:
            raise ValueError("execution does not match the frozen job")
        prompt = self.spec["config"]["task_prompts"][task]
        revision = task_revision(self.task["identity"], prompt, checkout_digest(workdir))
        expected_index = self.frozen["protocol"]["intervention"]["reflex_set_sha256"]
        index_path = live.home / "index.json" if intervention else self.objects.root / f"{expected_index}.json"
        inspected = artifact(self.adage, index_path, live.cfg["repo_key"], workdir)
        if inspected["sha256"] != expected_index:
            raise ValueError("frozen reflex set changed before execution")
        index_sha = inspected["sha256"] if intervention else None
        self.observed = {"model": live.cfg["model"], "configuration_sha256": digest(encoded(self.spec)),
                         "reflex_set_sha256": index_sha, "task_revision": revision,
                         "environment_revision": environment_revision(self.spec),
                         "evaluator_revision": evaluator_revision(self.spec), "budget": self.frozen["protocol"]["budget"]}
        expected = self.frozen["protocol"][self.job["side"]]
        if any(self.observed[k] != self.task[k] for k in ("task_revision", "environment_revision", "evaluator_revision")):
            raise ValueError("checkout or grading inputs changed before execution")
        if any(self.observed[k] != expected[k] for k in ("model", "configuration_sha256", "reflex_set_sha256")):
            raise ValueError("model, settings or reflex set changed before execution")
        if intervention and (env.get("ADAGE_AUTO_LEARN") != "0" or env.get("ADAGE_ARM") != "treated"):
            raise ValueError("frozen intervention must disable learning and deliver the treated arm")
        if any(key.startswith("ADAGE_") and key not in
               ("ADAGE_HOME", "ADAGE_ARM", "ADAGE_AUTO_LEARN", "ADAGE_DISABLE") for key in env):
            raise ValueError("frozen runtime environment contains unsupported Adage overrides")
        env["ADAGE_DISABLE"] = "1" if arm == "off" else "0"
        env["ADAGE_AUTO_LEARN"] = "0"

    def event(self, event):
        if event.get("type") == "system" and event.get("subtype") == "init":
            if self.native and self.init is not None:
                raise ValueError("native session initialized more than once")
            self.init = copy.deepcopy(event)
            self.version = event.get("claude_code_version")

    def graded(self, stdout, returncode, timed_out):
        self.grade = evidence.independent_grade(stdout, returncode, timed_out)
        self.grade_record = {"stdout": stdout, "returncode": returncode, "timed_out": timed_out,
                             "evaluator_revision": self.task["evaluator_revision"]}

    def finish(self, live):
        check_inputs(self.spec, self.adage)
        if self.native:
            native_runtime.check_home(live.home, intervention=self.job["side"] == "intervention")
            native_runtime.check_checkout(self.workdir)
        else:
            check_runtime_home(live.home)
        if self.observed is None or self.version != self.spec["harness_version"].split()[0]:
            raise ValueError("actual harness version or execution observations are missing/mismatched")
        if self.job["side"] == "intervention" and file_digest(live.home / "index.json") != self.observed["reflex_set_sha256"]:
            raise ValueError("reflex set changed during execution")
        run_id = self.job["id"]
        accounting = evidence.read_stream(live.root / "runs" / run_id / "stream.jsonl")
        metrics = live.state["runs"][run_id].get("metrics", {})
        result = evidence.receipt(self.frozen["protocol_sha256"], run_id, run_id, self.observed, accounting, self.grade,
                                  timed_out=bool(metrics.get("timed_out")), exit_code=metrics.get("exit_code", 1))
        result["evidence"] = {
            "accounting": self.objects.put({"adapter": ADAPTER, "stream_sha256": accounting.stream_sha256,
                                             "calls": accounting.calls, "complete": accounting.complete,
                                             "reasons": accounting.reasons, "terminal": accounting.terminal}),
            "grade": self.objects.put(self.grade_record),
            "execution": self.objects.put({"observed": self.observed, "harness_version": self.version,
                                            "exit_code": metrics.get("exit_code"), "timed_out": metrics.get("timed_out")})}
        if self.native:
            sid = (self.init or {}).get("session_id", "")
            history = native_runtime.history_for_session(live.transcripts, sid)
            observations = native_runtime.observe(
                init=self.init, accounting=accounting,
                debug_path=live.root / "runs" / run_id / "claude-debug.log", history_path=history,
                plugin=self.spec["native_inputs"]["plugin"], config_dir=self.config_dir, home=live.home,
                intervention=self.job["side"] == "intervention")
            result["evidence"]["native_runtime"] = self.objects.put(observations)
        return result


def bindings(frozen, objects, adage):
    p = frozen["protocol"]
    specs = {side: objects.get(p[side]["configuration_sha256"]) for side in ("baseline", "intervention")}
    for side, spec in specs.items():
        if digest(encoded(spec)) != p[side]["configuration_sha256"]:
            raise ValueError("adapter settings must use the canonical frozen-prepare encoding")
        check_inputs(spec, adage)
        cfg = spec["config"]
        require_profile_config(cfg, spec["runtime_profile"])
        budget = p["budget"]
        if (cfg["model"] != p[side]["model"] or cfg["repo_key"] != p["repo"] or cfg.get("concurrency") != 1
                or not cfg.get("confine") or cfg.get("confine_hide") or cfg.get("confine_expose")
                or cfg.get("capture_usage") is not True or cfg.get("setting_sources") != ""
                or cfg["max_turns"] != budget["max_model_calls"]
                or cfg["timeout_seconds"] != budget["timeout_seconds"]
                or Decimal(str(cfg["session_budget_usd"])) * 1_000_000 != budget["max_cost_microusd"]):
            raise ValueError("execution settings do not enforce the frozen protocol")
    profiles = {spec["runtime_profile"] for spec in specs.values()}
    if native_runtime.PROFILE in profiles and (len(profiles) != 1 or p.get("kind") != "same_model"
            or p["baseline"]["model"] != p["intervention"]["model"]):
        raise ValueError("native frozen E0 profile requires matching profiles and the same model")
    return specs


def confinement_probe():
    """Check the required namespace capability with a local no-op, never an agent."""
    if not runner.shutil.which("bwrap"):
        return "Bubblewrap is not installed; install bubblewrap on the Linux execution host"
    try:
        result = subprocess.run(["bwrap", "--die-with-parent", "--unshare-pid", "--ro-bind", "/", "/",
                                 "--proc", "/proc", "--dev", "/dev", "--", "/bin/true"],
                                capture_output=True, text=True, timeout=10)
    except (OSError, subprocess.SubprocessError) as exc:
        return f"Bubblewrap cannot run: {exc}; use a host that permits its namespaces"
    if result.returncode:
        return f"Bubblewrap confinement is unavailable: {result.stderr.strip()[:500]}; use a host that permits its namespaces"
    return None


def exposed_private_inputs(spec, root, objects, env):
    exposed = [p for p in env.get("PATH", "").split(os.pathsep) if p]
    exposed += [str(Path(env[k]).parent) for k in runner.CONFINE_FILE_ENV if env.get(k)]
    if spec.get("runtime_profile") == native_runtime.PROFILE:
        exposed.append(spec["config"]["plugin_dir"])
        if env.get("ADAGE_HOME"):
            exposed.append(env["ADAGE_HOME"])
    return [path for path in (*spec["private_inputs"], str(root), str(objects.root)) if runner.under(path, exposed)]


def validate_admission_journal(frozen, objects, journaled, receipts, ledger):
    """Original reservations and native cost estimates must survive every resume unchanged."""
    attempts = ledger.data.get("attempts")
    if not isinstance(attempts, list):
        raise ValueError("accounting attempts must be a list of original reservations")
    scheduled = {job["id"] for job in frozen["jobs"]}
    by_run = {}
    for attempt in attempts:
        if not isinstance(attempt, dict) or attempt.get("run") not in scheduled or attempt.get("run") not in journaled:
            raise ValueError("accounting contains an unknown or unjournaled job; reconcile the original ledger")
        run = attempt["run"]
        if run in by_run:
            raise ValueError(f"duplicate original accounting attempt for job {run}")
        cap = known_cost(attempt.get("cap_usd"))
        if cap is None or Decimal(str(cap)) * 1_000_000 != frozen["protocol"]["budget"]["max_cost_microusd"]:
            raise ValueError(f"original reservation cap differs from the frozen budget for job {run}")
        if attempt.get("status") == "cap_charged" and not cap_charge_valid(attempt):
            raise ValueError(f"invalid conservative cap charge for job {run}; restore the explicit original authorization")
        by_run[run] = attempt
    missing = journaled - by_run.keys()
    if missing:
        raise ValueError("missing original accounting attempt for journaled jobs: " + ", ".join(sorted(missing)))
    for run, receipt in receipts.items():
        attempt = by_run[run]
        if attempt.get("status") not in ("settled", "unknown", "cap_charged"):
            raise ValueError(f"receipted job {run} has a nonterminal accounting attempt")
        source_sha = receipt.get("evidence", {}).get("accounting")
        source = objects.get(source_sha) if source_sha else {}
        terminal = source.get("terminal") or {}
        estimate = known_cost(terminal.get("total_cost_usd"))
        if attempt["status"] == "settled":
            if estimate is None:
                raise ValueError(f"settled cost requires a known immutable native accounting estimate for job {run}; preserve unknown spend or reconcile an explicitly authorized cap charge")
            if known_cost(attempt.get("cost_usd")) != estimate:
                raise ValueError(f"settled cost disagrees with immutable native accounting for job {run}; reconcile the original estimate")
        if estimate is not None and attempt["status"] == "cap_charged" and estimate > attempt["charged_usd"]:
            raise ValueError(f"native accounting exceeds the conservative cap charge for job {run}; reconcile the overrun")


def receipt_evidence_issues(receipt, objects, *, native=False):
    """Evidence failures stop admission; an independently graded task failure does not."""
    accounting = objects.get(receipt["evidence"]["accounting"])
    grade = objects.get(receipt["evidence"]["grade"])
    issues = []
    if not receipt["usage_complete"] or not accounting["complete"]:
        issues.append("incomplete request usage")
    if any(call["model"] != receipt["observed"]["model"] for call in accounting["calls"]):
        issues.append("unplanned model call")
    if len(accounting["calls"]) > receipt["observed"]["budget"]["max_model_calls"]:
        issues.append("model-call budget exceeded")
    if evidence.independent_grade(grade["stdout"], grade["returncode"], grade["timed_out"]) is None:
        issues.append("unknown independent grade")
    if native:
        sha = receipt.get("evidence", {}).get("native_runtime")
        if not sha:
            issues.append("missing native runtime observations")
        else:
            observation = objects.get(sha)
            if observation.get("profile") != native_runtime.PROFILE or observation.get("history_matches_stream") is not True:
                issues.append("invalid native runtime observations")
    return issues


def preflight(frozen, objects, root, adage):
    """Read-only readiness: source checks, existing journal and local execution prerequisites."""
    root = Path(root).resolve()
    blockers, completed, interrupted, reflex_set = [], [], [], None
    journaled, receipts = set(), {}

    def block(code, message):
        blockers.append({"code": code, "message": message})

    try:
        specs = bindings(frozen, objects, adage)
        index = objects.root / f"{frozen['protocol']['intervention']['reflex_set_sha256']}.json"
        reflex_set = artifact(adage, index, frozen["protocol"]["repo"])
    except (ValueError, OSError, KeyError, TypeError) as exc:
        specs = {}
        block("frozen_inputs", f"Frozen inputs are unavailable or changed: {exc}; restore the pinned inputs or prepare a new study")
    if any(not runner.under(path, runner.CONFINE_HIDE) for path in (root, objects.root)):
        block("private_paths", "Run root and study objects must be under the confinement's hidden directories; use /workspace, /tmp, /var/tmp, /home, /root or /mnt")
    try:
        marker = root / "protocol.json"
        if root.exists() and not root.is_dir():
            raise ValueError("the run root is not a directory")
        if marker.exists():
            if read_json(marker) != {"protocol_sha256": frozen["protocol_sha256"], "adapter": ADAPTER}:
                raise ValueError("the run root belongs to another protocol or adapter")
        elif root.exists() and any(p.name != ".runner.lock" for p in root.iterdir()):
            raise ValueError("historical runs cannot be registered retroactively; choose a fresh run root")
        scheduled = {job["id"] for job in frozen["jobs"]}
        for path in (root / "receipts").glob("*.json"):
            saved = read_json(path)
            value = objects.get(saved["sha256"])
            if (path.stem not in scheduled or saved.get("protocol_sha256") != frozen["protocol_sha256"]
                    or value.get("job_id") != path.stem or value.get("protocol_sha256") != frozen["protocol_sha256"]):
                raise ValueError("a terminal receipt does not match its scheduled job")
            for sha in value.get("evidence", {}).values():
                objects.body(sha)
            job = next(job for job in frozen["jobs"] if job["id"] == path.stem)
            native = specs.get(job["side"], {}).get("runtime_profile") == native_runtime.PROFILE
            issues = receipt_evidence_issues(value, objects, native=native)
            if issues:
                block("receipt_evidence", f"Job {path.stem} has " + "; ".join(issues)
                      + "; retain and diagnose its original evidence before further paid dispatch")
            completed.append(path.stem)
            receipts[path.stem] = value
            journaled.add(path.stem)
        for path in (root / "started").glob("*.json"):
            saved = read_json(path)
            if (path.stem not in scheduled or saved.get("job_id") != path.stem
                    or saved.get("protocol_sha256") != frozen["protocol_sha256"]):
                raise ValueError("a start marker does not match its scheduled job")
            journaled.add(path.stem)
            if path.stem not in receipts:
                interrupted.append(path.stem)
        if interrupted:
            block("interrupted_jobs", "Jobs started without a terminal receipt: " + ", ".join(interrupted)
                  + "; reconcile each original attempt without rerunning or deleting its journal")
    except (ValueError, OSError, KeyError, TypeError) as exc:
        block("run_journal", f"Cannot resume this run root: {exc}; preserve its evidence and resolve the conflict")
    pending = [job for job in frozen["jobs"] if job["id"] not in completed]
    admission = None
    if specs:
        cfg = specs["baseline"]["config"]
        try:
            ledger_path = root / "state.json"
            executed = any(any((root / name).glob("*.json")) for name in ("started", "receipts"))
            if executed and not ledger_path.exists():
                raise ValueError("state.json accounting ledger is missing after execution; restore and reconcile the original ledger before resuming")
            state = read_json(ledger_path) if ledger_path.exists() else {}
            if executed and (not isinstance(state, dict) or not isinstance(state.get("accounting"), dict)):
                raise ValueError("state.json accounting ledger is missing after execution; restore and reconcile the original ledger before resuming")
            # Budget refreshes a private in-memory copy only; preflight never persists or reserves.
            ledger = Budget(state, cfg["budget_usd"], cfg["session_budget_usd"])
            validate_admission_journal(frozen, objects, journaled, receipts, ledger)
            admission = {"cap_usd": ledger.total, "accounted_usd": ledger.spent,
                         "remaining_reservation_usd": len(pending) * ledger.per_session}
            if ledger.data["blocked_reason"]:
                block("accounting", f"Dispatch is blocked: {ledger.data['blocked_reason']}; reconcile the original accounting ledger")
            elif pending and ledger.spent + len(pending) * ledger.per_session > ledger.total + 1e-9:
                block("budget", "The remaining budget cannot reserve every unfinished job; inspect the original ledger before starting a new study")
        except (ValueError, OSError, KeyError, TypeError) as exc:
            block("accounting", f"Cannot read the accounting ledger: {exc}; restore or reconcile its original records")
    if pending and specs:
        if sys.platform != "linux":
            block("platform", "Frozen execution requires Linux; move the study to its pinned Linux execution host")
        else:
            error = confinement_probe()
            if error:
                block("confinement", error)
        if any(os.environ.get(key) for key in runner.AGENT_MARKERS):
            block("agent_session", "Refusing paid execution inside an agent session; run --execute from an operator terminal")
        version = runner.harness_version()
        if any(version != spec["harness_version"] for spec in specs.values()):
            block("harness", "Claude Code is missing or its version differs from the frozen study; restore the pinned harness version")
        for side, spec in specs.items():
            cfg = spec["config"]
            forward = cfg.get("forward_env", [])
            if not runner.auth_source(os.environ, forward, bool(cfg.get("host_auth"))):
                block("authentication", f"{side}: configured Claude Code authentication is required; configure the selected authentication method before --execute")
            env = runner.isolation_env(root / "__preflight_home__", os.environ, forward)
            exposed = exposed_private_inputs(spec, root, objects, env)
            if exposed:
                block("private_mounts", f"{side}: a mounted tool/auth directory would expose independent grader/data or study evidence: "
                      + ", ".join(exposed) + "; move private inputs outside those mounted directories and prepare again")
    return {"execute": False, "ready_to_execute": not blockers, "protocol_sha256": frozen["protocol_sha256"],
            "jobs": frozen["jobs"], "completed_runs": len(completed), "pending_runs": len(pending),
            "interrupted_jobs": interrupted, "max_cost_microusd": frozen["max_cost_microusd"],
            "admission": admission, "blockers": blockers, "reflex_set": reflex_set,
            "outputs": {"run_root": str(root), "objects": str(objects.root), "report": str(root / "report.json"),
                        "submission": str(root / "submission.json"), "receipts": str(root / "receipts")}}


def require_ready(plan):
    if plan["blockers"]:
        raise ValueError("Frozen execution is not ready:\n" + "\n".join(item["message"] for item in plan["blockers"]))


def submission(frozen, root):
    receipts = []
    for path in sorted((Path(root) / "receipts").glob("*.json")):
        saved = read_json(path)
        if saved.get("protocol_sha256") != frozen["protocol_sha256"]:
            raise ValueError("receipt journal belongs to another protocol")
        receipts.append(saved["sha256"])
    return {"schema": 1, "protocol_sha256": frozen["protocol_sha256"], "receipts": receipts}


def bind_root(frozen, root):
    root = Path(root)
    root.mkdir(parents=True, exist_ok=True)
    marker = root / "protocol.json"
    if not marker.exists() and any(p.name != ".runner.lock" for p in root.iterdir()):
        raise ValueError("frozen execution requires a fresh root; historical runs cannot be registered retroactively")
    publish(marker, {"protocol_sha256": frozen["protocol_sha256"], "adapter": ADAPTER})


def report(frozen_path, frozen, objects, root, adage, required=None):
    bind_root(frozen, root)
    path = Path(root) / "submission.json"
    publish(path, submission(frozen, root), replace=True)
    args = [str(adage), "eval", "experiment", "report", str(frozen_path), str(path), "--objects", str(objects.root)]
    if required:
        args.append(required)
    result = subprocess.run(args, capture_output=True, text=True, timeout=120)
    if result.stdout:
        publish(Path(root) / "report.json", json.loads(result.stdout), replace=True)
    else:
        # A corrupt source must not leave an earlier successful report as the current view.
        publish(Path(root) / "report.json", {"schema": 1, "protocol_sha256": frozen["protocol_sha256"],
                "evidence_complete": False, "report_valid": False, "error": result.stderr.strip()}, replace=True)
    return result


def execute(frozen_path, frozen, objects, root, adage, *, live_factory=runner.Live):
    root = Path(root).resolve()
    require_ready(preflight(frozen, objects, root, adage))
    specs = bindings(frozen, objects, adage)
    with execution_lock(root):
        # Another collector may have finished after the first preflight read its receipts.
        # Revalidate while holding the dispatch lock before admitting untouched jobs.
        require_ready(preflight(frozen, objects, root, adage))
        bind_root(frozen, root)
        stopped = []
        for job in frozen["jobs"]:
            terminal = root / "receipts" / f"{job['id']}.json"
            started = root / "started" / f"{job['id']}.json"
            if terminal.exists():
                saved = read_json(terminal)
                existing = objects.get(saved["sha256"])
                if existing["job_id"] != job["id"] or existing["protocol_sha256"] != frozen["protocol_sha256"]:
                    raise ValueError("terminal receipt does not match its scheduled job")
                continue
            if started.exists():
                raise ValueError(f"job {job['id']} started without a terminal receipt; reconcile it without rerunning")
            spec = specs[job["side"]]
            observer = Observer(frozen, job, spec, objects, adage)
            live = live_factory(copy.deepcopy(spec["config"]), root, adage, 1, isolate=True, resume=False, observer=observer)
            live.home = root / "private" / "reflexes" / job["id"]
            live.home.mkdir(parents=True, exist_ok=True)
            if job["side"] == "intervention":
                sha = frozen["protocol"]["intervention"]["reflex_set_sha256"]
                # `object` preserved the exact index bytes; copy privately without reformatting.
                publish_bytes(live.home / "index.json", objects.body(sha))
            if live.budget.data["blocked_reason"]:
                break
            if not publish(started, {"protocol_sha256": frozen["protocol_sha256"], "job_id": job["id"]}):
                raise ValueError("job was already started")
            task = observer.task["identity"]["task_id"]
            prompt = spec["config"]["task_prompts"][task].replace("{", "{{").replace("}", "}}")
            try:
                treatment = "mod" if observer.native else "adage"
                live.run_one(job["id"], "eval", "off" if job["side"] == "baseline" else treatment, task, prompt)
                value = observer.finish(live)
                sha = objects.put(value)
                publish(terminal, {"protocol_sha256": frozen["protocol_sha256"], "sha256": sha})
            except Exception as exc:
                publish(root / "errors" / f"{job['id']}.json", {"error": str(exc)[:1000], "job_id": job["id"]})
                # Preserve the scheduled denominator and stop before spending on an invalid study.
                report(frozen_path, frozen, objects, root, adage)
                raise
            publish(root / "submission.json", submission(frozen, root), replace=True)
            stopped = receipt_evidence_issues(value, objects, native=observer.native)
            if live.budget.data["blocked_reason"]:
                stopped.append(live.budget.data["blocked_reason"])
            if stopped:
                break
        result = report(frozen_path, frozen, objects, root, adage)
        if result.returncode == 0 and (stopped or not json.loads(result.stdout)["evidence_complete"]):
            result.returncode = 2
            detail = ": " + "; ".join(stopped) if stopped else ""
            result.stderr = "frozen collection is incomplete" + detail + "; inspect report.json and the accounting ledger\n"
        return result


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    prep = commands.add_parser("prepare")
    prep.add_argument("config", type=Path)
    prep.add_argument("--training-audit", type=Path, required=True)
    prep.add_argument("--reflex-set", type=Path, required=True)
    prep.add_argument("--out", type=Path, required=True)
    prep.add_argument("--input", type=Path, action="append", required=True)
    prep.add_argument("--learner-revision", required=True)
    prep.add_argument("--timeout-seconds", type=int, default=600)
    prep.add_argument("--policy", type=Path)
    prep.add_argument("--identities", type=Path)
    prep.add_argument("--intervention-model")
    prep.add_argument("--runtime-profile", choices=(RUNTIME_PROFILE, native_runtime.PROFILE), default=RUNTIME_PROFILE)
    run = commands.add_parser("run")
    run.add_argument("frozen", type=Path)
    run.add_argument("--root", type=Path, required=True)
    run.add_argument("--execute", action="store_true")
    rep = commands.add_parser("report")
    rep.add_argument("frozen", type=Path)
    rep.add_argument("--root", type=Path, required=True)
    required = rep.add_mutually_exclusive_group()
    required.add_argument("--require-token-target", action="store_true")
    required.add_argument("--require-cost-target", action="store_true")
    for command in (prep, run, rep):
        command.add_argument("--objects", type=Path, required=True)
        command.add_argument("--adage", type=Path, default=runner.REPO / "target" / "release" / "adage")
    args = parser.parse_args(argv)
    objects, adage = Objects(args.objects), args.adage.resolve()
    if args.command == "prepare":
        result = prepare(args.config, args.training_audit, args.reflex_set, objects, args.out, adage, args.input,
                         timeout=args.timeout_seconds, learner_revision=args.learner_revision,
                         policy=read_json(args.policy) if args.policy else None,
                         identities=read_json(args.identities) if args.identities else None,
                         intervention_model=args.intervention_model, runtime_profile=args.runtime_profile)
        print(json.dumps({"protocol_sha256": result["protocol_sha256"], "runs": len(result["jobs"]),
                          "max_cost_microusd": result["max_cost_microusd"]}))
        return 0
    frozen = adage_json(adage, "validate", args.frozen, "--objects", objects.root)
    if args.command == "report":
        required = "--require-token-target" if args.require_token_target else "--require-cost-target" if args.require_cost_target else None
        with execution_lock(args.root):
            result = report(args.frozen, frozen, objects, args.root, adage, required)
    elif not args.execute:
        print(json.dumps(preflight(frozen, objects, args.root, adage), indent=2))
        return 0
    else:
        result = execute(args.frozen, frozen, objects, args.root.resolve(), adage)
    print(result.stdout, end="")
    if result.stderr:
        print(result.stderr, file=sys.stderr, end="")
    return result.returncode


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (ValueError, OSError, subprocess.SubprocessError) as exc:
        raise SystemExit(str(exc)) from None
