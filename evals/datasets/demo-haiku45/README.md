# demo-haiku45: 90 real Claude Code sessions

This dataset is the Adage demo of 2026-09-22 (`demo/results/20260922-2231-claude-haiku-4-5-20251001`),
curated for offline evaluation with `adage eval`. Every session is a real headless Claude Code run
(`claude -p`, model `claude-haiku-4-5-20251001`). Each ran in a fresh copy of the acme-billing
sandbox (`demo/sandbox/generate.py`), and hidden checks scored it afterwards.

| Phase | Arm | Round | Sessions | Notes |
| --- | --- | ---: | ---: | --- |
| train | no Adage | 1 | 18 | 6 work items (W-1041…W-1046) × 3 prompt phrasings |
| eval | holdout | 1 | 18 | Held-out items (W-2041…W-2046). Hooks ran and logged what they would have said; nothing was delivered |
| eval | adage | 1 | 18 | Same items, with directives delivered |
| eval | holdout | 2 | 18 | Round 2 learned from all 54 earlier sessions |
| eval | adage | 2 | 18 | |

Labels come from the demo's hidden checks. 77 of 90 sessions passed. One ended with stale generated
code. In 10, the harness stopped the agent at 30 turns (`result: error_max_turns`); no Stop hook fired
in those.

## Files

- `manifest.json` has each session's id, run, phase, arm and round, plus its labels (`passed`,
  `stale_generated`, `result`, `turns`, `turns_to_first_edit`, `dead_end_hits`, `cost_usd`). It also
  holds the named splits:
  - `round1`: train on the 18 training sessions, test on the 18 round-1 holdout sessions.
  - `round2`: train on the 54 sessions the live round-2 learner read, test on the 18 round-2
    holdout sessions.
  - `learn: "demo"` selects the thresholds the demo used (`adage learn --demo`).
- `sessions/<run>.jsonl` holds one Claude Code transcript per run, in the format `adage learn` reads.
- `sandbox.txt` lists the sandbox's files. Replay creates them as empty files, so that
  repository-path contracts behave as they did live.
- `recorded/` holds what the live hooks did:
  - `decisions.json` lists every hook event of every eval session: the would-have routes and the
    judgements, taken from the spools.
  - `index-round1.json` and `index-round2.json` are the indexes those rounds used.
  - `adage eval fidelity` replays against them.

## How it was made

```bash
python3 evals/datasets/tools/build_demo.py demo/results/20260922-2231-claude-haiku-4-5-20251001/raw
```

The raw archive is not in git (it is 70 MB and machine-specific). The script reads it and never
modifies it. Its output is deterministic.

- **Source.** The script reads the real on-disk transcripts (`raw/transcripts/`), which are the same
  files the live `adage learn` read. It uses the session file named in the run's state. One run
  directory also holds an earlier, superseded attempt at the same run, which is left out. It leaves out subagent
  transcripts, because `adage learn` never reads them.
- **Kept.** User prompts, assistant tool calls and text, tool results and the hook-visible
  `toolUseResult`, and Adage's own hook attachments (`hook_additional_context`).
- **Dropped.**
  - Harness context: environment, skill listings, dates, model and session context, file history,
    costs, queue operations, and other hooks' output, including the hook command path.
  - Thinking blocks and their signatures.
  - Record ids and parent links.
- **Trimmed.** Tool output longer than 3,000 characters and assistant text longer than 1,500
  characters are cut, with a marker.
- **Redacted.**
  - `/private/tmp/adage-demo` → `/sandbox` (so the sandbox root is `/sandbox/runs/<run>/acme-billing`).
  - `/Users/<name>` → `/home/user`, and the account name → `user` (it appeared in `ls -l` output).
  - The build fails if any of these survive. The `demo_dataset_is_redacted` test checks again.
  - Generic system paths (Xcode's bundled Python) are kept: they carry no personal data, and error
    signatures depend on them.

## What it reproduces

- Learning from the curated `round2` training sessions reproduces the live round-2 index's routes
  (ids and aspects). Every route matches byte for byte except the dead end, which the
  signature-keyed dead-end miner deliberately narrowed; before that change the whole index matched
  (generation `8dd5ec01ed76c4bd`).
- Replaying the 36 round-2 eval sessions against that index reproduces every live decision
  (36/36). In 35/36 sessions it also reproduces the exact hook event sequence. In the other session
  the agent ran a forked skill: its tool calls fired hooks in this session but are recorded in a
  subagent transcript.
- Round 1 was decided by an earlier build, whose command keys templated interpreter scripts
  (`exec python3 {path} {id}`). The current runtime no longer matches that index's linked-files
  trigger, so round-1 fidelity measures that drift. It is not gated.
