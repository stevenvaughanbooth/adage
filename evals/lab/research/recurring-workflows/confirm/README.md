# Confirmation: third family, hook delivery, automatic family detection

2026-09-29, `recurring-workflows` stage 2. Live Claude Haiku 4.5 (Claude Code 2.1.284), RepoBench
runner, `--host-auth --confine`. Spend was **$10.35 of the $12 cap** (`spend.json`). That includes
$0.12 of partial sessions killed by a container restart; they were rerun, and no finished session
was paid for twice. The design was pre-registered in `prereg.json` and `prompts_extra.json`, both
written before any detection score or live session of this stage.

This stage tests the two things stage 1 did not:

1. **Automatic family detection.** An embedding match at `UserPromptSubmit`, a secondary signal
   from edited files, a threshold chosen on the other families, and same-repository negatives.
2. **Hook delivery.** The card arrives as `UserPromptSubmit` `additionalContext` from a hook; no
   file is committed.

It uses a third family on a different ecosystem: Go, on urfave/cli.

## Answer

- **The lever holds, and is larger, on Go with hook delivery.**
  - With the card compiled from 4 prior sessions and delivered by hook, $ per task fell **33.8%
    (90% CI 23.6–44.3%)** and calls fell **31.5% (22.3–40.2%)** against control. That is 12
    pairs; CIs are bootstrapped over the 6 variants.
  - Pass rate was unchanged at 2/12 against 2/12. Both arms miss the same approval step, which no
    training session took either (below).
  - The live detector fired correctly on all 30 held-out sessions (29 at the prompt, 1 through the
    file signal).
- **Detection is precise, but only recognises near-template phrasing.**
  - At the pre-registered threshold there were **0 false fires on 53 same-repository negatives**
    (48 RepoBench bug-fix prompts plus 5 hard negatives; Wilson 90% upper bound 4.9%).
  - The file signal added **0 false fires on 58 real urfave-cli sessions** (upper bound 4.5%).
  - But paraphrased requests were never recognised: **0/6**, AUC 0.075.
  - A post-hoc contrastive weighting lifts paraphrase AUC to 0.92, yet no threshold gets both high
    recall and zero false fires from 4 templated examples.
- **The learning curve is not a pure step.** On Go, a card compiled from **1** session saved only
  **9.4% (−24.7% to +8.2%)** against 33.8% from 4 sessions. The single-session card cannot
  anti-unify the per-variant file names (`flag_url.go` → `flag_<x>.go`), so it names the previous
  variant's files. Stage 1's "saturated after one session" holds only where the family's files
  keep the same names every time (click, marshmallow).

## Family: `go-flag` (`family_urfave.py`, validated 10/10: `validation.json`)

The task is to add `cli.<Name>Flag` with a `cmd.<Name>(name)` accessor on urfave/cli at `58ec406e`.

- **Training (4):** URLFlag, PortFlag, RegexpFlag, IPFlag.
- **Held out (6):** PercentFlag, ByteSizeFlag, DateFlag, HexColorFlag, CIDRFlag, SemverFlag.
- **Procedure the hidden check requires:**
  - a `flag_<x>.go` built from a `FlagBase` alias, a value type and the accessor;
  - tests;
  - a bullet in `docs/v3/examples/flags/basics.md`;
  - the generated API docs: `godoc-current.txt` (`go doc -all`, which is `make generate`) **and**
    its approved copy `testdata/godoc-v3.x.txt` (`make v3approve`), which CI's `v3diff` compares.
- **Hidden check.** A Go test file (`zz_rw_hidden_test.go`) checks the valid and invalid parses
  through `Command.Run`, the three docs files and the agent's own test; then `go test -count=1 .`
  runs.

## Arms (`run.py`: the unchanged runner with a different settings file)

| arm | hooks | card |
| --- | --- | --- |
| control | Adage's hooks (directives withheld) + `card_hook.py` in **log** mode | detected and recorded, never injected |
| card-by-hook (n=4) | same, `card_hook.py` in **deliver** mode | card from the 4 training sessions |
| card-by-hook (n=1) | same, deliver mode, index from the first training session | 6 variants × 1 repeat |

The hook, its index and the embedding model are mounted read-only into the confined session.
Nothing of the benchmark is mounted. The hook's median latency was 232 ms (5.7 s on a cold start
after the restart).

## Live results (held out; paired by variant and repeat; 90% bootstrap over the 6 variants)

| | $/session | calls | turns | censored at 60 turns | passed | Δ$ vs control | Δcalls vs control |
| --- | ---: | ---: | ---: | ---: | ---: | --- | --- |
| control (12) | 0.347 | 52.4 | 53.4 | **6/12** | 2/12 | – | – |
| **card-by-hook, n=4 (12)** | **0.230** | **35.9** | 36.9 | 0/12 | 2/12 | **−33.8%** [−44.3, −23.6] | **−31.5%** [−40.2, −22.3] |
| card-by-hook, n=1 (6) | 0.314 | 47.3 | – | 1/6 | 1/6 | −9.4% [−24.7, +8.2] | −9.7% [−19.2, −0.3] |

- **The control's savings are understated.** Half of the control sessions hit the 60-turn cap, so
  their real cost is higher than measured and the saving is conservative.
- **The card sped up orientation, unlike stage 1's `CLAUDE.md` card.**

  | calls per session | card | control |
  | --- | ---: | ---: |
  | before the first edit | 9.8 | 18.9 |
  | searches | 9.4 | 17.3 |
  | reads | 9.1 | 14.4 |
  | failed calls | 1.6 | 2.8 |

- **Pass rate is low in both arms, for one reason.** In 10 of 12 sessions per arm the only failure
  is `TestRWHiddenDocs`: the approved copy `testdata/godoc-v3.x.txt` was not updated. **No
  training session did that step**, so the card, which is learned from traces, cannot contain it.
  A card only repeats what earlier sessions did. Outcome-aware compilation (for example "the
  failing check named this file, which no session edited") is the next piece; it is not tested
  here.
- **Caveat on the n=1 arm.** It ran after the container restart, 1 repeat per variant, and is
  compared with the control mean of both repeats. There is a time confound, and n = 6.

## Detection (`detect.py`, `detection.json`; offline, $0; also live via the hook log)

The pre-registered method:

- **Embedding and score.** potion-base-8M (model2vec: mean of token vectors, L2-normalised).
  The score is the max cosine between the new prompt and the prompts of the family's prior
  sessions **in the same repository**.
- **Threshold.** τ was chosen on the two stage-1 families only (click, marshmallow: held-out
  prompts and paraphrases against all their RepoBench prompts and hard negatives), then applied
  unchanged to go-flag. Those training classes were **not** separable (min positive 0.39, max
  negative 0.80), so the rule gave τ = max negative + 0.01 = 0.811. For a 1-prompt library it
  gave τ = 0.753.
- **Grey zone.** A prompt scoring in [τ − 0.10, τ) gets the card on its first edit of a card file.

| go-flag (never used to choose τ) | library of 4 | library of 1 |
| --- | --- | --- |
| recall, templated prompts (as run live) | **6/6** | 5/6 (live: 5 at the prompt + 1 through the file signal) |
| recall, informal paraphrases | **0/6** (AUC 0.075) | 0/6 (AUC 0.057) |
| false fires, 48 urfave-cli RepoBench prompts | **0/48** | 0/48 |
| false fires, 5 hard negatives (same area: flags, docs) | **0/5** | 0/5 |
| false-fire rate, Wilson 90% | 0 [0, 0.049] | 0 [0, 0.049] |
| precision | 1.00 | 1.00 |
| file signal on 58 real urfave-cli sessions (all arms) | 0 prompt fires, 7 in the grey zone, **0 file fires** (16 edit a card file, none from the grey zone); rate 0 [0, 0.045] | – |
| live, 30 held-out sessions (both arms log the decision) | 25 + 1 grey→file = 30/30 fired (scores 0.74–0.95) | |

potion-code-16m-v2 behaves the same: 6/6 templated, 0/6 paraphrased, 0/53 false fires, paraphrase
AUC 0.32.

**Paraphrases are where detection fails.** Whole-prompt cosine mostly measures style: terse
paraphrases score 0.41–0.56, below the bug-fix negatives (max 0.76).

**Post-hoc variants** (clearly labelled; chosen after seeing that failure):

- **Contrastive token weighting.** Per repository, token weights are learned as the log-odds that
  a prompt containing the token is a family prompt rather than one of the repository's other prior
  prompts, followed by cosine to the family centroid.
  - Paraphrase AUC rises from 0.075 to **0.92**, and templated prompts still score 6/6 with 0/43
    false fires.
  - At the τ transferred from the other families, paraphrase recall is still 0/6.
  - A per-family τ taken from the repository's own history catches **5/6 paraphrases, but with
    10/43 false fires** (23%).
  - So with 4 templated examples, no threshold has both.

**What to build for detection:**

- Ship the conservative threshold, which is precise.
- Add the file signal, whose false-fire rate is low here.
- Grow the family's prompt library with every session that matched, so the user's own phrasings
  become the examples. Real users' prior prompts are not templated, so the library will cover
  their style.
- Consider asking before delivering on low-confidence matches.

## Card compiler spec (for the Rust port; the reference is `../procedure.py`)

**Input.** For one repository:

- the transcripts of the family's prior sessions, in chronological order;
- their outcomes (the hidden or CI result; optional, but it steers exemplar choice);
- `max_tokens` (default 800).

A family is the set of sessions that matched each other under the detector above; stage 2
bootstraps it from the first session.

**Per session: the abstract actions** (`session_view`):

1. **edits**: `(k, path, tool, added_text, failed)`.
   - `path` is made relative to the session's cwd.
   - `added_text` is the lines of `new_string` that are not in `old_string` (the whole content for
     a Write; for MultiEdit, over all its edits).
   - A Bash command whose **first line** redirects (`>`/`>>`) to a relative path also counts as an
     edit of that path, with the command as its text (for example `go doc -all . > godoc-current.txt`).
2. **commands**: `(k, raw, norm, failed, output)`, with `norm = norm_cmd(raw)`:
   1. Take the first line, drop anything after `<<`, and split on `&&` and `;`.
   2. Drop `cd …` segments.
   3. From each remaining segment, drop pipes (`|…`) and redirections (`N>…`, `>&N`).
   4. Rejoin with `&&`, shell-split, cut `::` suffixes, and keep at most 6 tokens.
   5. Replace the sandbox's absolute cwd with `.`.

**Learn** (`learn`), with n sessions and `need = 1 if n == 1 else ceil(n/2)`:

1. **Files.**
   - For each session, take its distinct successfully edited paths in first-edit order, with each
     one's position normalised to [0,1].
   - Keep every path edited in at least `need` sessions.
   - **Varying names (n > 1).** Paths edited in exactly one session are grouped by
     (directory, token count, first token, extension), with names tokenised on `_ . -`. In each
     group, aligned tokens that agree are kept and the others become `<x>`. The pattern is kept if
     its sessions number at least `need` and it contains `<x>` (for example `flag_<x>.go`,
     `flag_<x>_test.go`, `tests/test_types/test_<x>.py`).
   - Order all kept files and patterns by their median normalised first-edit position.
2. **Exemplars.**
   - For each kept file or pattern, search sessions from most recent to oldest, passing sessions
     first.
   - Take the longest `added_text` among its edits in that session (pattern files match by
     `fnmatch` with `<x>` → `*` in the same directory).
   - Clip it to 14 lines.
3. **Commands.**
   - Group commands by `norm`. For each group, count sessions, runs, failures, and the first error
     line that isn't `Exit code…`.
   - Keep groups run in at least `need` sessions.
   - **use** is failures < runs/2, sorted by session count, top 6. **avoid** is failures ≥ runs/2,
     top 4, each with its error line.
4. **Finishing check.**
   - Loop commands are the 2-token heads a session ran at least twice before its last edit.
   - Among passing sessions, collect the loop commands run successfully after the last edit, and
     take the head that occurs in the most sessions.
   - Anti-unify its commands token by token.
   - Drop it if half or more of its tokens are `<…>`.

**Render** (`render(card, max_tokens)`) as Markdown with fixed headings:

- the provenance line: "Learned from n earlier session(s) … (k passed) … use it as a checklist";
- "Files such a change touched (in order)", one line per file or pattern with its k/n;
- "What the edit looked like last time (another variant; copy the format, not the content)", with
  one fenced exemplar per file;
- "Commands", with the use lines ("ran in k/n sessions, s/r succeeded") and the avoid lines
  (with the error);
- "Finishing check".

**Budget.** If the text exceeds `max_tokens × 4` characters, re-render with exemplars clipped to
10, then 6, then 3 lines, then no exemplars. As a last resort, hard-truncate. The observed cards
were 350–700 tokens (`../cards/*.md`, `cards/*.md`). Carrying one over about 35 responses costs
about $0.002–0.003 per session with Haiku.

**Runtime** (`card_hook.py`, about 150 lines; for Rust, this becomes a new `intent`-triggered
route kind):

- **Index.** Per repository key (normalised `origin`), per family: prompt embeddings, the card
  text, exact card files, the card's glob patterns, τ and τ_low.
- **`UserPromptSubmit`.** Embed the prompt and score it against every family of this repository.
  - Score ≥ τ: emit `additionalContext` = the card.
  - Score in [τ_low, τ): remember the family for the session.
- **`PostToolUse`** (Edit, Write, MultiEdit) on a remembered family: if the edited path is a card
  file or matches a card pattern in the same directory, emit the card once.
- **Log mode** records the decision without injecting, for control arms and shadow evaluation.
  Every error exits 0 silently.

**Hard-coded parts** (unchanged from stage 1, plus these):

- the edit tool set (Edit, Write, MultiEdit, NotebookEdit);
- the shell normalisation and redirect parsing;
- the name tokenisation on `_ . -`;
- the need = ceil(n/2) rule;
- the clip lengths and the 800-token budget;
- the card's headings;
- τ's selection rule and the 0.10 grey margin;
- in the post-hoc detector, 0.5 smoothing and the +0.02 margin.

Nothing names a file, command or error of any repository.

**Learner changes made in this stage.** They were made **after** seeing the 4 Go training traces
and **before** any held-out session:

- Exemplar lookup for varying files was Python-only (`<new>.py`); it is now generic.
- New files were anti-unified by directory only, which merged `flag_url.go` with
  `flag_url_test.go` and with scratch files; they now use name shapes.
- Shell-redirect writes now count as edits.
- The render has a token budget.

Re-running stage 1's analysis with the patched learner reproduces its numbers exactly.

## So what

A learned procedure card, detected automatically and delivered by hook with no committed file,
saved **33.8% of $ (CI 23.6–44.3%) and 31.5% of calls** on a third family in a different ecosystem
(Go), with pass rate unchanged. Together with stage 1 (−16% and −23% of $ on click and
marshmallow), three families in three repositories agree in sign.

- **Detection** fired on every held-out family task and on **none of 53 same-repository negatives
  or 58 real non-family sessions**. But it recognises only phrasing close to earlier prompts
  (paraphrase recall 0/6), so the realised benefit depends on users repeating themselves or on the
  prompt library growing.
- **The card needs at least 2 sessions** when the family's file names vary per task (n=1: −9%, CI
  includes 0).

Confidence that it survives a live A/B: **moderate to good** for the effect when the card fires.
Detection recall on real phrasing is **unproven**.

**Next micro-slice:** real users' repeated prompts in their own words, to measure recall with a
growing library, plus an outcome-aware card that adds the step every failing session missed.

## Files

| file | what |
| --- | --- |
| `family_urfave.py` | the Go family: prompts, hidden Go test, gold patches (registered in `../families.py`) |
| `prereg.json`, `prompts_extra.json` | pre-registration; hard negatives and paraphrases, written before any score |
| `card_hook.py` | the runtime hook (detection + delivery + log mode) |
| `build_index.py` | compiles a family's card and prompt embeddings into the hook's index |
| `detect.py`, `detection.json` | offline detection: threshold on training families, held-out family, negatives, file signal, post-hoc variants |
| `run.py` | runner driver: the card-hook settings for both arms |
| `analyze_confirm.py`, `analysis.json` | live results, uptake, hook log, n=1 point |
| `configs/`, `cards/` | runner configs; the compiled Go cards (n=4, n=1) |
| `validation.json`, `spend.json`, `interrupted_spend.json` | gold/base validation; spend |

Raw runs (transcripts, hook logs, state; 60 MB after pruning) are in
`/tmp/adage-research/recurring-workflows/confirm/runs`.
