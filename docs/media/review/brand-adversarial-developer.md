# Brand book v1: adversarial developer read

## Verdict

The book's headline, "Ships with instincts", describes a feature the binary does not have: nothing ships in the box, and `adage add` installs routes at probation, never green, so an instinct cannot fire green on day one. The reflex half is real and the evidence figures in the mockups match `index-round2.json` exactly, which is the book's strongest asset. Fix the fabrications (shipped instincts, `community/finish-line@1.2.0`, `kind="reflex"`, the `adage status` screen), label OpenCode/Codex as unverified, and put p-values and "toy repo" beside the tiles, or the guardrail "Do not fabricate a screen that the binary cannot produce" indicts the book itself.

## Describes a product that does not exist

- **"Ships with 6 instincts" / "shipped · community/finish-line@1.2.0".** No bundled tracks: `plugin/claude-code/` holds only `hooks/hooks.json` and `scripts/session-start`; `crates/adage-tracks/src/model.rs:121` knows provenance `desire-path`, `proving`, `authored`; imports live under `@imported` (`index.rs:43`). No registry, no "community" namespace. **Lie** as written in the init copy and status screen. Fix: "Learns reflexes. Adopt a teammate's track with `adage add`." Ship one real track before using "ships".
- **Instincts "fire from day one", green** (`adage watch` mockup: "→ instinct finish-line/regenerate"). `crates/adage-cli/src/add.rs:276-286`: imports start yellow, "never green on import (O2)"; README: no green without first-hand local evidence. **Plan** that contradicts a deliberate security decision. Fix: instincts are amber until your repo confirms them; sell that as trust.
- **`adage reflexes` / `adage instincts` / `adage routes --kind`.** `crates/adage-cli/src/main.rs:125-159` lists learn, config, status, report, share, add, review, graph, eval, models, survey, init, doctor, watch, ui, install. **Roadmap**; mark "planned" or cut.
- **`kind="reflex"` in the tag.** `crates/adage-runtime/src/render.rs:55`: only `track`, `route`, `aspect`; the book itself freezes the schema until a version bump. Drop it.
- **The `adage status` screen.** Real output (`main.rs:340-372`): `track @ version [repo]` then `🟢 id 23/33 sessions rate 0.70 lb 0.53 Observe`. No gen, hooks, turns saved or kind split. **Fabricated.** Mock only `adage watch` (README 133-147 is close; harness column reads `cc`, not `claude`).
- **Harness "(Claude Code, OpenCode, Codex)".** README Status: adapters "not yet run against a live installation"; `docs/harnesses.md` Verification has five "needs a live install" rows and Codex cannot fire after-failure routes. **Roadmap**; say "Claude Code today; OpenCode and Codex adapters, unverified live".
- **`brew install stevenvaughanbooth/tap/adage`.** README: "none published yet"; no git tags. The Ask fails today.
- **"First reflex by the end of the day."** Default thresholds are `min_support=5, lb=0.8` (demo: 3/0.5). Say "after a handful of sessions".

## Eye-roll lines

- "Adage gives coding agents a nervous system." → "Adage hooks into your agent's harness and replays the steps that repeat."
- "Reasoning is expensive. Instinct is free." → Training cost $4.87. "Reasoning is paid every session. A learned route is paid once."
- "Reflexes skip the brain." → Breaks your own no-brains rule; the lexicon retires "brain". Keep "Known routes skip the GPU."
- "The GPU explored once; the CPU remembers." → "remembers" is the banned "Memory". "The route is on disk."
- "Nobody writes anything." / "Never written by hand." → `adage review`, `authored: true` and pinning exist. "Nobody writes the first draft."
- "The distinction is biological and it is also literally how the index is stored." → It is not (`provenance`, `authored`). Cut.
- Pillar proof lines in mono ("a track installs with one command") break "Never set marketing copy in mono"; `adage add <dir> --trust` also refuses without a terminal.

## Claims stronger than the code

- **"Never runs a command."** True of directives (fixed templates, `render.rs`; safety oracle). But the hook spawns `adage learn --auto` detached on Stop/SessionEnd (`autolearn.rs:210-229`, `hook.rs:63-68`), `init` runs `curl`, and `session-start` is a shell script that copies and pre-runs a binary. Say "never runs what it learned".
- **"Adage advises; the agent acts."** Stop returns `decision: block` with a reason (`harness/claude.rs:79`); on Codex that reason becomes a user prompt (`docs/harnesses.md:149`). Bounded, but not advice. Say "text only; may ask the agent to continue once".
- **"Fail-open"** holds (`catch_unwind` + exit 0, `main.rs:73`; `fail-open.log`; kill switches), but a stuck hook still costs the 5 s harness timeout (`hooks.json`).
- **"Nothing is written without asking"** is true of `init` only; every hook writes state, spools and, via auto-learn, tracks.
- **"10 MB RSS."** `target/release/adage` is 10,012,792 bytes; `Architecture.md §9` has no RSS figure. Binary size, not memory.

## Numbers

- **10×, −61%, 27 → 9**: match `summary.json` and the manifest (2 vs 20; $0.0531 vs $0.1354; 9 vs 27.5; p ≤ 0.0002). Write 27.5.
- **100% vs 78%**: real, p = 0.11, n.s. at n=18 (README 46). Print it or it reads as cherry-picked.
- **0.24 s / 54 sessions**: `state.json` 0.2436 s; `sessions_used` 51 of 54.
- **5 ms**: 1.8–6.4 ms p95 on an M3 Pro (`Architecture.md §9`). Name the machine.
- **4 → 24 (W-2045)**: absent from `demo-haiku45/manifest.json` (round-2 W-2045: with 6/3/3, without 11/37/30); it comes from a later scratchpad run on track 0.1.1. Fails the book's own traceability rule; cite a file or drop it.
- Framing: one sandbox built so the agent "cannot guess" its conventions, Haiku 4.5. The Sonnet 5 run (README 53-83) shrinks to −24% cost. Say "toy repo, weak model, best case" once, loudly.

## Lexicon load

- **Keep**: instinct, reflex, track, signal, dead end, finish line, harness.
- **Merge**: lamp/aspect (aspect schema-only); route (code-only, already so). "signal" must absorb **directive**, the word the README, CLI (`adage report`) and evals use today; the lexicon never mentions it.
- **Cut from marketing**: desire path, signal box (say `adage graph`), "pack". Eleven terms plus directive, gen, arm, holdout, probation and ceiling is too many for a README.

## Questions for the founder

1. Which six routes are "true of nearly every repo", and who signs them?
2. Will you ship an instinct that can only ever be amber?
3. Does the first release land before the film, so `brew install` works?
4. Will you print p = 0.11 under "100%"?
5. Is `directive` renamed to `signal` in the CLI, or does marketing diverge from the product?
6. Codex: claim it or footnote it, given no failure signal?
