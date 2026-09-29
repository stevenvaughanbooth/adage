# Token cost: where can a hook save the most dollars?

Slug `token-cost`, 2026-09-29. Offline work over RepoBench v1+v2 traces; it cost $0.

**So what, up front.** Shaping tool outputs with a hook (trim, window, dedupe) does **not** reliably
reach the North star. Under the harm models I consider credible, learned output shaping nets
**−2% to +4% of $ per task on Haiku, and −2% to +3% on Sonnet**. Even a hindsight oracle that
picks the best window per output reaches only **4.5%** (Haiku) and **2.3%** (Sonnet). The 11–18%
figures you get elsewhere depend on two assumptions:

- content that was trimmed but later needed never comes back into the context;
- or the agent never needs lines it doesn't quote verbatim.

Neither assumption survives a look at the traces.

Two other findings matter more:

- **The largest harm-free dollar lever is not a hook.** These runs write the 1-hour prompt cache,
  at 2× the input price. Every inter-call gap except one of 16.6k is under 5 minutes, so the
  5-minute cache, at 1.25×, would have served the same reads. That is **−11.7% of $** on Haiku
  [11.3, 12.1] and **−10.9%** on Sonnet [10.5, 11.5], with no behaviour change at all.
- **A learned "you're done" stop signal has the largest gross effect: 19% of $.** But it would
  cut later real work in 28% of the sessions where it fires. Its worst-case pass-rate loss is
  −13.9 points [10.0, 19.1], so it cannot ship as a stop.

Token shaping is therefore **not** a bigger realisable lever than call saving. It is comparable
at best. It has a large oracle ceiling (29% of $ is tool-output lines never referenced again), but
no hook can see which lines those are ahead of time.

## Question

Dollars in agent loops come from tokens, and mostly from context re-read on every call. Where
can a hook save the most dollars? The candidates were:

- large tool outputs;
- re-reads of the same file;
- early exploration carried to the end of the session;
- compaction;
- learned output shaping (scoped commands, PreToolUse rewrites, read ranges);
- avoiding re-reads.

Is any of these bigger than saving calls?

## Data and pricing

- **Held-out set.** I used exactly the common harness's held-out control sessions: 158 Haiku
  4.5 sessions over 38 tasks, and 32 Sonnet 5 sessions over 16 tasks. The IDs come from
  `harness.load().held_out(model=…)`, so there is one session per run. The harness test-set IDs are `6a795a362ccb` (Haiku) and
  `3c412b6cdb2e` (Sonnet); its baselines, $0.260 and $0.568 per task, match mine.
- **Training set.** 47 training-split Haiku sessions; there are no Sonnet training traces. The
  CIs are 90% task-clustered bootstraps (`common.boot_cluster`).
- **Usage.** Per-call usage is reconstructed from assistant `usage` records, one per unique
  message id (Claude Code writes a row per content block). The fields are input, output, cache
  read and cache write, with the 1h/5m split.
- **Prices ($/MTok).** Haiku 4.5 is 1 in, 5 out, 0.10 cache read, 1.25 for a 5m write and 2.00
  for a 1h write. Sonnet 5 is 2, 10, 0.20, 2.50 and 4.00. These reproduce Claude Code's own
  `cost-state.totalCostUSD` to within 0.00% on all 549 transcripts that carry it.
- **Cache TTL.** Every write in these traces is a 1-hour write (42,730 of 42,750 calls). Every
  $ figure below is given at the observed 1h TTL, and the main rows also at a 5m TTL, since a
  deployment on the 5m TTL is about 12% cheaper to begin with.
- **Tokens per tool output.** I fitted a least-squares regression of each call's cache write on
  the new tool-result characters, the other new characters and the previous output tokens. The
  fit is R² = 0.994 for Haiku at 0.324 tok/char, and 0.972 for Sonnet at 0.387.
- **Carry cost.** A token first seen at call k costs one write plus one read on every later
  call. With a 1h write at 2× and a read at 0.1×, a token costs as much to write as to carry for
  20 calls. This is the same model as the harness's `costmodel`.

## Where the dollars go (held-out control, `describe.py`, `levers.py`)

| share of $ | Haiku (n=158, $0.260/session) | Sonnet (n=32, $0.568/session) |
| --- | --- | --- |
| cache reads / cache writes / output tokens | 45% / 32% / 23% | 41% / 30% / 29% |
| system prompt + tools prefix (re-read every call) | 20.0% | 22.0% |
| **all tool results (write + carry)** | **36.5%** [35.0, 38.0] | **24.6%** [22.2, 27.0] |
| Read, whole file | 20.9% | 8.0% |
| Read, ranged | 7.2% | 8.7% |
| Bash test runs (full-suite output etc.)¹ | 3.7% | 2.9% |
| grep/find/ls/Grep/Glob¹ | 2.4% | 2.2% |
| tool outputs ≥ 2k tokens | 23.4% | 12.3% |
| tool results first seen in the first third of the session (early exploration) | 27.5% | 16.8% |
| **tool-output lines never referenced later (oracle ceiling for shaping)²** | **29.3%** [27.9, 30.8] | **18.5%** [16.5, 20.5] |
| re-reads of already-shown, unchanged file lines | 1.0% (1.46 re-reads/session) | 0.4% |
| compaction | none: 0 compactions, max context 109k / 125k | none |
| one tool call (average) | $0.0078 = 3.0% | $0.0183 = 3.2% |

¹ The buckets are a hand-written *measurement instrument*; no policy uses them.

² **Was this output used? A learned data-flow measure (`usage.py`).** A line of a tool output
counts as *referenced* in two cases:

- **(a)** its text reappears verbatim in something the agent later writes (an Edit
  `old_string`, a command or a message);
- **(b)** it contains an identifier, path or number that the agent writes later, that did not
  appear in the prompt or in anything the agent wrote before, and that is rare in the output.

The agent's thinking is omitted from these transcripts, so this is a **lower bound** on use.
"Strict" means rule (a) only.

The money is in **Read**: 28% of Haiku $ is Read output, and 72% of tool-result text. Test output
is 3.7%. Even a perfect test-output filter is worth less than one saved call.

## Levers and held-out results

All shaping policies are learned per repository from the training split. The learner chooses,
per context key, the action with the best net training $, or no action. The context keys back
off in this order: `repo|template|ranged` → `repo|key` → `repo|tool` → `all|…`. The action
grid is:

- **POS(H,T)**: keep the first H and last T lines;
- **REL(w,h,t)**: keep ±w lines around *anchor* lines (identifiers the agent had already written,
  known at hook time), plus a head and a tail;
- **GREP(w)**: for a Read, a window around the lines an earlier grep named as `file:123:`.

Every trimmed output carries an elision note, so the agent can tell something was trimmed and
fetch it by range. I evaluated three harm models for a *miss*, which is a later-referenced line
that was elided:

- **H1**: one extra call at the session's mean $/call. This is `waste-anatomy/trim.py`'s model.
  The missed content itself never re-enters the context.
- **H2**: one extra call that re-reads the context at that point, plus the missed lines (±10,
  merged) re-entering the context and being carried from there.
- **H3**: (1 + regions) extra calls plus the regions, or 2 calls plus the whole elided part,
  whichever is cheaper.

`evaluate.py` gives the net $ saved as a share of the baseline $ per task, with 90%
task-clustered CIs. "Split" means trained on the training split and scored on held-out
sessions; "LORO" means the parameters were learned only on the other repositories.

| policy | use measure | harm | Haiku 1h TTL | Haiku 5m TTL | Haiku LORO (1h) | Sonnet 1h TTL |
| --- | --- | --- | --- | --- | --- | --- |
| all actions (POS+REL+GREP) | loose (default) | H1 | +13.8% [11.0, 16.9] | +12.7% | +13.9% [10.5, 17.4] | +1.5% [−1.7, 4.4] |
| all actions | loose | **H2** | **−0.7% [−1.4, 0.0]** | −1.3% | −1.6% [−2.3, −1.0] | −2.3% [−3.5, −1.4] |
| all actions | loose | H3 | −2.1% [−2.8, −1.4] | −2.6% | −1.6% | −1.7% |
| all actions | strict (optimistic) | H2 | +12.6% [9.4, 15.9] | +10.5% | +12.2% [8.9, 15.5] | +3.0% [1.3, 4.9] |
| all actions | strict | H3 | +4.0% [1.7, 6.4] | +2.1% | +3.6% [1.3, 6.0] | −1.6% |
| PreToolUse-only (Read `limit`, GREP `offset/limit`, Bash wrapper) | loose | H2 | +0.1% [−0.5, 0.7] | −0.4% | −1.4% | −2.2% |
| PreToolUse-only | strict | H2 | +12.6% [9.4, 15.9] | +10.4% | +12.4% | +3.0% |
| Drain line-template filter, non-Read outputs (`drainfilter.py`)³ | loose / strict | H3 | −6.4% / +1.1% | | −2.7% | −5.3% / +0.7% |
| re-read dedupe (Read stub for already-shown, unchanged lines) | – | none | **+0.95%** [0.6, 1.4] | | | +0.4% [0.2, 0.5] |
| **hindsight ceiling of the POS/REL/GREP class** | loose | – | 4.5% [3.9, 5.1] | 3.9% | | 2.3% [1.8, 2.9] |
| hindsight ceiling of the class | strict | – | 15.8% [13.6, 18.0] | 14.1% | | 8.9% |
| oracle: delete every unreferenced line, free | loose | – | 29.3% [26.9, 31.8] | 27.3% | | 18.5% [15.8, 21.2] |

³ `drainfilter.py` uses the older 176/32-session set (it includes discarded-rerun transcripts),
session-level CIs and harm model H3. Its lines are elided when their Drain template (drain3 core,
numbers masked) was never referenced in training. Gross savings are only 1–2% of $, because test
and search output is small.

### Reading the table: the harm model decides everything

- **H1 overstates the saving.** In the loose H1 row, the learned policy trims 4.2 outputs per
  session and misses referenced content in 3.0 of them. H1 then charges only one call per miss
  and lets the needed content vanish. If the agent really needed those lines (by definition it
  wrote something derived from them), they must come back into the context. H2 charges that,
  and the win disappears: **−0.7%**. The reason is that referenced lines are scattered. In 94% of
  whole-file reads the last referenced line lies past 75% of the file, so no head/tail/window
  captures them. This is why the class ceiling is only 4.5%.
- **The strict measure overstates what is unused.** It counts only verbatim quotation as use.
  Under it the learner cuts source-file Reads to **10 head + 10 tail lines**
  (`policy_haiku_ALL_H3.json`), which assumes that reading code to understand it has no value.
  I don't believe that, so treat the +12.6% row as an optimistic bound, not an estimate.
- **Natural-experiment calibration (`refetch.py`).** When the *agent itself* truncated an output,
  a well-informed cut, it went back for more:
  - after a binding `| head -N` / `| tail -N`: **11.8%** [9.6, 14.4] (Haiku), 5.7% (Sonnet);
  - after its own ranged Read: a continuation Read of the same file in **24.9%** [22.6, 27.3]
    (Haiku), 24.8% (Sonnet).

  A blind hook cut should miss at least as often. The learned policies' miss rates per trimmed
  output are 56% (loose, H2) and 31% (strict, H2), which bracket that.

Two more checks:

- **Harm to the task is not estimable offline.** Every trim leaves a note and parks the full
  output, so a miss should cost calls rather than the task. In the loose H2 row, 1.8 misses per
  session means 1.8 extra fetches that the agent must *choose* to make. The live risk is that it
  doesn't, and proceeds on partial information.
- **Positional windows on Bash output** (test runs, greps) are capped by their small share:
  Bash plus search is about 8% of Haiku $, and about 7% of that is unreferenced lines. The
  learned Drain filter nets about 0.

### What a hook can actually do (Claude Code, code.claude.com/docs/en/hooks)

- **PreToolUse `updatedInput`** can set Read `offset`/`limit` (head-only windows, grep windows).
  It can also wrap a Bash command so it writes its full output to a file and prints the head and
  tail (`{ cmd; } > f 2>&1; rc=$?; head -H f; echo "[… N lines elided, full output: f]"; tail -T
  f; exit $rc`). That is safe for non-interactive commands. It is unsafe for commands that
  background (`&`), need a TTY or colour, or stream, and it changes nothing when output ≤ H+T.
  Use **`additionalContext`** to tell the agent what was elided.
- **PostToolUse**: the current docs list `updatedToolOutput` ("replaces the tool's result"). That
  would allow content-aware trimming (REL, Drain) of built-in tool output. The docs page I could
  fetch was truncated, so **verify in a live spike** that it applies to Bash and Read and not only
  to MCP tools before relying on it. The Adage Rust harness (`crates/adage-hook/src/harness/
  claude.rs`) uses only `additionalContext` today. In the table, ALL (which needs PostToolUse)
  and PreToolUse-only perform almost identically, so this capability is not what limits the win.

### (b) Learned "you're done" and redundant re-run signals (`done.py`)

- **Done signal.** The event is the Bash command key × the Drain template of the output's last
  line, after an edit. P(no further edit | event) is learned from training traces with a
  Beta(1,1) prior and support ≥ 3 sessions, and θ is learned (0.8). No runner names or pass
  strings are used. If the agent stopped at the first firing, keeping one call to answer:
  - Haiku: fires in 78.5% of sessions and cuts 6.5 calls, **19.3% of $** [16.4, 22.1]. LORO:
    20.0% [16.6, 23.5].
  - **Harm:** in 28.2% [22.1, 35.3] of the firings the agent later made a successful edit,
    which is work a stop would cut. Pass rate was 62.9% in those sessions versus 71.9% where it
    didn't. The worst-case pass-rate loss is **−13.9 points** [10.0, 19.1] (22 of 158 sessions
    passed after post-signal edits).
  - Counting only the harmless firings, the saving is 8.8% [7.2, 10.6]. That is an oracle,
    since it needs to know no edit follows.
  - Sonnet: the Haiku-trained events barely fire (16%), so there is no transfer.
  - Verdict: do not ship it as a stop. At most, a non-blocking "tests are green; finish unless
    something is still missing" note, which would need a live A/B for its pass-rate effect.
- **Redundant re-runs.** A redundant re-run is the exact same command re-run with no edit in
  between. Haiku does this 0.37 times per session, and only 11 of 65 such re-runs returned
  identical output (the environment changed, e.g. through pip installs). No command key reaches
  the learned idempotency bar of 80% identical output over ≥ 3 repeats. The ceiling is 0.17% of
  $. **This is a dead end.**

## Verdict

1. **Token shaping is not a bigger realisable lever than call saving on these repositories.**
   One call is worth about 3% of $. Honest learned shaping is worth −2% to +4%, and the ceiling
   of any window a hook can place is 4.5% (Haiku) or 2.3% (Sonnet). The large pool (29% of $ in
   unreferenced tool-output lines, 21% in whole-file Reads) is real, but it is *not addressable
   by position or by what the hook knows*. Referenced lines are scattered through the file.
2. **The only harm-free dollar levers found:**
   - the cache TTL: 11–12%, a harness/config setting rather than a hook, which is the single
     biggest item;
   - re-read dedupe: +1.0% (Haiku) and +0.4% (Sonnet), about a third of a saved call.
3. **The realistic path to smaller contexts is not trimming.** It is reading less by choice, a
   behaviour change that only a live A/B can measure. Options include delegating exploration to
   a sub-agent, whose context is discarded (27.5% of Haiku $ is first-third tool results carried
   to the end), or targeted reads suggested *before* the read. The GREP windows here were rarely
   chosen, because an earlier grep named lines of the file before only 7.4% (22 of 298) of Haiku's
   whole-file reads of 40 or more lines.

## What to build

- **Configuration, not a reflex.** Where the harness lets you choose, use the 5-minute cache TTL
  for interactive sessions whose inter-call gaps stay under 5 minutes (p99.9 here was 124 s).
  That is −11.7% (Haiku) and −10.9% (Sonnet) of $ at zero behaviour risk. I did not verify
  whether or how Claude Code exposes the TTL choice, so check that before promising it.
- **Rust, small:** a PreToolUse Read dedupe. Track (path → shown line ranges, invalidated by
  Edit/Write or a Bash command that touches the path). When a Read would only re-show unchanged
  lines, answer with `updatedInput` narrowing the range to the new lines, or deny with a stub
  ("lines a–b of f are unchanged since call k"). Expected: +$0.0025 per session (Haiku), +1.0% of
  $ [0.6, 1.4], with no hand-written lists.
- **Not recommended:** blind output trimming (head/tail/budget caps, Drain filters) and a stop
  signal. If either is tried, try it only as a live micro-A/B. The candidate is the
  PreToolUse-only learned policy with elision notes and parked full output, on about 40
  sessions per arm on attrs and click; it would settle the loose-versus-strict question directly.
  Measure the $ per task and the pass rate.

## Hard-coded parts (honest list)

- `normalize.py` (lab, hand-written) supplies the command `key`/`template` used as context keys.
- The usage measure's tokeniser regex, the ±10/30-line region model and the 40-token elision note
  are fixed instruments.
- The Edit/Write tool set (`EDIT_TOOLS`) defines "edit" in `done.py` and invalidates dedupe.
- Number masking for Drain.
- The measurement buckets in `describe.py` and the regexes in `refetch.py` are instruments only.
- The action grids (H, T, w values) are fixed candidate sets; the choice among them is learned.

## Files

- `parse.py` reads the transcripts into per-call usage and tool-result records
  (`/tmp/adage-research/token-cost/sessions.pkl`).
- `usage.py` is the learned data-flow "was it used" annotation. `anchors.py` gives the anchor
  lines known at hook time.
- `common.py` holds pricing, the carry model, the harness-aligned splits and the clustered
  bootstrap.
- `describe.py` gives the cost anatomy; `levers.py` covers the TTL, dedupe and $ per call.
- `evaluate.py` is the headline shaping evaluation (families, harm models, TTLs, LORO,
  ceilings), writing `evaluate_*.json`.
- `policies.py` is the first positional-only pass (session-level CIs, 176-session set).
  `drainfilter.py` is the Drain template filter.
- `refetch.py` holds the natural-experiment re-fetch rates. `done.py` holds the done and
  redundant re-run signals.
