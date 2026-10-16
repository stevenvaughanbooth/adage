#!/usr/bin/env python3
"""Pre-dispatch training-isolation audit for launch-gate-1. Offline and read-only.

Checks one learning run (an `attempt.json` beside its fresh `adage-home` and history copies)
against the registered task splits. Fails closed when:
- a training input's task is not in a training-split group (near-duplicates share a group),
  or the input is not native, hook-free training of a declared regime;
- any input session id belongs to a non-training session found anywhere under the scanned
  authorities (held-out arms, canaries, cold pilots, diagnostics, development tasks);
- the parse corpus (cache/sessions) holds anything but exactly the declared inputs;
- route examples, fitted follow weights or route evidence exceed the declared inputs;
- per-session state that could carry other sessions exists (spools, ledger, card verdicts,
  models, config, approvals, execution journal);
- any learned-state file or input transcript names a development or evaluation task id,
  marker, or forbidden session id.
Per-route contributor lists are not recorded by the product beyond three examples, so every
learned artifact is attributed to the run's complete input set: a bound, not a finer claim.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path

PROTOCOL = Path(__file__).with_name("launch_gate_protocol.json")
TRAINING_KINDS = ("organic-training", "instruction-guided-training")
FOREIGN_STATE = ("sessions", "ledger", "train/cards.json", "models", "config.toml", "approvals.json", "exec", "learn-queue")
PRUNE = {"repo", ".venv", "private-score", "cache", "runtime", "bases", "adage-home", "history", "source", "bin", ".git"}


def sha(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def variant(task):
    return task.split("--", 1)[1] if "--" in task else task


def splits(protocol, repo):
    spec = protocol["splits"][repo]
    out = {}
    for split, groups in spec["splits"].items():
        for group in groups:
            for task in spec["groups"][group]:
                if variant(task) in out:
                    raise ValueError("task assigned to two groups or splits: " + task)
                out[variant(task)] = (split, group)
    return out


def validate_splits(protocol):
    """Every repository's groups are disjoint and every group has exactly one split."""
    for repo, spec in protocol["splits"].items():
        named = [g for groups in spec["splits"].values() for g in groups]
        if sorted(named) != sorted(spec["groups"]) or len(named) != len(set(named)):
            raise ValueError(repo + ": every group needs exactly one split")
        splits(protocol, repo)
    return True


def scan(authorities):
    """Every native session record under the authorities, with its kind and task."""
    rows = []
    for authority in authorities:
        for directory, dirs, files in os.walk(authority):
            dirs[:] = [d for d in dirs if d not in PRUNE]
            if "transcript-provenance.json" in files:
                d = Path(directory)
                prov = json.loads((d / "transcript-provenance.json").read_text())
                manifest = json.loads((d / "manifest.json").read_text()) if (d / "manifest.json").exists() else {}
                rows.append(dict(session_id=prov.get("session_id"), path=str(d), kind=manifest.get("kind"),
                                 task=manifest.get("task"), arm=manifest.get("arm"),
                                 hooks=manifest.get("training_hooks_enabled")))
    return rows


def markers(protocol, repo):
    spec, table = protocol["splits"][repo], splits(protocol, repo)
    out = set()
    for task, (split, _) in table.items():
        if split != "training":
            out.update(prefix + "--" + task for prefix in spec["families"])
            out.update(spec.get("markers", {}).get(task, []))
    return sorted(out)


def audit(attempt_dir, repo, authorities, protocol=None):
    protocol = protocol or json.loads(PROTOCOL.read_text())
    validate_splits(protocol)
    table, problems = splits(protocol, repo), []
    attempt_dir = Path(attempt_dir)
    record = json.loads((attempt_dir / "attempt.json").read_text())
    home = attempt_dir / "adage-home"
    inputs = record.get("inputs", [])
    ids = {r["session_id"] for r in inputs}
    for r in inputs:
        split = table.get(variant(r.get("task", "")), ("unassigned", None))[0]
        if split != "training":
            problems.append("input task outside the training split: %s (%s)" % (r.get("task"), split))
        if (r.get("kind") not in TRAINING_KINDS or r.get("synthetic") is not False or r.get("hooks_enabled") is not False):
            problems.append("input is not native hook-free training: " + r["session_id"])
        if sha(r["copy"]) != r["sha256"]:
            problems.append("input copy changed: " + r["copy"])
    sessions = scan(authorities)
    # One non-training record anywhere taints the id, even if a training record also names it.
    eligible = lambda s: (s["kind"] in TRAINING_KINDS and s["hooks"] is False
                          and table.get(variant(s.get("task") or ""), ("",))[0] == "training")
    forbidden = {s["session_id"] for s in sessions if s["session_id"] and not eligible(s)}
    for leaked in sorted(ids & forbidden):
        problems.append("input session is a non-training session elsewhere: " + leaked)
    cache = home / "cache/sessions"
    corpus = json.loads((cache / "manifest.json").read_text()).get("entries", {}) if (cache / "manifest.json").exists() else {}
    copies = {str(Path(r["copy"]).resolve()) for r in inputs}
    if {str(Path(p).resolve()) for p in corpus} != copies:
        problems.append("parse corpus differs from the declared inputs")
    cached = set()
    for p in cache.glob("*.json") if cache.is_dir() else []:
        if p.name != "manifest.json":
            cached.add(json.loads(p.read_text()).get("id"))
    if cached - ids:
        problems.append("parse corpus holds undeclared sessions: " + ", ".join(sorted(cached - ids)))
    why_examples = set()
    for why in home.glob("tracks/*/*/.why.json"):
        for route in json.loads(why.read_text()).values():
            why_examples.update(route.get("examples", []))
    if why_examples - ids:
        problems.append("route examples name undeclared sessions")
    for weights in home.glob("tracks/*/.follow-weights.json"):
        if json.loads(weights.read_text()).get("sessions", 0) > len(ids):
            problems.append("fitted follow weights count more sessions than declared")
    index = json.loads((home / "index.json").read_text()) if (home / "index.json").exists() else {"routes": []}
    for r in index["routes"]:
        ev = r["route"].get("evidence", {})
        if max(ev.get("support", 0), ev.get("opportunities", 0), ev.get("execution", {}).get("sessions", 0)) > len(ids):
            problems.append("route evidence exceeds declared sessions: " + r["route"]["id"])
    present = [name for name in FOREIGN_STATE if (home / name).exists()]
    if present:
        problems.append("learning home carries state beyond the declared inputs: " + ", ".join(present))
    needles = sorted(forbidden) + markers(protocol, repo)
    files = [p for p in home.rglob("*") if p.is_file()] + [Path(r["copy"]) for r in inputs]
    for path in files:
        text = path.read_text(errors="replace")
        hit = next((n for n in needles if n in text), None)
        if hit:
            problems.append("%s names development/evaluation material: %s" % (path, hit))
    state = {str(p.relative_to(home)): sha(p) for p in sorted(home.rglob("*")) if p.is_file()}
    return dict(passed=not problems, problems=problems, repo=repo, attempt=str(attempt_dir / "attempt.json"),
                attempt_sha256=sha(attempt_dir / "attempt.json"), contributing_sessions=sorted(ids),
                attribution="every learned artifact below is bounded by these contributing sessions",
                learned_state_sha256=state, scanned_sessions=len(sessions), forbidden_sessions=len(forbidden),
                markers=markers(protocol, repo))


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("attempt_dir", type=Path)
    parser.add_argument("--repo", required=True)
    parser.add_argument("--authority", type=Path, action="append", required=True)
    args = parser.parse_args()
    result = audit(args.attempt_dir, args.repo, args.authority)
    print(json.dumps(result, indent=2))
    raise SystemExit(0 if result["passed"] else 1)
