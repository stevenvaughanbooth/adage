# Changelog

All notable changes to Adage are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow
[Semantic Versioning](https://semver.org/). The release workflow uses the section for a tag's
version as the GitHub Release notes.

## Unreleased

### Changed

- Branding: the pitch is now **"Agent traces become reflexes."** Reflexes (learned) and instincts
  (shipped) are the units; tracks and packs are the artefacts. "Put your agent on rails" is retired
  as the tagline (README, plugin and npm descriptions, Design Spec §17).

### Added

- **E2: actions from hooks (Track C, milestone C5).** When an approved action reflex's stimulus
  fires in a hook Adage already owns (an edit matching its paths arms a finish-line obligation, or
  that obligation's Stop request is due), `adage-exec hook` runs the response itself:
  - **Staged** in the sandbox with the repository as a writable overlay: `bwrap --overlay-src`
    where bubblewrap has overlays (0.10 and later), else an overlayfs (`userxattr`) mounted in the
    sandbox's own user and mount namespaces under Landlock and seccomp (Landlock treats an overlay
    as distinct from its layers, so the rule is added on the mount itself); as root, a copy of the
    worktree owned by nobody; on macOS an APFS `clonefile` copy (compile-checked). The upper layer
    lives in a staging directory the sandbox is never granted.
  - **Verified**: the change set is read from the upper layer (whiteouts, opaque directories;
    redirects and metacopy refuse). `.git` always refuses. A regenerate action may change only
    regular files under its declared `outputs` globs (cache globs are discarded); a verify action
    may change no source (cache globs are kept). Symlinks and special files are never applied.
  - **Applied** atomically per file (temporary file, fsync, rename) and only if each path's state
    still equals the state recorded before the run; otherwise nothing is applied.
  - The directive becomes a fixed-template result ("Adage ran `make gen` after your edit to
    registry/currencies.csv: updated 1 file (…). Nothing else changed.", `exec="e2"` on the
    wrapper, so the session counts as steered), the obligation is marked met, and no Stop request
    follows. Refused or failed: the plain E0 directive and the reason. At most one action per
    hook event, never in the control arm, never under OpenCode (its bridge waits one second),
    within 20 s; registrations that run `adage-exec` give the edit and Stop events a 30 s hook
    timeout. The outcome (`applied | refused | failed`) goes to `$ADAGE_HOME/exec/actions.spool`
    and the event's `exec` record, never the output. Three refused change sets under one approval
    revoke it automatically; `adage review` lists it as `exec-revoked`.
  - `adage review allow-exec <track> <route> --level e2 --outputs <glob>… [--cache <glob>…]
    [--effect verify|regenerate]`: always explicit, per reflex, never automatic. It stages the
    action once, applies nothing, and shows the full diff of the declared outputs; a staged run
    that fails or changes anything else approves nothing. Policy must opt in (`max_level = "E2"`;
    the default is now E1); environment-effect actions need the managed file's
    `allow_environment_actions`. `adage-exec run --level e2` stages and applies by hand.
  - Pack routes declare `exec: { level: E2, outputs: [...], cache: [...] }`.
  - Gate G4: `crates/adage-exec/tests/stage.rs` runs 500 randomised hostile regenerate commands per
    staging backend (writes outside the outputs, deleted sources, symlink and hardlink tricks,
    `.git`, huge files, FIFOs, escapes to the parent and outside) and finds 0 writes outside the
    declared outputs. `crates/adage-cli/tests/actions.rs` drives an `evals/gen` python-make
    finish-line bundle through `adage review allow-exec` and `adage-exec hook claude …`.
- **Vendored scripts (L-vendor).** `adage share --vendor` carries each repository script's bytes in
  the pack as `scripts/sha256-<hex>` (checked against its pin; only scripts run by an interpreter
  named in argv, `python3`, `node` or `sh`, and without deps). `adage add` installs them read-only
  in `$ADAGE_HOME/cas/`. A reflex that runs one is exec-only: E0 never directs it, and
  `adage-exec hook` brings it into play only once it is approved. It runs from the read-only CAS
  (or a read-only private copy when the sandbox runs as nobody), hash-checked at every run, by the
  interpreter in argv (the shebang is ignored). Approval needs policy's `allow_vendor_scripts`,
  shows the script's full text, and is never automatic.
- **Attestations.** `adage share --attest <results.jsonl>` embeds bench result records (`arms`,
  `scorecard`, `aggregate`; at most 16) as `attestations/<id>.json`, each validated against the
  bench result schema (now version 2: tiers `l0-sec`, `l1`, `l2`, `exec_level`, and the `arms`
  record with deltas and harm counts). `adage review` and `allow-exec` show them marked
  "self-reported by signer <fingerprint>"; they never grant a strength or a level.

- **Graph engine G2 (Track G): prototypes, closed-episode mining, scoring, MDL selection**, in
  `crates/adage-graph` (not wired into `adage learn` yet). `events.rs`: the miner's event-sequence
  input (typed events with batches, prompt segments, value-flow and same-artifact edges), with a
  converter from `input::Session`. `proto.rs`: per-family prototypes (blocked exact kNN,
  deterministic Louvain, must/cannot-links, singleton rules, a five-level label taxonomy, τ
  calibrated by successor purity, epoch files). `mine/`: target-rooted, gap-bounded episode growth
  with flow/same_art/last constraints, taxonomy climb, closure, a per-target node budget, and
  incremental mining with a negative border (property-tested equal to a full re-mine).
  `stats.rs` (Beta quantile, Wilson), `score.rs` (Beta posterior with parent-context backoff,
  lift, contrast, consistency, value, conformance, held-out next-action and ECE), `select.rs`
  (MDL code tables per delivery shape, `Reflex` with a `why`). `adage eval mine
  <dataset|gen-dir>` reports reflexes, held-out next-action accuracy against an order-1
  baseline, ECE and convention parity. On the committed datasets and the six `evals/gen` smoke
  bundles every lookup, linked-files, dead-end, instinct-violation and finish-line convention is
  mined and no decoy is selected (`tests/kinds.rs`). Next-action accuracy only matches the
  order-1 baseline so far, and ECE is 0.06–0.12 on the datasets (targets: +5 points, ≤ 0.05).
- **E1 prefetch (Track C, milestone C3).** `adage-exec hook <harness> <event>` runs the same hook
  pipeline as `adage hook`, now the shared `adage-hook` library. For a directive whose route has
  an E1 approval, it runs the route in the sandbox (the L-ref script hash is re-read at execution),
  re-checks the typed `<adage-data>` block with `check_data_block`, and delivers it beside the
  directive ("That lookup has been run; its result is below."; `exec="e1"` on the wrapper). Exit
  predicates are evaluated on the projected fields. Any failure delivers exactly the E0 output;
  with nothing approved the output equals `adage hook`'s byte for byte (the goldens run against
  both binaries). Spools record `exec` (level, exit, bytes, sha256, truncated, duration), never
  the output. E1 sessions count as steered for the route's kinds and the intent miner.
  - `adage review allow-exec <track> <route> --level e1 [--raw] [--fields] [--exit-when]
    [--timeout-ms]` and `revoke-exec`, and `adage exec enable|disable|status`: a person's
    decisions, refused inside agent sessions. `enable` swaps installed hook registrations to
    `adage-exec hook` and writes the marker the Claude Code plugins coordinate on.
  - A second Claude Code plugin, `adage-exec` (`plugin/claude-code-exec`); the Adage plugin's
    session-start shim goes quiet while execution is on, so hooks never fire twice.
  - `adage eval <dataset> --exec-e1`: an offline estimate of the lookup calls E1 would have
    replaced and the turns saved beyond E0.
  - `crates/adage-exec/examples/exec_latency.rs`: no-match latency of `adage-exec hook` against
    `adage hook` (within 1 ms at p95), and what a delivered E1 adds.

- **Packs are what `adage share` and `adage add` exchange (Track C, milestone C1).**
  - `adage share` emits a signed pack, not a v1 track. It upgrades a learned track (live routes
    only, with repository scripts pinned L-ref against the repository it runs in), re-signs an
    installed pack, or signs a pack directory. The redaction check runs over every byte, vendored
    scripts included. A vendored script that names URLs or hosts, runs network tools, uses
    absolute paths or reads `*_TOKEN`/`*_KEY`-style variables is listed and needs `--ack-scripts`.
    Approvals, `review.yaml`, proposals and `trust.json` never travel. The signature uses the
    namespace `adage-pack` (`adage_sign::PACK_NAMESPACE`). `--format v1` still writes a v1 track.
  - `adage add` accepts packs. It reads the directory strictly: no symlinks and no smuggled files.
    It verifies the digest and signature against the trust anchors: personal `trusted-signers`,
    then the TOFU-pinned repository file, then `--trust`. An `allowed_signers` entry restricted to
    `adage-track` does not authorise a pack. The pack is validated in full, including its closure.
    Scripts are re-classified locally as environment, and a minimal policy file is honoured
    (`[publishers] deny`, `[exec] allow_vendor_scripts`). The pack is installed exactly as verified
    under `tracks/@imported/<owner>/<name>/`, and its vendored scripts are copied read-only to
    `$ADAGE_HOME/cas/<sha256>`. It creates no execution approvals. v1 tracks are still accepted
    (namespace `adage-track`) and are upgraded in place to a pack.
  - Resolution against a repository (`adage_pack::bind`) gives each route either live, with the
    highest level it may be approved at, or inert, with a reason. It checks, in turn:
    - applicability by repository key (including the `<name>@<root commit>` form), by fingerprint
      over the declared facts, or by a compiled detector;
    - `requires.os`, and tools by PATH lookup only (nothing is run);
    - that templates and models are known to this build;
    - the SHA-256 of each L-ref script, which must be a regular file inside the root with no
      symlink on the way and at most 1 MiB, plus its deps;
    - that the route has a runtime form. A shape with none becomes inert with the reason "shape
      not yet supported at runtime".

    Only a signature, schema or closure failure refuses a whole pack. Live routes compile into the
    index as bound tracks under `tracks/@imported/@bound/<owner>/<name>/<repo>/`, at probation and
    yellow at most, and the recipient's review decisions carry across re-binds. Each carries
    `IndexedRoute.exec` (route exec digest, resolved scripts and hashes, `max_level`), which the E0
    hot path ignores. `adage doctor` (a new `packs` check) and `adage review` (text and `--json`)
    show inert routes with their reasons and the pack's version and digest.
  - The core instincts ship as the pack `adage/core-instincts` (`adage_pack::instincts`), with
    detector applicability (`detector = "*"` over this build's catalogue). Binding it gives exactly
    the instinct binder's routes; a test pins this.
  - `adage_tracks::index::Index::compile_root` attaches the exec bindings. `adage learn`, `add`,
    `review` and `doctor` all compile through it.
- **Track packs, format `adage-pack/1` (Track C, milestone C0).** A new crate, `adage-pack`,
  defines the pack: one track namespace in route format v2 (shapes run, avoid, after, read,
  advise, obligation and sequence; typed inputs and outputs; argv steps; exit predicates; an
  execution request that grants nothing), closed over everything it references and
  content-addressed. It reads packs as untrusted input (a file allowlist with no symlinks and at
  most 256 files, 1 MiB per file and 64 KiB per script; UTF-8 without NUL, control or invisible
  characters; unknown fields refused; strict, anchored and bounded contract types; plain argv
  tokens; repository-relative paths and globs; effects re-classified locally), computes the
  closure (types, L-ref and L-vendor scripts, tools, templates, models, links, the implicit
  after-command and avoid-instead edges; no dangling references, no unreferenced bytes, no after
  cycles or chains over four) and checks it against the pack's `CLOSURE` byte for byte. It
  computes the route exec digest (canonical JSON without evidence and aspect, with type
  definitions and script hashes inlined: the future approval key) and the pack digest (the track
  digest algorithm, with `CLOSURE` included and `pack.sig` excluded), signs and verifies in the new
  sshsig namespace `adage-pack`, and upgrades v1 track directories to packs (L-none, or L-ref
  pinned against a repository). Nothing uses packs yet: `adage share` and `adage add` move to them
  in C1, and nothing executes. A debugging binary, `adage-pack inspect|verify|digest|upgrade`, ships
  with the crate. See docs/packs.md.
- **`adage-exec`: the execution core (Track C, milestone C2)** (new crate and binary, optional;
  the `adage` binary still never runs a repository or agent command, now checked by a test that
  scans the adage crates for `Command::new`). `adage-exec run --repo <dir> --argv '<json>'
  --effect observe --level e1 --output typed --fields '<json>' [--dry-run]` runs a step only when
  every gate passes, in order: slots bound to values their strict contracts accept (no shell,
  ever); the declared effect not below the local classification (scripts and interpreters are
  environment); policy; an approval equal on every field of the key (repository, route exec
  digest, argv templates, scripts and hashes, effect, level, output mode, sandbox profile id,
  signer), or automatic approval of the inert-observe class, off by default until gate G3; and an
  enforced sandbox, else nothing runs. Linux: Landlock (ABI ≥ 1, best effort to 9) plus a seccomp
  filter applied before `execve`, user and network namespaces where available, and root dropped
  to nobody; bubblewrap as the fallback; macOS: `sandbox-exec` with a generated SBPL profile;
  Windows: unavailable. The environment is built from nothing (sanitised `PATH`, scratch `HOME`
  and `TMPDIR`, fixed locale and git variables; never agent sockets, tokens or cloud
  credentials); git runs with `-c` hardening, and a repository whose config sets `core.fsmonitor`,
  `sshCommand`, `filter.*`, textconv, aliases or includes loses git's inert status. Limits per
  level (E1: 1.5 s, 64 KiB captured, 4 KiB returned; E2: 10 s, 256 KiB, 16 KiB), `RLIMIT_AS`
  2 GiB, `RLIMIT_NPROC` 256, `RLIMIT_FSIZE` 64 MiB, and the process group killed and reaped.
  Typed output keeps only declared fields that pass their types (`repo-path` inside the root,
  `repo-path-list`, `git-rev`, `int`, `bool`, `enum`, `pattern`), re-serialised by Adage; raw
  output (separately approved, policy-gated) is stripped, defanged and scanned for secrets,
  canaries and instruction-shaped text; any failure withholds the whole block. Output reaches the
  agent only in the fixed `<adage-data>` template. `adage-exec approve`/`revoke` refuse inside an
  agent session and need a terminal; approvals live in `$ADAGE_HOME/approvals.json` (0600, atomic)
  and lapse with a field diff when the route changes. Policy: `$ADAGE_HOME/policy.toml` plus a
  managed file whose denies win and whose `require_sandbox` the personal file cannot lower.
  `adage-exec doctor` reports the sandbox (Landlock ABI, namespaces, bwrap, sandbox-exec). The
  L0-sec escape suite (`crates/adage-exec/tests/escape/`) runs every payload under each available
  backend: 0 escapes. See docs/execution.md.
- **`commands.toml`: an `inert` field** (default false), true only for `cat`, `head`, `tail`,
  `wc`, `ls`, `rg`, `grep`, `jq` and the `git` subcommands `status`, `log`, `diff`, `show` and
  `rev-parse`: programs whose behaviour repository files cannot turn into running code. It gates
  automatic approval in `adage-exec` and is pinned by a test.
- **`adage_eval::safety::check_data_block`**: the safety oracle for execution data blocks (the
  fixed template, route, byte count, no tags or control characters in the payload, declared and
  valid typed fields or no free text, no instruction-shaped raw text, no canaries).
- **`adage-train`: skill → reflex training** (new crate and binary, optional; the `adage` binary
  still never executes anything). `adage-train plan --from <SKILL.md|CONTRIBUTING.md|Makefile|
  package.json|workflow.yml|runbook.md>` extracts typed claims deterministically (verify,
  regenerate_after(glob), lookup, avoid, environment; commands normalised and effect-classified,
  anything above regenerate reported as unsafe and never exercised; Makefile, package.json and CI
  through adage-instincts' declared facts), generates seeded tasks (implicit and explicit phrasings,
  at least 25% controls) and prints a budget estimate; it is free. `adage-train run --plan` is a
  dry run unless `--execute`, which refuses inside an agent session or without a terminal, and
  drives Claude Code headlessly on `git clone --local` copies with no origin, a per-session home and
  config directory, an environment allowlist, restricted tools, the sandbox settings, a spending cap
  and bounded concurrency. Exploration transcripts are mined into a separate staging directory under
  the repository's key with provenance `synthetic:skill:<sha256>@<run>` (never green, never beyond
  probation, not activated); `--validate N` compares baseline, skill-in-context and reflex arms on
  held-out tasks. `adage-train report` says per claim `compiled → route <id>`, `contradicted` (the
  skill is stale), `never exercised` or `unsafe`, with cost and an acceptance table. A pluggable
  `Harness` with a scripted fake runs the whole pipeline end to end in tests. See docs/training.md.
- **Instincts, integrated end to end.** Adage's built-in defaults (`adage-instincts`: prebuilt
  routes bound from a repository's Makefile, package.json, lockfiles, pyproject and generated-file
  markers) now reach agents on day 0. `adage learn` binds them per repository after mining, into the
  track `<repo>/instincts`; `adage learn --instincts-only [--repo-path <dir>]` binds a repository
  with no sessions, and so does the first automatic learn; `adage init` shows what would bind.
  Routes carry their origin (`Evidence.origin`: learned, instinct, imported, authored, synthetic)
  and origin counts (`Evidence.by_origin`); a new advisory step (`Step::Advise`) renders fixed
  sentences for generated files, lockfiles and the package manager. Instincts have the lowest
  precedence (index de-duplication by provenance, interlocking), are always yellow, never make a
  Stop request, say "Adage default for … repositories (from this repository's Makefile)" instead
  of "Learned by Adage …", and a finish line from one says "Makefile declares this.". A new
  judgement, `followed_failed`, retires an instinct route at the next learn after three (or a
  derail rate whose Wilson lower bound reaches 0.5 over five judgements), recorded in
  `review.yaml` and `learn.log`. Approving one environment-effect instinct in `adage review`
  approves its pack's for the repository. `adage doctor` counts instincts per repository; `adage
  review` lists them with their origin. Config: `instincts.enabled`, `instincts.packs`,
  `instincts.disabled`. `adage eval --instincts-only` measures day-0 value (demo: 10 of 11
  directives correct, 3 of 4 failed verify runs answered, 0 unsafe) and is a gate; offline evals
  and the L0 baseline stay learned-only by default (`--instincts` includes them). See
  docs/instincts.md.
- **Learned value types, core (Track G, G1).** A new crate, `adage-graph`, finds value slots and
  learns a type for each, from sessions alone: `flow.rs` matches tokens of prompts, earlier
  outputs and earlier paths against later argv positions and option values (keyed by HeadSub
  identity, position or option, and affix), and keeps a sink as a slot when it sees at least 3
  distinct values, at least 60% of them flow-bound and none holding half. `types/` reads values
  as run skeletons (`BIL-1041` → `U{3} '-' D{4}`), anti-unifies them pointwise, with star folding
  or a union of at most 3, chooses along the lattice by MDL, and enforces the safety constraints
  (anchored, `plain_token` alphabet, no leading `-`, at most 128 bytes, not word-like, rejects
  every negative). `adage-core` gains the shared tokeniser (`tokens.rs`) and the O(len) run
  matcher and regex writer (`vtype.rs`); the two agree on every string (property test). Not yet
  wired into `adage learn`, the runtime or tracks: behaviour is unchanged. New
  `adage eval types <gen-dir|dataset-dir>` compares learned types with today's hard-coded ticket
  detector. `evals/gen` specs take `id_shape` (`prefix_dash`, the default and unchanged,
  `hash_num`, `uuid`, `path_num`, `branch`, `mixed`), `specs/idshapes.json` covers them, and
  `rehearse.py` writes a holdout of 200 fresh ids and 200 near misses. On that suite learned
  types reach lookup recall 1.00, accept 1.00 and reject 1.00 with no unsafe acceptance on every
  shape; the hard-coded detector finds nothing outside `X-123` ids. See
  docs/tracks/track-g-graph-engine.md, "Status".
- **Track G, G0 (event-graph substrate) and G1 integration (learned value types made live).**
  Importers keep every prompt (`Session.prompts: Vec<PromptObs { text, at_step }>`,
  `StepObs.prompt`; `Session.prompt` is still the first; the parse cache re-parses older entries,
  `cache::SESSION_FORMAT`). `adage-graph` gains session facts (`facts.rs`: prompt and act events,
  artifact, output and failure nodes, `follows`/`binds`/`reads`/`writes`/`yields`/`fails_with`/
  `recovers_with` edges, token occurrences, bounded), node embeddings (`embed.rs`: SHA-256-seeded
  feature hashing plus the static model when installed, quantised) and a content-keyed,
  owner-only facts cache (`$ADAGE_HOME/cache/facts`). `adage-learn/src/bridge.rs` converts
  sessions for the graph engine. New `adage graph --learned --dump <repo>`. New learn parameter
  `learn.engine` (`miners`, the default and unchanged, `shadow`, `graph`) and `adage learn
  --engine`: under `shadow`/`graph` the lookup route (`lookup-work-item`, same id) takes its slot
  from a learned value type (`SlotSource::PromptValue`, with `OutputValue` and `EditSegment` also
  added and bound by the runtime), effect ceilings are applied before any typed slot can direct,
  and session facts are computed. The hook binds a typed slot with the shared tokeniser and the
  O(len) matcher, compiling no regex for it; `plain_token` and the contract stay the final gate;
  the fast path accounts for typed prompt slots exactly. `io::validate` checks that a type's
  runs and regex agree and that each run step's slot is bound by its trigger. `adage eval types
  --end-to-end` learns with the graph engine and replays prompts through the hook: on
  `specs/idshapes.json` lookup recall 1.00 and 0 unsafe binds. `rehearse.py` takes `--engine`,
  learns without instincts, matches lookups by ids and dead ends by canonical runner (this fixes
  `test_learn_recovers_every_convention`, which failed on instinct routes and on `node --test` /
  `npm test` runners). With default parameters the L0 baseline is reproduced exactly.

- **Tunable engine parameters and the L0 optimisation bench.** Every number the runtime and the
  learner tune on (follow window, directive budget, stop requests, history; support, green and
  yellow bars, retain rate, dead-end consistency and signatures, finish-line and intent contrasts,
  review materiality, stale exposure) is now one `EngineParams` (`adage_tracks::params`) with
  defaults equal to the old constants, a canonical JSON form and a `params_hash`. The hook still
  decides with the defaults and learning still uses the `default`/`demo` presets: behaviour is
  byte-identical. `adage eval` and `adage eval routes` take `--params <file.toml|json>`. New
  `adage eval bench run|sweep|report|bless`: the `core` suite (demo and synthetic on task-grouped
  folds, a temporal final check, the adversarial set for safety), one objective (turns saved at
  window − 2 × false directives + route F1, per session, averaged over the folds), guardrails
  (no unsafe directive, no fast-path mismatch, route precision within 0.05 of the baseline), a
  deterministic JSON Lines result schema (`evals/bench/result.schema.json`), grid, random, Sobol
  and Halton sweeps over `evals/bench/spaces/learner.toml`, Pareto and baseline reports, and a
  committed baseline (`evals/baselines/l0-core.jsonl`) that the defaults must reproduce byte for
  byte. See docs/evals.md, "Bench".

- **Live learning.** Adage now learns by itself while agents work: when a session stops or ends
  (Claude Code and Codex `Stop`/`SessionEnd`, OpenCode `session.idle`/`session.deleted`), the hook
  starts a detached background learner for that repository, at most once per
  `learn.min_interval_secs` (a request inside the interval is learned when it ends, never
  dropped), one learner per repository at a time. The hook never waits for it and its output never
  changes (Stop p95 +0.3 ms, +0.7 ms when it starts a learner). `adage learn` still works, and
  both write the same tracks. Turn it off with `adage config set learn.auto false` or
  `ADAGE_AUTO_LEARN=0`. See docs/live-learning.md.
- **`$ADAGE_HOME/config.toml` and `adage config`** (`get`, `set`, `--json`): `learn.auto`,
  `learn.min_interval_secs`, `learn.thresholds` (`default` or `demo`; also used by `adage learn`),
  history sources for automatic learning, `learn.parse_cache`, and `runtime.holdout`.
  Environment variables still win. `config set` validates, writes atomically, and refuses inside an
  agent session.
- **`learn.log` records what changed.** Every learn, manual or automatic, appends one line per
  repository with `kind`, `trigger` (`auto`/`manual`), `repo`, `duration_ms` and `changes`: each
  route that is new, drafted, retired, changed aspect (yellow → green), changed evidence, or got a
  proposal, in fixed-template summaries with no prompt, command, output or path. Failures are
  `learn_error` lines. Existing fields are unchanged.
- **Parse cache.** Parsed transcripts are kept in `$ADAGE_HOME/cache/sessions`, keyed by path,
  size and modification time, so re-learning parses only what changed: 60 ms instead of about
  300–670 ms for 207 real transcripts (130 MB). Results are byte-identical with and without it
  (`--no-cache`, `learn.parse_cache = false`).
- `adage_store::spool::read_from(path, offset)` for following spools live, safe against records
  still being written; `adage_store::lock` file locks.
- `adage doctor` checks the configuration and live learning: on or off, each repository's last
  automatic learn and its outcome, a stuck learner, and requests left waiting.
- **A terminal UI.** `adage watch` (also `adage doctor --watch`) is a live pane for a split
  terminal: health lamps re-checked every few seconds, every hook call as it happens (directives
  by aspect, holdout sessions withheld, followed and derailed judgements, finish lines met, stop
  requests), and what learning changed, from `learn.log`. `adage ui`, or plain `adage` at a
  terminal, is a full-screen app with Live, Routes (evidence, live counts, exact directive text,
  and review decisions through `adage review`'s own code), Health (every doctor check with one-key
  fixes: install hooks after showing the file and diff, fetch the model with progress, learn this
  repository now as a background learner would, turn auto-learn on, open the fail-open log) and
  Settings (every `adage config` key with its source, edited through `adage config set`'s code).
  The learning ticker says when a background learner is running. `--demo`
  replays a scripted scenario through real hook calls into a temporary data directory; `adage
  watch --once` prints one frame as text. Built on ratatui and crossterm; the terminal is restored
  on every exit, including a panic, and `NO_COLOR` is honoured. Hooks never load any of it.
- **`adage init` is a guided wizard at a terminal.** It asks before each step (register hooks per
  harness, fetch the model, run a first learn, turn auto-learning on) and ends with the doctor's
  summary. Piped, inside an agent session, or with the new `--print` (or `--yes`), it prints the
  report it always has.
- `adage init`, `adage survey`, `adage doctor` and `adage graph --serve` (the signal box served
  from the binary).
- `adage eval`, which replays held-out sessions offline and scores what Adage would have said. It
  ships with committed datasets (real demo, synthetic, adversarial) and CI gates.
- A Codex rollout importer: `adage learn --codex-sessions`.
- A harness simulator (`crates/adage-sim`), which replays mock sessions end to end through each
  harness's native hooks.
- Windows support. CI covers macOS arm64 and x64, Linux x64 and arm64, and Windows x64.
- **Hooks record their fail-open errors.** A caught panic or internal error (unreadable stdin, a
  malformed payload, a state file that cannot be updated, a spool that cannot be appended) still
  makes the hook say nothing and exit 0, and now also appends one line to
  `$ADAGE_HOME/fail-open.log`, which `adage doctor` reports. Entries carry the time, harness,
  event, an error class and a fixed message or panic location; never a prompt, tool output,
  command or path. The log rotates at 256 KiB, keeping one previous file. "Nothing to do" (no
  tracks for the repository, a kill switch, an ignored event) is not recorded.

### Fixed

- **Failure attribution in command chains.** A failed `make gen && pytest` marked `make gen` as
  failed too. The learners and the eval now attribute a failed call to its last sub-action (every
  operand keeps it in an `||` chain, where all of them ran and failed).
- **Over-broad dead-end warnings.** A dead end named every runner that failed before the working
  command, including `python3 -m unittest`, which passes once `ACME_ENV` is set. Dead ends are now
  keyed by failure signature: a runner is named only if its failures share the route's signature
  and it failed in at least 9 in 10 of the sessions that ran it. Route ids are unchanged.
- **A second way to fail went unnamed.** There was one dead end per working command, so a second
  failure signature before it (the demo's `python -m pytest`, "command not found", 4 sessions)
  was not warned about. There is now one dead end per (working command, signature), at most three
  per working command, each runner named under one signature only. The most common signature's
  route keeps its id; each further one gets an id derived from the command and signature. Offline
  recall rose (demo `hash:0.3` 0.81 → 0.84, synthetic 0.97 → 1.00) with no precision lost.
- **Unrelated repositories without `origin` shared routes** when their directories had the same
  name. Such a repository is now keyed `<directory name>@<root commit>`; the root commit is read
  straight from `.git` (loose objects, packs, packed refs, linked worktrees) without running git,
  and remembered in `$ADAGE_HOME/repo-keys`. A repository whose root cannot be read (no commits
  yet, SHA-256, shallow, alternates) is keyed by a hash of its path. Routes learned under the bare
  directory name keep working until `adage learn` re-keys a repository of that name. New
  dependency: `miniz_oxide` (MIT OR Zlib OR Apache-2.0), with `adler2`.
- **A corrupt session state started the session over**, so a dead end could be announced twice
  and a finish-line obligation forgotten. When neither state slot can be read, the hook now
  rebuilds the state from the session's spool (up to 4 MiB): emitted routes, obligations, stop
  requests, arm, repository and counters. Spool records gain `kind` and `repo` fields.
- **Oversized payloads were cut off, not drained** (Design §13.3). A payload over 8 MiB is now
  read to the end and discarded, so the harness writing it never sees a broken pipe; the same
  with a kill switch on. The refusal is recorded in `fail-open.log`.
- **Environment steps reached agents without a person's approval** (Design O11). Unknown commands
  are classified as environment, and a learned route running one was delivered at yellow at once.
  Learned and imported environment routes now start as drafts, which are never delivered, and
  wait in `adage review` (reason "approval") until `approve` puts them into service. Re-learning
  keeps the approval while the step is unchanged; a different command waits again.
- **Dead-end and linked-file directives stated no exit condition** (Design W2). Both are now
  learned with one ("Ignore this if the user is asking for something else."), and the dead-end
  template renders it on its own line after the command. Routes stored without one render as
  before.
- **Normalisation depended on machine speed.** Pathological commands were cut off by a 50 ms
  parse timeout, so a slow machine could record a command as unknown where a fast one parsed it.
  The cut-off is now a budget of bytes the lexer may read (2^20, re-reads included), the same on
  every machine; a 1 s wall-clock backstop remains for input shapes not yet known.
- **Linux segfault in hooks.** A segfault in tree-sitter-bash's scanner (it calls `isdigit()` on
  full code points) could crash a hook on Linux, given a `{` followed by a large code point. The
  parser now only sees an ASCII shadow of each command, with the same byte offsets.
- **Windows finish-line routes never fired.** Paths under `\\?\` roots, and `C:\…` paths, did
  not resolve or relativize.
- **Eight bugs found by property tests**, including a stack overflow on long `&&` chains, slow
  parses of malformed input (now cut off by a fixed lexing budget, about 7 ms), invisible
  characters passing sanitisation, and a track able to direct a command naming a path outside the
  repository.
- **`adage learn --codex-sessions <dir>` fell back to all of `~/.claude/projects`** when the
  directory held nothing. An explicit source that finds nothing is now an error.

### Changed

- Design Spec principle 1 is now "The agent is the only actor, unless you delegate" (Track C
  §5.1), and Architecture Q4 splits into Q4a (`adage` spawns nothing) and Q4b (`adage-exec` runs
  only approved, sandboxed routes).
- Repository scripts run by `adage-exec` run from a private copy inside a shadow of the
  repository in scratch, so scripts that find their data relative to themselves work.
- Execution approvals are filed under `<track>/<route>` without the track's version.
- **One command-pattern algebra** (`adage_core::pattern`) decides when an observed command is
  the directed one, everywhere: follow judgements, finish-line obligations, after-command and
  after-failure triggers, `adage graph`, interlocking and the eval scorer. A directed command is
  met by the same command with options added (`make gen -j4` meets `make gen`;
  `./scripts/wi-lookup W-1 --json` triggers a route mined from `python3 scripts/wi-lookup W-1`),
  never by another subcommand or script (`npm run build` no longer counts as following
  `npm run check`, nor `python3 -m pytest` as `-m pytest`). Multiplexers (npm/pnpm/yarn/bun,
  make, cargo, go, `python -m`, uv/poetry run, npx, just, task, git, docker compose, gh) and
  interpreter flag modes (`node --test`) are a data table. A dead end names its runner at that
  level: `node --test`, not `node`. Eval numbers moved (docs/evals.md): demo round 2 precision
  0.7424 → 0.7571, recall 0.7826 → 0.913; round 1 precision 0.6786 → 0.6667. Replay fidelity is
  checked under the matching the recorded hooks used (`adage eval fidelity --exact-keys`, 36/36);
  under the new matching exactly four holdout sessions gain one linked-files directive.
- **Interlocking** (Design §10.6, `adage_tracks::interlock`). `Index::compile` computes a conflict
  matrix per repository: a route that directs a command a dead end avoids loses (left out of the
  index when both speak at the prompt, otherwise filtered at runtime), and of two greens at one
  decision point one is compiled yellow (learned beats imported or authored, then evidence).
  `adage learn` and `adage doctor` report the conflicts. At runtime the first prompt's dead ends
  are chosen before anything else under the caps, and approach locking never emits a directive
  that a red directive already emitted in the session, or in the same decision, avoids. The eval's
  safety oracle and a new property test check both invariants.
- Finish-line globs generalise, conservatively: `a/b/*.ext` and `a/c/*.ext` followed by the same
  regenerate step become `a/**/*.ext` when every session editing such a file under `a/` ran it and
  none ran it after editing one elsewhere.
- `learn.log` has one line per repository per run instead of one per run (a run that found no
  usable session writes one line with `repo` null).
- `adage learn`, automatic learners, review decisions and `adage add` take `$ADAGE_HOME/tracks.lock`
  while they write tracks and the index, so they never interleave.
- The demo and live eval runners set `ADAGE_AUTO_LEARN=0`, so the index stays fixed within a round.
- **Evidence integrity (E1).**
  - *Steering by directive kind.* Directives now carry `kind="…"` in their `<adage-signal>`
    wrapper (the route's kind, from its trigger and step, never its id). Learning excludes a
    session from each miner its directives could influence: a dead end also steers the intent
    miner, and a directive of kind `other`, of unknown kind, or from an old wrapper whose route id
    no miner produces (an import) is excluded from every miner. Old transcripts fall back to the
    route id.
  - *Stale routes.* A route in service that learning no longer finds only because it steered its
    own evidence away (at least half the sessions where it applies were steered on its kind, or
    too little unsteered support is left) is kept as `state: stale` on its last evidence, with
    `evidence.stale_since`, and delivered yellow at most (red stays red). Found again, it returns
    to its normal state; contrary evidence among unsteered sessions (a support rate below the
    retain bar) drops it as before. Every learned route records `evidence.exposed`, the steered
    sessions it could not learn from. `learn.log` has a new change kind, `stale`. Tracks without
    the new fields read as before.
  - *Propensity logging.* Every spool record carries `propensity`, the probability of its `arm`
    at assignment (the holdout share, or one minus it; 1 when `ADAGE_ARM` forces the arm), kept in
    the session state. Older spools and states still read.
  - *Finish-line wording.* "CI fails if this is skipped." is said only when sessions that skipped
    the step are known to have failed (`deviations_failed > 0`); otherwise the directive says
    "Agents in this repository run it after such changes."

- **`adage learn` writes `learn.log`**: one JSON line per run with the sessions read and used and,
  per repository, the tracks written (version, change, route count). No prompt, command or path.
  Rotated at 256 KiB.
- **CI builds the aarch64 musl release target**, natively on the arm64 runner, as it did x86_64.
- **Hooks return early on events that cannot match** (Design §13.2). The index now stores, per
  repository, what can make its routes act, and a hook skips an event that cannot change any
  decision before loading the command table, locking the state or writing the spool: a no-match
  tool batch now costs 1.9 ms p50 instead of 2.8 ms. The eval replay checks on every event of
  every dataset that the fast path changes no decision. Skipped events are not written to the
  spool, so `adage graph` shows fewer off-route commands.

## 0.1.0 - Unreleased

The first version: Adage learns the routes coding agents take and, at each decision point, tells
the agent deterministically what to do next. It never executes anything itself.

### Learning

- `adage learn` imports Claude Code transcripts (and OpenCode exports), normalises every tool call
  into canonical actions (tree-sitter-bash plus a command table kept as data; 315 golden fixtures),
  and mines five route kinds: prompt → lookup, linked files, dead ends, finish lines, and (optional)
  intent families.
- Every route carries its evidence (distinct sessions, Wilson lower bound), an effect class that caps
  how strongly it may be directed, and typed slot contracts learned from real values.
- Intent families (optional): `adage models fetch` / `adage models install <dir>` add the pinned
  potion-base-8M static embedding model (SHA-256 verified); prompts are clustered per repository
  with exact kNN and deterministic Louvain, then split and merged by what the sessions did next.

### Runtime

- Hook client for Claude Code (plugin with exec-form hooks), OpenCode (bridge plugin) and Codex
  (`hooks.json`); `adage install <harness>` prints, or with `--write` merges, the registration.
- Directives rendered from fixed templates; slot values only through validated contracts and safe,
  existing repository-relative paths. Hooks always fail open.
- Daemonless per-session storage: two-slot state files and CRC-framed spools.
- A holdout arm (`ADAGE_HOLDOUT`, default 10%) measures every effect against a control; kill
  switches `ADAGE_DISABLE=1` and `.adage/disabled`.

### Tracks, review and sharing

- The track format with validation (tracks are untrusted input), a canonical digest, and a compiled
  index.
- `adage review` lists routes that need a person's decision; `approve`, `reject`, `demote`,
  `promote` and `pin` are recorded in `review.yaml`.
- `adage share` redaction-checks a track and signs it (native `sshsig`, ssh-agent or key file);
  `adage add` verifies it against trusted signers and installs it at probation.
- `adage status`, `adage report` and `adage graph` (situations → routes → steps → agent responses,
  with live counts).

### Platforms and distribution

- Windows support in the store (positional I/O and `LockFileEx` locking through `std`), the data
  directory (`%APPDATA%\Adage`), home-directory lookup and path handling. Compiles for every release
  target; Windows tests run in CI.
- Release engineering: CI on macOS (arm64, x64), Linux (x64, arm64) and Windows (x64) with rustfmt,
  clippy, tests, cargo-deny, and plugin validation; releases built by dist for macOS, static Linux
  (musl) and Windows, with shell, PowerShell and Homebrew installers, Sigstore build-provenance
  attestations and a CycloneDX SBOM.
- npm packaging: a main package whose launcher runs the binary from a per-platform package chosen
  by `os`/`cpu`; nothing is downloaded at install time. Publishing is off until the package name is
  chosen.
- A Claude Code plugin marketplace (`.claude-plugin/marketplace.json`). The plugin uses its bundled,
  checksum-verified binary, else `adage` on `PATH`, and never downloads at hook time.
- `SECURITY.md`, `CONTRIBUTING.md`, and a cargo-about NOTICE generator.
