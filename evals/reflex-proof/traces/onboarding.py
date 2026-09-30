#!/usr/bin/env python3
"""Offline production onboarding proof using committed live-sandbox TRAIN traces only.

No paid/model calls, hidden outcome labels, synthetic rules, or person-only guard bypass.
The installed registration is a fixture from `install --print`, not an installer test.
All mutable state is temporary. Only aggregate evidence is exported.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tempfile


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    repo = Path(__file__).resolve().parents[3]
    binary = repo / "target/debug/adage"
    dataset = repo / "evals/datasets/live-20260928-py-clinic-s12"
    manifest = json.loads((dataset / "manifest.json").read_text())
    train = [s for s in manifest["sessions"] if s["phase"] == "train"]
    assert len(train) == 9
    # HEAD alone cannot identify an uncommitted implementation. Bind the proof to every Rust
    # source/manifest and lockfile, including new files; paths are repository-relative.
    source_paths = sorted(p for p in (repo / "crates").rglob("*")
                          if p.is_file() and (p.suffix == ".rs" or p.name == "Cargo.toml"))
    source_paths.extend([repo / "Cargo.toml", repo / "Cargo.lock"])
    source_hashes = {str(p.relative_to(repo)): hashlib.sha256(p.read_bytes()).hexdigest()
                     for p in sorted(source_paths)}
    with tempfile.TemporaryDirectory(prefix="adage-onboarding-") as tmp:
        root = Path(tmp).resolve()
        spec = next(s["spec"] for s in json.loads((repo / "evals/gen/specs/smoke.json").read_text())["sandboxes"] if s["seed"] == 12)
        checkout = root / "wardline-clinic"
        subprocess.run(["python3", "-B", str(repo / "evals/gen/gen.py"), "--spec-json", json.dumps(spec),
                        "--seed", "12", "--repo-only", str(checkout)], check=True, capture_output=True)
        (checkout / ".git").mkdir(exist_ok=True)
        env = dict(os.environ, HOME=str(root / "home"), USERPROFILE=str(root / "home"),
                   ADAGE_HOME=str(root / "data"), CLAUDE_CONFIG_DIR=str(root / "claude-custom"),
                   CODEX_HOME=str(root / "codex"), XDG_CONFIG_HOME=str(root / "xdg"),
                   ADAGE_ARM="treated", ADAGE_TEST_NO_SPAWN="record", ADAGE_AUTO_LEARN="1")
        # Agent-session guards are deliberately retained. Every invoked command is agent-safe.
        project = root / "claude-custom/projects" / "".join(c if c.isascii() and c.isalnum() else "-" for c in str(checkout))
        project.mkdir(parents=True)
        hashes = []
        for s in train:
            original = (dataset / s["file"]).read_text()
            old = json.loads(original.splitlines()[0])["cwd"]
            # Rebase the recorded checkout consistently, preserving calls/results/prompts and time.
            (project / Path(s["file"]).name).write_text(original.replace(old, str(checkout)))
            hashes.append({"file": s["file"], "sha256": hashlib.sha256(original.encode()).hexdigest()})
        # A second repository proves the wizard learner does not train on all discovered history.
        foreign = root / "claude-custom/projects/-unrelated-repository"
        foreign.mkdir()
        (foreign / "unrelated.jsonl").write_text(json.dumps({"type": "user", "cwd": "/unrelated/repository",
            "sessionId": "unrelated", "message": {"role": "user", "content": "unrelated"}}) + "\n")

        def run(argv, payload=None):
            result = subprocess.run([str(binary), *argv], cwd=checkout, env=env,
                input="" if payload is None else json.dumps(payload), text=True, capture_output=True)
            if result.returncode:
                raise RuntimeError(f"{argv}: {result.stderr}")
            return result.stdout

        preview = run(["init", "--print", "--repo", str(checkout)])
        registration = json.loads(run(["install", "claude-code", "--print"]))
        (root / "claude-custom/settings.json").write_text(json.dumps(registration))
        run(["condition", "--auto", "--repo-root", str(checkout)])  # Exact wizard learner argv.
        index = json.loads((root / "data/index.json").read_text())
        logs = [json.loads(line) for line in (root / "data/learn.log").read_text().splitlines()]
        entries = index["routes"]
        learned = [r for r in entries if r["track"].endswith("/learned")]
        # Execute the registered hook, not a second evaluator implementation.
        def hook(event, extra):
            h = registration["hooks"][event][0]["hooks"][0]
            assert h["command"] == str(binary)
            return run(h["args"], dict(session_id="offline-onboarding", cwd=str(checkout),
                hook_event_name=event, **extra))
        hook("SessionStart", {"source": "startup"})
        delivered = hook("UserPromptSubmit", {"prompt": "Can you resolve CARE-279?"})
        reports = [r for r in logs if "sessions_read" in r]
        delivered_ids = [r["route"]["id"] for r in entries if r["route"]["id"] in delivered]
        delivered_learned_ids = [r["route"]["id"] for r in entries
            if r["route"]["id"] in delivered_ids and (r["track"].endswith("/learned") or r["route"]["id"].startswith("card-"))]
        assert "9 session(s)" in preview, "init must discover this repository's past history"
        assert reports and reports[-1]["sessions_read"] == 9, "the unrelated repository must remain excluded"
        assert delivered_learned_ids, "a learned artifact must reach the registered hook"
        result = {
            "source_revision": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=repo, text=True).strip(),
            "binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
            "source_tree_sha256": hashlib.sha256(json.dumps(source_hashes, sort_keys=True).encode()).hexdigest(),
            "source_files": source_hashes,
            "working_tree_dirty": bool(subprocess.check_output(["git", "status", "--porcelain"], cwd=repo)),
            "proof_script_sha256": hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
            "dataset_kind": "live_agent_sessions_on_generated_sandbox", "training_sessions": len(train),
            "hidden_outcome_labels": False, "default_thresholds": True, "paid_calls": 0,
            "inputs": hashes, "cwd_rebase_for_isolated_discovery": True,
            "installer_tested": False, "registration_fixture": "production install --print output",
            "wizard_learner_argv": ["condition", "--auto", "--repo-root", "<isolated checkout>"],
            "init_preview_detected_history": "9 session(s)" in preview,
            "learning_logs": reports,
            "index_route_count": len(entries), "learned_track_route_count": len(learned),
            "routes": [{"id": r["route"]["id"], "track_kind": r["track"].rsplit("/", 1)[-1],
                        "trigger": r["route"]["trigger"]["on"], "step": r["route"]["step"]["do"]} for r in entries],
            "heldout_prompt": "CARE-279", "hook_output_bytes": len(delivered.encode()),
            "delivered_route_ids": delivered_ids, "delivered_learned_route_ids": delivered_learned_ids,
            "delivered_learned_context": bool(delivered_learned_ids),
            "execution_savings_claim": False,
        }
        # No checkout paths or private prompt/command text leave the fixture home.
        result["learning_logs"] = [{k: r[k] for k in ["kind", "trigger", "sessions_read", "sessions_used", "thresholds"] if k in r} for r in reports]
        text = json.dumps(result, indent=2, sort_keys=True) + "\n"
        if args.output:
            args.output.parent.mkdir(parents=True, exist_ok=True)
            args.output.write_text(text)
        print(text)


if __name__ == "__main__":
    main()
