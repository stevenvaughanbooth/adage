# Developer review: Adage film v4

## Verdict

Yes, I would install it, but only after the pitch answered "why not CLAUDE.md?" and I had confirmed the `brew` line works (zero tags; README says "none published yet"). First search: `adage hooks what does it record ~/.claude/projects`, then `ADAGE_DISABLE`. The film earns trust with real transcripts, evidence-bearing route cards and "Deterministic. Text only."; it spends it on the GPU/CPU section and on hiding the Sonnet result.

## Trust up / trust down

Up, ranked:
1. 33–44 s: the real `<adage-signal>` text with route id, aspect and "not requested by the user." Matches `eval2-W-2045-r2-adage.jsonl` line 1 almost verbatim.
2. 48–50 s: "23 of 33 sessions · 8 turns saved", "lower bound 0.72", "11 of 43". All match `recorded/index-round2.json`.
3. 27 s: `adage init` panel, "Nothing is written without asking."
4. 64 s: "Deterministic. Text only. Never runs a thing."
5. 66 s: "Measured · 18 tasks · Claude Haiku 4.5". Naming n and the model is rare.
6. 79 s: "One Rust binary · 10 MB · no daemon".

Down, ranked:
1. 56–64 s: "Known routes replay on the CPU", "$0 of inference", "takes another decision off the model." The model still makes every decision, including whether to obey; what is removed is exploration turns. The GPU/CPU area chart is generated from invented weights (`cpuShare` in index4.html: 0.30/0.12/0.08/0.17), not measured. This is the slide HN screenshots.
2. 66–71 s: 10× / −61% / 100% only from Haiku on a sandbox "whose conventions they cannot guess". The README's Sonnet 5 run shows 6→3 turns, −24% cost. Omitting it looks cherry-picked; the p = 0.11 on "100%" is dropped too.
3. 44 s: "First edit at turn 4. Not 24. Recorded, not staged." The panel silently drops turns 6–7 (`git status`, `Read models.py`); watch timestamps (14:02) are invented (real: 05:27 UTC).
4. 53 s: "Inference explores. Once." Auto-learn re-runs after every session and the holdout keeps exploring 10% of the time.
5. 8–12 s: node tags come from the round-1 holdout; "pytest ✗" appears in neither W-2045 holdout, both ran `make test`.

## Demo section: does it look like my day

Half. True to life: the prompt, Claude Code's `⏺`/`⎿` rendering, the yellow "read them together" nudge, a Stop-time finish line. Not: nobody's ticket lookup is `scripts/wi-lookup` in a 12-file repo with 3 tests; my day is `gh issue view`, `pnpm test --filter`, codegen after protobuf edits. Fixes: (1) show the holdout as it was, because it beats "turn 24": that agent never found the ticket, ran a code-review skill for 96 s, edited `billing.py` for bugs nobody asked about, hit the 30-turn cap and cost $0.28 against $0.055; (2) keep every turn, or label the panel "abridged"; (3) show one `◌ withheld` line in `adage watch`.

## Accuracy versus the repo

- 27–28 s chips say "PostToolUse". The plugin and `adage install claude-code` register `PostToolBatch` and `PostToolUseFailure` (`plugin/claude-code/hooks/hooks.json`, `crates/adage-cli/src/harness/claude.rs` `settings`). Six hooks, not four.
- 33 s: the dead-end text shown is the post-run narrowed version; the recorded agent saw "python3 -m pytest / python3 -m unittest / python -m pytest fails" (`eval2-W-2045-r2-adage.jsonl` line 1).
- 44 s: `manifest.json` labels this pair `turns 10 / turns_to_first_edit 3` and `31 / 37`, not 9/4 and 30/24. The film counts tool calls; the dataset does not. Pick one convention.
- 10 s: "14 sessions · one desire path" vs 16 ghost curves vs 18 training sessions.
- 50 s: "54 sessions in 0.24 s": `summary.json` has `sessions_read 54, sessions_used 51`, no duration. Unverified.
- 79 s: "OpenCode · Codex": README Status says "not yet run against a live installation". "Signed releases" with no release tag.
- 64 s: "Never runs a thing" is true, but Stop can `decision: block` and force another model turn (`claude.rs:79`); the cost story omits it.
- "Ships with instincts": no bundled tracks exist in the tree.

## Jargon audit

- routes: keep; concrete, shown with evidence.
- tracks: keep; show one `@0.1.0` name so it reads as a package.
- aspects: cut; say green/yellow/red.
- signal box: rename "graph view"; nobody under 40 outside the UK knows it.
- desire path: keep; the one metaphor that explains the learning.
- finish line: keep; "CI fails without it" sells itself.
- dead end: keep.
- reflexes: keep; nobody hand-writes reflexes, which answers CLAUDE.md.
- instincts: hold until a pack ships.

## Questions for the founder

1. Why is this better than a 20-line CLAUDE.md, and does it still help once one exists?
2. How much of the 10× survives on Sonnet/Opus in a real monorepo, given the README's 2×?
3. What is written where, any network call besides `models fetch`, and how do I uninstall? (No command; only `ADAGE_DISABLE=1` and `.adage/disabled`.)
4. Do I learn about the 10% holdout before it confuses me?
5. Hook latency on a cold laptop with 200 cached sessions?

## Must change before the pitch

1. Replace 52–65 s: drop "$0 of inference" and the synthetic chart; say "fewer exploration turns", 27.5→9.
2. Add the Sonnet 5 row to 66–71 s, and "n.s." on 100%.
3. Show the real holdout ending (max turns, wrong files, $0.28) instead of "Not 24".
4. Fix the hook chips and dead-end text to match the plugin and transcript.
5. Ship a tagged release before the film goes out, or change the CTA to `cargo install`.
6. One caption on privacy and off-switch: "Local only. No telemetry. `ADAGE_DISABLE=1`."
