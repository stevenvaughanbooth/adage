# Track G: Graph and prediction engine v2

> **Status (2026-09-27): G1 core implemented, not yet integrated.** `crates/adage-graph` (`input.rs`, `flow.rs`, `types/`), `adage-core/src/tokens.rs` and `adage-core/src/vtype.rs`, `evals/gen` `id_shape` and `specs/idshapes.json`, and `adage eval types`. Nothing learned is used by `adage learn`, tracks or the runtime yet (G1 integration: `SlotSource`s, `io.rs:214`, `bind_prompt_route`, `bridge.rs`). On `specs/idshapes.json` (11 bundles, rehearsal transcripts): learned lookup recall 1.00 on every shape, accept 1.00, reject 1.00, unsafe 0; today's hard-coded detector: recall 0 on `hash_num`, `uuid`, `path_num` and `branch`, and half the ids of `mixed`. Deviations from this design, decided while building it: (1) the 2^64 language cap exempts **rigid** types (only fixed-length digit or hex runs and literals, at least two punctuation literals), or no UUID (2^122) could ever be a type; bare letter runs (`L{…}`, `U{…}`, `W{…}`, short `H`) are rejected as word-like whatever their size; (2) when both star folding and a union apply, the cheaper by MDL wins (folding first would turn two trackers `BIL-1041` and `#482` into `(BIL|482|…)(-D){0,1}`); (3) when the MDL choice accepts a negative, the least general type (literal sets where ≤ 4) is tried before rejecting; (4) positives outside `plain_token` are moved to the negatives rather than failing the slot; (5) the shared tokeniser also splits on the other shell metacharacters (`$&|*!?{}\~^%`) and trims a trailing `.` or `:`; (6) `adage-graph` depends only on `adage-core` for now.
>
> Status: design, 2026-09-27. Track G of the maturity roadmap ([roadmap](../roadmap/2026-09-maturity-roadmap.html)). The learner stops knowing route kinds: sessions become event graphs whose nodes carry embeddings, and learning mines frequent, discriminative subgraphs that become reflexes.

## 0. Decisions in brief

1. **The learner stops knowing route kinds.** Each session becomes a small typed event graph. Nodes carry embeddings. Learning finds frequent, discriminative, closed subgraphs over **prototype labels**, meaning nodes clustered by embedding for each node type. Each selected subgraph is a **reflex**: its early nodes are the context (trigger), its later nodes are the continuation, and its value-flow edges are typed inputs.
2. **The algorithm is target-rooted, gap-bounded sequential episode mining over temporal DAGs.** It is PrefixSpan-style pattern growth, extended with value-flow and same-artifact constraint edges. It keeps closed patterns only (BIDE-style checks). An SQS/Krimp-style MDL cover chooses among overlapping patterns. It is not gSpan, CloseGraph or GraMi (§3.2 says why).
3. **Kinds survive only at the output boundary.** A deterministic compiler maps each reflex onto a closed set of **delivery shapes**: `run`, `avoid`, `read-batch`, `obligation`, `sequence`, `advise`. Each shape has a fixed template, typed slot contracts, effect ceilings and interlocking. `route_kind`, `steers`, `KIND_*` and `MINER_KINDS` (`crates/adage-tracks/src/model.rs:209-239`, `crates/adage-learn/src/import.rs:61-100`) are removed from the learner.
4. **Value-flow induction is the first deliverable.** It replaces the three hand-written ID detectors that disagree with each other: `spans.rs` TICKET `[A-Z][A-Z0-9]{0,9}-\d{1,7}`, `template.rs` TICKET_RE `[A-Z]{1,6}-\d+`, and the normaliser's `{ticket}` rule. In their place are learned, anchored, bounded value types.
5. **Determinism boundary.** Learning is statistical but a pure function of (sessions, prototype epoch, params). Embeddings are static (potion-base-8M plus feature hashing), kNN is exact, Louvain is deterministic, and every ordering is by content-derived ids. The hook never embeds anything except the prompt, which it already does for intent. Everything else at runtime is interned-symbol lookups and a bounded suffix-trie walk.

---

## 1. Event-graph substrate

### 1.1 Per-session event DAG (instance level)

A session is a DAG `G_s = (V_s, E_s)`. Time totally orders batches. Calls inside one model turn (`StepObs.turn`) are unordered with respect to each other, so a batch is an itemset.

| Node type | Instance content | Label levels (fine → coarse) |
|---|---|---|
| `Prompt` | normalised text, token list, prompt index | exact-normalised → **intent prototype** → `prompt` |
| `Act` | canonical action and literal argv, effect, ok/fail, batch | Exact template key (value types substituted) → **act prototype** → HeadSub identity → Head (`pattern::Level`) |
| `Art` | repo-relative path | file → **artifact prototype** → compiled glob (`dir/*.ext`, `p/**/*.ext`) → extension |
| `Obs` | output shape: JSON key set, lists-paths flag, line-class histogram, first 256 normalised bytes | exact shape hash → **obs prototype** |
| `Fail` | `signature_of` line (mine.rs:223) and normalised error text | exact signature → **failure family** (Design §8.2.3) |
| `Outcome` | success / failure / unknown | exact |
| `Ctx` | repo, harness, model, day | partition keys and weights only, never pattern items |

| Edge | Meaning | Attributes |
|---|---|---|
| `follows(a→b)` | temporal order | gap in alphabet events, same-batch flag, same-prompt-segment flag |
| `binds(src→Act.slot)` | value-flow: a token from a prompt, an earlier output field, or an earlier path segment reappears in argv | sink (identity, position or option, affix), source kind, value type |
| `reads` / `writes(Act→Art)` | file access | — |
| `yields(Act→Obs)`, `fails_with(Act→Fail)` | results | — |
| `recovers_with(Fail→Act)` | next *successful* act with the same effect class within 5 events and a different key | the rule mine.rs:492-498 encodes today |
| `ends(→Outcome)` | terminal | — |

`co_occurs` is not stored as an edge. It is computed at the type level, as session-level PMI, where the scorer needs it (§5).

**Import gap to fix first.** `Session` keeps only the first prompt's text (`import.rs:40`, `prompts: u32` is a count), and `StepObs` does not record which prompt it served. G0 adds `prompts: Vec<PromptObs { text, at_step }>` and `StepObs.prompt: u16` to the Claude Code, Codex and OpenCode importers. The parse-cache stamp (`cache.rs`) invalidates old entries automatically. Sessions are then split into **prompt segments**. Patterns may span segments only through an explicit `Prompt` item at their start.

### 1.2 Node embeddings

All embeddings are unit vectors and are quantised to `i16` with `adage_intent::quantize` before any comparison that affects a decision.

- **Prompt:** the existing potion-base-8M embedding of `normalize()` text (`crates/adage-intent/src/normalize.rs`). After G1, learned value types add a substitution pass (`{t:<type-id>}`). The normaliser version becomes part of the model id, so centroids never cross versions.
- **Act (structural + lexical, 384-d):**
  - 256 dimensions of signed feature hashing (fixed seed, SHA-256-derived) over: program, identity, identity⊕option-name for each option, template token uni- and bigrams, positional count and slot types, effect class.
  - 128 dimensions of potion embedding of the identity words split on `/._-` and camelCase, so `npm run check` and `make check` land near each other.
  - Halves are weighted 0.75/0.25, then renormalised.
- **Art (256-d):**
  - hashed features: every directory prefix (weighted by depth), extension, stem tokens, sibling-manifest flags (`package.json`, `Cargo.toml` or `pyproject.toml` in an ancestor, so package roots are features);
  - plus potion embedding of the segment words.
- **Obs / Fail:** potion embedding of the normalised text. Paths, numbers, hex and temporary names become placeholders, as §8.2.3 prescribes. Used offline only.

### 1.3 Storage, incremental build, bounds

The memoisation is hand-rolled and salsa-like, extending the existing parse cache (`crates/adage-learn/src/cache.rs`). Every stage is a pure function keyed by `sha256(stage_version ‖ input hashes)`:

```
S1 parse (existing cache) → S2 session facts → S3 repo label tables
→ S4 prototypes (epoch) → S5 value types → S6 mining → S7 scoring/selection → S8 compile
```

- **S2 `SessionFacts`** is stored at `$ADAGE_HOME/cache/facts/<hash>.json`, owner-only as the parse cache is. It holds the event list, the edges, raw embeddings of the session's distinct nodes, and token occurrences. Limits: ≤ 2,000 events, ≤ 512 token occurrences, ≤ 64 KiB of output per event.
- **S3** holds per-repo interned node tables: distinct nodes, the sessions and days each appears in, and embeddings. They update by exact add/subtract of per-session contributions.
- **Bounds per repo:** ≤ 50k distinct nodes (older than 90 days with fewer than 2 sessions: evicted), ≤ 4,096 prototype labels, ≤ 20k mined closed patterns before selection, ≤ 256 compiled routes.

**Determinism rules:**
- `BTreeMap` only.
- Reductions are partitioned by session and folded in session-id order, so results do not depend on thread count. Checked the way the bench checks 1 vs 4 jobs.
- Label ids are ranks of the sorted canonical content, never discovery order.
- f32 dot products use `cluster::dot`, which has a fixed summation order.

**Crate layout.** A new crate, `crates/adage-graph`, depends on `adage-core`, `adage-intent` and `adage-tracks`. It does not depend on `adage-runtime` or `adage-learn`. Its input types live in `input.rs`, and `adage-learn/src/bridge.rs` converts `Session` into them. The runtime never links `adage-graph`: it sees only compiled data in the index.

---

## 2. Prototypes: from embeddings to labels

Mining over raw embeddings would need approximate subgraph isomorphism, and that is intractable. So each node type is first **canonicalised into prototypes** (`adage-graph/src/proto.rs`). The method is the one intent already uses (`crates/adage-intent/src/cluster.rs`): exact cosine kNN (k = 15), a graph thresholded at τ_type, deterministic Louvain, and a minimum size.

**Blocking keeps it cheap and safe:**
- Act prototypes form only within the same effect class and program. Clusters never merge `Verify` with `Regenerate`.
- Art prototypes form only within the same top-level directory and extension family.
- Must-link: commands equal under `pattern::meets`, meaning they differ only by options.
- Cannot-link: different Exact identities when both are *targets*. See the asymmetry below.

**Asymmetry between context and target.**
- As *context* items, prototypes may be broad. `pytest tests/a.py` and `pytest tests/b.py`, or `edit schema/a.json` and `edit schema/b.json`, are one label.
- As *continuation* (directed) items, a prototype compiles to exactly one concrete command, its dominant variant. That variant must account for ≥ 80% of the prototype's support; otherwise the pattern is reported "ambiguous" and not compiled. Embeddings widen what triggers a reflex. They never invent what a reflex directs.

**Soft matching and thresholds.**
- Assignment is hard: nearest centroid, ties broken by prototype id. A node whose best cosine is below τ_type, or whose top-2 margin is below δ = 0.02, becomes a **singleton** keeping its exact label. It still participates at the exact level.
- Multi-assignment (top-k soft labels) is rejected. It inflates support, breaks the one-session-one-count property that anti-monotone pruning needs, and makes the evidence uncountable.
- "Softness" comes instead from a **label taxonomy**: exact ⊂ prototype ⊂ coarse prototype (a second Louvain at τ − 0.1) ⊂ Head/extension. Mining is taxonomy-aware, as in GSP with taxonomies (Srikant & Agrawal 1996): an item matches an event whose label is the item's label or a descendant of it.
- τ_type is calibrated per type and per repo, by **successor purity** (Design §8.2.1 step 5: "embeddings propose; behaviour decides"). Pick the τ on a grid {0.80 … 0.95} that maximises the held-out log-likelihood of the next alphabet event given the label. Ties go to the higher τ. Defaults (Act 0.90, Art 0.85, Fail 0.88, Prompt: the per-family calibration intent already does) live in `LearnParams.graph`.

**Epochs.** Prototypes are recomputed on a full `adage learn` and written to `tracks/<repo>/.epoch.json` (centroids quantised, label table). `learn --auto` assigns new nodes to the frozen epoch. It starts a new epoch when 200 sessions have been added or the singleton share exceeds 20%, whichever comes first. The output is a pure function of (sessions, epoch, params). A full learn is a pure function of (sessions, params).

---

## 3. Learning = frequent, discriminative, closed subgraph mining

### 3.1 Pattern language

A pattern `P = (items, order, constraints)`:

- **items:** 2 to 6 labelled items (Prompt / Act / Art-access / Fail / Obs), with at most 3 context items and at most 3 continuation items. Each item is a label at some taxonomy level, plus an optional polarity (Act ok/fail).
- **order:** a total order of item positions. Items in the same batch are sorted by label id, so the canonical form is a string.
- **gap:** consecutive items lie within G = 8 alphabet events, inside one prompt segment, or with a leading Prompt item.
- **constraints (≤ 3):** `flow(i→j.slot, type)` value-flow edges, `same_art(i, j)` (e.g. a read of a path listed by i's output), and `last(i)` ("after the last occurrence", which finish lines need).

Because items are time-ordered, **canonical form is free**. Two occurrences are the same pattern iff their label strings and constraint sets are equal, with no isomorphism testing. That is the decisive simplification.

### 3.2 Algorithm choice

| Option | Fit | Verdict |
|---|---|---|
| gSpan / CloseGraph | Transactional, general graphs. DFS-code canonical forms; subgraph-isomorphism cost exponential in pattern size. Our graphs are nearly sequences, and gSpan would spend its effort rediscovering an order we already have. | Rejected |
| GraMi | Frequent subgraphs in **one** large graph with MNI support | Wrong support notion: we need distinct sessions and days |
| GSP | Apriori candidate generation, many database passes | Too slow at 1.5M events |
| **PrefixSpan / episode growth over DAGs** | Pattern growth by projected database, bounded gaps; closure by BIDE backward-extension checks; constraint edges added as "attribute extensions" | **Chosen** |

**Target-rooted growth** (`adage-graph/src/mine/seq.rs`). Only patterns that end in something the compiler could deliver are useful. So mining is rooted at **target labels**:
- Act prototypes whose effect is ≤ Environment and whose support is ≥ s_min;
- read-sets of Art;
- `Fail→recovers_with` pairs.

For each target *y*, in label-id order:

1. **Occurrence list.** Pseudo-projections `(session, pos)` of y. When the context is an edit, keep only the first occurrence of y after the last edit, since finish lines need `last`.
2. **Backward growth.** Prepend a context item x found within G events before the current leftmost item. For every frequent x (support counted as distinct sessions), recurse, up to depth 3. This is PrefixSpan run on reversed sequences.
3. **Forward growth.** Append continuation items after y (at most 2 more), giving multi-step reflexes.
4. **Constraint extension.** At each node, try adding the `flow`, `same_art` and `last` constraints that hold in ≥ s_min of its occurrences.
5. **Taxonomy climb.** An item may be generalised one level up (exact → prototype → coarse) only if that raises support.
6. **Closure.** A pattern is kept only if no one-item or one-constraint extension has equal session support (the BIDE forward/backward extension check). Maximal patterns are reported for explanation only; closed ones are scored.

**Support.** `supp(P)` is the number of distinct sessions with an occurrence. `days(P)` is the number of distinct days. The discounted weight of each session is `0.5^(age/60d)` (roadmap E5). Session support is anti-monotone under extension, so pruning at `s_min = max(min_support, ⌈0.01·N⌉)` is exact. The discriminative measures are not anti-monotone and are applied after mining (§5). One sound early prune is used: a pattern whose support is below the number needed for its best-case Beta lower bound to reach the yellow bar is cut.

**Steering without kinds (Design §11).** A session that received directive D is not evidence for any pattern whose continuation contains a label in `influence(D)`: the Exact and HeadSub symbols D names (its command, its avoided runners, its read set), counted from D's emission onward. Such occurrences are counted as **exposed** instead, which feeds `stale.rs` unchanged. This generalises `steers()` with no kind table, and it is stricter where kinds were too coarse. An intent directive, for example, no longer blanks the lookup miner's view of unrelated commands.

---

## 4. Value-flow induction: learned input types (first deliverable, G1)

### 4.1 Finding flows (`adage-graph/src/flow.rs`)

- **Shared tokeniser.** It lives in `adage-core/src/tokens.rs` and is shared by learner and runtime. It splits on whitespace and on ``,;()[]<>"'` ``. It keeps `-_./#:@+=` inside tokens, and also emits sub-tokens split on `/`, `#`, `:` and `=`. So `ops/412` yields `ops/412`, `ops` and `412`; `#123` yields `#123` and `123`; `--issue=123` yields `123`.
- **Sources:**
  - prompt tokens;
  - earlier outputs: JSON string leaves by field path, otherwise text tokens;
  - earlier edited or read paths, split into segments with their depth index.
- **Sinks:** later Exec argv tokens and option values, recorded as `(HeadSub identity, positional index | option name, literal prefix/suffix inside the token)`.
- **Flow observation.** Exact string equality between a source token and a sink token (or sink sub-token), where the source comes earlier in the session.
- **Value slot test.** A sink is a value slot when, across sessions:
  - values are *variable*: ≥ 3 distinct values;
  - values are *flow-bound*: ≥ 60% of the sink's occurrences carry a value seen in an earlier source;
  - it is *not a literal*: no single value holds ≥ 50% of occurrences.
- **Result.** This learns which argv positions are identity and which are data. That is also the evidence to *propose* new `MULTIPLEXERS` entries (`pattern.rs:80`) in a report. The table itself stays hand-specified (§11).

### 4.2 Grammar induction by anti-unification with MDL (`adage-graph/src/types/`)

**Run skeletons.** Each value becomes a run sequence over the classes `U`=[A-Z], `L`=[a-z], `D`=[0-9], `H`=hex (a mixed run that fits [0-9a-f]), `W`=[A-Za-z0-9] and single punctuation literals:
- `BIL-1041` → `U{3} '-' D{4}`
- `#123` → `'#' D{3}`
- a UUID → `H{8} '-' H{4} '-' H{4} '-' H{4} '-' H{12}`
- `fix/login-42` → `L{3} '/' L{5} '-' D{2}`

**Lattice per run.** Literal → literal set (≤ 4) → class with exact length → class `{lo,hi}` → wider class (`U`∪`D` → `W`).

**Anti-unification.**
- Values with the same punctuation skeleton unify pointwise, taking the least general join.
- Different skeletons first try **star folding** of a repeated `(sep class)` group, so `a/b-c` and `a/b-c-d` both become `L+ '/' L+ ('-' L+){1,3}`.
- If that fails, the type is a union of at most 3 skeletons.

**MDL choice.** Choose G minimising `L(G) + Σ_v L(v|G)`, where:
- `L(v|G) = log2 |{strings of G of length |v|}|`;
- `L(G)` = 8 bits per literal char, plus 4 bits per class, plus the length-range code.

This keeps a constant prefix literal from two values upward (`BIL` costs 24 bits once, against about 14 bits per value as `U{3}`). It generalises digits once the literal set costs more than the class (three 4-digit values: 97 bits as a set against 46 as `D{4}`). The length range is exactly the observed `[lo, hi]`, with no slack, as `ticket_contract` does today (`spans.rs:85`).

**Safety constraints, all mandatory:**
- the regex is anchored `^…$`;
- the alphabet stays inside `plain_token` (`crates/adage-runtime/src/lib.rs:563`), with no leading `-`;
- maximum length 128;
- the type's language is ≤ 2^64 strings, otherwise it is rejected as "word-like" (bare lowercase slugs never become prompt-bound types);
- it rejects every **negative**: prompt tokens in the same prompts that did not flow, and values seen at that sink from no source;
- path-sourced types always use `Contract::RepoPath` and never a regex.

### 4.3 From types to slots

A type is stored as `ValueType { id, runs, regex, samples_hash }`. `id` is the stable hash of the sink identity plus the skeleton, not the lengths, so a widening range keeps the id.

The slot contract is `Contract::Pattern { regex }`, the existing variant, so the runtime validation path is unchanged. New slot sources in `crates/adage-tracks/src/model.rs`:

```rust
SlotSource::PromptValue { vtype: String }            // first prompt token matching the type
SlotSource::OutputValue { field: Option<String>, vtype: String }
SlotSource::EditSegment { glob: String, depth: u8 }  // segment of the triggering edit's path
```

`io.rs:214` ("only a prompt binds a run step's slots") is relaxed: any trigger whose event supplies the source may bind. The runtime matches `runs` with a hand-written O(len) matcher in `adage-core/src/vtype.rs`, so the hook compiles no regex. `io::validate` checks that `runs` and `regex` agree on the stored samples.

`SpanKind::Ticket` and `Trigger::Prompt{span}` remain readable for existing tracks. New learning never emits them.

### 4.4 Evaluation on evals/gen

- **New id shapes.** `evals/gen/skins.py` gains `id_shape` ∈ {`prefix_dash` (today), `hash_num` (`#482`), `uuid`, `path_num` (`ops/412`), `branch` (`fix/<slug>-<n>`), `mixed` (two trackers in one repo)}. `grammar.py` validates it. `conventions.json` already carries `id_pattern`, `samples` and `counter_samples`, and `conventions.rs` already checks that lookup contracts accept and reject them.
- **Holdout metric.** `adage eval types` generates 200 fresh ids from the generator's distribution and 200 near-miss counter-samples: wrong prefix, length ± 1, a leading `-`, shell metacharacters.
- **Targets:** accept ≥ 0.99, reject = 1.00, lookup recall on non-`X-123` shapes from 0 to ≥ 0.95, and zero unsafe directives on the adversarial set.

---

## 5. Scoring: frequent is not useful

Each closed pattern `P` and each cut point `c` gives a candidate reflex: context `X = P[..c]`, continuation `Y = P[c..]`. The scorer (`adage-graph/src/score.rs`) computes the following.

**Support gates.** `supp ≥ min_support` distinct unexposed sessions and `days ≥ 2`. Today `days` is recorded but not gated.

**Confidence (predictive).** `conf = supp(X ⇒ Y within W) / opp(X)`.
- `opp(X)` is the number of sessions where X occurs with at least W events remaining, or with the session ending. It is computed by joining the per-label posting lists (`(session, pos)`, sorted), so it costs the sum of the posting-list lengths, not a rescan.
- Posterior: discounted `Beta(α, β)` with `α = s + κ·m`, `β = (n − s) + κ·(1 − m)`. The prior mean `m` is the parent pattern's confidence (X with its earliest item dropped), with κ = 2 pseudo-sessions.
- That parent chain *is* the variable-order backoff of Design §8.5. The longest context with evidence speaks; shorter contexts only shrink it.
- The lower bound is the 5% Beta quantile (regularised incomplete beta by bisection, `stats.rs`, tested against known values). It replaces Wilson, with a parity switch (§9).

**Lift against background.** `lift = conf / P(Y within W | a random anchor of the same segment type)`. Contrast is `conf − P(Y within W | ¬X)`. A single `learn.contrast` parameter (default 0.3) replaces `finish_line_contrast` and `intent_contrast`. This is what kills `ls` and `cat`: they are frequent everywhere, so their lift is about 1.

**Consistency, for fail items.** `P(fail | X-runner) ≥ 9/10` in sessions that ran it. This is `dead_end_consistency`, now a property of any pattern with a `fail` item.

**Value.** `V = p_need · (1 − p_soon) · Δturns − λ_fp · (1 − p_need)`, with λ_fp = 2 as in the bench objective:
- `p_need` is the posterior mean of Y after X;
- `p_soon` is P(Y is the immediately next batch | X), i.e. the agent would do it anyway;
- `Δturns` is the median turn gap from X to Y in supporting sessions. For fail-recovery patterns it is the turns from the failed run to the recovery.

**Causal lift (roadmap E4).** Knockouts are `hash(session, route) < share` exclusions. From them, `lift_c = p(Y|X, treated) − p(Y|X, knocked out)`, IPW-weighted by recorded propensity (`adage_runtime::assign`). When a CI exists, a compiled reflex whose upper bound is < 0 is demoted to probation. Until E4 data exists, the holdout arm gives a noisier estimate, and the observational `V` rules.

**Conformance.** Per-session mean `−log p(next alphabet event)` under the selected reflexes plus the backoff. It is used for:
- the Design §8.5 bar (top-1 next-action accuracy ≥ baseline + 5 points before the engine replaces miners);
- drift and model-churn detection (weekly mean surprise);
- explaining derails.

**MDL selection** (`select.rs`, SQS-style: Tatti & Vreeken 2012). Candidates passing the gates are sorted by `(V desc, supp desc, canonical string asc)`. Each is greedily added to a code table if it reduces `L(CT) + L(D | CT)`, where D is the set of prompt-segment sequences covered with gaps. Overlapping patterns — `edit schema/*.json → make gen` against `edit schema/*.json → make gen → make check` — compete. The longer one survives only if it compresses the data further, i.e. the check really follows the gen. The result is deterministic, non-redundant and explainable: every kept reflex has a stated compression gain.

---

## 6. Compiler: reflexes → delivery shapes

`adage-graph/src/compile/` is fully deterministic: a pure function of the selected reflexes and the repo tree.

### 6.1 Shape choice (structural, closed set)

| Continuation Y | Context X | Shape | Maps to |
|---|---|---|---|
| one Act, effect ≤ ceiling | Prompt / intent / Act / Edit / start | `run` | `Step::Run` |
| one Regenerate/Verify Act | ends in `last(edit Art)` | `obligation` (a stop request is possible) | `AfterEdit` + `Run` (existing semantics) |
| Act X′ fail F, then `recovers_with` Y′ | any | `avoid` | `Step::Avoid { avoid, instead, signature }` |
| read set with `same_art`/flow from an Obs field | Act | `read-batch` | `Step::ReadBatch` |
| 2–3 Acts | any | `sequence` | new `Step::Sequence { commands ≤ 3 }`; aspect = min over steps; follow judged on step 1 |
| no directable Y, but a strong `Art`-typed pattern (generated file, lockfile) | Edit | `advise` | fixed advisory templates (instinct U4/U6 style) |

**Ceilings are unchanged** (`model.rs:251`): Environment is Draft until review, Author and Publish are never directed, and Regenerate is green only under an edit-triggered obligation.

**Where an `avoid` is announced.** Session start if `P(X′ in segment | start) ≥ 0.5`, otherwise `AfterFailure { X′, signature }`. That unifies the dead-end miner with instinct U1.

**Aspect.** Green needs the Beta lower bound ≥ `green_lower_bound` and a green ceiling. Yellow needs posterior mean ≥ `yellow_rate` and `V > 0`. Hysteresis (`retain_rate`) is kept.

### 6.2 Context compilation

Context items compile to symbolic triggers, never to embeddings, except Prompt-intent items, which compile to the existing `Trigger::Intent { centroid, threshold }`.

- **Act prototype:** the sorted list of member Exact keys, matched with `pattern::meets`.
- **Art prototype:** a glob chosen by the **glob lattice**. Of the candidate globs covering all member paths (file → `dir/*.ext` → `p/**/*.ext`), pick the least general one whose precision over the repo tree at learn time is ≥ 0.9, i.e. it matches few files that no session touched. This generalises the existing unanimity rule (`mine.rs:626`). Prototypes with no common prefix become several routes; `**/*.ext` is never allowed.
- **Length-1 contexts** use the legacy triggers (`Prompt`, `AfterCommand`, `AfterFailure`, `AfterEdit`, `SessionStart`, `Intent`).
- **Longer contexts** use `Trigger::Context { items: Vec<CtxItem> (≤ 3), within: u8 (≤ 8) }`. `CtxItem` is one of `Act{keys}`, `Fail{keys, signature}`, `Edit{glob}`, `PromptValue{vtype}` or `Intent{…}`.

**Slot binding.** Every `{slot}` in a directed command must have a flow constraint whose source is inside the context. A target with a variable argv and no bound source is not directable and is dropped.

### 6.3 Stable ids, review and interlocking

- **Ids.** `id = <shape>-stable(canonical context ‖ canonical step)`. Canonical content means representative keys, compiled globs, value-type ids and slot *names*, never evidence or epoch label ids. So re-clustering does not rename a route. `compile/legacy.rs` reproduces the exact ids today's miners emit when a reflex has a legacy shape (`lookup-work-item`, `read-linked-files`, `dead-end-<stable(W)>` / `<stable(W,S)>`, `finish-line-<stable(g,c)>`, `intent-<stable(group)>`), so `review.yaml` decisions, proposals and the instinct ids that already match learned ids all survive.
- **Interlocking** is unchanged in `interlock.rs`. `avoids` is extended so a `sequence` or `obligation` conflicts if *any* of its commands matches an avoided runner. `same_point` gains a `Context` arm: identical item lists, or one list a suffix of the other. The compiler also drops, before emission, any `run` whose target is avoided by a selected `avoid`.
- **Instincts as priors.** The bound instinct routes (`crates/adage-instincts`) become pseudo-evidence on the matching (context, continuation), with κ = 3 at mean 0.7. On day 0 they still ship as today (yellow at most). Once sessions exist, the posterior dominates, and "a reflex that says otherwise always wins" holds by construction.
- **Stale.** `stale::verdict` uses the scorer's exposed and unsteered counts for the reflex's own pattern, with no kind dispatch.
- **Review.** Each route gets a non-agent `why` block, never rendered to the agent: the pattern string, backoff depth, compression gain, lift, prototype members with their counts, and three example session ids. It is shown in `adage review` and `adage graph --learned`.
- **Track layout.** Through G3–G4, reflexes go into the legacy track directories via `legacy_track(shape)`; new shapes go to `<repo>/graph`. G5 consolidates everything into `<repo>/learned`, migrating `review.yaml` and proposals (ids are unchanged). The directive wrapper's `kind` attribute becomes the shape name, and `import.rs` keeps reading old kinds.

---

## 7. Runtime implications (G4)

### 7.1 Session state

`SessionState.history` is written but never read by `decide` (`lib.rs:358`). It is replaced for decisions by:

```rust
pub struct CtxState { ring: [u32; 8], len: u8, since: [u8; 8], last_fail: Option<(u32, u32)> }
```

`ring` holds recent **alphabet symbol ids** (u32), interned in the index per repo. Unknown events are not pushed. `history` strings stay for spools and `adage graph` and may be capped at 16. This fits comfortably in the 64 KiB state slot (`adage-store/src/state.rs:10`).

### 7.2 Discrimination net

`Index` gets two things per repo:
- `alphabet: BTreeMap<key, u32>`, holding Exact keys, HeadSub identities, compiled globs and signature ids;
- `dnet`: a suffix trie over reversed symbol sequences. Each node lists the route ids whose context ends there, with leaf guards (glob, value type, `within`).

An event is symbolised: template key → `Shape` → alphabet lookup, with Exact first and HeadSub second. Then the dnet is walked from the newest symbol for at most 3 steps. That is O(3) map lookups plus guards, microseconds on top of today's parse.

### 7.3 Fast-path equivalence

Context matching is defined as **subsequence within a gap over alphabet events only**, so events outside the alphabet cannot change any match. `Triggers.words` (`triggers.rs`) becomes the words of *every* symbol in *any* context, not only the last. A call containing none of them is non-alphabet, so skipping it is exactly equivalent. The `fast_path_changes_no_decision` replay gate (`fast.rs` docs) is kept and extended to `Context` triggers.

### 7.4 Latency

- Prompt: tokenise (≤ 256 tokens) and run the value-type matchers (≤ 16 types). This costs well under 0.1 ms and needs no regex compilation.
- Intent: unchanged (about 6.4 ms p95 end to end).
- Batch: one symbol lookup and trie walk per call.
- The index is still JSON loaded by every hook. If the per-repo alphabet and dnet push parsing past 0.5 ms, G4 shards it into `index/<repo-hash>.json` so the hook reads only its own repo.
- Target: `hook_latency.rs` p95 within +0.3 ms of today in every scenario, and still inside Design §13.2 (10/15/25 ms).

---

## 8. Today's five route kinds are special cases of mined patterns

| Legacy kind (miner) | Mined pattern | Shape |
|---|---|---|
| lookup (`prompt_routes`) | `[Prompt] →flow(T)→ [Act scripts/wi-lookup {T}]` | `run`, slot `PromptValue{T}` |
| linked files (`read_batch_routes`) | `[Act lookup] –yields→ [Obs field f] →same_art→ [Read Art]+` (at least half of the listed paths) | `read-batch`, slot `OutputValue{f}` / RepoPath |
| dead end (`dead_end_routes`) | `[Act X′ fail F] → [Act Y′ ok]` with `recovers_with`; consistency ≥ 9/10 | `avoid` |
| finish line (`obligation_routes` + `generalise_globs`) | `[last(Edit Art-proto A)] → [Act Y Regenerate ok]`, contrast ≥ 0.3; A becomes a glob by the lattice | `obligation` |
| intent (`intent_routes`) | `[Prompt intent-proto I] → [Act Y observe/verify]` within the segment before the first edit; contrast ≥ 0.3 | `run`, `Trigger::Intent` |
| **new: scope** | `[Edit Art in crates/{p}/**] →flow(segment p)→ [Act cargo test -p {p}]` | `run`/`obligation`, slot `EditSegment` |
| **new: env_sync** | `[Edit package.json] → [Act npm install]` (Environment) | `run`, Draft until review |
| **new: sequence** | `[last(Edit registry/*.csv)] → [make gen] → [make check]` | `sequence` |

**Parity test** (`crates/adage-graph/tests/kinds.rs`, plus `adage eval routes --engine graph`). For every dataset (`evals/datasets/*`) and every `evals/gen` smoke bundle, the graph engine's routes must include each miner route with the same id, trigger, step and slot contract. Each extra route must either match a `conventions.json` entry or be counted as a false route.

---

## 9. Complexity, bounds, incremental mining

**Scale.** N = 5,000 sessions × 300 events is 1.5M events. Prompt segmentation, and dropping labels with support below s_min before mining, leaves typically 300–1,500 alphabet labels and ≤ 200 target labels.

**Prototypes.** Exact kNN is O(n²d) within blocks. Only nodes with support ≥ 2 are clustered, typically ≤ 3k Act and ≤ 5k Art in blocks of ≤ 1k, which is about 1–3 s. S7 measured 17 s per 10k prompts in a single block.

**Mining.** Each target's search is independent (parallel by target; results merged in label order):
- cost ≈ occ(y) × (G · |taxonomy levels|)^d for d ≤ 3, with support pruning;
- hard budget: 2M pattern-nodes expanded per repo, explored best-support-first with ties by label id, so truncation is deterministic and reported;
- memory: pseudo-projections of 8 bytes per occurrence;
- expected: 2–10 s for a full learn at this scale, against 0.6–1.3 s today for 4k simulator sessions.

**Incremental** (`mine/border.rs`, in the IncSpan style).
- Persist the closed set and its **negative border**: patterns with support in `[s_min − 2, s_min)`, with their occurrence counts.
- For a new or changed session: compute its facts (S2), assign it to the frozen epoch, match it against stored patterns and the border in O(|patterns| × L), and update the counts exactly.
- A full re-mine of that repo runs when a border pattern crosses s_min, when a new target label appears, or when an epoch rolls.
- Selection and compile always re-run; they are cheap.
- Correctness gate: incremental output equals a full re-mine on the same (sessions, epoch). This is property-tested across random arrival orders.
- Target: `learn --auto` with one transcript changed stays ≤ 150 ms (today 44–138 ms).

---

## 10. Migration plan and milestones

Configuration: `LearnParams.engine = "miners" | "shadow" | "graph"` and `LearnParams.graph { … }` in `crates/adage-tracks/src/params.rs`. The bench sweeps them like any other parameter.

| M | Deliverable (files) | Effort | Metric it moves / gate |
|---|---|---|---|
| **G0** Substrate | Per-prompt import (`import.rs`, `codex.rs`, `opencode.rs`); `adage-core/src/tokens.rs`; `adage-graph/{input,facts,embed,cache}.rs`; `adage graph --learned --dump` | 2–3 wk | No behaviour change. Facts are byte-identical across thread counts and input orders; the learn-time increase is ≤ 1.3× |
| **G1** Value types | `adage-graph/src/{flow.rs,types/}`; `adage-core/src/vtype.rs`; new `SlotSource`s in `model.rs`; relaxed `io.rs:214`; runtime binding in `lib.rs` (`bind_prompt_route`); `skins.py`/`grammar.py` id shapes; `adage eval types`. The lookup miner uses learned types when `engine != miners` | 2–3 wk | Lookup recall on the new id shapes 0 → ≥ 0.95; contract accept ≥ 0.99 and reject 1.00; L0 route F1 unchanged; unsafe = 0 |
| **G2** Prototypes, mining, scoring | `proto.rs`, `mine/{seq,closed,border}.rs`, `score.rs`, `stats.rs`, `select.rs`; `adage eval mine` (pattern and conformance report) | 3–4 wk | Next-action top-1 ≥ order-1 baseline + 5 pts (Design §8.5); `calibration_ece` ≤ 0.05; determinism and incremental-equals-full property tests |
| **G3** Compiler, shadow | `compile/{shapes,glob,ids,legacy}.rs`; `Step::Sequence`, `Trigger::Context`; interlock extensions; `engine=shadow` writes `<repo>/.shadow/` and a diff report; `conventions.rs` gains `Scope`, `EnvSync` and `Sequence` kinds; `grammar.py` generates `scope`, `env_sync` and `sequence`; `rehearse.py --engine graph` | 3–4 wk | **Parity:** conventions route recall ≥ miners and false routes ≤ miners on every dataset and gen bundle; L0 J ≥ baseline − 0.01 with all guardrails. **New capability:** learns scope, env_sync and sequence from `evals/gen` with no learner code change after G3 freezes (the capability PR touches only `evals/gen/**` and the scorer) |
| **G4** Runtime contexts | `adage-runtime/src/context.rs`, `CtxState`, dnet in `adage-tracks/src/index.rs`, `triggers.rs` words; optional index sharding | 2 wk | Hook p95 within +0.3 ms; fast-path mismatches 0; replay fidelity unchanged |
| **G5** Retire miners, one per bench bless | Order: lookup → linked files → finish line → dead ends → intents; then delete `route_kind`, `steers`, `KIND_*`, the `unsteered(kind)` plumbing in `lib.rs:239-262`, `score::kind`; consolidate to `<repo>/learned` | 3 wk | Each retirement needs parity on its conventions kind in two consecutive blessed baselines |

Total is about 15–19 engineer-weeks. G1 can ship alone and is useful even if G2 slips.

---

## 11. Risks and mitigations

- **Overgeneralisation from embeddings.** A prototype merges commands that behave differently.
  - Targets must be one dominant concrete variant (≥ 80%).
  - Cannot-link across effect classes.
  - τ is calibrated by successor purity.
  - Globs need ≥ 0.9 tree precision.
  - Contexts widen triggers only when the widened pattern has its own support and passes the contrast test, never on similarity alone.
- **Nondeterminism.**
  - Content-ranked label ids, `BTreeMap`, fixed-order dot products, quantised centroids, and deterministic budgets and truncation.
  - Epoch files are versioned inputs.
  - Gates: same bytes for 1 and 4 jobs, and incremental equals full.
- **Latency.** No act, path or output embedding in the hook; symbol interning; a dnet walk of at most 3; index sharding held in reserve; hook benches in CI.
- **Sparsity in small repos.**
  - Beta shrinkage toward shorter contexts.
  - Instinct priors.
  - The demo preset's `min_support` of 3.
  - No reflex compiles from prior mass alone above yellow.
  - Prototypes raise support by pooling variants.
- **Pattern explosion.** Target-rooted growth, at most 6 items, gap ≤ 8, closed-only, the node budget, and MDL selection.
- **Explainability.** Each route's `why` block: pattern string, counts, lift, compression gain, prototype members, example sessions. `adage review` shows it. Directed text is still fixed templates only.
- **Learned types leaking identifiers.** Literal prefixes such as `BIL` already appear in contracts. Samples are never stored in tracks, only `samples_hash`.
- **Value-flow false positives** (common words flowing into argv). The variability, flow-rate and non-literal tests, the 2^64 language cap, and the negatives.

---

## 12. What stays hand-specified, and why

- **Effect classification** (`crates/adage-core/data/commands.toml` and the read-verb heuristic). It is the safety ceiling. Learning may *propose* reclassifications from observed file writes, as a report only.
- **Delivery shapes, their templates** (`crates/adage-runtime/src/render.rs`), **ceilings, `plain_token`, RepoPath checks, stop-hook bounds, directive budgets.** These form the output-boundary contract with the agent (Design §8.3, §15).
- **Interlocking rules** (`interlock.rs`), meaning polarity and two-greens. They are policy, not statistics.
- **The command-pattern algebra and `MULTIPLEXERS`** (`pattern.rs`). This is identity semantics for follow judgement and safety. Value-flow evidence proposes additions in a report.
- **Shell and tool normalisation** (`shell.rs`, `tools.rs`): perception, not learning.
- **Prompt normalisation for the pinned embedding model**, plus learned-type substitution. Stored centroids are only valid for exactly that pipeline.
- **The instinct catalogue** (`crates/adage-instincts/data/catalogue.toml`). These are declared priors that work on day 0.

---

### Critical files for implementation
- /home/user/adage/crates/adage-learn/src/mine.rs (the logic to subsume, and the legacy ids to reproduce)
- /home/user/adage/crates/adage-learn/src/import.rs (per-prompt anchoring, steering-by-influence)
- /home/user/adage/crates/adage-tracks/src/model.rs (new `SlotSource`s, `Step::Sequence`, `Trigger::Context`; removing kinds)
- /home/user/adage/crates/adage-runtime/src/lib.rs, plus fast.rs and /home/user/adage/crates/adage-tracks/src/triggers.rs (context state, dnet, fast-path equivalence)
- /home/user/adage/crates/adage-core/src/spans.rs and template.rs, /home/user/adage/crates/adage-intent/src/normalize.rs (the hand-written ID detectors learned value types replace)
- /home/user/adage/crates/adage-eval/src/conventions.rs and /home/user/adage/evals/gen/grammar.py, skins.py (parity and new-capability ground truth)