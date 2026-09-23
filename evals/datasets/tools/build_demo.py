#!/usr/bin/env python3
"""Curate the committed demo eval dataset from a raw demo archive.

  python3 evals/datasets/tools/build_demo.py <raw-archive-dir> [out-dir]

<raw-archive-dir> is a demo run's `raw/` directory (transcripts/<project>/<session>.jsonl,
runs/<run>/stream.jsonl, state.json, outcomes.json, adage-home/). The archive is read, never
modified. The output (default evals/datasets/demo-haiku45) holds:

  manifest.json          sessions, labels, named train/test splits, learn thresholds
  sessions/<run>.jsonl   one Claude Code transcript per run, the format `adage learn` reads
  sandbox.txt            the sandbox repository's files (for repository-path contracts)
  recorded/              what the live hooks actually decided (for replay fidelity)

Sessions come from the real Claude Code transcripts (`transcripts/`, what `adage learn` read
live). Each is curated: only prompts, tool calls, tool results (with the hook-visible
`toolUseResult`) and Adage's own hook attachments are kept; harness context (environment,
skill listings, dates, file history, costs), thinking blocks and signatures are dropped; long
tool output is trimmed; machine-specific text is redacted. Subagent transcripts are left out
(`adage learn` never reads them).

Archives without transcripts fall back to converting `runs/<run>/stream.jsonl` (the `claude -p
--output-format stream-json` console stream, which has no prompt record and no `cwd` per record:
the prompt comes from state.json, and hook output becomes `attachment` records).

Deterministic: the same archive gives the same bytes.
"""

import json
import re
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
OUT_DEFAULT = HERE.parent / "demo-haiku45"
MAX_TEXT = 3000
MAGIC = bytes([0xAD, 0xA6, 0xE5, 0x01])

# Machine-specific text -> placeholders. Order matters: longest prefixes first.
REDACTIONS = [
    ("/private/tmp/adage-demo", "/sandbox"),
    ("/tmp/adage-demo", "/sandbox"),
    ("/private/var/folders", "/tmp"),
    ("/Users/stevenbooth", "/home/user"),
    ("stevenbooth", "user"),
]
FORBIDDEN = re.compile(r"stevenbooth|/Users/|stevenvaughanbooth|/var/folders", re.I)


def redact(s):
    for a, b in REDACTIONS:
        s = s.replace(a, b)
    return s


def trim(s, n=MAX_TEXT):
    return s if len(s) <= n else s[:n] + f"\n[... {len(s) - n} characters trimmed by build_demo.py]"


def walk(v, f):
    if isinstance(v, str):
        return f(v)
    if isinstance(v, list):
        return [walk(x, f) for x in v]
    if isinstance(v, dict):
        return {k: walk(x, f) for k, x in v.items()}
    return v


def tool_use_result(v):
    """Keep only what a hook's `tool_response` text is built from (harness::claude::response_text)."""
    if isinstance(v, str):
        return trim(v)
    if isinstance(v, dict):
        out = {}
        for k in ("stdout", "stderr", "error", "content", "output"):
            x = v.get(k)
            if isinstance(x, str) and x:
                out[k] = trim(x)
            elif isinstance(x, list):
                out[k] = [{"type": "text", "text": trim(i.get("text", ""))} for i in x if isinstance(i, dict) and "text" in i]
        return out
    return None


def convert(run_id, run, stream_path, iso_start):
    sid = run["metrics"]["session_id"]
    cwd = None
    lines = []
    ts = iso_start
    for raw in stream_path.read_text().splitlines():
        try:
            m = json.loads(json.loads(raw)["line"])
        except (ValueError, KeyError, TypeError):
            continue
        typ, sub = m.get("type"), m.get("subtype")
        if typ == "system" and sub == "init":
            cwd = m.get("cwd")
        if m.get("timestamp"):
            ts = m["timestamp"]
        base = {"sessionId": sid, "cwd": cwd, "timestamp": ts}
        if typ == "system" and sub == "hook_response" and m.get("output"):
            try:
                out = json.loads(m["output"])
            except ValueError:
                continue
            text = ((out.get("hookSpecificOutput") or {}).get("additionalContext")) or (
                out.get("reason") if out.get("decision") == "block" else None)
            if text:
                lines.append({"type": "attachment", **base, "hookEvent": m.get("hook_event"), "content": text})
        elif typ == "assistant":
            msg = m.get("message", {})
            content = []
            for c in msg.get("content", []):
                if c.get("type") == "tool_use":
                    content.append({"type": "tool_use", "id": c.get("id"), "name": c.get("name"), "input": c.get("input")})
                elif c.get("type") == "text":
                    content.append({"type": "text", "text": trim(c.get("text", ""), 1500)})
            if content:
                lines.append({"type": "assistant", **base,
                              "message": {"id": msg.get("id"), "model": msg.get("model"), "role": "assistant", "content": content}})
        elif typ == "user":
            content = []
            for c in (m.get("message", {}).get("content") or []):
                if isinstance(c, dict) and c.get("type") == "tool_result":
                    body = c.get("content")
                    if isinstance(body, list):
                        body = "\n".join(i.get("text", "") for i in body if isinstance(i, dict))
                    content.append({"type": "tool_result", "tool_use_id": c.get("tool_use_id"),
                                    "content": trim(body or ""), "is_error": bool(c.get("is_error"))})
            if content:
                rec = {"type": "user", **base, "message": {"role": "user", "content": content}}
                tur = tool_use_result(m.get("tool_use_result"))
                if tur is not None:
                    rec["toolUseResult"] = tur
                lines.append(rec)
    if cwd is None:
        raise SystemExit(f"{run_id}: no init record")
    prompt = {"type": "user", "sessionId": sid, "cwd": cwd, "timestamp": iso_start,
              "message": {"role": "user", "content": run["prompt"]}}
    # The prompt precedes everything the agent did; hooks that fired before init keep their order.
    first_turn = next((i for i, x in enumerate(lines) if x["type"] != "attachment"), len(lines))
    lines.insert(first_turn, prompt)
    for x in lines:
        x["cwd"] = cwd
    return [walk(x, redact) for x in lines]


HOOK_ATTACHMENTS = {"hook_additional_context", "hook_blocking_error", "hook_stopped_continuation"}


def message(m, role):
    content = m.get("content")
    if isinstance(content, str):
        return {"role": role, "content": content}
    out = []
    for c in content or []:
        if not isinstance(c, dict):
            continue
        t = c.get("type")
        if t == "tool_use":
            out.append({"type": "tool_use", "id": c.get("id"), "name": c.get("name"), "input": c.get("input")})
        elif t == "tool_result":
            body = c.get("content")
            if isinstance(body, list):
                body = "\n".join(i.get("text", "") for i in body if isinstance(i, dict))
            out.append({"type": "tool_result", "tool_use_id": c.get("tool_use_id"), "content": trim(body or ""),
                        "is_error": bool(c.get("is_error"))})
        elif t == "text":
            out.append({"type": "text", "text": trim(c.get("text", ""), 1500)})
    msg = {"role": role, "content": out}
    for k in ("id", "model"):
        if m.get(k):
            msg[k] = m[k]
    return msg


def curate(path):
    """A real transcript, curated: what learning and replay read, and nothing else."""
    lines = []
    for raw in path.read_text().splitlines():
        try:
            r = json.loads(raw)
        except ValueError:
            continue
        typ = r.get("type")
        base = {k: r[k] for k in ("sessionId", "cwd", "timestamp", "isSidechain", "isMeta") if k in r}
        if typ in ("user", "assistant"):
            rec = {"type": typ, **base, "message": message(r.get("message") or {}, typ)}
            if typ == "assistant" and not rec["message"]["content"]:
                continue
            tur = tool_use_result(r.get("toolUseResult"))
            if tur is not None:
                rec["toolUseResult"] = tur
            lines.append(rec)
        elif typ == "attachment":
            a = r.get("attachment") or {}
            if a.get("type") in HOOK_ATTACHMENTS:
                lines.append({"type": "attachment", **base,
                              "attachment": {k: a[k] for k in ("type", "hookEvent", "content") if k in a}})
    return [walk(x, redact) for x in lines]


def spool_records(p):
    data = p.read_bytes()
    i, out = 0, []
    while True:
        j = data.find(MAGIC, i)
        if j < 0 or j + 12 > len(data):
            break
        n = int.from_bytes(data[j + 4:j + 8], "little")
        try:
            out.append(json.loads(data[j + 12:j + 12 + n]))
        except ValueError:
            pass
        i = j + 12 + n
    return out


def main():
    raw = Path(sys.argv[1]).resolve()
    out = Path(sys.argv[2]).resolve() if len(sys.argv) > 2 else OUT_DEFAULT
    state = json.loads((raw / "state.json").read_text())
    runs = {k: v for k, v in state["runs"].items() if v.get("metrics") and (raw / "runs" / k / "stream.jsonl").exists()}
    (out / "sessions").mkdir(parents=True, exist_ok=True)
    (out / "recorded").mkdir(parents=True, exist_ok=True)
    import datetime
    sessions = []
    sources = {}
    for run_id in sorted(runs):
        r = runs[run_id]
        iso = datetime.datetime.fromtimestamp(r["t0"], datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%S.000Z")
        tx = raw / "transcripts" / f"-private-tmp-adage-demo-runs-{run_id}-acme-billing" / f"{r['metrics']['session_id']}.jsonl"
        if tx.exists():
            recs, source = curate(tx), "transcript"
        else:
            recs, source = convert(run_id, r, raw / "runs" / run_id / "stream.jsonl", iso), "stream-json"
        sources[source] = sources.get(source, 0) + 1
        text = "".join(json.dumps(x, ensure_ascii=False, sort_keys=True) + "\n" for x in recs)
        (out / "sessions" / f"{run_id}.jsonl").write_text(text)
        m = r["metrics"]
        sessions.append({
            "file": f"sessions/{run_id}.jsonl",
            "id": m["session_id"],
            "run": run_id,
            "phase": r["phase"],
            "arm": r["arm"],
            "round": r.get("round", 1),
            "task": r["task"],
            "passed": bool(m.get("passed")),
            "stale_generated": bool(m.get("stale_generated")),
            "turns": m.get("turns"),
            "turns_to_first_edit": m.get("turns_to_first_edit"),
            "dead_end_hits": m.get("dead_end_hits"),
            "ran_regen": bool(m.get("ran_regen")),
            "cost_usd": m.get("cost_usd"),
            # `error_max_turns`: the harness stopped the agent, and no Stop hook fired.
            "result": m.get("result_subtype"),
        })

    def ids(pred):
        return sorted(s["id"] for s in sessions if pred(s))

    train = lambda s: s["phase"] == "train"
    eval_round = lambda s, n: s["phase"] == "eval" and s["round"] == n
    splits = {
        # What the live round-1 index learned from, and the round-1 held-out control arm.
        "round1": {"train": ids(train), "test": ids(lambda s: eval_round(s, 1) and s["arm"] == "holdout")},
        # Round 2 learned from every earlier session (both round-1 arms), then ran new held-out work.
        "round2": {"train": ids(lambda s: train(s) or eval_round(s, 1)),
                   "test": ids(lambda s: eval_round(s, 2) and s["arm"] == "holdout")},
    }
    manifest = {
        "name": "demo-haiku45",
        "description": "Real Claude Code sessions (Claude Haiku 4.5) from the Adage demo on the acme-billing sandbox",
        "repo": "acme-billing",
        "learn": "demo",
        "sandbox": "sandbox.txt",
        "model": state.get("model"),
        "source": sources,
        "sessions": sessions,
        "splits": splits,
    }
    (out / "manifest.json").write_text(json.dumps(manifest, indent=1, sort_keys=True) + "\n")

    # What the live hooks decided, per eval session, and the index each round used.
    by_sid = {s["id"]: s["run"] for s in sessions}
    decided, gens = {}, {}
    for p in sorted((raw / "adage-home" / "sessions").glob("*.spool")):
        run_id = by_sid.get(p.stem)
        if not run_id:
            continue
        evs = []
        for rec in spool_records(p):
            d = rec.get("decision", {})
            gens.setdefault(runs[run_id].get("round", 1), rec.get("generation"))
            evs.append({
                "event": rec.get("event"),
                "arm": rec.get("arm"),
                "generation": rec.get("generation"),
                "would_have": [x.get("route") for x in d.get("would_have", [])],
                "would_continue": bool(d.get("would_continue")),
                "judgements": [[j.get("kind"), j.get("route")] for j in d.get("judgements", [])],
            })
        decided[run_id] = evs
    (out / "recorded" / "decisions.json").write_text(json.dumps(decided, indent=0, sort_keys=True) + "\n")
    idx2 = json.loads((raw / "adage-home" / "index.json").read_text())
    (out / "recorded" / "index-round2.json").write_text(json.dumps(idx2, indent=1, sort_keys=True) + "\n")
    idx1 = {"generation": gens.get(1), "routes": state["rounds"]["1"]["tracks"]}
    (out / "recorded" / "index-round1.json").write_text(json.dumps(idx1, indent=1, sort_keys=True) + "\n")

    leaks = []
    for f in sorted(out.rglob("*")):
        if f.is_file() and f.suffix in (".json", ".jsonl", ".txt") and FORBIDDEN.search(f.read_text()):
            leaks.append(str(f))
    if leaks:
        raise SystemExit("machine-specific text survived redaction in: " + ", ".join(leaks))
    print(f"wrote {len(sessions)} sessions to {out} (from {sources})")


if __name__ == "__main__":
    main()
