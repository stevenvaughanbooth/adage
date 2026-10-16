# S3 · Daemonless session state — Findings

**Verdict: pass.** Per-session state files and append-only spools replace a daemon's memory
correctly and faster than a daemon round trip — provided the state file uses the **two-slot** design
and is treated as a cache over the spool.

> Written by the coordinator from the spike's raw results (`results/`), after the spike agent was
> unable to write this file itself. Every number below is copied from `results/summary.md`,
> `results/correctness-full.json`, `results/stalls.txt`, `results/disk-compression.json`, and
> `results/loc.txt`. Reproduce everything with `./run.sh` (`./run.sh quick` for a smoke check).

## Environment

Apple M3 Pro (5P + 6E), 18 GB, macOS 26.6.2, rustc 1.98.1. **Other spike agents were running
concurrently**; one-minute load average ranged from about 6 to 28 during the runs. Treat absolute
latencies as upper bounds.

## What was built

A Rust prototype (`src/`) modelling real Claude Code hook traffic: `SessionStart`,
`UserPromptSubmit`, 8 parallel `PostToolUse` per batch, `PostToolBatch`, sub-agent start/stop,
`Stop`, `SessionEnd`. Three backends:

| Backend | State storage | Code (lines) |
| --- | --- | --- |
| `slots` | Two 8 KiB slots in one file, each CRC-checked; write the older slot with one `pwrite` under `flock`; no `fsync` | `state.rs` 282, `store.rs` 299 |
| `rename` | Write a temp file, then rename over the state file | (same modules) |
| `daemon` | Minimal Unix-socket daemon holding state in memory | `daemon.rs` 132 |

The spool (`spool.rs`, 293 lines) is CRC-framed records appended with one `O_APPEND` write each.

## Latency

Paced traffic, µs. *Decide* is the full decision path: lock, load state, fold spool records since
the state's watermark, append the decision record, store state. *State access* is lock + load +
store only.

### One process per hook (the realistic shape)

| Backend | Sessions | Decide p50 / p95 / p99 | State access p95 |
| --- | ---: | --- | ---: |
| slots | 1 | 270 / **539** / 679 | 169 |
| slots | 4 | 141 / **352** / 639 | 120 |
| slots | 16 | 154 / **538** / 1,524 | 159 |
| rename | 1 | 697 / 1,172 / 1,377 | 774 |
| rename | 16 | 312 / 877 / 1,800 | 681 |

### Minimal daemon, for comparison

| Sessions | Decide (connect + round trip) p50 / p95 / p99 |
| ---: | --- |
| 1 | 134 / 607 / 877 |
| 4 | 94 / 796 / 2,710 |
| 16 | 100 / 495 / 1,108 |

The daemonless `slots` design matches or beats a daemon round trip at every concurrency level, and
adds well under 1 ms to the ~2 ms hook spawn cost measured in S1.

### Stress (back-to-back, no pacing)

`slots` holds its p95 (in-process 16 sessions: 1,220 µs; one process per hook: 477 µs) but the
in-process driver's **p99 reached 104 ms**, from 50–480 ms `open()`/`write()` stalls that hit all
threads at once. The stall census (`results/stalls.txt`): **95 appends over 10 ms at ~16,000
events/s; 0 at ~2,900 events/s.** Realistic hook traffic is far below the stall regime and the
separate-process runs never showed it. The cause is unconfirmed (plausibly filesystem journal
contention under saturation); `rename` degrades much worse under stress (16 sessions: 6.1 ms p95).

### Durability options (paced)

| Option | 1 session decide p95 | 16 sessions decide p95 |
| --- | ---: | ---: |
| no `fsync` (recommended) | 339 | 830 |
| `fsync` | 1,564 | 582 |
| `F_FULLFSYNC` | 5,559 | 20,986 |

`F_FULLFSYNC` is far outside the budget. Since the state file is a cache rebuildable from the spool,
no `fsync` is needed on the hot path.

## Correctness — 13/13 tests pass

| Test | Result |
| --- | --- |
| **Concurrent spool writers** — threads (200 cases) and processes (60 cases), records up to 65 KB | 1,054,264 thread records + 229,467 process records; **no loss, duplication, or tearing** |
| **Ordering reconstruction** from the spool, observations delayed up to 0, 1, and 5 decisions | **2,000 / 2,000** sessions reconstructed exactly at every lag; 12,710 / 12,710 injected duplicates detected |
| **Live-state convergence** (state file equals the reference without a rebuild) | lag 0: 2,000 / 2,000 · lag 1: 2,000 / 2,000 · **lag 5: 0 / 2,000** |
| **kill -9 on state writes** — 2,000 kills each | `slots`: 0 torn, 0 regressed · `rename`: 0 torn, 322 stale temp files · unsafe in-place control: **448 torn** |
| **kill -9 mid-decision** — 1,000 kills each | Decision sequence contiguous; state after replay equals a full rebuild; 0 corrupt regions |
| **kill -9 on spool writers** — 1,800 writers killed | 0 corrupt regions, 0 gaps or duplicates |
| **Torn-frame injection** — 36,915 damaged frames | 186,134 records recovered, **0 false records** |
| **Concurrent reader** during writes | Always saw a complete prefix |

### The one qualification: late observations

When asynchronous observations arrive **more than one decision late**, the live state file no longer
matches the reference, although the spool still reconstructs perfectly. In other words: *the spool is
the truth; the state file is a cache that is exact only when observations are prompt.*

This matters less than it looks, because of S5: Claude Code's synchronous `PostToolBatch` payload
already carries every tool call and response in the batch. **Decisions should read the batch they are
deciding on from the synchronous payload**, and use the state file only for older history. Async
observations then affect only history older than the current batch, where a late arrival changes a
learning input — which `adage learn` rebuilds from the spool anyway — not the decision itself.

## Disk

16 sessions × 32 turns, 17,067 events: spool **240 KiB per 1,000 events** (158 KiB with zstd-19),
versus 411 KiB as JSON; state image 1.2 KB per session; rebuilding a session's state from its spool
takes **0.7 ms** (p50 700 µs, max 815 µs).

## Recommendations

**A1 — no daemon for session state or single-writer duty.** Hooks read and write per-session state
files and spools directly. The only remaining argument for a resident process is a warm transformer
embedding model (S2); if one is ever needed, it should be a stateless embed server that holds no
session state.

**A5 — hooks never touch SQLite.** Hooks write only per-session spools and state files. `adage learn`
is the single SQLite writer and ingests spools. This also resolves review finding F6.

**Formats:**

- **State:** two 8 KiB slots with CRC, one `pwrite` into the older slot under `flock`, no `fsync`.
  Treated as a cache with a spool watermark; rebuilt from the spool on any CRC failure or version
  mismatch.
- **Spool:** one file per session, CRC-framed records, one `O_APPEND` write per record. Decision
  records carry a sequence number; `PostToolBatch` records carry the ordered tool-use ids, so order is
  reconstructed from ids, never from arrival time.
- **Decisions** fold spool records newer than the state's watermark, and take the current batch from
  the synchronous hook payload.

## Not done

- Linux measurements (`flock`, `O_APPEND`, and the stall behaviour may differ).
- Real Claude Code traffic — the traffic model is synthetic. (S5 has since confirmed `PostToolBatch`
  carries the tool calls.)
- Physical power-loss testing.
- The fast-evidence / demotion overlay.
- Root cause of the stress-regime stalls.
