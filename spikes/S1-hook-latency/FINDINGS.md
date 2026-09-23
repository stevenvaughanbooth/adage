# S1 · Hook latency — Findings

**Question:** Can a hook client — spawned by the harness for every event — start, read its stdin
payload, make one local round trip, and exit inside the budget on macOS?

**Machine:** Apple M3 Pro, macOS 25.6.0, Rust 1.98.1, Node v26.5.1. **Other agents were running
concurrently on this machine for the whole measurement window** (`load averages` climbed from
9.78/7.12/4.89 at the start of the final run to 37.52/14.99/8.20 at the end — see
`results/uptime.log` and the `uptime_before`/`uptime_after` field in every `results/*.json`). The
numbers below are therefore a **worst-case-ish, not best-case**, reading. The coordinator should
rerun `./rerun.sh` on a quiet machine to confirm the margin is even larger there; given how far
under budget every core scenario already came in, a quiet rerun is expected to confirm the pass,
not change it.

## Results (n per scenario in table; all times ms, wall clock spawn→exit)

| Scenario | n | p50 | p95 | p99 | max | mean |
| --- | --- | --- | --- | --- | --- | --- |
| A. spawn + exit only (floor) | 3000 | 1.670 | 2.143 | 3.134 | 6.712 | 1.744 |
| B. + read 2 KB stdin | 3000 | 1.625 | 1.991 | 2.279 | 3.348 | 1.666 |
| C. + read 300 KB stdin | 3000 | 1.685 | 2.131 | 2.587 | 5.590 | 1.747 |
| D. + socket round trip, server warm | 3000 | 1.688 | 2.163 | 2.834 | 6.463 | 1.757 |
| E. + socket connect, missing socket (fail-open) | 3000 | 1.640 | 2.067 | 2.502 | 4.029 | 1.697 |
| F. same as D, shell form (`/bin/sh -c`) | 3000 | 5.166 | 10.402 | 20.199 | 60.126 | 5.945 |
| G. fresh, never-executed binary copy | 300 | 119.4 | 172.0 | 294.7 | 352.5 | 127.3 |
| Node client, 2 KB stdin (comparison) | 1000 | 35.08 | 53.95 | 82.29 | 106.99 | 39.35 |
| Node client, socket round trip (comparison) | 1000 | 45.97 | 82.60 | 132.13 | 347.06 | 50.06 |

Raw samples and per-scenario stats: `results/*.json`. Combined table: `results/summary.csv` /
`results/summary.json`. Reproduce with `./rerun.sh` (builds release binaries, starts the echo
server, runs every scenario, records `uptime` before/after each scenario and the whole run).

## Verdict against pass/fail

Budgets from SPIKE.md: **D ≤ 5 ms p95 and C ≤ 7 ms p95 → pass.**

- D (socket round trip, warm server): **1.688 ms p50 / 2.163 ms p95** — passes with a >2x margin
  even while the machine was under heavy concurrent load.
- C (300 KB stdin): **1.685 ms p50 / 2.131 ms p95** — passes with a >3x margin.

**Verdict: PASS.** Command hooks + a Unix socket are viable for v1 on macOS. All of A–E (exec-form
invocation of the static Rust binary, with or without a socket hop, with or without a large
payload, including the fail-open path to a missing socket) land in the same tight band —
**p50 ≈ 1.6–1.7 ms, p95 ≈ 2.0–2.2 ms** — comfortably inside every tier of the §9 performance
budget (no-match ≤10 ms, structured ≤15 ms, prompt-with-intent ≤25 ms), with room to spare for T1
lookup, translate, and render on the daemon side. Reading 300 KB of stdin costs no more than
reading 2 KB, confirming the "stream, don't buffer-parse" approach (raw byte-scan for two field
markers, no JSON deserialize) keeps the large-payload case cheap. The missing-socket path (E) is
just as fast as the warm one (D) because `connect(2)` on a nonexistent path fails immediately
(`ENOENT`) rather than blocking — the fail-open design costs nothing extra.

## Recommendation for A3 (transport)

**Command hooks with a Unix socket, invoked in exec form, are sufficient for v1.** `http` hooks are
not needed to hit the budget — the spike's stated reason for considering them (spawn cost is
already near the 10 ms floor) does not hold: spawn + stdio + socket round trip together cost
~1.7–2.2 ms p95, an order of magnitude under budget. Recommend closing the `http`-hooks option for
v1 and revisiting only if T1/T2 daemon-side costs (not measured here) erode the margin.

One operational finding for A3/installer work: **hooks.json must invoke the binary directly
(exec form), never `sh -c "..."` (shell form).** Shell form (F) roughly **3x'd p50 and 5x'd p95**
versus exec form (D), and its tail is far worse (p99 20.2 ms vs 2.8 ms, max 60 ms vs 6.5 ms) — the
extra `fork+exec` of `/bin/sh` plus its own dynamic-linker and shell-startup work adds real,
variable cost. This is a plain configuration detail to get right in the plugin's `hooks.json` and
Codex's installer, not an architecture change.

## Recommendation for A2 (Rust) — reconfirmed, and Node is disqualified

The Node-client comparison answers the spike's item 4 directly: a Node/TS hook client costs
**35 ms p50 / 54–83 ms p95** just to start the runtime, read stdin, and (for the socket variant)
round-trip — **already over the 25 ms "prompt with intent" budget on p50 alone**, and 3–8x over
every budget tier on p95/p99, before any real work happens. The Rust static binary stays the
right choice for the hook client; a Node-based client is not viable at any tier of this budget.

## Surprise: first-run code-signing/Gatekeeper cost is large and needs mitigation (G)

This was the biggest and most consequential finding. A binary that has never been executed before
(a fresh, uniquely-named copy on the same volume — same content, different inode) costs
**119 ms p50 / 172 ms p95 / up to 350 ms** to spawn and exit, vs ~1.7 ms for a binary already
warmed up. A follow-up isolated test (see transcript notes) confirms this is a **per-inode,
first-execution** cost, not something that decays over a session: the *same* fresh copy went from
**266 ms** (execution 1) to **8.5 ms** (execution 2) to **3.7 ms** (execution 3). This matches
macOS's on-first-launch code-signature/Gatekeeper validation for an ad-hoc-signed, linker-signed
binary (confirmed via `codesign -dv`: `flags=0x20002(adhoc,linker-signed)`), which the kernel/AMFI
apparently re-validates the first time a *specific file* (inode) is executed, independent of
content already seen elsewhere.

This is two to three orders of magnitude over every budget tier, and it will hit **every plugin
update** (F4 in the architecture review: `${CLAUDE_PLUGIN_ROOT}` changes on every update, and the
verified binary is installed fresh into the plugin data directory by digest) and **every fresh
machine setup**. The SPIKE.md mitigation is necessary, not optional: **install the binary and
execute it once (a real invocation, e.g. `adage hook claude SessionStart` against a locally-bound
throwaway/no-op path) at plugin `SessionStart` or at install time, off the hot path**, so the
first real per-event hook call never pays this cost. This should be added to A3/A8 as a hard
requirement, not left as a "nice to have."

## What I could not do

- No access to another physical Mac, so the "rerun on a quiet machine" step is left for the
  coordinator to execute via `./rerun.sh`; this run's own numbers already show wide margins under
  a genuinely loaded machine (load average reached 37.5 during the run), so I don't expect the
  verdict to change, only to firm up.
- Did not measure T1/T2 (daemon-side backoff lookup, prompt embedding) — those are out of scope for
  S1 per the SPIKE brief (S2 covers embeddings) and are separate, already-budgeted stages in §9.
- Did not measure Linux/systemd `--user` socket activation cold-start, cgroups, or Codex's actual
  installer path — S1's brief and budget are macOS/Claude-Code-shaped; the client/server code is
  portable (std-only, Unix sockets), but no Linux measurement was taken here.
