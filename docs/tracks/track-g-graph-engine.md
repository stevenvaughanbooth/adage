# Track G: Graph and prediction engine v2

> **Status (2026-09-28, later): the learner forms real repositories' reflexes** (command variants, sibling-directory globs, checks after an edit, distinct tasks; [status note](#status-note-real-traces-2026-09-28)).
>
> **Status (2026-09-28): G5 implemented: the graph engine is the only learner.** The miners, route kinds and `learn.engine` are gone; every reflex is written to `<repo>/learned` ([status note](#status-note-g5-2026-09-28)). The notes below describe earlier milestones as they were.
>
> **Status (2026-09-27, later): G0 implemented; G1 integrated behind `LearnParams.engine`.** The default engine is still `miners`, and with it behaviour is unchanged: the L0 bench reproduces the committed baseline exactly (same `params_hash`, J 0.6307).
>
> *G0, substrate.* All three importers keep every prompt: `Session.prompts: Vec<PromptObs { text, at_step }>` (`Session.prompt` is still the first) and `StepObs.prompt: u16`; the parse-cache stamp carries a session format (`cache::SESSION_FORMAT = 2`), so older entries are re-parsed. `adage-graph` gains `facts.rs` (`SessionFacts`: time-ordered `Prompt` and `Act` events; `Art`, `Obs` and `Fail` nodes; `follows`, `binds`, `reads`, `writes`, `yields`, `fails_with` and `recovers_with` edges; token occurrences; ≤ 2,000 events, ≤ 512 token occurrences, ≤ 64 KiB of output per event, truncations recorded), `embed.rs` (prompt: the static model on `normalize()`, skipped without one; Act 384-d: 256 dimensions of SHA-256-seeded signed feature hashing plus 128 of the model's identity-word embedding folded in two, weighted 0.75/0.25; Art 256-d; all quantised to `i16`) and `cache.rs` (`$ADAGE_HOME/cache/facts/<sha256>.json`, keyed by facts format, embedding version, model id and the session's bytes; directory 0700, files 0600). `input.rs` gains `StepMeta` and `Annotated` beside `Session` (nothing existing changed). `adage-learn/src/bridge.rs` replaces `adage eval types`' adapter. `adage graph --learned --dump <repo> [--transcripts …] [--json]` prints the facts' counts, the value slots and the sinks that failed the slot test. Facts are the same bytes for 1, 2, 4 or 7 threads and any input order (`facts.rs` test). Learn time (`learn_bench`, 600 simulator sessions in 2 repositories, release build, two runs each; base = the commit before G0): `--engine miners` is unchanged (no cache 142–146 ms vs 140–147 base; warm cache 124–147 vs 121–126). `--engine graph`, which also computes facts: no cache 175–192 ms (1.2–1.35× base), warm facts and parse caches 145–150 ms (1.15–1.2×), but a cold cache 355–385 ms (about 1.5×: writing 600 facts files; embeddings are stored as hex to keep that down). So the ≤ 1.3× target holds warm and roughly holds without caches, and does not yet hold for the first, cold learn.
>
> *G1, integration.* `SlotSource::{PromptValue { vtype, mentions }, OutputValue { field, vtype }, EditSegment { glob, depth }}` and a stored `ValueType { id, runs, regex, samples_hash }` (`model.rs`); old tracks read unchanged. `io.rs`: a run step's slot must be bound by its trigger's event (prompt → `Prompt`/`PromptValue`, after-command → `OutputValue`, after-edit → `EditSegment`), and a value type's `runs` must write exactly its `regex`, the slot's contract must be that regex, the type must accept its shortest and longest members under both, and the matcher must refuse the strict-contract probes. `LearnParams.engine` (`miners` | `shadow` | `graph`, default `miners`, left out of the canonical JSON when `miners`) and `adage condition --engine`: under `shadow` or `graph` the lookup miner (`adage-learn/src/graph.rs`) takes its slot and contract from `adage_graph::learn` (prompt-sourced typed slots, most sessions first, **the effect ceiling checked before anything else**, so a `gh issue close {item}` slot is dropped however common), keeps the id `lookup-work-item`, and session facts are computed through the cache. The runtime binds a `PromptValue` from `adage_core::tokens` with `vtype::matches` and compiles no regex for it (`contract_ok` still runs, with the matcher standing in for the regex only when the type's runs write exactly that regex; `plain_token` stays the final gate); `OutputValue` and `EditSegment` bind in after-command and after-edit run steps. `Triggers.prompt_types` keeps the fast path exact for typed prompt slots. `rehearse.py` matches lookups by ids (tracker, fresh and near-miss ids), not by spelling, and takes `--engine`. Results: `adage eval types specs/idshapes --end-to-end` (learn with `engine = graph`, then the hook's decision on every session prompt, 200 fresh ids and the adversarial strings): recall 1.00 on all 11 bundles, controls fired 0, unsafe binds 0; `rehearse.py --engine graph` recovers every convention of every idshapes bundle. L0: `engine = graph` J 0.6307, the same as `miners` on every ranked term; on the adversarial safety set the graph engine learns no lookup (half of its sessions run the lookup on a constant id no prompt names, which fails the value slot test), so its turns saved there fall from 0.7 to 0.5 with unsafe still 0.
>
> Deviations decided while integrating: (1) the value type is stored inline in its slot source (`PromptValue { vtype: ValueType }`) rather than referenced by id from a route-level table, so no existing `Route` literal changes and one slot has one type; (2) a learned `PromptValue` may carry `mentions`, how people wrote the value (`#` in `#482`): when every supporting session wrote it the same one or two non-bare ways, only such a token binds, so a stray `120` in a prompt is not taken for a `#120` id; (3) the lookup route keeps `Trigger::Prompt { span: ticket }` (the legacy shape and kind, so ids, `route_kind` and review decisions survive); the span is ignored for a typed slot; (4) the linked-files route's trigger key comes from the lookup's real calls, since a value's shape decides its placeholder. **Deferred:** the normaliser's `{t:<type-id>}` substitution (it changes the intent model id and every stored centroid, for no G1 metric) — G2, with prototypes.
>
> **Status (2026-09-27): G1 core implemented, not yet integrated.** `crates/adage-graph` (`input.rs`, `flow.rs`, `types/`), `adage-core/src/tokens.rs` and `adage-core/src/vtype.rs`, `evals/gen` `id_shape` and `specs/idshapes.json`, and `adage eval types`. Nothing learned is used by `adage condition`, tracks or the runtime yet (G1 integration: `SlotSource`s, `io.rs:214`, `bind_prompt_route`, `bridge.rs`). On `specs/idshapes.json` (11 bundles, rehearsal transcripts): learned lookup recall 1.00 on every shape, accept 1.00, reject 1.00, unsafe 0; today's hard-coded detector: recall 0 on `hash_num`, `uuid`, `path_num` and `branch`, and half the ids of `mixed`. Deviations from this design, decided while building it: (1) the 2^64 language cap exempts **rigid** types (only fixed-length digit or hex runs and literals, at least two punctuation literals), or no UUID (2^122) could ever be a type; bare letter runs (`L{…}`, `U{…}`, `W{…}`, short `H`) are rejected as word-like whatever their size; (2) when both star folding and a union apply, the cheaper by MDL wins (folding first would turn two trackers `BIL-1041` and `#482` into `(BIL|482|…)(-D){0,1}`); (3) when the MDL choice accepts a negative, the least general type (literal sets where ≤ 4) is tried before rejecting; (4) positives outside `plain_token` are moved to the negatives rather than failing the slot; (5) the shared tokeniser also splits on the other shell metacharacters (`$&|*!?{}\~^%`) and trims a trailing `.` or `:`; (6) `adage-graph` depends only on `adage-core` for now.
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

**Epochs.** Prototypes are recomputed on a full `adage condition` and written to `tracks/<repo>/.epoch.json` (centroids quantised, label table). `condition --auto` assigns new nodes to the frozen epoch. It starts a new epoch when 200 sessions have been added or the singleton share exceeds 20%, whichever comes first. The output is a pure function of (sessions, epoch, params). A full learn is a pure function of (sessions, params).

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

**Support.** `supp(P)` is the number of distinct sessions with an occurrence. `days(P)` is the number of distinct days. The discounted weight of each session is `0.5^(age/60d)` (roadmap E5). Session support is anti-monotone under extension, so pruning at `s_min = max(min_support, ⌈0.01·N⌉)` is exact. The discriminative measures are not anti-monotone and are applied after mining (§5). One sound early prune is used: a pattern whose support is below the number needed for its best-case Beta lower bound to reach the prime bar is cut.

**Steering without kinds (Design §11).** A session that received directive D is not evidence for any pattern whose continuation contains a label in `influence(D)`: the Exact and HeadSub symbols D names (its command, its avoided runners, its read set), counted from D's emission onward. Such occurrences are counted as **exposed** instead, which feeds `dormant.rs` unchanged. This generalises `steers()` with no kind table, and it is stricter where kinds were too coarse. An intent directive, for example, no longer blanks the lookup miner's view of unrelated commands.

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
- **Held-out metric.** `adage eval types` generates 200 fresh ids from the generator's distribution and 200 near-miss counter-samples: wrong prefix, length ± 1, a leading `-`, shell metacharacters.
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

**Causal lift (roadmap E4).** Knockouts are `hash(session, route) < share` exclusions. From them, `lift_c = p(Y|X, treated) − p(Y|X, knocked out)`, IPW-weighted by recorded propensity (`adage_runtime::assign`). When a CI exists, a compiled reflex whose upper bound is < 0 is weakened to forming. Until E4 data exists, the control arm gives a noisier estimate, and the observational `V` rules.

**Conformance.** Per-session mean `−log p(next alphabet event)` under the selected reflexes plus the backoff. It is used for:
- the Design §8.5 bar (top-1 next-action accuracy ≥ baseline + 5 points before the engine replaces miners);
- drift and model-churn detection (weekly mean surprise);
- explaining ignores.

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
| 2–3 Acts | any | `sequence` | new `Step::Sequence { commands ≤ 3 }`; strength = min over steps; follow judged on step 1 |
| no directable Y, but a strong `Art`-typed pattern (generated file, lockfile) | Edit | `advise` | fixed advisory templates (instinct U4/U6 style) |

**Ceilings are unchanged** (`model.rs:251`): Environment is Nascent until review, Author and Publish are never directed, and Regenerate is fire only under an edit-triggered obligation.

**Where an `avoid` is announced.** Session start if `P(X′ in segment | start) ≥ 0.5`, otherwise `AfterFailure { X′, signature }`. That unifies the dead-end miner with instinct U1.

**Strength.** Fire needs the Beta lower bound ≥ `fire_lower_bound` and a fire ceiling. Prime needs posterior mean ≥ `prime_rate` and `V > 0`. Hysteresis (`retain_rate`) is kept.

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
- **Instincts as priors.** The bound instinct routes (`crates/adage-instincts`) become pseudo-evidence on the matching (context, continuation), with κ = 3 at mean 0.7. On day 0 they still ship as today (prime at most). Once sessions exist, the posterior dominates, and "a reflex that says otherwise always wins" holds by construction.
- **Dormant.** `dormant::verdict` uses the scorer's exposed and unsteered counts for the reflex's own pattern, with no kind dispatch.
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
| **new: env_sync** | `[Edit package.json] → [Act npm install]` (Environment) | `run`, Nascent until review |
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
- Target: `condition --auto` with one transcript changed stays ≤ 150 ms (today 44–138 ms).

**Skills** (plan §2.1.2). A Skill tool use is an `input::Step::Skill` and a `NodeType::Skill`
event labelled by its name alone (no prototype or coarser level: two skills are never one item).
It is a target and a continuation item, never a context item; a continuation of one skill is the
`skill` shape (`after_edit`, or a selected context, `→ Step::Skill`, prime at most), and a skill
used before a family's first edit is that family's intent continuation. A directive that named the
skill exposes that skill's events only (`Influence::skills`).

**Candidates** (`mine::mine_candidates`, plan D1). The negative border is also what a person can
watch form, so the learner keeps it, in a pass of its own: lowering the reflexes' floor would add
alphabet events between the items of today's patterns (`gap` counts alphabet events) and change
reflexes in service. A second `Db` over the same sessions has its floor at
`max(s_min − border, 2)`; `seq::mine` runs on it with the same parameters, and only the patterns
with support in `[floor, s_min)` and two or more context items are kept, marked closed among
themselves. Each becomes a `select::Reflex` with its counts (support, days, opportunities, the
median turns from its context to its step), never scored against the gates or selected; the
compiler reads them apart (`compile::compile_candidates`) through the same context shape, so each
has the id it would have as a reflex, with no strength bar, and keeps one per habit (the same
supporting sessions and step: the context nearest the step, then the shortest). The engine
branches on `Compiled::candidate` before judging, and the learner writes the rest through
`Store::write_candidates`. `select::Params::candidates` is off by default (only the learner's engine turns it on, from
`LearnConfig::candidates`), so other callers of `select::learn` pay nothing for it.

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
  - No reflex compiles from prior mass alone above prime.
  - Prototypes raise support by pooling variants.
- **Pattern explosion.** Target-rooted growth, at most 6 items, gap ≤ 8, closed-only, the node budget, and MDL selection.
- **Explainability.** Each route's `why` block: pattern string, counts, lift, compression gain, prototype members, example sessions. `adage review` shows it. Directed text is still fixed templates only.
- **Learned types leaking identifiers.** Literal prefixes such as `BIL` already appear in contracts. Samples are never stored in tracks, only `samples_hash`.
- **Value-flow false positives** (common words flowing into argv). The variability, flow-rate and non-literal tests, the 2^64 language cap, and the negatives.

---

## 12. What stays hand-specified, and why

- **Effect classification** (`crates/adage-core/data/commands.toml` and the read-verb heuristic). It is the safety ceiling. Learning may *propose* reclassifications from observed file writes, as a report only.
- **Delivery shapes, their templates** (`crates/adage-runtime/src/render.rs`), **ceilings, `plain_token`, RepoPath checks, stop-hook bounds, directive budgets.** These form the output-boundary contract with the agent (Design §8.3, §15).
- **Interlocking rules** (`interlock.rs`), meaning polarity and two-fires. They are policy, not statistics.
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
---

## Status note: G2 (2026-09-28)

**Implemented, not integrated.** `crates/adage-graph`: `events.rs` (the miner's own event-sequence input and a converter from `input::Session`; the integration switches the source to `SessionFacts`), `proto.rs`, `mine/{mod,seq,closed,border}.rs`, `stats.rs`, `score.rs`, `select.rs`; `adage eval mine` (`crates/adage-eval/src/graph_mine.rs`); tests `tests/{kinds,mining,border,profile}.rs`.

**Results.**

- **Parity (`tests/kinds.rs`, `adage eval mine`).** Recall 1.00 on every convention of `demo-haiku45` (4), `synthetic` (4) and the six smoke bundles (28: lookup, linked files, dead end, instinct violation, finish line); decoys selected 0 of 13. Reflexes matching no convention: 7 (demo-haiku45), 1 (synthetic), 0–1 per smoke bundle.
- **Held out** (task-grouped folds, pooled): next-action top-1 engine vs order-1 baseline 0.402 vs 0.399 (demo-haiku45), 0.801 vs 0.801 (synthetic), 0.506 vs 0.506 (smoke mean). The Design §8.5 bar (+5 points) is **not met**. ECE of `conf`: 0.120 (demo-haiku45), 0.063 (synthetic), 0.22 (smoke mean over 33–39 pairs per bundle); the ≤ 0.05 target is **not met**.
- **Time.** demo-haiku45 (90 sessions): 0.3 s for the whole pipeline in release. Synthetic scale test (2,000 sessions × 200 events, 402k events, 8 threads, release): prototypes 1.9 s, mining 2.0 s, scoring 16.1 s, selection 0.4 s, 20.5 s in all; the 10 s target holds for mining, not for mining plus scoring.
- **Determinism.** Same bytes for 1 and 4 threads and any input order (`tests/mining.rs`); incremental equals full over random arrival and removal orders (`tests/border.rs`).

**Deviations from this design, decided while building it.**

1. **Constraints join adjacent items only** (`flow(i→i+1)`, `same_art(i→i+1)`, `last(i)` on context edits). Every constraint is then local, so matching is an exact dynamic program and growth by projection agrees with it.
2. **Levels:** five per event (exact, prototype, coarse, head, kind). Act heads are HeadSub identities; cannot-link is "different HeadSub identity", so an act prototype never spans identities and the taxonomy stays a tree. A kind-level read or edit is an item only with a constraint; continuation reads must be bound (`same_art`/`flow`). Kind labels are never targets.
3. **Search:** a deterministic **beam** (8 children per node, evidence-carrying extensions first, then P(pattern | label)) and an **equal budget share per target** (2M / targets), so frequent noise targets cannot starve the rest. Truncation is per target.
4. **Closure** absorbs a pattern only by super-patterns with the same number of context items; longer contexts with the same sessions compete in MDL instead.
5. **Gates:** consistency ≥ 0.9 replaces contrast for patterns whose context ends with a failed act; V > 0 for directives; guards (context ending in a failed act or `last(edit)`) need a Beta lower bound ≥ 0.5 instead of V > 0 (p_soon is 0 for a failure context). The days gate is capped at the corpus's distinct days.
6. **MDL:** reflexes code their continuation events (hit/miss stream per context end, offset codes, refinement bits); context events stay free. One code table per delivery shape (avoids per failing runner), filled best-gain-first (lazy greedy); V, support and the canonical string break ties.
7. **Anchor-level measures** (lift, contrast, p_soon, p_next, Δturns, target variant) use a deterministic sample of at most 300 sessions; support and the posterior use every session. Lift/contrast anchors are events of the context's last item's type.
8. **Next-action predictor:** a reflex fires while its context ended ≤ G events ago and its act has not happened, if its training hazard beats the order-1 model's training accuracy for its own guess.
9. **Incremental mining** re-mines when s_min, the alphabet or the target set changes, when more than `border` sessions were added since the last full mine, or when any expansion decision (climb or beam) flips. On small random corpora most additions flip a climb decision, so the full re-mine is the common path there; equality always holds.
10. **Embeddings:** without substrate embeddings, a 256-d signed feature hash of structural features stands in (the hashed half of §1.2).

**Gaps.** The next-action (+5 points) and ECE (≤ 0.05) targets; scoring cost at 400k events; steering-by-influence is implemented on the input (`Influence`) but no importer fills it yet; Obs/Fail nodes are not emitted (polarity on acts and `same_art` edges carry the dead-end and linked-files evidence); causal lift (E4) is not computed.

## Status note: G3 (2026-09-28)

**Implemented and integrated behind `learn.engine`.** The default is still `miners`: with it the L0 bench reproduces the committed baseline (J 0.6307).

- **Compiler** (`crates/adage-graph/src/compile/{mod,shapes,glob,ids,legacy}.rs`). Selected reflexes, and every candidate that passed its gates (see deviation 2), become routes by structure alone:
  - the lookup (`prompt =flow=> act`);
  - read-batch (`act =same_art=> read`);
  - avoid: a failed check, then the check that works. It is announced at session start when at least half the traces run the runner, otherwise as `after_failure` → `run`;
  - obligation: `last(edit)` → one regenerate act;
  - scope: `edit` → an act whose argument is a segment of the edited path. This compiles to `run` with an `edit_segment` slot;
  - an environment step after an edit: `run`, a nascent reflex;
  - sequence: 2–3 regenerate or verify steps after an edit, the first being a regenerate or environment step. A step repeated at once counts once;
  - contexts of 2–3 events (`Trigger::Context`).

  Legacy shapes keep the miners' ids (`lookup-work-item`, `read-linked-files`, `dead-end-<stable(W)>` / `<stable(W,S)>`, `finish-line-<stable(g,c)>`). New shapes get `<shape>-<stable(canonical trigger ‖ step ‖ slot names)>`. A run or sequence that directs an avoided runner is dropped before emission. The glob lattice never produces `**/*.ext`.
- **Model** (`adage-tracks`):
  - `Step::Sequence { commands }` has 2–3 plain commands and no slots. Its strength is at most the lowest ceiling of its commands, and it is never inhibit. It is rendered as a numbered list from a fixed template (checked by the safety oracle), judged on its first command, and never produces a Stop request.
  - `Trigger::Context { items: Vec<CtxItem> (2–3), within (1–8) }` takes items from `CtxItem::{Act, Fail, Edit, PromptValue, Intent}`. Both are validated in `io.rs`.
  - **Context triggers are compiled but inert until G4.** The runtime never matches one, and `Triggers` adds nothing for it, so the fast path is unchanged. `adage eval routes` lists them separately, as neither true nor false, and `rehearse.py` ignores them.
  - `interlock.rs`: `avoids` covers every command of a sequence, and `same_point` gains `Context` (identical items, or one list a suffix of the other).
  - Hidden directories under the tracks root are never read as tracks.
- **Engines** (`adage-learn/src/{engine,shadow}.rs`).
  - `graph` compiles into the normal track directories: legacy shapes in their legacy tracks, new shapes in `<repo>/graph`.
  - `shadow` writes the miners' tracks as usual and the graph engine's to `<repo>/.shadow/<track>`. It also writes `<repo>/.shadow/shadow-report.json`, listing routes only the miners learned, routes only the graph engine learned, and routes that differ by trigger, step, slots, strength or state. `adage condition --engine shadow` prints the same report.
  - Instinct routes are priors (κ = 3 at 0.7), used in the prime test only.
  - The dormant verdict for a graph route reads the scorer's counts for its own pattern (`dormant::verdict_counts`), with no kind dispatch.
  - Each graph route's `why` block holds the pattern, backoff depth, gain, lift, confidence, prototype members, three example traces and the prior. It is stored in the track's hidden `.why.json`, which is outside the canonical digest and never rendered to the agent. `adage review` shows it in text and in `--json`.
- **Evals.**
  - `conventions.rs` gains `Scope`, `EnvSync` and `Sequence` (with a `steps` field). It reads a generated bundle's conventions and scores bundles and suites with `adage eval routes <bundle|suite> --engine miners|shadow|graph`. Every route counts in any state except extinguished, so nascent reflexes count.
  - `evals/gen`: the python-make stack generates three new conventions:
    - `scope`: `packages/<name>/`, checked with `make -C packages/{part} check`;
    - `env_sync`: a `requirements.txt` edit calls for `make deps`, and CI fails on a stale lock;
    - `sequence`: `make gen` then `make check` after a registry edit, replacing the finish line.

    It also adds `scoped` and `deps` tasks with their selfchecks and scripted behaviour, and a new suite, `specs/newkinds.json` (3 bundles).
  - Existing specs regenerate byte for byte: the new keys are left out when off, and the new tasks are drawn after everything else.
  - `rehearse.py --engine graph` matches routes by shape as well as by id.

**Results.**

- **Parity** (`crates/adage-eval/tests/g3.rs`), recall and false routes, miners / graph:

  | Unit | Miners | Graph |
  | --- | --- | --- |
  | demo-haiku45, default split | 4/4, 0 false | 4/4, 0 false |
  | synthetic, default split | 4/4, 0 false | 4/4, 0 false |
  | adversarial, default split | 4/4, 0 false | 4/4, 0 false |
  | each of the 6 smoke bundles | — | same recall as the miners, 0 false |

  On the smoke suite, `rehearse.py --engine graph` recovers 28/28 conventions with no unexpected route. On other splits (task-grouped folds, temporal) the graph engine loses the dead end on demo-haiku45 fold 2, and one or two conventions on three adversarial splits. The causes are support too thin for the guard's Beta lower bound, and failure outputs with no renderable signature. False routes are 0 on every split.
- **New capability** (newkinds suite, 28 traces per bundle). On all 3 bundles the graph engine recovers:
  - scope: `make -C packages/{part} check` after edits to `packages/**/*.py`;
  - env_sync: `make deps then make check` after edits to `requirements.txt`, as a **nascent**;
  - sequence: `make gen then make check` after edits to `registry/*.csv`.

  It also recovers every other convention, with no decoy and no unexpected route. The miners recover none of the three; their `make gen` finish line counts as part of the sequence. No learner code names these kinds.
- **L0**, `engine = graph` (the core suite, task-grouped folds): J 0.9759 against 0.6307 for the miners. False directives per session fall from 0.49 to 0.32, and turns saved at the window are equal (0.66). Route F1 is 0.979 against 1.0: one fold misses a dead end. All guardrails pass: unsafe 0, fast-path mismatches 0, route precision 1.0.

**Deviations decided while building it.**

1. **Judgement by shape.** A compiled route's evidence and strength are counted again on the traces, the way its trigger and step read them. Lookup and linked files use the miners' counters; after-edit, avoid and after-failure routes use counters in `engine.rs`. The strength uses the miners' Wilson rule. This is the §5 parity switch, so changing engines does not move a strength by itself. Context routes keep the reflex's posterior.
2. **Gated but unselected candidates compile.** MDL may not select a deliverable seen in few traces: a dead end in 5 of 30 traces can cost more bits than it saves. `Learned::unselected` (at most 2,000, best value first) feeds the compiler after the selected reflexes. This applies to every shape except contexts, which compile from selected reflexes only.
3. **The lookup** is G1's value-slot route (`graph::lookup_routes`), whether or not MDL selected a `prompt =flow=>` reflex: the value-slot test is the lookup's pattern.
4. **Glob tree.** The lattice's tree is the set of paths the traces touched, a pure function of the traces, so in G3 a directory glob is always precise. A repository tree can be passed to `Corpus::of`.
5. **Not compiled in G3:**
   - a check or observation after an edit: this is the agent's own habit and was never a convention on any dataset;
   - `after_command` → `run`: off by default, because on the datasets it directs the agent's next check, which is a false route;
   - prompt-intent contexts: intent families still come from `adage-learn/src/intent.rs`, because a prompt prototype needs the pinned model. G5 moves them.
6. **Scope binding.** An edit's path segments are recorded as flow sources. An earlier read of the same path is the token's first source, however, so the flow edge may start at the read. A first continuation command is bound as `edit_segment` either way when its variable argument is a segment of every edited member path.

**Gaps.** Parity on some non-default splits (above). `Trigger::Context` is inert until G4. The glob lattice has no repository tree. The `why` block is not yet shown in `adage graph --learned`. The node-npm stack does not generate the new kinds.

## Status note: G2 tuning (2026-09-28)

The three G2 targets the first note missed, revisited. Measured on the same corpora (`adage eval mine`: demo-haiku45 and synthetic on `group:task:3`, the six `evals/gen` smoke bundles on their `tasks.json` split both ways) and the same 2,000 × 200 synthetic scale test (`tests/mining.rs`, release). This machine has 4 cores, shared with other builds, so timings are at 4 threads (`min(8, cores)`).

| | before | after |
|---|---|---|
| next-action top-1, demo-haiku45 (721 positions) | 0.402 (order-1 0.399) | **0.449** (order-1 0.399, order-2 0.395): +5.0 points (36 positions; 4.99 strictly) |
| synthetic (136) | 0.801 (0.801) | **0.824** (0.801; order-2 0.824): +2.2; ceiling 0.815 (below) |
| smoke mean (6 bundles, 371 positions) | 0.506 (0.506) | **0.525** (0.506; order-2 0.555): +1.9 |
| ECE, demo-haiku45 | 0.120 (268 selected pairs) | **0.017** over 106,953 (session, candidate) pairs (0.077 before calibration); selected reflexes 0.116 over 268 (90% CI 0.084–0.163) |
| ECE, synthetic | 0.063 (197) | **0.022** over 6,253 (0.041 raw); selected 0.068 over 197 (CI 0.040–0.102) |
| ECE, smoke (per bundle) | 0.001–0.484 (33–39) | 0.013–0.119 over 1,038–1,772 pairs each, mean 0.084 (0.144 raw); selected 0.07–0.31 over 33–39 (CIs 0.04–0.46) |
| 2k × 200, 4 threads | 18.0 s (scoring 13.0 s) | **9.0 s** with other builds running (load 3–4): prototypes 1.7, mining 2.3, scoring 4.5 (with calibration), selection 0.4; 6.9 s unloaded (scoring 2.8) |
| 2k × 200, 1 thread | 37.9 s (scoring 29.6 s) | **17.8 s** (scoring 9.4 s, mining 6.0 s) |

**Speed (`score.rs`).** The scorer no longer rescans sessions. `Stats` keeps, per label, the positions in each session of its posting list, and a join is a posting-list intersection followed by the same dynamic program as `Db::reach` over the items' position lists (merging, or probing around the previous row when it is much shorter; an existence join stops at its first match). `Cache::build` collects every join the patterns need before scoring (Y alone; each context suffix X_k; X_k ++ Y), computes each context's end positions once and extends them for every continuation, reads mined patterns' sessions from the miner, and runs these units and the background counts on all threads through `par_map` (an atomic cursor; results in input order, so the thread count changes nothing). The background count first finds the Y matches whose continuation completes and then every anchor they can follow, instead of trying Y from every anchor near a Y. The learned output is byte-identical to the old scorer's (checked on a 400 × 120 corpus with `SCALE_DUMP`; 20,000 closed patterns). On an unloaded machine the 4-thread run was 6.9 s (prototypes 1.8, mining 1.9, scoring 2.8, selection 0.4) and the 1-thread run 18.1 s (scoring 9.4 s); the rest of the single-thread time is mining (6.4 s).

**Next action (`score::NextModel`).** The engine's prediction is now the variable-order backoff model of §5, not selected reflexes over an order-1 fallback: a chain of contexts, coarse to fine, each shrinking its counts towards the previous distribution with a Beta/Dirichlet prior of κ = 2 (a context with no evidence stops the walk: the longest context with evidence speaks). The base rate escapes to a novel act PPM-C style, a tenth of the mass stays on the base rate (conformance), and the session's own earlier transitions count twice (a cache: agents repeat themselves). The chain is chosen per training set, by task-grouped inner cross-validation (most top-1 hits, then fewest nats), among three: taxonomy over mixed labels `h0,m0,h1,m1,m2` (head, then the exact act or the head-level path, orders 1–3), taxonomy over exact labels `h0,x0,h1,x1,x2`, and exact `x0,x1,x2`. The eval is fair: all three predictors (engine, order-1, order-2) see the same alphabet-only history of the held-out session and predict the exact label of the next alphabet event when it is an act, on the same folds. Explored and dropped (no held-out gain, or a loss on another corpus): the prompt's intent prototype (−3 to −5 points: held-out tasks have new prompts), prototype and coarse levels instead of heads, PPM-C escapes at every level, κ 1 or 4, the last act, edit phase and position features, averaging chains, predicting the head first, and letting selected reflexes override the model (they fired at 10 of 721 positions).

**Ceilings.** `HeldOut.oracle` is, per context order k = 1–3, the held-out data's own best top-1 for that exact context (what a predictor knowing the test answers per context would get) and H(next | context). Synthetic: 0.809 / 0.838 / 0.846 (0.53 / 0.43 / 0.42 bits). Its generator (`adage eval generate synthetic`) is known exactly: after `edit acme/billing.py` the next act is `make test` with probability 1/2 and one of two failing runners with 1/4 each, and every other act position is determined (the lookup, `make gen` after a schema edit, `make test` after a failure). With about 2.62 act positions per work-item session and 1 per chore session, the Bayes-optimal accuracy is (48 × 2.12 + 4) / (48 × 2.62 + 4) = **0.815**: order-1 + 5 points (0.851) is not attainable in expectation on synthetic; order-1 already gets 0.801 and the engine 0.824 on this sample. Smoke: 0.66–0.73 / 0.70–0.77 / 0.74–0.80 in-sample; each bundle has 8 training sessions and 53–64 held-out positions (one position is 1.6 points), and its scripted sessions vary by session index (`rehearse.py` `work`), which no history shows. Order-2 is ahead there (0.555) because the exact successor of an exact pair recurs verbatim in scripted sessions; on demo-haiku45 it is behind order-1.

**Calibration (`score::Calibration`).** Each learn fits a monotone map from a candidate's posterior mean to held-out outcomes on its own training sessions: task-grouped inner folds (3), candidates re-mined and re-scored on each inner training side exactly as the outer evaluation does (below 400 sessions; above, the cheap form recomputes each candidate's posterior from its own opportunity and support lists), pairs binned by 1/200, isotonic regression by pool adjacent violators, linear between blocks. `Scored`/`Reflex` gain `conf_cal` (the mapped mean) and `conf_lb_cal` (the same quantile of a Beta with the calibrated mean and the posterior's concentration); `Learned.calibration` holds the map. The held-out ECE counts every (held-out session, candidate) pair where the candidate's context occurs (n above), not only selected reflexes. Re-mining matters: with the cheap form on demo-haiku45 the map made ECE worse (0.090), because a pattern mined from all training sessions is flattered by the inner fold's outcomes. Selected reflexes stay over-confident (winner's curse: they are chosen for high training confidence); their intervals are wide at 33–268 pairs.

**Kept.** Determinism (1/4 threads, input order) and incremental-equals-full (`tests/mining.rs`, `tests/border.rs`); parity (`tests/kinds.rs`), recall 1.00 on every dataset and smoke bundle. `DbSession.task` added (inner folds group by it). Public API: fields added only (`Scored`, `Reflex`, `Learned`, `DbSession`); `score::score` takes the prebuilt `&Cache`; `HeldOut` drops the reflex-firing counts.

**Gaps.** Smoke misses both accuracy (+1.9) and ECE (0.084 mean) on 8-session training sets; demo-haiku45 reaches +5 points to within one position, well inside sampling noise at 721 positions; the chain candidates were chosen while exploring these corpora (the per-fold choice among them is honest). Selected-reflex ECE is 0.07–0.12 on the datasets. Single-thread time is still dominated by mining. The compiler still reads `conf`/`conf_lb`; switching strengths to `conf_lb_cal` is a G3 decision.

## Status note: G4 (2026-09-28)

**Implemented: context triggers fire at hook time.** The default engine is still `miners`.

- **Index** (`adage-tracks/src/context.rs`). Per repository with context triggers,
  `Index::compile` stores a `CtxNet` under `contexts`:
  - the **alphabet**: every distinct `CtxItem`, a symbol id being its rank among the sorted
    canonical forms, with a digest of the whole;
  - the **dnet**: a trie over reversed item sequences, each node listing `(route, within)`.

  Like `triggers`, it is derived from the routes and is outside `generation`.
- **State** (`adage-runtime/src/context.rs`). `SessionState.ctx` is a `CtxState`: at most 16
  symbols, each with `since` (alphabet events after it; none older than 16), `seen`, the alphabet
  digest, and `last_fail`. Serialised, it is a few hundred bytes at most.
- **Symbolisation in `decide`.**
  - A batch call gives `act` symbols: exact key first, then every item key of the same HeadSub
    identity that the key meets.
  - A failure gives `fail` symbols (the signature must be in the output).
  - An edit or write gives `edit` symbols (the glob).
  - A prompt gives `prompt_value` symbols (a token the type accepts) and `intent` symbols (one
    embedding per decision, shared with intent routes).
  - After each alphabet event the net is walked from its symbols through strictly older ones: at
    most 3 deep, each step ≤ 8 events back, and fired when the path's largest gap ≤ `within`.
- **Delivery.** A fired context route directs a run step, or a sequence, from the fixed template,
  once per session, with a follow judgement on its (first) command. It makes no obligation and no
  Stop request, and approach locking applies.
- **Interlocking.** `same_point(Context, x)` holds when one event can be the last item of both (two
  contexts), or the last item's event fires the other trigger (after-edit on an overlapping glob,
  after-command on an overlapping key, the same failure).
- **Fast path.** `Triggers` holds the words, globs, value types and intents of every item. The
  replay compares `ctx` too, and the runtime tests run every event through both paths.
- **Rebuild.** A decision records `ctx` when its event changed it, and `rebuild` takes the last
  one. A state whose digest differs from the index's is reset, both on rebuild and live at the
  next alphabet event.

**Results.**

- **Fast path.** 0 mismatches on the three datasets with `engine = graph` (the `g3.rs` gate, now
  with context state compared). On all three newkinds bundles, with the learned routes plus a
  context route (`g4.rs`), it skipped 167/396, 183/358 and 177/396 events with 0 mismatches.
- **A context fires in replay** (`g4.rs`). `[edit registry/*.csv, make gen] → make check` fired in
  exactly the 4 traces per bundle that ran `make gen` after a registry edit, at that turn, and
  nowhere else.
- **End to end through `adage hook`** (`crates/adage-cli/tests/g4_hook.rs`).
  - Setup: learn with `--engine graph` on 21 traces per bundle, then approve the environment step
    in review.
  - Held out, per bundle: both traces of the last scoped, deps and control task, and the second
    trace of the last registry task. That is 7 traces (21 over the suite), each event replayed as
    a Claude Code payload through a fresh `adage hook` process.
  - Scope fired once, at the batch of the first package edit, bound to the edited package
    (`ledger`, `records`, `routing`), with no Stop request after it.
  - env_sync fired at the first `requirements.txt` edit.
  - The sequence fired at the first registry edit, and the context route at the next `make gen`
    batch.
  - Controls received only the dead ends announced at the first prompt.
- **Latency.** Measured with `hook_latency` (release, no model: the model files cannot be
  downloaded here). The G3 build (`base`) and the G4 build were run by turns, 1,000 runs each, two
  rounds, on a shared, loaded 4-core machine:

  | Scenario | p50 base → G4 (ms) | p95 base → G4 (ms) |
  | --- | --- | --- |
  | no routes for the repository | 2.00 → 1.98, 3.06 → 2.54 | 6.46 → 7.15, 8.20 → 8.19 |
  | structured, no match (prompt) | 4.71 → 4.68, 5.87 → 5.70 | 9.28 → 9.36, 11.95 → 12.06 |
  | structured, ticket prompt (fires) | 5.38 → 5.34, 5.40 → 5.40 | 11.68 → 11.38, 10.54 → 10.26 |
  | intent routes, model missing | 4.59 → 4.60, 5.10 → 5.07 | 9.60 → 9.99, 8.37 → 8.10 |
  | batch, no match (context routes) | 2.00 → 1.99, 2.03 → 2.03 | 8.03 → 8.05, 5.11 → 5.44 |
  | batch, a context route fires | 5.06 → 5.30, 4.81 → 4.90 | 11.91 → 12.01, 8.91 → 8.42 |

  - Medians agree within 0.25 ms. The largest is the batch where a context fires, +0.09 to +0.23
    ms, which is the symbolisation and walk on top of a full decision.
  - The p95s move by −0.5 to +0.7 ms between rounds, both ways. This is contention noise that the
    +0.3 ms target cannot be resolved against on this machine, so the target is **not
    demonstrated**. It is not contradicted either.
  - The index parse is unchanged for repositories without contexts. A net is a few hundred bytes
    per repository, so **sharding was not needed** and is not built.
  - The intent-with-model scenario was not measured. The embedding path is unchanged, apart from
    sharing one embedding between intent routes and intent items.
- **Fidelity: unchanged.** demo-haiku45 round 2: 36/36 decisions with `--exact-keys` (32/36 under
  the current matching), events 35/36. Round 1: 11/18 control and 0/18 adage, as before.
- **L0 bench.** `engine = graph`: J 0.9759, the same as G3, with every guardrail passing
  (fast-path mismatches 0). The miners: J 0.6307, unchanged. The objective holds but does not
  improve, because the graph engine selects no context reflex on the core datasets (see gaps).

**Deviations decided while building it.**

1. **The ring is 16 symbols, not 8.** One event may carry several symbols, and a three-item
   context can span 16 alphabet events. `since` counts alphabet events after each entry rather
   than holding positions.
2. **The gap is counted over the repository's context alphabet**, which is smaller than the
   miner's alphabet (every frequent label), so a runtime gap is never larger than the mined one.
   Contexts may span prompt segments: resetting at a prompt would make every prompt matter to
   the fast path.
3. **`last_fail` is `(key hash, seen)`.** It keeps a failed call that Claude Code lists again in
   the next batch from counting as a successful command. It lapses at the next alphabet event.
4. **No slot binds from a context** (as `io::validate` already required), and a context never
   makes an obligation.
5. **An index compiled with `triggers` but without `contexts`** (a G3 index) matches no context:
   its triggers lack the items' words, so matching would break the fast path's exactness.
6. `adage eval routes` scores a context route as the one-event trigger on its last item. The
   python rehearsal still ignores context routes (`rehearse.py`, not changed here).

**Gaps.**

- No dataset or gen bundle yields a *selected* context reflex yet: the newkinds conventions
  compile to one-event triggers. So the gates use an authored context route, and the L0 objective
  cannot move.
- The +0.3 ms p95 target is unresolved on a shared machine.
- Intent-with-model latency was not measured (no model files).
- `rehearse.py` does not score contexts.
- `last_fail` remembers only one failure.

## Status note: G5 (2026-09-28)

**Implemented: the graph engine is the only learner.** The hand-written miners (`adage-learn`'s
`mine.rs`, `intent.rs` and the shadow engine) are deleted, and with them `route_kind`, `KIND_*`,
`MINER_KINDS`, `steers`, the `unsteered(kind)` plumbing, `score::kind`, `learn.engine` and
`--engine`. The retirements were not done one per bench bless: every dataset was measured against
the miners' last build first (the parity table below), and the engine was fixed until it kept their
floor everywhere, then the miners went in one change.

- **One track.** Every reflex of a repository is written to `<repo>/learned`
  (`adage_learn::TRACK`). The legacy tracks (`work-items`, `finish-line`, `dead-ends`, `intents`,
  `graph`) are folded into it on the next learn, with their `review.yaml` decisions and proposals,
  and removed. Ids are unchanged (`compile::stable`, renamed from `legacy`).
- **Shapes, not kinds.** `adage_tracks::shape(route)` is the delivery shape (`run`, `avoid`,
  `read-batch`, `obligation`, `sequence`, `advise`); the directive wrapper's `kind` carries it. The
  eval scores directives per `shape@trigger` (`adage_eval::score::point`).
- **Steering by influence (§3.2).** Importers parse each directive's body for what it names
  (`adage_runtime::render::named`: directed and avoided commands, listed files), anchor it to the
  step it arrived before (`import::Directed`), and the bridge marks every later event it names
  exposed (`events::Influence`). Exposed events are no evidence, and exposed sessions are
  subtracted from a pattern's opportunities before the posterior. A directive whose body cannot be
  read (old or unknown) influences everything after it. The judge and the dormant verdict count
  only traces not exposed to the route.
- **Prompt intents** are compiled in the graph pipeline (`compile::intent`, a port of the old
  families: prototype prompts, pinned model, `Trigger::Intent`). Without the model, prior intent
  reflexes are kept as they are.
- **Claims** (`adage_learn::claims`): the verify-claim route moved here, and `adage-train` also
  compiles a lookup claim where the exploration sessions looked the prompt's item up.

**Deviations, decided while closing the gaps.**

1. *Two-value types (gap a).* A slot with only two distinct values, every one flowed from the
   prompt at a rate ≥ 0.6, is a *thin* slot: it becomes a type only when its runs are one literal
   prefix (with a letter, ending in punctuation) followed by class runs, and only through the
   outcome lift, so it is prime at most. W-204's rejection is kept. Literal uses (a value typed
   into commands of ≥ 2 sessions without flowing from a prompt) are left out of a slot's test and
   negatives. `top_share` is now `≤`.
2. *Dead ends (gap b).* The beam roots recoveries first (a failing root 2, a last-edit root 1),
   a failed act's kind label is its failure family (`fails: <signature>`), and the guard lower
   bound gate is removed (the exposure subtraction replaces what it protected against). Dead ends
   are announced at session start (`announce_at_start = 0`). A dead end with several runners keeps
   those whose failures share the signature consistently, at most `learn.dead_end_signatures`,
   ordered by traces.
3. *Obligation beside a sequence.* An after-edit sequence whose first step regenerates also
   compiles that step's obligation (the finish line's id and Stop request). The sequence itself is
   then kept only when ≥ 75% of its traces ran its steps one right after another
   (`engine::together`); otherwise its later steps are separate habits and only the obligation is
   delivered. On the demo, round 2 keeps `make gen then make test then make ci` beside the
   obligation (a deliberate addition, prime); the simulator's histories and the review tests keep
   only the obligation.
4. *Read batches* are judged by the old linked-files counter, moved into `engine::judge_read_batch`.

**Parity** (`crates/adage-eval/tests/parity.rs`, conventions recovered; miners = their last build):

| Dataset / split | Miners | Graph |
| --- | --- | --- |
| adversarial default, 3:1, temporal | 4/4 | 4/4 |
| adversarial group:task:3:0 | 1/4 | 2/4 |
| adversarial group:task:3:2 | 2/4 | 2/4 |
| demo-haiku45, every split but round1 | 4/4 | 4/4 |
| demo-haiku45 round1 | 3/4 | 3/4 |
| live acme-billing-sonnet5 / acme-billing | 3/4 / 2/4 | 3/4 / 2/4 |
| live harbor-dispatch-sonnet5 / harbor-dispatch | 2/4 / 3/4 | 2/4 / 3/4 |
| live node-billing-s22 / node-billing-uuid-s42 | 1/5 / 0/5 | 1/5 / 0/5 |
| live node-clinic-s23 / node-logistics-s21 | 2/4 / 2/5 | 2/4 / 2/5 |
| live py-billing-hash-s32 / py-billing-s11 | 0/5 / 1/5 | 0/5 / 1/5 |
| live py-clinic-s12 | 2/5, 1 false | 2/5, 1 false (`dead-end-60822e`, the same: every generic runner fails there, the convention names one) |
| live py-logistics-s13 | 2/4 | 2/4 |
| synthetic, every split | 4/4 | 4/4 |
| gen smoke (6 bundles) | all | all |
| gen idshapes (11 bundles) | 3–5/5, 2 false | 5/5, none false |
| gen newkinds (3 bundles) | 4/7, 3/6, 4/7 | 7/7, 6/6, 7/7 |

Unsafe directives 0 and fast-path mismatches 0 everywhere. `parity.rs` holds, per dataset and
split, the conventions the miners recovered, and fails if the engine recovers fewer or any false
route beyond theirs; `every_generated_convention_is_learned` runs the three gen suites when
`ADAGE_GEN_SMOKE`, `ADAGE_GEN_IDSHAPES` and `ADAGE_GEN_NEWKINDS` point at them.

**Replay fidelity.** The recorded decisions came from the miners' index, so the recorded-index
tests (`replay_reproduces_the_live_round2_decisions`) are unchanged. Against today's learned index
(`adage eval fidelity --learned`, `fidelity::run_learned`), a recorded directive counts when it
arrives at its event or earlier, and a recorded id from before content-derived ids is matched by
its trigger and step (`finish-line-1` is `finish-line-e7e809`). Both demo rounds: 36/36 sessions
get every recorded directive in time. Round 1: 34/36 identical (two control sessions get one more
linked-files directive, the command-pattern algebra). Round 2: none identical, and the
differences are exactly: every session also gets the second dead end (`dead-end-21cb2c`), ten
schema-editing sessions get the sequence beside the obligation, four control sessions get one more
linked-files directive, and one session (`eval2-W-2045-r2-holdout`, a recorded run id) gets its lookup and dead end
at session start, one event earlier than live (`gates.rs`,
`replay_with_the_learned_index_gives_every_recorded_directive`). The recorded transcripts were
not re-recorded.

**Bench.** Re-blessed (`evals/baselines/l0-core.jsonl`, "G5: graph engine is the only
learner"): J 0.6307 (miners) → 0.6148. Turns saved at the window 0.6604 → 0.6604, recall 0.9722 →
0.9722, route F1 1.0 → 1.0, precision@window 0.3484 → 0.3603, false directives per session 0.4906
→ 0.5000, ECE 0.2754 → 0.2730; final check (temporal) J 0.5937 → 0.5937. The one difference is
the sequence `make gen then make test` in two demo folds, 8 of its 9 directives followed; the one
not followed costs 0.0159. Guardrails: unsafe 0, fast-path mismatches 0, route precision 1.0. The
G3 measurement of the engine (J 0.9759, 0.32 false directives per session) came with a missed
dead end in one fold (route F1 0.979). The G5 engine gives up that difference for the miners'
floor (the deviations above); which change costs how much was not attributed.

**Gaps.**

- The live datasets' misses are the miners' misses (most of their conventions have too few takers).
- The sequence rule (deviation 3) is a threshold on adjacency, chosen on the demo and newkinds.
- ~~A directive delivered twice at one edit (obligation and sequence)~~ Closed in the launch polish: a sequence that begins with the obligation's command carries it (the runtime delivers the sequence alone; met by its first step, a Stop request if not), which recovered the L0 bench to J 0.6307.

## Status note: real traces (2026-09-28)

**The learner forms the reflexes a developer would expect from real traces.** Dogfood: 38 real
Claude Code traces of engineers building Adage (a Rust workspace; sub-agent transcripts included),
conditioned with default thresholds in a scratch `ADAGE_HOME`. Before this change, no learned reflex
formed. After it (`adage review`):

| Route | Trigger | Step | Strength | Evidence |
| --- | --- | --- | --- | --- |
| `finish-line-0ba99a` (obligation) | after an edit to `crates/**/*.rs` | `cargo fmt --all` | fire | 25/26 traces, Wilson lower bound 0.81 |
| `run-b53bf7` (a check) | after an edit to `crates/**/*.rs` | `cargo clippy --workspace --all-targets --locked -- -D warnings` | prime | 20/26, 0.58 |
| `run-6e3b86` (a check) | after an edit to `crates/**/*.rs` | `cargo test --workspace --locked` | prime | 19/26, 0.54 |

Nothing else is delivered: the checks after `docs/*.md` edits are explained by the Rust edits in
the same traces, and `cargo fmt --all --check` after a Rust edit checks what the obligation
already owes. The causes, and what changed (`adage-graph/src/compile/{variants,glob,shapes}.rs`,
`adage-graph/src/mine/seq.rs`, `adage-learn/src/{engine,bridge}.rs`):

1. **Command variants** (`compile::variants`). Agents ran `cargo test -p adage-cli`,
   `cargo test --workspace --locked --no-fail-fast`, `cargo test -j4 --workspace --locked`, …: no
   spelling held 80% of the runs, so nothing was directed. A continuation item now directs its
   family's **covering variant** when no spelling dominates: a command line agents actually ran,
   successfully, in at least `learn.min_support` traces, that covers at least 80% of the family's
   runs. W covers V when V meets W (W plus options), or when V narrows W: the same identity and
   effect class, every option of W that selects a part of the repository (`-p`, `--lib`, `-C`,
   `--test`, …) present in V, and each operand of W selecting at least what V's does (a
   directory selects the files under it, `tests` covers `tests/test_x.py::t`); options that select
   nothing (`-j4`, `--locked`, `--no-fail-fast`) may differ. So the directed command is always one
   agents ran, and the most common repo-wide form wins only when it covers the narrowings. The
   judge counts a trace's run of any covered variant as evidence for W (`engine::Want`).
2. **Glob fragmentation** (`glob::widen`, `shapes::widen_repo_wide`). Evidence was split per
   sibling directory (`crates/adage-cli/src/*.rs`, `crates/adage-eval/src/*.rs`, …). After-edit
   routes that direct the same repo-wide step (no operand or option selects a part), or the same
   scoped step with its `{part}` slot, are generalised together to the least general glob that
   covers them and is precise over the tree (precision over the tree's files of that extension):
   the siblings' glob (`crates/*/src/*.rs`), then their common prefix (`crates/**/*.rs`). As in
   the lattice, a glob is never rooted in a wildcard (`**/*.rs`, `*/*.py`): top-level
   directories with nothing in common keep their own globs. Evidence is counted again on the new
   glob.
3. **Effects of compound and redirected commands** (`bridge::effect_of`). `cargo fmt --all &&
   cargo clippy …` was classified as a whole (author, when a later part commits or the output is
   redirected), so every part was dropped as a target. Each part now keeps its own class, and a
   command whose output is redirected to a log is still the command it ran. The judge reads
   compound steps part by part.
4. **Checks after an edit compile** (shape table: `one Regenerate/Verify Act` after
   `last(edit)`). A verify after an edit compiles to `after_edit → run`, delivered at prime at
   most (never fire, no Stop request). To keep it from being the agent's habit rather than the
   repository's (the G3 reason for leaving it out), the contrast must hold between the bounds:
   the one-sided 90% Wilson lower bound of its rate after the edit must beat the upper bound of
   its rate in traces that made no such edit by `learn.finish_line_contrast` (a handful of
   traces on either side proves little; this is what keeps the demo's `make ci` after schema
   edits out on its temporal split, 4 of 4 traces after the edit against 14 of 25 without; a
   check scoped to the edited part keeps the plain contrast). It must also be worth suggesting
   (V's factor 1 − p_soon): at least 2 traces, and a tenth of them, missed it after such an edit;
   a check every trace already ran after the edit (`make ci` after `acme/*.py` on the adversarial
   set, 10 of 10) adds nothing. Its runs count only when the edit called for them, not a failed
   check just before (`python -m pytest` fails, then `make test`), and a check the traces show
   as a dead end's working command (run within five steps of a different check's failure in at
   least `min_support` traces), or one a learned avoid already names, is left to that dead end.
   It must span two distinct tasks (below), a check of what an obligation already owes is
   dropped, and among routes directing the same step on different globs, a route is kept only
   when the traces that edited its files and none of a stronger route's still ran the step
   (**explaining away**: clippy after docs edits). Where agents always ran the checks in order
   after the format step, they form a sequence instead (`cargo fmt --all`, then clippy, then the
   tests: the anonymised fixture).
5. **The miner's beam** (`mine::seq`) keeps one slot, at the root, for the best head-level edit
   before the target: in busy traces housekeeping between the last edit and the finishing steps
   outranked it and no `edit → check` pattern was grown.
6. **Distinct tasks** (`engine::one_task`). A directing route's supporting traces must span at
   least two tasks (a trace's task is its normalised first prompt): RepoBench's nascent reflexes
   were one task's scratch script, repeated by arms and reruns. An avoid is exempt: a failing
   environment (`No module named pytest`) is the same whatever the task.
7. **Parameterised test runs** (`shapes::segment_template`). A test run whose path argument goes
   on past the edited part (`npm test -- packages/runtime-core/__tests__/x.spec.ts`) also stands
   for its part's directory (`npm test -- packages/{part}`); the template covering most runs wins,
   bound at runtime from the edited path (`edit_segment`). The parts are the item's own edited
   segments, or, below a directory they share (never at the top level), the segments the traces
   edited under it, so one item's single part still shows the part varying. A glob that fixes
   the segment writes it into the command.

**RepoBench-Local traces** (offline, `--demo`, no instincts, every archived trace; `adage eval
routes` against `evals/repobench/conventions`):

| Repository | Traces | Before | After | Conventions |
| --- | --- | --- | --- | --- |
| attrs | 58 | 0 routes (4 nascent scratch-script routes in the report's run) | 0 routes | 0/1 |
| click | 98 | 0 | 0 | 0/1 |
| marshmallow | 43 | 0 | 0 | 0/1 |
| urfave-cli | 43 | 0 | 0 | 0/1 |
| vue-core | 35 | 0 | `npm test -- packages/{part}` after an edit to `packages/*/*/*.ts` (prime, 20/30) | 0/2 (scored false: the convention spells it `pnpm test {part} --run`; its notes call `npx vitest run packages/<pkg>` the same) |

The Python repositories' convention is a dead end (`pytest` is only in `.venv`), but their traces
recover by installing (`pip install pytest`, `pip install -e .`), an environment step, not by the
`.venv` interpreter, so no avoid is learnable from them; click's `.venv/bin/python -m pytest tests
-xvs` after test edits misses the contrast bar. urfave-cli's `make generate` sequence was
never run by the agents. No scratch-script reflex forms any more.

**Gates.** L0 bench J 0.6307, unchanged (route precision 1.0; not re-blessed). Parity, gates,
fidelity, G4 and the gen suites pass with no new false route. `crates/adage-learn/tests/rust_workspace.rs`
holds an anonymised fixture distilled from the dogfood traces (invented crates, prompts and paths)
and asserts the `cargo fmt --all` obligation and a repo-wide check (a run, or the later steps of a sequence) form after Rust edits, with no docs reflex, no `--check` beside the
obligation and no narrow run directed. `live_learn.rs`: a first learn may now log an instinct the
traces confirm as extinguished (the learned dead end replaces it). `compile.rs`: a check after an edit is now a prime run (G3 dropped it).

**Gaps.** Dead ends whose working command varies (a per-trace test file) direct the covering form
only when agents ran it; none did on RepoBench. A parameterised command is bound only from an
edited path segment: a test file matching the edited source (`tests/test_x.py` for
`src/x.py`), or a placeholder left to the agent, is not built. Task identity is the first prompt,
not an intent cluster. The explaining-away rule needs traces that edited one glob and not the
other.
