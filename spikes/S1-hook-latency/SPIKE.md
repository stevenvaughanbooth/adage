# S1 · Hook latency

**Informs:** A2 (Rust single binary), A3 (hook transport) · **Wave:** 1 · **Budget:** half a day, no tokens

## Question

Can a hook client — spawned by the harness for every event — start, read its stdin payload, make one
local round trip, and exit inside the budget on macOS?

Budgets (end to end, p95): no match ≤10 ms · structured match ≤15 ms · prompt with intent ≤25 ms.

## Why it matters

Every command hook spawns a process. If spawn plus stdin plus socket is already near 10 ms, the design
needs Claude Code `http` hooks (no spawn) or in-process plugins, and the "dumb client + daemon"
architecture changes shape.

## Hypothesis

A small static Rust binary, invoked in exec form (no `/bin/sh`), starts in under 3 ms p95 on Apple
silicon; a Unix-socket round trip adds under 0.5 ms; reading a large stdin payload (hundreds of KB)
adds a measurable but bounded cost that streaming can avoid.

## Method

1. Build a minimal Rust hook client (`client/`) that: reads stdin (stream, don't buffer-parse),
   extracts a couple of JSON fields, optionally connects to a Unix socket, sends a small request,
   reads a reply, prints it, exits 0. Build release, stripped, with LTO.
2. Build a trivial Rust echo server on a Unix socket (`server/`).
3. Harness (`bench/`) that spawns the client N ≥ 2,000 times per scenario, the way a harness would
   (`posix_spawn` / `child_process.spawn` from Node — Claude Code is a Node app), and records
   wall-clock from spawn to exit:
   - A. spawn + exit only (floor)
   - B. + read 2 KB stdin
   - C. + read 300 KB stdin (typical large `PostToolBatch`)
   - D. + socket round trip, server warm
   - E. + socket connect to a missing socket (fail-open path)
   - F. same as D but spawned via `/bin/sh -c` (shell form) for comparison
   - G. a freshly written, never-executed binary copy (first-run code-signing / Gatekeeper cost)
4. Also measure a Node-based client for comparison (what a TS client would cost).
5. Report p50/p95/p99 per scenario. Note machine load; rerun the key scenarios if other work was
   running concurrently.

## Pass / fail

- **Pass:** D ≤ 5 ms p95 and C ≤ 7 ms p95 → command hooks + socket are viable for v1.
- **Marginal:** D in 5–10 ms → viable only with very cheap daemon work; recommend `http` hooks for
  Claude Code.
- **Fail:** D > 10 ms p95 → command hooks cannot meet the no-match budget; switch transport.

## Deliverables

`FINDINGS.md` with the table of percentiles, the verdict, and a recommendation for A3 (and whether
the first-run signing cost in G needs mitigation, e.g. installing and pre-running the binary at
`SessionStart`).
