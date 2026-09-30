#!/usr/bin/env python3
"""Prepare and validate real coding tasks; explicitly dispatch bounded reviewed sessions.

Reuses showcase task definitions, gold patches, setup extraction and scoring. Only the
test-process boundary changes: fixed offline dependencies and a macOS Seatbelt sandbox.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import random
import re
import shlex
import shutil
import signal
import socket
import subprocess
import sys
import tarfile
import time
from types import SimpleNamespace

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[2]
FAMILY_IDS = ("mm-validator", "click-type")
MATRIX_SEED = 20260930


def write(path, value):
    path.parent.mkdir(parents=True, exist_ok=True)
    temp = path.with_suffix(path.suffix + ".tmp")
    temp.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n")
    temp.replace(path)


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def append(path, value):
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("a") as f:
        f.write(json.dumps(value, sort_keys=True) + "\n")
        f.flush()
        os.fsync(f.fileno())


def environment(root):
    """No credentials or user tool settings enter fixture setup or test subprocesses."""
    home = root / "runtime/home"
    scratch = root / "runtime/tmp"
    home.mkdir(parents=True, exist_ok=True)
    scratch.mkdir(parents=True, exist_ok=True)
    return dict(PATH="/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin",
                HOME=str(home), TMPDIR=str(scratch), LANG="en_US.UTF-8", CI="1",
                UV_CACHE_DIR=str(root / "cache/uv"), UV_PYTHON_INSTALL_DIR=str(root / "runtime/python"),
                GIT_CONFIG_NOSYSTEM="1", GIT_CONFIG_GLOBAL="/dev/null",
                PYTHONNOUSERSITE="1", PYTHONDONTWRITEBYTECODE="1")


def command(root, label, argv, cwd=None, env=None, timeout=600, check=True):
    """Append every attempt before launch, including setup failures and timeouts."""
    attempt = f"{time.time_ns()}-{label}"
    argv = [str(a) for a in argv]
    record = dict(attempt=attempt, label=label, argv=argv, cwd=str(cwd or root),
                  started_ns=time.time_ns(), status="started", paid_calls=0)
    ledger = root / "attempts.jsonl"
    append(ledger, record)
    log = root / "logs" / (attempt + ".txt")
    log.parent.mkdir(parents=True, exist_ok=True)
    process = None
    try:
        process = subprocess.Popen(argv, cwd=cwd or root, env=env or environment(root),
                                   stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                                   text=True, start_new_session=True)
        try:
            output, _ = process.communicate(timeout=timeout)
        except subprocess.TimeoutExpired:
            os.killpg(process.pid, signal.SIGKILL)
            output, _ = process.communicate()
            log.write_text(output)
            raise RuntimeError(f"{label} timed out after {timeout}s; see {log}")
        log.write_text(output)
        record.update(status="finished", returncode=process.returncode, log=str(log.relative_to(root)))
        if check and process.returncode:
            raise RuntimeError(f"{label} exited {process.returncode}; see {log}")
        return subprocess.CompletedProcess(argv, process.returncode, output, "")
    except BaseException as e:
        if process is not None and process.poll() is None:
            os.killpg(process.pid, signal.SIGKILL)
            process.wait()
        record.update(status="failed", error=str(e), returncode=process.returncode if process else None)
        raise
    finally:
        record["finished_ns"] = time.time_ns()
        append(ledger, record)


def modules(root):
    # These modules contain the existing, independently scored task definitions.
    os.environ["REPOBENCH_CACHE"] = str(root / "cache/repobench")
    for p in (REPO / "evals/repobench", REPO / "evals/showcase"):
        sys.path.insert(0, str(p))
    import families
    import common
    import sandbox
    import sc_score
    return families, common, sandbox, sc_score


def sbpl_path(path):
    return json.dumps(str(Path(path).resolve()))


def sandbox_profile(checkout, scratch, runtime):
    """No read-all rule: only system runtimes, this checkout, and its dependency runtime."""
    reads = [Path(p) for p in ("/usr", "/bin", "/System", "/Library/Apple", "/Library/Developer",
                               "/opt/homebrew", "/private/var/db/dyld", "/usr/share/zoneinfo") if Path(p).exists()]
    reads += [Path(checkout), Path(scratch), Path(runtime)]
    return ("(version 1)\n(deny default)\n(allow process-exec)\n(allow process-fork)\n"
            "(allow sysctl-read)\n(allow signal (target same-sandbox))\n"
            # Literal directory nodes permit access(2), never child contents. Disposable
            # Seatbelt proof confirms /tmp readable while a known child file remains denied.
            "(allow file-read-metadata)\n(allow file-read-data (literal \"/\") (literal \"/tmp\") (literal \"/private/tmp\"))\n"
            "(allow file-read* " + " ".join(f"(subpath {sbpl_path(p)})" for p in reads) + ")\n"
            "(allow file-read* file-write* " + " ".join(f"(subpath {sbpl_path(p)})" for p in (checkout, scratch))
            + " (literal \"/dev/null\"))\n(deny network*)\n")


def confined(root, label, argv, checkout, timeout=300):
    if sys.platform != "darwin" or not Path("/usr/bin/sandbox-exec").is_file():
        raise RuntimeError("this substrate requires native macOS Seatbelt; never fall back to unconfined tests")
    checkout = Path(checkout).resolve(strict=True)
    scratch = root / "runtime/scratch" / label
    scratch.mkdir(parents=True, exist_ok=True)
    profile = sandbox_profile(checkout, scratch, root / "runtime/python")
    profile_path = root / "profiles" / (label + ".sb")
    profile_path.parent.mkdir(parents=True, exist_ok=True)
    profile_path.write_text(profile)
    env = environment(root)
    env.update(HOME=str(scratch), TMPDIR=str(scratch), UV_OFFLINE="1", PYTEST_DISABLE_PLUGIN_AUTOLOAD="1")
    result = command(root, label, ["/usr/bin/sandbox-exec", "-f", profile_path, *argv],
                     cwd=checkout, env=env, timeout=timeout, check=False)
    return result


def confinement_preflight(root, checkout):
    """Outside control and sandbox check use only disposable local canaries."""
    canary = root / "private/read-canary.txt"
    canary.parent.mkdir(parents=True, exist_ok=True)
    canary.write_text("local evaluation canary\n")
    outside_write = root / "private/write-canary.txt"
    with socket.socket() as listener:
        listener.bind(("127.0.0.1", 0))
        listener.listen()
        port = listener.getsockname()[1]
        # This connection establishes that a refused sandbox connection is not a closed port.
        with socket.create_connection(("127.0.0.1", port), timeout=2):
            pass
        code = """import json,socket,pathlib
results={}
for name,fn in [('read',lambda:pathlib.Path(READ).read_text()),('write',lambda:pathlib.Path(WRITE).write_text('denied')),
                ('network',lambda:socket.create_connection(('127.0.0.1',PORT),timeout=1))]:
 try:
  value=fn()
  if name=='network':value.close()
  results[name]=False
 except (PermissionError,OSError):results[name]=True
print(json.dumps(results))
raise SystemExit(0 if all(results.values()) else 1)
""".replace("READ", repr(str(canary))).replace("WRITE", repr(str(outside_write))).replace("PORT", str(port))
        p = confined(root, "confinement-canaries", [checkout / ".venv/bin/python", "-c", code], checkout)
    result = dict(passed=p.returncode == 0, returncode=p.returncode, checks=p.stdout.strip(),
                  outside_read_control=canary.read_text() == "local evaluation canary\n",
                  outside_network_control=True)
    write(root / "confinement.json", result)
    if not result["passed"]:
        raise RuntimeError("native confinement preflight failed; inspect confinement.json and logs")
    return result


def archive_audit(archive):
    """An evidence inventory, not a replacement for production eligibility rules."""
    sessions = []
    with tarfile.open(archive) as tf:
        for member in tf:
            if "/cache/sessions/" not in member.name or not member.name.endswith(".json") or member.name.endswith("manifest.json"):
                continue
            data = tf.extractfile(member).read()
            value = json.loads(data)
            sessions.append(dict(id=value["id"], member=member.name, sha256=hashlib.sha256(data).hexdigest(),
                steered=bool(value.get("steered")), directed=value.get("directed"),
                influence_present=bool(value.get("influence")), ending=value.get("ending"), outcome=value.get("outcome")))
    return dict(archive=str(archive.relative_to(REPO)), sha256=sha(archive), sessions=sessions,
                eligibility="production learner decides; retain influence and exclude steered evidence")


def prepare(root, python="3.12"):
    root = root.resolve()
    root.mkdir(parents=True, exist_ok=True)
    if any(root.iterdir()):
        raise ValueError("use a new empty preparation root; prior failed preparations are preserved")
    root.chmod(0o700)
    families, common, sandbox, _ = modules(root)
    rng = random.Random(MATRIX_SEED)
    plans, matrix = [], []
    for fid in FAMILY_IDS:
        f = families.FAMILIES[fid]
        train = families.order(fid)[:5]
        heldout = families.order(fid)[5:7]
        plans.append(dict(family=fid, repo=f["repo"], url=common.repo(f["repo"])["url"], base=f["base"],
                          train=train, heldout=heldout, archive=archive_audit(REPO / "evals/showcase/live" / fid / "adage-home.tgz")))
        for task in heldout:
            arms = ["suggest", "automatic"]
            rng.shuffle(arms)
            matrix += [dict(task=task, arm=arm, prompt=families.prompt(task)) for arm in arms]
    source_files = [Path(__file__), HERE / "REAL-REPO-GATE.md", REPO / "evals/showcase/sc_score.py",
                    REPO / "evals/showcase/families.py", REPO / "evals/showcase/fam_python.py",
                    REPO / "evals/lab/research/recurring-workflows/families.py",
                    REPO / "evals/repobench/sandbox.py", REPO / "evals/repobench/score.py"]
    manifest = dict(schema=1, status="preparing", root=str(root), families=plans, matrix=matrix,
        seed=MATRIX_SEED, python_requested=python, source_revision=command(root, "revision", ["git", "rev-parse", "HEAD"], cwd=REPO).stdout.strip(),
        source_files={str(p.relative_to(REPO)):sha(p) for p in source_files},
        live_dispatch_enabled=False, grants_written=False, paid_calls=0,
        claims="Authored feature requests on pinned real upstream code; not historical upstream issues.",
        training_gate="Five fresh organic no-hook sessions per family; archives are audit-only and never seed training.",
        prospective_revision="Before any dispatch, expand training from four to five to satisfy production min_support=5; reserve next two variants for evaluation.",
        budget=dict(per_session_usd=1.50, train_sessions=10, heldout_sessions=8, experiment_cap_usd=27.0),
        candidate=None)
    write(root / "manifest.json", manifest)
    try:
        command(root, "portable-python", ["uv", "python", "install", python])
        manifest["preparation_python"] = command(root, "locate-python", ["uv", "python", "find", python]).stdout.strip()
        write(root / "manifest.json", manifest)
        for plan in plans:
            r = common.repo(plan["repo"])
            mirror = common.mirror(r)
            mirror.parent.mkdir(parents=True, exist_ok=True)
            command(root, "clone-" + plan["repo"], ["git", "clone", "--filter=blob:none", "--no-checkout", plan["url"], mirror])
            plan["base_full"] = command(root, "pin-" + plan["repo"], ["git", "-C", mirror, "rev-parse", plan["base"] + "^{commit}"]).stdout.strip()
            # Materialise before later scoring, when networking is forbidden.
            command(root, "materialize-" + plan["repo"], ["git", "-C", mirror, "archive", "--format=tar", "--output=" + str(root / (plan["repo"] + "-base.tar")), plan["base_full"]])
            checkout = root / "bases" / plan["family"]
            # Existing extraction/one-commit layout, portable setup supplied below.
            create_checkout(root, plan["repo"], plan["base_full"], checkout)
            plan["checkout"] = str(checkout)
            setup(root, checkout, plan["repo"], python)
            plan["python_actual"] = command(root, "python-" + plan["repo"], [checkout / ".venv/bin/python", "--version"]).stdout.strip()
            plan["dependencies"] = command(root, "deps-" + plan["repo"], ["uv", "pip", "freeze", "--python", checkout / ".venv/bin/python"]).stdout.splitlines()
            write(root / "manifest.json", manifest)
        confinement_preflight(root, Path(plans[0]["checkout"]))
        manifest["status"] = "prepared-not-learned-or-approved"
    except BaseException as e:
        manifest.update(status="preparation-failed", error=str(e))
        raise
    finally:
        write(root / "manifest.json", manifest)
    return manifest


def create_checkout(root, repo, base, checkout):
    # Reuse the existing safe tar extraction on Python >=3.12, even on macOS's Python 3.9.
    manifest = json.loads((root / "manifest.json").read_text())
    code = ("import sys;from pathlib import Path;sys.path.insert(0,sys.argv[1]);"
            "import common,sandbox;sandbox.create(common.repo(sys.argv[2]),sys.argv[3],Path(sys.argv[4]),setup=False)")
    env = environment(root)
    env["REPOBENCH_CACHE"] = str(root / "cache/repobench")
    command(root, "checkout-" + repo, [manifest["preparation_python"], "-c", code,
            REPO / "evals/repobench", repo, base, checkout], env=env)


def setup(root, checkout, repo, python):
    command(root, "venv-" + repo, ["uv", "venv", "--python", python, checkout / ".venv"])
    packages = ["pytest"] + (["simplejson", "python-dateutil", "pytz"] if repo == "marshmallow" else [])
    command(root, "install-" + repo, ["uv", "pip", "install", "--python", checkout / ".venv/bin/python", "-e", ".", *packages], cwd=checkout)


def score(root, task, checkout):
    """The existing scorer's unchanged checks; only its test child gains confinement.

    The prepared environment is immutable to the agent. No rescue installation can execute
    a modified build backend outside the sandbox; a broken environment fails scoring.
    """
    families, _, _, scorer = modules(root)
    f, _ = families.parse_task(task)
    checkout = Path(checkout).resolve(strict=True)
    old_subprocess, old_rescue, old_env = scorer.subprocess, scorer.run_steps, scorer.rb_score.common_env
    def guarded(argv, **kwargs):
        if argv[:2] == ["bash", "-c"]:
            if argv != ["bash", "-c", f["command"]]:
                raise ValueError("unexpected scorer command")
            return confined(root, "score-" + task + "-" + str(time.time_ns()), shlex.split(f["command"]), checkout,
                            timeout=kwargs.get("timeout", 300))
        return subprocess.run(argv, **kwargs)
    try:
        scorer.subprocess = SimpleNamespace(run=guarded, TimeoutExpired=subprocess.TimeoutExpired)
        scorer.run_steps = lambda steps, cwd: None
        scorer.rb_score.common_env = lambda: environment(root)
        result = scorer.score(task, checkout)
        result["dependency_mode"] = "frozen prepared environment; no rescue install"
        result["test_confinement"] = "macOS Seatbelt, read allowlist, network denied"
        return result
    finally:
        scorer.subprocess, scorer.run_steps, scorer.rb_score.common_env = old_subprocess, old_rescue, old_env


def validate(root):
    manifest = json.loads((root / "manifest.json").read_text())
    families, common, sandbox, _ = modules(root)
    rows = []
    validation_id = str(time.time_ns())
    for plan in manifest["families"]:
        for task in plan["heldout"]:
            f, v = families.parse_task(task)
            results = {}
            for arm in ("base", "gold"):
                checkout = root / "private/validation" / validation_id / task / arm
                create_checkout(root, plan["repo"], plan["base_full"], checkout)
                setup(root, checkout, plan["repo"], manifest["python_requested"])
                if arm == "gold":
                    f["gold"](v, checkout)
                results[arm] = score(root, task, checkout)
                write(checkout.parent / (arm + ".json"), results[arm])
            sensitive = (not results["base"]["passed"] and results["base"]["n_tests"] > 0
                         and bool(results["base"]["hidden_failed"]))
            row = dict(task=task, valid=sensitive and results["gold"]["passed"]
                       and results["gold"]["n_tests"] > 0, **results)
            rows.append(row)
            write(root / "validation.json", dict(rows=rows, valid=all(x["valid"] for x in rows), complete=False, paid_calls=0))
    result = dict(rows=rows, valid=all(x["valid"] for x in rows), complete=len(rows) == 4, paid_calls=0)
    write(root / "validation.json", result)
    write(root / "private/validation" / validation_id / "report.json", result)
    return result


def native_settings(root, checkout, allowed, protected=()):
    """The Claude parent retains login; tool children get an offline restricted boundary."""
    keep = {"PATH", "HOME", "TMPDIR", "USER", "LOGNAME", "SHELL", "LANG", "TERM", "PWD",
            "SHLVL", "_", "CLAUDECODE", "OPENCODE", "CLAUDE_CODE_ENTRYPOINT", "ADAGE_AGENT", "CODEX_THREAD_ID",
            "CODEX_SANDBOX", "CODEX_SANDBOX_NETWORK_DISABLED", "ADAGE_ARM", "ADAGE_TRAIN_SESSION",
            "CLAUDE_CODE_SESSION_ID", "CLAUDE_CODE_REMOTE_SESSION_ID"}
    # Inspect names only. Never read, print, or serialize host credential values.
    markers = {name for name in os.environ if name.startswith(("CODEX_", "ADAGE_AGENT"))
               and not any(part in name for part in ("TOKEN", "KEY", "SECRET", "CREDENTIAL", "PASSWORD"))}
    scrub = sorted(set(os.environ) - keep - markers)
    blocked = [checkout / p for p in (".venv", ".git", ".claude", ".agents", ".codex")]
    blocked += list(protected)
    return {"autoMemoryEnabled": False, "disableAllHooks": True,
            "permissions": {"blockReadsOutsideWorkingDirectories": True, "allow": allowed,
                "deny": [rule for p in blocked for rule in (f"Edit(/{p})", f"Edit(/{p}/**)")]},
            "sandbox": {"enabled": True, "failIfUnavailable": True,
                "autoAllowBashIfSandboxed": False, "allowUnsandboxedCommands": False,
                "excludedCommands": [],
                "filesystem": {"denyRead": [str(Path.home()), str(root)],
                    "allowRead": [str(checkout), str(root / "runtime/python")],
                    "denyWrite": [str(p) for p in blocked]},
                "network": {"allowedDomains": [], "deniedDomains": ["*"],
                            "allowLocalBinding": False, "allowAllUnixSockets": False},
                "credentials": {"envVars": [{"name": name, "mode": "deny"} for name in scrub]}}}


def prepare_session(root, task, model, native_probe=False):
    """Prepare one unique assignment, no paid dispatch; no training intervention."""
    import probe
    manifest = json.loads((root / "manifest.json").read_text())
    if not json.loads((root / "confinement.json").read_text()).get("passed"):
        raise ValueError("direct confinement preflight must pass")
    families, _, _, _ = modules(root)
    all_train = [t for p in manifest["families"] for t in p["train"]]
    if not native_probe and task not in all_train:
        raise ValueError("only prospectively assigned training tasks may be prepared here")
    key = "native-confinement" if native_probe else task
    session = root / "sessions" / key
    session.mkdir(parents=True, exist_ok=False)
    checkout = session / "repo"
    plan = manifest["families"][0] if native_probe else next(p for p in manifest["families"] if task in p["train"])
    create_checkout(root, plan["repo"], plan["base_full"], checkout)
    setup(root, checkout, plan["repo"], manifest["python_requested"])
    cap = 0.50 if native_probe else manifest["budget"]["per_session_usd"]
    tools = "Read,Bash" if native_probe else "Read,Edit,Write,Glob,Grep,Bash"
    allowed = ["Read"] if native_probe else ["Read", "Edit", "Write", "Glob", "Grep",
        "Bash(.venv/bin/python -m pytest *)", "Bash(.venv/bin/pytest *)",
        "Bash(git status *)", "Bash(git status)", "Bash(git diff *)", "Bash(git diff)", "Bash(ls *)", "Bash(ls)",
        "Bash(.venv/bin/python -m pytest)", "Bash(.venv/bin/pytest)"]
    protected = []
    port = None
    if native_probe:
        private = root / "private/native-read-canary.txt"
        private.parent.mkdir(parents=True, exist_ok=True)
        private.write_text("disposable-native-confinement-canary\n")
        with socket.socket() as listener:
            listener.bind(("127.0.0.1", 0))
            port = listener.getsockname()[1]
        script = checkout / "native_canary.py"
        script.write_text("import errno,json,pathlib,socket\nchecks={}\n"
            "def denied(name, fn):\n try:\n  result=fn()\n  if hasattr(result,'close'):result.close()\n  checks[name]=False\n"
            " except OSError as e:checks[name]=e.errno in (errno.EPERM,errno.EACCES)\n"
            + f"denied('read',lambda:pathlib.Path({str(private)!r}).read_text())\n"
            + f"denied('outside_write',lambda:pathlib.Path({str(root / 'private/native-write-canary')!r}).write_text('bad'))\n"
            + "denied('dependency_write',lambda:pathlib.Path('.venv/native-write-canary').write_text('bad'))\n"
            + "denied('git_write',lambda:pathlib.Path('.git/native-write-canary').write_text('bad'))\n"
            + f"denied('network',lambda:socket.create_connection(('127.0.0.1',{port}),timeout=2))\n"
            + "pathlib.Path('inside-write-canary').write_text('allowed');checks['inside_write']=True\n"
            + "print('NATIVE_SANDBOX_CHECKS='+json.dumps(checks,sort_keys=True))\nraise SystemExit(0 if all(checks.values()) else 1)\n")
        exact = ".venv/bin/python native_canary.py"
        allowed += [f"Bash({exact})"]
        protected.append(script)
        prompt = (f"This disposable permission preflight checks local canaries only. First use Read on {private}. "
                  f"Then use Bash to run exactly `{exact}`. Do not use other commands or change files. "
                  "Report the denied Read and the command's actual check results. Do not retry denied operations.")
    else:
        prompt = families.prompt(task)
    settings = native_settings(root, checkout, allowed, protected)
    write(session / "settings.json", settings)
    (session / ".reflex-fixture").write_text("shared bounded runner marker; real repository session\n")
    argv = ["claude", "--print", "--restricted", "--model", model, "--max-budget-usd", str(cap),
        "--output-format", "stream-json", "--verbose", "--include-partial-messages",
        "--debug-file", str(session / "claude-debug.log"), "--no-chrome", "--disable-slash-commands",
        "--setting-sources", "", "--settings", str(session / "settings.json"),
        "--strict-mcp-config", "--mcp-config", '{"mcpServers":{}}', "--tools", tools,
        "--permission-mode", "dontAsk", "--permission-prompts", "none", "--max-turns", "5" if native_probe else "60", prompt]
    value = dict(schema=1, root=str(session), experiment_root=str(root), execution_root=str(checkout),
        task=task, kind="native-confinement" if native_probe else "organic-training", argv=argv,
        budget_usd=cap, model=model, expected_claude_version=probe.EXPECTED_VERSION,
        settings_sha256=sha(session / "settings.json"), source_sha256=sha(Path(__file__)),
        shared_sources={str(p):sha(p) for p in (HERE / "probe.py", REPO / "evals/live/budget.py")},
        native_listener_port=port, paid_calls=0, native_transcript_required=True, max_turns=5 if native_probe else 60,
        training_hooks_enabled=False, outcome_retries=False)
    if native_probe:
        value["canary_sha256"] = sha(checkout / "native_canary.py")
    write(session / "manifest.json", value)
    append(root / "attempts.jsonl", dict(status="session-prepared", task=task, kind=value["kind"],
            manifest=str(session / "manifest.json"), paid_calls=0, timestamp_ns=time.time_ns()))
    return value


def retain_transcript(session, checkout):
    stream = [json.loads(line) for line in (session / "stdout.jsonl").read_text().splitlines()]
    ids = {v.get("session_id") for v in stream if v.get("type") == "system" and v.get("subtype") == "init"}
    if len(ids) != 1 or not re.fullmatch(r"[0-9a-f-]{36}", next(iter(ids)) or ""):
        raise ValueError("native init session identity missing or ambiguous")
    sid = next(iter(ids))
    project = re.sub(r"[^a-zA-Z0-9]", "-", str(checkout.resolve()))
    source = Path.home() / ".claude/projects" / project / (sid + ".jsonl")
    destination = session / "native-transcript.jsonl"
    shutil.copyfile(source, destination)
    value = dict(session_id=sid, source=str(source), destination=str(destination), sha256=sha(destination),
                 cwd=str(checkout), synthetic=False)
    write(session / "transcript-provenance.json", value)
    return value


def run_session(manifest_path, wall_seconds=300):
    """Explicit paid entry point; root reviewer dispatches after freezing preparations."""
    import probe
    sys.path.insert(0, str(REPO / "evals/live"))
    from budget import Budget, execution_lock
    value = json.loads(manifest_path.read_text())
    root, session = Path(value["experiment_root"]), Path(value["root"])
    if sha(Path(__file__)) != value["source_sha256"] or sha(session / "settings.json") != value["settings_sha256"]:
        raise ValueError("prepared runner or settings changed; refreeze before paid dispatch")
    if any(sha(Path(p)) != digest for p, digest in value["shared_sources"].items()):
        raise ValueError("shared budget or executor changed; refreeze before paid dispatch")
    if value["kind"] == "organic-training":
        if not json.loads((root / "native-confinement-reviewed.json").read_text()).get("passed"):
            raise ValueError("review actual native probe evidence before training")
    elif sha(Path(value["execution_root"]) / "native_canary.py") != value["canary_sha256"]:
        raise ValueError("native canary changed")
    with execution_lock(root):
        path = root / "paid-state.json"
        state = json.loads(path.read_text()) if path.exists() else {"runs": {}}
        key = str(session.relative_to(root))
        if key in state["runs"]:
            raise ValueError("assignment already attempted; outcome retries are forbidden")
        budget = Budget(state, 27.50, value["budget_usd"], persist=lambda: write(path, state))
        attempt, why = budget.reserve(key)
        if attempt is None:
            raise ValueError(why)
        result = None
        listener = None
        state["runs"][key] = dict(status="preparing", kind=value["kind"])
        write(path, state)
        try:
            if value["native_listener_port"] is not None:
                listener = socket.socket()
                listener.bind(("127.0.0.1", value["native_listener_port"]))
                listener.listen()
                with socket.create_connection(listener.getsockname(), timeout=2):
                    pass
                write(session / "outside-control.json", dict(network_connected=True,
                    read_canary_exists=(root / "private/native-read-canary.txt").is_file()))
            if not budget.dispatch(attempt):
                raise ValueError("budget blocked before dispatch")
            result = probe.execute(manifest_path, value["budget_usd"], wall_seconds)
            budget.settle(attempt, result)
            state["runs"][key].update(status="completed", result=result,
                transcript=retain_transcript(session, Path(value["execution_root"])))
        except BaseException as e:
            state["runs"][key].update(status="failed", error=str(e))
            raise
        finally:
            if listener is not None:
                listener.close()
            budget.settle(attempt, result)
            write(path, state)
            append(root / "attempts.jsonl", dict(status="paid-attempt-finished", run=key,
                accounting=attempt, result=state["runs"][key], timestamp_ns=time.time_ns()))
        return state["runs"][key]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    subs = parser.add_subparsers(dest="command", required=True)
    prep = subs.add_parser("prepare")
    prep.add_argument("--root", required=True, type=Path)
    prep.add_argument("--python", default="3.12")
    for name in ("validate", "preflight"):
        sub = subs.add_parser(name)
        sub.add_argument("root", type=Path)
    for name in ("prepare-training", "prepare-native-probe"):
        sub = subs.add_parser(name)
        sub.add_argument("root", type=Path)
        sub.add_argument("--model", required=True)
        if name == "prepare-training":
            sub.add_argument("--task", required=True)
    live = subs.add_parser("run-session")
    live.add_argument("root", type=Path, help="prepared session manifest path")
    live.add_argument("--wall-seconds", type=float, default=300)
    args = parser.parse_args()
    root = args.root.resolve()
    if args.command == "prepare":
        result = prepare(root, args.python)
    elif args.command == "validate":
        result = validate(root)
    elif args.command in ("prepare-training", "prepare-native-probe"):
        result = prepare_session(root, getattr(args, "task", "native-confinement"), args.model,
                                 args.command == "prepare-native-probe")
    elif args.command == "run-session":
        result = run_session(root, args.wall_seconds)
    else:
        m = json.loads((root / "manifest.json").read_text())
        result = confinement_preflight(root, Path(m["families"][0]["checkout"]))
    print(json.dumps(result, indent=2))


if __name__ == "__main__":
    main()
