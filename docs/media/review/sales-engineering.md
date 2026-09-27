# Sales-engineering review: Adage film v4

## Verdict

The film earns the room and its numbers survive the manifest, but it opens the Q&A on the two questions it never touches: what stops a learned route from being wrong or hostile, and why this beats ten lines in CLAUDE.md. The demo (30–46 s) shows only the happy path, so a skeptic sees a scripted transcript rather than a control. Three on-screen claims (brew tap, three harnesses, the 24/30-turn comparison) are not supported by the repo and must change before anyone in the room opens it.

## Objections the room will raise

1. **Prompt injection through hooks.** "What stops a poisoned session teaching `curl | sh`?" Answer on screen: at 64 s, under "Deterministic. Text only. *Never runs a thing.*", add "Fixed templates. Slot contracts. 0 unsafe in 84 adversarial directives." (docs/evals.md safety oracle; `W-3102;whoami` binds nothing.)
2. **Wrong route.** "The route says `make gen`; my repo moved on." Answer on screen, one line at 46–52 s: "10% holdout. Evidence fades, routes retire. `adage review` before green." Hold in reserve: the `unittest` dead end was wrong once (ACME_ENV) and the miner was narrowed (docs/evals.md).
3. **CLAUDE.md.** "I'd write these four rules in ten minutes." Answer on screen: "Never written by hand" (50 s) never says why it matters. Rules are unconditional and rot; routes fire at the moment and carry evidence (Design Spec, "Why existing tools don't close the gap"). One line.
4. **n = 18, one model, a 15-file sandbox.** The eyebrow (66–71 s) is honest. Add the harbor-dispatch line the README already has: Sonnet 5, TypeScript, 84 sessions, −24% cost, quality unchanged. Two stacks beats "toy".
5. **Hidden checks 100% vs 78%** (71 s). README: p = 0.11. Caption "n.s. at n = 18" or cut; a reader finds this first.
6. **Transcript privacy.** Closing chip (79 s): "Local only · no prompt text in logs" (live-learning.md: logs and spools hold no prompt, command or path).
7. **Fail-open** ("a silent placebo?"). Verbal; `fail-open 0` in the watch pane (31 s) suffices.
8. **Other harnesses.** Verbal (see claims).
9. **Prompt caching.** Verbal: caching cuts a turn's price; Adage removes the turn (Design Spec line 73).

## Demo section critique

30–46 s: prompt, four signals, edit at turn 4, commit. Missing:

- **The control.** The turn counter (33–44 s) runs alone. Run the no-Adage counter beside it, greyed: 1…30, "max turns · failed". Two of three round-2 no-Adage runs of W-2045 hit the cap and failed; stronger than "Not 24".
- **What the agent received.** Show the raw `<adage-signal>` block once, so the room sees it is only attributed text.
- **Graded confidence.** Caption the yellow route (39 s) "yellow · 15 of 16 · consider" so signals read as evidence, not commands.
- **`adage watch` is the proof** ("✓ followed lookup-work-item") and is unreadable at 1080p. Give it 40% of the frame.
- Drop "5 ms · CPU" from every callout (33, 42 s); it repeats and reads manufactured. Architecture §9 measures 3.18 ms p95; say "3 ms" once.

## Claims to verify or soften

- **"First edit at turn 4. *Not 24.*"** (42 s), "9 turns · done" (44.5 s). Commit f247fa1 is `eval2-W-2045-r2-adage`: manifest turns 10, first edit 3. No round-2 W-2045 holdout has 30/24 (r1 18/11; r2 31/37 and r3 31/30, both max-turns). State the convention or use manifest numbers.
- **`brew install stevenvaughanbooth/tap/adage`** (27, 79 s). README: "none published yet"; `git tag` empty; tap configured in dist-workspace.toml, nothing published. Ship a release or show `cargo install --git`.
- **"Claude Code · OpenCode · Codex"** (79 s). docs/harnesses.md: "not yet run against a live installation"; Codex cannot fire after-failure routes. Soften to "Claude Code today · OpenCode, Codex in progress".
- **"Under 6 ms"** (27 s). 3.18 ms p95 structured; 6.39 ms with the intent model. Say "under 10 ms".
- **"54 sessions in 0.24 s"** (50 s). Not in demo/results or docs; verify.
- **Route evidence** (48–50 s): 23/33, 8 turns saved, 15/16, 7/7, 11/43 match index-round2.json. Supported. But lookup-work-item is green at lower bound 0.53 under `learn: "demo"` thresholds; at the default `green_lower_bound=0.8` it is yellow. Expect the question.
- **10× / −61%** (66–68 s): medians 2 vs 20, means $0.0531 vs $0.1354 from the manifest. Supported. "28 turns" (56 s) is the 27.5 median; fine.
- **100% vs 78%**: 18/18 vs 14/18, p = 0.11. Soften.

## Questions for the founder

1. Day one in a repo with no history: does anything fire (the "instincts" story), or is it nothing until ~20 sessions?
2. Has the static-guidance arm (Design Spec: the same knowledge as CLAUDE.md lines) been run? Objection 3 has no data without it.
3. What does the same history learn at default thresholds rather than demo?
4. Which harness besides Claude Code has a live install, and when?
5. Does injected `additionalContext` break the cached prefix on any harness?
6. Who reviews shared tracks in a team, and what does probation look like in practice?

## Must change before the pitch

1. Replace or footnote the brew line; there is no release.
2. Fix the 4/24 and 9/30 turn figures to the manifest, or state the convention.
3. Soften the three-harness chip.
4. Add one line on injection safety and one on "how is this not CLAUDE.md".
5. Mark 100% hidden checks as not significant, or cut it.
6. Show the no-Adage counter beside the demo counter at 33–44 s.
