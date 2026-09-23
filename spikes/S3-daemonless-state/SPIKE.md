# S3 · Daemonless session state

**Informs:** A1 (daemon), A5 (storage) · **Wave:** 1 · **Budget:** one day, no tokens

## Question

Can per-session **state files** guarded by `flock`, plus append-only per-session **spools**, replace
a resident daemon's in-memory session state — correctly under concurrency and fast enough for the
hot path?

## Why it matters

The review found the daemon is mainly justified by (1) a warm embedding model, (2) session state
across hook calls, and (3) a single writer. S2 tests (1). This spike tests (2) and (3). If both fall,
Adage needs no resident process at all: hooks are short-lived processes over files, and learning is
a batch job.

## Hypothesis

A per-session state file (small, binary or compact JSON) read-modify-written under an exclusive
`flock` costs well under 1 ms. Append-only spool writes with `O_APPEND` of records < `PIPE_BUF` are
atomic across concurrent writers. Observation hooks that run **async** can do their writes off the
critical path.

## Method

Rust prototype in this folder:

1. **Model the real hook traffic** of a Claude Code session: `SessionStart`, `UserPromptSubmit`,
   parallel `PostToolUse` ×N arriving concurrently, one `PostToolBatch`, sub-agent start/stop with
   their own agent ids under the same session, `Stop`, `SessionEnd`.
2. **State design:** `state/<session>.bin` holding the fields in Architecture §4.4 (history ring,
   carried intent, pending directives, obligations, arm, counters), updated under `flock`;
   `spool/<session>.log` append-only for every observation.
3. **Decision path:** a `decide` invocation reads state (shared lock), computes a dummy decision,
   writes back (exclusive lock) — measure latency p50/p95/p99 with 1, 4, 16 concurrent sessions and
   8 parallel tool events per batch.
4. **Correctness tests:**
   - concurrent `PostToolUse` writers never lose or tear records (property test with many threads/
     processes; verify every record present exactly once)
   - batch ordering can be reconstructed deterministically from the spool (by batch/turn/tool-use
     id, not arrival order)
   - crash safety: kill -9 mid-write; state is either old or new, never torn (write-to-temp +
     rename, or checksummed records)
   - a `learn`-style reader ingesting spools concurrently with writers sees a consistent prefix
5. **Compare** against a minimal resident daemon (Unix socket, state in memory) on the same
   workload for latency and code complexity.
6. Measure disk usage per 1,000-event session and a retention/compaction approach.

## Pass / fail

- **Daemonless viable** if decision-path state access is ≤ 1 ms p95 at 16 concurrent sessions, no
  lost or torn records under the property tests, and crash safety holds.
- Otherwise document which property fails and what a daemon buys.

## Deliverables

`FINDINGS.md` with latency tables, correctness results, a recommendation for A1 and A5, and the
state/spool format proposal.
