#!/usr/bin/env python3
"""Read-only trace sufficiency audit. Never executes trace commands or learns a policy.

Raw values live only in memory/source files; reports contain counts and opaque hashes.
Source references preserve unknown fields without creating a second raw-data export.
"""
from __future__ import annotations

import argparse
from collections import Counter, defaultdict
from dataclasses import dataclass, field
import hashlib
import json
from pathlib import Path
import re
import shlex
from typing import Any


VERSION = "trace-audit-v1"
SEED = "reflex-proof-task-split-v1"


def digest(value: Any) -> str:
    return hashlib.sha256(json.dumps(value, sort_keys=True, ensure_ascii=False).encode()).hexdigest()


def text(value: Any) -> str:
    if isinstance(value, str):
        return value
    if isinstance(value, list):
        return "\n".join(text(x.get("text", "")) for x in value if isinstance(x, dict))
    return ""


@dataclass
class Prompt:
    order: int
    content: Any
    role: str = "user"
    source_ref: str = ""


@dataclass
class Call:
    order: int
    tool: str
    args: Any
    original_args: Any
    source_ref: str
    call_id: str | None = None
    event_id: str | None = None
    parent_event_id: str | None = None
    batch: str | None = None
    cwd: str | None = None
    cwd_basis: str | None = None
    env: Any = None
    model: str | None = None
    request_id: str | None = None
    start: Any = None
    end: Any = None
    duration: Any = None
    output: Any = None
    result_ref: str | None = None
    result_order: int | None = None
    result_id: str | None = None
    exit_code: int | None = None
    failed: bool | None = None
    result_metadata: Any = None
    provenance: str = "unknown"
    link_ambiguous: bool = False


@dataclass
class Trace:
    dataset: str
    kind: str
    schema: str
    source_id: str
    path: Path
    checksum: str
    task_group: str
    session_id: str | None = None
    calls: list[Call] = field(default_factory=list)
    prompts: list[Prompt] = field(default_factory=list)
    outcome: bool | None = None
    provenance: str = "unknown"
    notices: Counter = field(default_factory=Counter)

    def original(self, pointer: str) -> Any:
        """Resolve a source JSON location; unknown fields remain accessible and unchanged."""
        raw = read_document(self.path)
        for key in pointer.strip("/").split("/") if pointer else []:
            key = key.replace("~1", "/").replace("~0", "~")
            raw = raw[int(key)] if isinstance(raw, list) else raw[key]
        return raw


def read_document(path: Path) -> Any:
    if path.suffix == ".jsonl":
        return [json.loads(line) for line in path.read_text().splitlines() if line.strip()]
    return json.loads(path.read_text())


def parsed_args(value: Any) -> Any:
    if isinstance(value, str):
        try:
            return json.loads(value)
        except (ValueError, TypeError):
            return value
    return value


def attach_result(trace: Trace, ident: Any, order: int, output: Any, ref: str,
                  *, event_id=None, metadata=None, end=None, failed=None, exit_code=None):
    matches = [c for c in trace.calls if c.call_id == ident and c.result_ref is None]
    if ident is None or len(matches) != 1:
        trace.notices["unmatched_or_ambiguous_result"] += 1
        for call in matches:
            call.link_ambiguous = True
        return
    c = matches[0]
    c.output, c.result_ref, c.result_order = output, ref, order
    c.result_id, c.result_metadata, c.end = event_id, metadata, end
    c.failed, c.exit_code = failed, exit_code


def parse_claude(t: Trace, rows: list):
    for i, r in enumerate(rows):
        m = r.get("message", {})
        content = m.get("content")
        if t.session_id is None:
            t.session_id = r.get("sessionId")
        if r.get("type") == "user" and isinstance(content, str):
            t.prompts.append(Prompt(i, content, source_ref=f"/{i}/message/content"))
        if not isinstance(content, list):
            continue
        for j, block in enumerate(content):
            ref = f"/{i}/message/content/{j}"
            if block.get("type") == "tool_use":
                args = block.get("input")
                t.calls.append(Call(i, block.get("name", ""), args, args, ref,
                    call_id=block.get("id"), event_id=r.get("uuid"), batch=m.get("id"),
                    parent_event_id=r.get("parentUuid"),
                    cwd=r.get("cwd"), cwd_basis="record" if r.get("cwd") else None,
                    env=args.get("env") if isinstance(args, dict) else None,
                    model=m.get("model"), request_id=r.get("requestId"),
                    start=r.get("timestamp"), provenance=t.provenance))
            elif block.get("type") == "tool_result":
                meta = r.get("toolUseResult")
                code = meta.get("exitCode", meta.get("exit_code")) if isinstance(meta, dict) else None
                attach_result(t, block.get("tool_use_id"), i, block.get("content"), ref,
                    event_id=r.get("uuid"), metadata=meta, end=r.get("timestamp"),
                    failed=block.get("is_error"), exit_code=code)
            elif m.get("role") == "user" and block.get("type") == "text":
                t.prompts.append(Prompt(i, block.get("text"), source_ref=ref))


def parse_codex(t: Trace, rows: list):
    cwd = model = None
    for i, r in enumerate(rows):
        p = r.get("payload", {})
        if r.get("type") == "session_meta":
            t.session_id, cwd = p.get("id"), p.get("cwd")
        if r.get("type") == "turn_context":
            cwd, model = p.get("cwd", cwd), p.get("model", model)
        if r.get("type") != "response_item":
            continue
        kind, ref = p.get("type"), f"/{i}/payload"
        if kind == "message" and p.get("role") == "user":
            t.prompts.append(Prompt(i, p.get("content"), source_ref=ref + "/content"))
        if kind in ("function_call", "custom_tool_call"):
            original = p.get("arguments", p.get("input"))
            args = parsed_args(original)
            explicit = (args.get("workdir") or args.get("cwd")) if isinstance(args, dict) else None
            t.calls.append(Call(i, p.get("name", ""), args, original, ref,
                call_id=p.get("call_id"), event_id=p.get("id"),
                cwd=explicit or cwd, cwd_basis="argument" if explicit else "session_context" if cwd else None,
                env=args.get("env") if isinstance(args, dict) else None,
                model=model, start=r.get("timestamp"), provenance=t.provenance))
        if kind in ("function_call_output", "custom_tool_call_output"):
            out = p.get("output")
            obj = parsed_args(out)
            meta = obj.get("metadata") if isinstance(obj, dict) else None
            code = meta.get("exit_code") if isinstance(meta, dict) else None
            # Header extraction is explicitly observed output metadata, never a generic error regex.
            if code is None and isinstance(out, str):
                match = re.search(r"(?m)^(?:Process exited with code |Exit code: )(-?\d+)\s*$", out)
                code = int(match[1]) if match else None
            attach_result(t, p.get("call_id"), i, out, ref, event_id=p.get("id"),
                metadata=meta, end=r.get("timestamp"), exit_code=code,
                failed=code != 0 if code is not None else None)


def parse_opencode(t: Trace, data: dict):
    t.session_id = data.get("info", {}).get("id")
    for i, message in enumerate(data.get("messages", [])):
        info = message.get("info", {})
        for j, part in enumerate(message.get("parts", [])):
            ref = f"/messages/{i}/parts/{j}"
            if info.get("role") == "user" and part.get("type") == "text":
                t.prompts.append(Prompt(i, part.get("text"), source_ref=ref))
            if part.get("type") != "tool":
                continue
            state = part.get("state", {})
            args, meta = state.get("input"), state.get("metadata", {})
            timing = state.get("time", {})
            cwd = info.get("path", {}).get("cwd") or data.get("info", {}).get("directory")
            done = state.get("status") in ("completed", "error")
            t.calls.append(Call(i, part.get("tool", ""), args, args, ref,
                call_id=part.get("callID"), event_id=part.get("id"), batch=info.get("id"),
                parent_event_id=info.get("parentID"),
                cwd=cwd, cwd_basis="message_context" if cwd else None,
                env=args.get("env") if isinstance(args, dict) else None,
                model=info.get("modelID"), start=timing.get("start"), end=timing.get("end"),
                output=state.get("output", state.get("error")), result_ref=ref + "/state" if done else None,
                result_order=i if done else None, result_metadata=meta, exit_code=meta.get("exit"),
                failed=state.get("status") == "error" if done else None, provenance=t.provenance))


def parse_chat_history(t: Trace, data: Any):
    """Public OpenHands/SWE-agent original tool history, not lossy S9 symbols."""
    history = data.get("history", []) if isinstance(data, dict) else data
    prefix = "/history" if isinstance(data, dict) else ""
    model = None
    if isinstance(data, dict):
        config = parsed_args(data.get("replay_config", {}))
        if isinstance(config, dict):
            model = config.get("agent", {}).get("model", {}).get("name")
    for i, message in enumerate(history):
        ref = f"{prefix}/{i}"
        if message.get("role") == "user":
            t.prompts.append(Prompt(i, message.get("content"), source_ref=ref + "/content"))
        for j, tool in enumerate(message.get("tool_calls", []) or []):
            f = tool.get("function", {})
            original = f.get("arguments")
            args = parsed_args(original)
            t.calls.append(Call(i, f.get("name", ""), args, original, ref + f"/tool_calls/{j}",
                call_id=tool.get("id"), event_id=message.get("id"), batch=f"history-message-{i}",
                cwd=args.get("cwd") if isinstance(args, dict) else None,
                cwd_basis="argument" if isinstance(args, dict) and args.get("cwd") else None,
                env=args.get("env") if isinstance(args, dict) else None,
                model=message.get("model", model), request_id=message.get("request_id"),
                start=message.get("timestamp"), provenance=t.provenance))
        if message.get("role") == "tool":
            ids = message.get("tool_call_ids", [message.get("tool_call_id")])
            if len(ids) != 1:
                t.notices["multi_call_result_not_assigned"] += 1
                continue
            attach_result(t, ids[0], i, message.get("content"), ref,
                event_id=message.get("id"), end=message.get("timestamp"))
    if not t.calls:
        t.notices["no_structured_tool_calls"] += 1


def load_trace(path: Path, *, dataset: str, kind: str, schema: str, source_id: str,
               task_group: str | None = None, outcome=None, provenance="unknown") -> Trace:
    content = path.read_bytes()
    data = read_document(path)
    t = Trace(dataset, kind, schema, source_id, path, hashlib.sha256(content).hexdigest(),
              task_group or "", outcome=outcome, provenance=provenance)
    {"claude": parse_claude, "codex": parse_codex, "opencode": parse_opencode,
     "public-chat": parse_chat_history}[schema](t, data)
    if not t.task_group:
        # Committed simulator task IDs are explicit in user prompts. No command/output lookup.
        ids = sorted(set(re.findall(r"\bW-\d+\b", "\n".join(text(p.content) for p in t.prompts))))
        t.task_group = "acme-billing:" + ids[0] if len(ids) == 1 else "unknown:" + t.checksum
    return t


def split(task_group: str) -> str:
    if task_group.startswith("unknown:"):
        return "quarantine"
    n = int(digest([SEED, task_group])[:8], 16) % 100
    return "train" if n < 60 else "calibration" if n < 80 else "test"


def precedes(a: Call, b: Call) -> bool:
    """A tool response was available before invocation, not proof its process terminated.

    Calls in one assistant batch are never assumed dependent, even with record ordering.
    No process chronology is fabricated: a returned asynchronous job can still be running.
    """
    return (a.result_order is not None and a.result_order < b.order
            and not (a.batch is not None and a.batch == b.batch))


def simple_argv(call: Call) -> list[str] | None:
    args = call.args
    if not isinstance(args, dict):
        return None
    if call.tool not in ("Bash", "bash", "shell", "exec_command", "execute_bash"):
        return None
    command = args.get("command", args.get("cmd"))
    if isinstance(command, list):
        # This is an original argv, not a shell command to be split or unwrapped.
        return command if command and all(isinstance(x, str) for x in command) else None
    if not isinstance(command, str) or re.search(r"[\n\r;&|<>`$(){}*?~]", command):
        return None
    try:
        argv = shlex.split(command)
    except ValueError:
        return None
    if not argv or "=" in argv[0]:
        return None
    return argv


def slot_type(value: str) -> str | None:
    if re.fullmatch(r"[A-Z][A-Z0-9_]*-\d+", value):
        return "id"
    if "/" in value or re.fullmatch(r"[\w.-]+\.[A-Za-z0-9]+", value):
        return "path"
    if re.fullmatch(r"\d+", value):
        return "number"
    return None


def shape(argv: list[str]) -> tuple[str, ...]:
    interpreter = Path(argv[0]).name in ("python", "python3", "node", "bash", "sh", "ruby", "perl")
    result = []
    for i, value in enumerate(argv):
        identity = i == 0 or (interpreter and i == 1)
        kind = None if identity or value.startswith("-") else slot_type(value)
        result.append("{" + kind + "}" if kind else value)
    return tuple(result)


def scalar_leaves(value: Any):
    if isinstance(value, dict):
        for v in value.values():
            yield from scalar_leaves(v)
    elif isinstance(value, list):
        for v in value:
            yield from scalar_leaves(v)
    elif isinstance(value, (str, int, float)) and not isinstance(value, bool):
        yield str(value)


def values(content: Any) -> set[str]:
    """Exact scalars plus bounded literal tokens; token presence is NOT causal dataflow."""
    out = set(scalar_leaves(content))
    raw_text = text(content)
    parsed = parsed_args(raw_text)
    if not isinstance(parsed, str):
        out.update(scalar_leaves(parsed))
    out.update(re.findall(r"[A-Za-z0-9_./:-]+", raw_text))
    return out


def binding_audit(trace: Trace, call: Call, argv: list[str]) -> Counter:
    sources = []
    # Only earlier prompts and completed results: never current/future arguments/results.
    for prompt in trace.prompts:
        if prompt.order < call.order:
            sources.append(("prompt", values(prompt.content)))
    for prior in trace.calls:
        if precedes(prior, call):
            sources.append(("result", values(prior.output)))
    counts = Counter()
    for value, templ in zip(argv, shape(argv)):
        if not templ.startswith("{"):
            continue
        matches = [kind for kind, available in sources if value in available]
        counts["slots"] += 1
        if len(matches) == 1:
            counts["unique_prior_" + matches[0]] += 1
        elif len(matches) > 1:
            counts["ambiguous_multiple_prior_sources"] += 1
        else:
            counts["unavailable_before_call"] += 1
    return counts


def source_fields(c: Call) -> dict[str, bool]:
    return {
        "source_call_id": c.call_id is not None, "source_event_id": c.event_id is not None,
        "source_parent_event_id": c.parent_event_id is not None,
        "original_arguments": c.original_args is not None, "linked_result": c.result_ref is not None,
        "original_output": c.result_ref is not None and c.output is not None,
        "numeric_exit_code": isinstance(c.exit_code, int), "failure_bit": c.failed is not None,
        "cwd": c.cwd is not None, "explicit_env_field": c.env is not None,
        "model_id": c.model is not None, "model_request_id": c.request_id is not None,
        "start_timestamp": c.start is not None, "completion_timestamp": c.end is not None,
        "known_intervention_arm": c.provenance != "unknown",
    }


def audit(traces: list[Trace]) -> dict:
    report = {}
    for dataset in sorted({t.dataset for t in traces}):
        ts = [t for t in traces if t.dataset == dataset]
        field_counts, bindings, notices = Counter(), Counter(), Counter()
        train_shapes = defaultdict(set)
        for t in ts:
            if split(t.task_group) == "train" and t.provenance != "steered":
                for c in t.calls:
                    argv = simple_argv(c)
                    if argv:
                        train_shapes[shape(argv)].add(t.task_group)
        repeated = {s for s, groups in train_shapes.items() if len(groups) >= 2}
        windows = Counter()
        candidate_calls = Counter()
        for t in ts:
            notices.update(t.notices)
            for c in t.calls:
                field_counts.update(k for k, present in source_fields(c).items() if present)
                if "characters trimmed by build_demo.py]" in text(c.output):
                    notices["output_explicitly_trimmed_by_curator"] += 1
                argv = simple_argv(c)
                if c.tool in ("Bash", "bash", "shell", "exec_command", "execute_bash"):
                    candidate_calls["shell_tool_calls"] += 1
                    if argv is None:
                        candidate_calls["shell_calls_outside_literal_argv_subset"] += 1
                else:
                    candidate_calls["other_structured_tool_calls"] += 1
                if argv:
                    candidate_calls["simple_argv_calls"] += 1
                    if split(t.task_group) in ("calibration", "test"):
                        candidate_calls["heldout_simple_argv_calls"] += 1
                        candidate_calls[split(t.task_group) + "_simple_argv_calls"] += 1
                        if shape(argv) in repeated:
                            candidate_calls["heldout_train_repeated_shape_calls"] += 1
                            candidate_calls[split(t.task_group) + "_train_repeated_shape_calls"] += 1
                            bindings.update(binding_audit(t, c, argv))
                prior = [a for a in t.calls if precedes(a, c)]
                for n in (1, 2, 4):
                    if len(prior) >= n:
                        windows[f"boundaries_with_{n}_available_prior_tool_responses"] += 1
            windows["all_call_boundaries"] += len(t.calls)
            windows["same_batch_unordered_pairs"] += sum(
                1 for i, a in enumerate(t.calls) for b in t.calls[i + 1:]
                if a.batch is not None and a.batch == b.batch)
        total = sum(len(t.calls) for t in ts)
        report[dataset] = {
            "kind": ts[0].kind, "schemas": sorted({t.schema for t in ts}),
            "traces": len(ts), "task_groups": len({t.task_group for t in ts}), "calls": total,
            "prompts_retained": sum(len(t.prompts) for t in ts),
            "traces_with_task_outcome": sum(t.outcome is not None for t in ts),
            "trace_splits": dict(Counter(split(t.task_group) for t in ts)),
            "task_group_splits": dict(Counter(split(g) for g in {t.task_group for t in ts})),
            "fields_present": {k: {"present": field_counts[k], "missing": total - field_counts[k]}
                               for k in source_fields(Call(0, "", None, None, ""))},
            "descriptive_windows": dict(windows), "argv_recurrence_audit": dict(candidate_calls),
            "train_shapes_seen_in_two_distinct_tasks": len(repeated),
            "recurrence_support_provenance_verified": all(t.provenance != "unknown" for t in ts if split(t.task_group) == "train"),
            "heldout_repeated_shape_slot_audit": dict(bindings), "notices": dict(notices),
            "interpretation": "Descriptive exact-copy opportunity screen, not executable coverage. Shape matches are not full-argument predictions; unique prior literals are not verified causal bindings. Unknown intervention origin is included only in this descriptive audit and is flagged. No policy is trained or executed.",
        }
    return report


def discover(repo: Path, public_root: Path | None, public_tasks: int) -> list[Trace]:
    traces = []
    for schema in ("claude", "codex", "opencode"):
        root = repo / "sim/fixtures" / schema
        paths = sorted(root.rglob("*.jsonl")) + sorted(root.rglob("*.json"))
        for path in paths:
            traces.append(load_trace(path, dataset="sim-" + schema, kind="synthetic-harness-fixture",
                schema=schema, source_id=str(path.relative_to(repo)), provenance="synthetic"))
    manifests = [repo / "evals/datasets/demo-haiku45/manifest.json"]
    manifests += sorted((repo / "evals/datasets").glob("live-*/manifest.json"))
    for mp in manifests:
        root = mp.parent
        manifest = json.loads(mp.read_text())
        for row in manifest["sessions"]:
            path = root / row["file"]
            traces.append(load_trace(path, dataset=manifest["name"], kind="live-agent-synthetic-repository",
                schema="claude", source_id=str(path.relative_to(repo)),
                task_group=manifest["repo"] + ":" + row["task"],
                outcome=row.get("passed"), provenance="steered" if row["arm"] in ("adage", "instincts", "e1") else "unsteered"))
    if public_root:
        # Same tasks across every submission: choose by fixed hash, independent of outcomes/length.
        groups = sorted(p for p in public_root.iterdir() if p.is_dir())
        names = sorted({p.stem for d in groups for p in d.iterdir()
                        if "__" in p.stem and p.suffix in (".traj", ".json")}, key=lambda s: digest([SEED, s]))
        chosen = set(names[:public_tasks])
        for directory in groups:
            results_path = directory / "_results.json"
            results = json.loads(results_path.read_text()) if results_path.is_file() else {}
            for path in sorted(directory.iterdir()):
                if path.stem not in chosen or path.suffix not in (".traj", ".json"):
                    continue
                traces.append(load_trace(path, dataset="public-" + directory.name, kind="public-real-repository-benchmark",
                    schema="public-chat", source_id=directory.name + "/" + path.name,
                    task_group="swebench:" + path.stem, provenance="unknown",
                    outcome=path.stem in results["resolved"] if "resolved" in results else None))
    return traces


def model_metadata(model_dir: Path | None) -> dict:
    result = {"implementation": "crates/adage-intent/src/model.rs", "family": "potion-base-8M",
        "expected_id": "potion-base-8m@f65d0f325faa", "dimensions": 256,
        "embeddings_computed": False, "similarity_measured": False,
        "status": "not supplied; no download and no lexical substitute"}
    if model_dir:
        manifest = json.loads((model_dir / "manifest.json").read_text())
        result["manifest"] = {k: manifest.get(k) for k in ("id", "dim", "vocab_size")}
        result["files_verified"] = {}
        for name, entry in manifest.get("files", {}).items():
            if Path(name).name != name:
                raise ValueError("model manifest file must be a basename")
            path = model_dir / name
            result["files_verified"][name] = path.is_file() and hashlib.sha256(path.read_bytes()).hexdigest() == entry["sha256"]
        result["status"] = "metadata inspected only; embedding experiment remains separate"
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repo", type=Path, default=Path(__file__).resolve().parents[3])
    parser.add_argument("--public-root", type=Path)
    parser.add_argument("--public-tasks", type=int, default=12)
    parser.add_argument("--model-dir", type=Path)
    parser.add_argument("--revision", default="unspecified", help="Git source revision, supplied explicitly for provenance")
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    if args.public_tasks < 1:
        parser.error("--public-tasks must be positive")
    traces = discover(args.repo, args.public_root, args.public_tasks)
    manifest = [{"dataset": t.dataset, "kind": t.kind, "schema": t.schema,
        "source_id": t.source_id, "source_sha256": t.checksum,
        "session_id_hash": digest(t.session_id) if t.session_id else None,
        "task_group_hash": digest(t.task_group), "split": split(t.task_group)} for t in traces]
    result = {"audit_version": VERSION, "seed": SEED, "source_revision": args.revision,
        "audit_source_sha256": hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
        "scope": "CPU trace sufficiency and descriptive opportunity audit; no command execution, predictor, embedding computation or causal utility claim",
        "split": "60/20/20 stable task-group hash; repeats and cross-harness/model copies stay together. Unresolved/multi-task groups are quarantined. Hash partition is not chronological validation.",
        "datasets": audit(traces), "embedding_metadata": model_metadata(args.model_dir),
        "gates": {"source_bytes_addressable": all(t.checksum for t in traces),
                  "unknown_task_group_count": sum(t.task_group.startswith("unknown:") for t in traces),
                  "task_group_split_leakage": len({(m["task_group_hash"], m["split"]) for m in manifest}) != len({m["task_group_hash"] for m in manifest}),
                  "execution_readiness": "NOT ESTABLISHED: missing environment/state, uncertain bindings and no execution/quality experiment"},
        "manifest": manifest}
    if args.public_root:
        result["public_source_inventory"] = {
            directory.name: dict(Counter("structured_history" if p.suffix in (".json", ".traj") else "unsupported_source_format"
                for p in directory.iterdir() if "__" in p.name))
            for directory in sorted(args.public_root.iterdir()) if directory.is_dir()}
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(result, indent=2, sort_keys=True) + "\n")
    print(json.dumps({"report": str(args.output), "traces": len(traces), "calls": sum(len(t.calls) for t in traces), "gates": result["gates"]}))


if __name__ == "__main__":
    main()
