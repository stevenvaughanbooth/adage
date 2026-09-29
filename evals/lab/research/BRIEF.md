# Research sprint brief: find the lever

2026-09-29. This is shared by every research agent in this sprint, so read all of it before you start.

## The mission

Adage turns agent traces (Claude Code, Codex and OpenCode sessions) into **reflexes**: learned
responses that a hook delivers to the agent at the right moment. Today it saves roughly 1–2.4 tool
calls per affected session on real Python repositories. Most of that comes from one situation:
using the project `.venv` instead of system Python. This is an experiment. We are looking for the
lever that makes it a brilliant product: large, general savings in calls, tokens and dollars, or
higher task success, on **any repository**, learned automatically.

## North star: what counts as finding it

We are searching for **the missing piece that drives value and gets better results**. Everything
you do must answer one question: **would this, shipped, make a user's agent measurably cheaper or
measurably more successful on a repository it has worked in before, or on a new one?**

- **The target metrics** are dollars per task (tokens under the cumulative-context cost model),
  tool calls per task, and pass rate on hidden tests, all on held-out work.
- **Proxies such as next-action accuracy, cluster purity, F1 and recall@k are instruments, not
  results.** Always translate a proxy into the target metrics: "recall@5 = 0.8 → X calls, $Y
  saved, Z pass-rate points". A proxy win that doesn't translate is a negative result, so report
  it as one.
- **Size matters.** The current system saves about 1–2 calls on sessions of 27–38 calls, which is
  roughly 5%. The missing piece should plausibly reach **≥ 15–20% cost reduction or a
  pass-rate gain of several points**, or be a clear step toward it. If your lever's oracle
  ceiling is small, say so early and spend your time on the part of your area with the biggest
  ceiling.
- **Every result must end with the "so what" line:** the expected $/calls/pass-rate effect if
  shipped, its CI, and how confident you are that it survives a live A/B.

## Principles (non-negotiable, set by the product owner)

1. **Nothing hard-coded.** Don't match against lists of tool names, error strings or regexes
   written by hand. The learner must discover structure from the traces: equivalence of actions,
   success and failure, what a situation is, and what helps. Hand-written features may be used
   **only as a measurement instrument or a baseline to beat**, and you must say so when you use
   them. Today's hand-written parts include `crates/adage-core/src/action.rs` (a taxonomy of
   tool names), the signature lists in `evals/replay/README.md` and the U11 `.venv` instinct.
2. **Pattern-match roughly the same behaviour, not exact matches.** Use similarity, embeddings,
   learned equivalence and abstraction.
3. **Optimise outcomes, not frequency.** A reflex is worth shipping only if it changes what
   happens for the better: fewer calls, tokens or dollars, or more tasks passed, net of what it
   costs to carry and the harm when it misfires.
4. **Prove the primitive empirically, then apply it.** Every claim needs a held-out number with a
   confidence interval: a 90% bootstrap over sessions, or Wilson intervals for rates. Train on the
   training split and report on held-out sessions or tasks. Where you can, also report
   **leave-one-repo-out**: a new repository is the real test of "any repo".
5. **Rely on existing technology.** That means local embedding models (potion-base-8M/32M, and
   potion-code-16M if you can get it; bge-small and MiniLM through onnxruntime are fine offline),
   classic algorithms (process mining, Drain log templates, PrefixSpan, MDL, kNN, BM25, TD
   learning, contextual bandits, off-policy evaluation), and the literature. Don't invent where a
   known method exists.
6. **Fast inner loop.** Everything here is offline, over traces already on disk, and costs $0.
   Only the data-expansion agent spends money on live sessions.

## Data (read-only, never modify)

- **RepoBench v1** is at `/tmp/adage-repobench/runs/<repo>/`, and **v2** at `/tmp/adage-rb2/runs/<repo>/`.
  - Repos: attrs, click, marshmallow, vue-core and urfave-cli (v1), plus click-sonnet5 and
    vue-core-sonnet5 (v2, Sonnet).
  - The transcripts are under `private/transcripts/**.jsonl` (Claude Code JSONL).
  - Outcome is in each run's `state.json`: hidden tests passed or not.
  - Arms: `control` has hooks on and directives withheld; `adage` has directives delivered;
    `agents-md` has an AGENTS.md baseline; `train` holds the training sessions.
  - Sessions from v1 and v2 overlap, so de-duplicate by session id.
- **The split** is in `evals/repobench/splits`, with tasks in `evals/repobench/tasks`. Train on
  `train`, test on held-out **control** sessions. `adage`-arm sessions can be used for uptake and
  off-policy questions.
- **Existing Python lab code to reuse:**
  - `evals/lab/`: `traces.py`, `situations.py`, `metrics.py`, `embed.py`, `normalize.py`,
    `predict.py`, `bench_knn.py`. Read `evals/lab/README.md` for its verdict and numbers, and
    don't repeat what it already showed.
  - `evals/lab/abstraction/` (the action grammar lab).
  - `evals/replay/` (the calibrated replay metric Ŝ, which matches live results).
- **Reports to read first:**
  - `evals/launch/architecture-review.md`;
  - `evals/launch/outcome-optimisation.md`;
  - `evals/launch/learner-diagnosis.md`;
  - `evals/repobench/narrative-v2.md`.
- **Models:** fetch them with `evals/lab/fetch_models.sh` into `ADAGE_LAB_MODELS=/tmp/adage-lab-models`.
  They are shared by all agents, so check whether they're already there before fetching. Hugging
  Face is blocked; npm and the qdrant GCS bucket work.

## Machine limits (shared with about 20 other agents)

- **CPU:** 4 cores. Run Python with `nice -n 10` and `OMP_NUM_THREADS=1`. Keep single experiments
  under about 10 CPU-minutes, and cache intermediate results.
- **Disk:** only a few GB are free. Put caches in `/tmp/adage-research/<your-slug>/`, keep them
  under 500 MB, and delete what you don't need. **Don't build the Rust workspace.** If you need
  the `adage` binary, use an existing one: `ls /home/user/adage/target/*/adage`.
- **Python:** numpy, scipy, scikit-learn and onnxruntime are available, torch is not. You may
  `pip install --user` small pure-Python packages you need, such as `drain3` or `prefixspan`.
- **Git:** don't commit. Other agents are editing the Rust crates concurrently; don't touch
  `crates/`.

## Deliverable

1. Write everything under `evals/lab/research/<your-slug>/`:
   - the code;
   - a `README.md` with the question, the method, the held-out results table with CIs, the
     verdict and what to build;
   - `result.json`.
2. Put these fields in `result.json`:
   - `slug`;
   - `headline`: one sentence;
   - `metric_name`;
   - `held_out`: the value, the CI and the number of sessions;
   - `leave_one_repo_out`, if applicable;
   - `vs_baseline`: compare against the current state, which is U11 plus the learned reflexes,
     about +1.0 Ŝ calls/session post-hoc and +2.11 pre-emptive;
   - `hard_coded_parts`: an honest list;
   - `ceiling`: an oracle upper bound for your lever;
   - `build_recommendation`: what to implement in Rust, and its expected value.
3. A negative result, clearly shown, is valuable. A hyped result will be caught: an adversarial
   review round re-runs every claim.
