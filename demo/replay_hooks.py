#!/usr/bin/env python3
"""Replay a recorded stream of hook payloads through `adage hook claude <event>` and record what
Adage answered; compare two recordings (chunk E1, T-E1-03: the mod's observe mode changes nothing).

  # replay: one record per event (the hook's stdout, exit code, and the spool records it wrote)
  python3 demo/replay_hooks.py STREAM --out REC.jsonl [--adage BIN] [--home DIR] [--cwd DIR]
                               [--arm treated|control] [--observe] [--env K=V ...]

  # compare two recordings byte for byte (exit 0 when identical, 1 with a diff)
  python3 demo/replay_hooks.py REC_A.jsonl --compare REC_B.jsonl

  # the identity check in one go: replay STREAM twice, into two fresh homes, without and with
  # ADAGE_MOD_OBSERVE=1, and compare
  python3 demo/replay_hooks.py STREAM --identity [--adage BIN] [--seed-home LEARNED_HOME] [--keep DIR]

STREAM is JSON lines, `{"event": "PostToolBatch", "payload": {...}}`. In a payload the string
"$CWD" is replaced by --cwd (default: a fresh temporary directory, the same one for both runs of
--identity). Each hook runs with only ADAGE_HOME, ADAGE_ARM, ADAGE_AUTO_LEARN=0 and (with
--observe) ADAGE_MOD_OBSERVE=1 among the ADAGE_* variables.

A recording is JSON lines, one per event: {"i", "event", "exit", "stdout", "spool"}. To make two
runs comparable, the ADAGE_HOME and cwd paths are written as "$ADAGE_HOME" and "$CWD" in stdout and
spool records, and the spool records' clock fields (`ts`, `duration_us`) are dropped; nothing else
is normalised. The binary defaults to $ADAGE_BIN, else target/release/adage.
"""

import argparse
import difflib
import json
import os
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import adage_metrics  # noqa: E402 - demo/adage_metrics.py

VOLATILE = ("ts", "duration_us")


def default_bin():
    return Path(os.environ.get("ADAGE_BIN") or HERE.parent / "target" / "release" / "adage")


def load_jsonl(path):
    out = []
    for n, line in enumerate(Path(path).read_text().splitlines(), 1):
        if line.strip():
            try:
                out.append(json.loads(line))
            except ValueError as e:
                raise SystemExit(f"{path}:{n}: not JSON: {e}")
    return out


def _subst(value, old, new):
    if isinstance(value, str):
        return value.replace(old, new)
    if isinstance(value, list):
        return [_subst(v, old, new) for v in value]
    if isinstance(value, dict):
        return {k: _subst(v, old, new) for k, v in value.items()}
    return value


def normalise(value, home, cwd):
    """Paths as placeholders, clock fields dropped (recursively)."""
    if isinstance(value, dict):
        return {k: normalise(v, home, cwd) for k, v in value.items() if k not in VOLATILE}
    if isinstance(value, list):
        return [normalise(v, home, cwd) for v in value]
    if isinstance(value, str):
        # The longer path first, in case one contains the other.
        for path, mark in sorted(((str(home), "$ADAGE_HOME"), (str(cwd), "$CWD")), key=lambda x: -len(x[0])):
            value = value.replace(path, mark)
        return value
    return value


def hook_env(home, arm="treated", observe=False, extra=None, base=None):
    env = {k: v for k, v in (os.environ if base is None else base).items() if not k.startswith("ADAGE_")}
    env.update(ADAGE_HOME=str(home), ADAGE_ARM=arm, ADAGE_AUTO_LEARN="0")
    if observe:
        env["ADAGE_MOD_OBSERVE"] = "1"
    env.update(extra or {})
    return env


def _spool_counts(home):
    return {p: len(adage_metrics.read_spool(p)) for p in sorted((Path(home) / "sessions").glob("*.spool"))}


def replay(stream, adage, home, cwd, arm="treated", observe=False, extra_env=None, timeout=30):
    """Feed every payload of `stream` ([{"event", "payload"}]) to `adage hook claude <event>`; return
    the recording ([{"i", "event", "exit", "stdout", "spool"}])."""
    home, cwd = Path(home), Path(cwd)
    home.mkdir(parents=True, exist_ok=True)
    env = hook_env(home, arm, observe, extra_env)
    # Only what the replay writes: a seeded home's spools (a learned home's earlier sessions) are
    # not records of this stream, and two copies of a live home may hold different ones.
    out, seen = [], _spool_counts(home)
    for i, item in enumerate(stream):
        event = item["event"]
        payload = _subst(item.get("payload") or {}, "$CWD", str(cwd))
        r = subprocess.run([str(adage), "hook", "claude", event], input=json.dumps(payload),
                           capture_output=True, text=True, env=env, cwd=cwd, timeout=timeout)
        new = []
        for p, n in _spool_counts(home).items():
            recs = adage_metrics.read_spool(p)
            new += recs[seen.get(p, 0):]
            seen[p] = n
        out.append({"i": i, "event": event, "exit": r.returncode,
                    "stdout": normalise(r.stdout, home, cwd), "spool": normalise(new, home, cwd)})
    return out


def write_recording(rec, path):
    Path(path).write_text("".join(json.dumps(r, sort_keys=True) + "\n" for r in rec))


def compare(a, b):
    """The differences between two recordings, as unified-diff lines (empty when identical). The
    stdout of each event is compared as bytes; spool records as canonical JSON."""
    def lines(rec):
        out = []
        for r in rec:
            out.append(f"#{r.get('i')} {r.get('event')} exit={r.get('exit')}")
            out += ["  stdout| " + s for s in (r.get("stdout") or "").split("\n")]
            out += ["  spool | " + json.dumps(s, sort_keys=True) for s in r.get("spool") or []]
        return out
    if a == b:
        return []
    return list(difflib.unified_diff(lines(a), lines(b), "a", "b", lineterm="")) or ["(recordings differ in a field the diff does not show)"]


def main(argv=None):
    p = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    p.add_argument("input", help="a payload stream (replay, --identity) or a recording (--compare)")
    p.add_argument("--compare", metavar="OTHER", help="diff INPUT and OTHER, two recordings")
    p.add_argument("--identity", action="store_true", help="replay without and with ADAGE_MOD_OBSERVE=1 and diff")
    p.add_argument("--out", help="replay: where to write the recording (default stdout)")
    p.add_argument("--adage", default=None, help="the adage binary (default $ADAGE_BIN or target/release/adage)")
    p.add_argument("--home", help="replay: ADAGE_HOME (default a fresh temporary directory)")
    p.add_argument("--cwd", help="the directory \"$CWD\" names (default a fresh temporary directory)")
    p.add_argument("--arm", default="treated", choices=["treated", "control"])
    p.add_argument("--observe", action="store_true", help="replay with ADAGE_MOD_OBSERVE=1")
    p.add_argument("--env", action="append", default=[], metavar="K=V", help="extra environment for the hook")
    p.add_argument("--keep", help="--identity: write both recordings into this directory")
    p.add_argument("--seed-home", help="copy this ADAGE_HOME (e.g. a learned one) into each fresh home first")
    a = p.parse_args(argv)

    if a.compare:
        diff = compare(load_jsonl(a.input), load_jsonl(a.compare))
        print("\n".join(diff) if diff else "identical")
        return 1 if diff else 0

    # Absolute: each hook runs in the replay's own working directory, where a relative path (the
    # documented `--adage target/release/adage`) names nothing.
    adage = (Path(a.adage) if a.adage else default_bin()).resolve()
    if not adage.exists():
        print(f"missing adage binary {adage} (--adage, or ADAGE_BIN)", file=sys.stderr)
        return 2
    stream = load_jsonl(a.input)
    extra = dict(kv.split("=", 1) for kv in a.env)
    with tempfile.TemporaryDirectory(prefix="adage-replay-") as tmp:
        cwd = Path(a.cwd) if a.cwd else Path(tmp) / "cwd"
        cwd.mkdir(parents=True, exist_ok=True)
        def fresh(name):
            h = Path(a.home) if (a.home and not a.identity) else Path(tmp) / name
            if a.seed_home:
                shutil.copytree(a.seed_home, h, dirs_exist_ok=True)
            return h
        if a.identity:
            base = replay(stream, adage, fresh("home-adage"), cwd, a.arm, False, extra)
            obs = replay(stream, adage, fresh("home-observe"), cwd, a.arm, True, extra)
            if a.keep:
                Path(a.keep).mkdir(parents=True, exist_ok=True)
                write_recording(base, Path(a.keep) / "adage.jsonl")
                write_recording(obs, Path(a.keep) / "mod-observe.jsonl")
            diff = compare(base, obs)
            print("\n".join(diff) if diff else f"identical: {len(base)} events, observe mode changed nothing")
            return 1 if diff else 0
        rec = replay(stream, adage, fresh("home"), cwd, a.arm, a.observe, extra)
        if a.out:
            write_recording(rec, a.out)
        else:
            sys.stdout.write("".join(json.dumps(r, sort_keys=True) + "\n" for r in rec))
    return 0


if __name__ == "__main__":
    sys.exit(main())
