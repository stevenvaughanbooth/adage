# Track packs (format `adage-pack/1`)

> Status: format reference for Track C milestones C0 and C1 ([design](tracks/track-c-compiled-skills.md)).
> The crate `adage-pack` reads, validates, writes, digests, upgrades and binds packs. `adage share`
> emits packs and `adage add` installs them ([Using packs](#using-packs)). Nothing here executes
> anything.

A **pack** is the unit of distribution and approval for compiled skills: one track namespace
(`owner/name`) in route format v2, closed over everything it references, content-addressed and
signed. A pack is **inert**: it can *request* an execution level, but only a local approval store
can grant one. A **track** stays the unit of learning and lifecycle (probation, active, demoted,
review, evidence); a pack is a track in format v2 plus its closure, requirements, optional vendored
scripts and attestations.

## Layout

```text
acme-work-items/                 a directory; tar or zip is only transport
├── pack.toml                    manifest (strict TOML)
├── routes/<id>.yaml             route v2
├── types/<id>.yaml              named contract types that are not built in
├── scripts/sha256-<hex>         L-vendor script bytes, named by their own SHA-256
├── evidence.json                aggregate evidence per route, by origin (optional)
├── attestations/<id>.json       optional bench/eval results (informational only)
├── CLOSURE                      canonical closure listing (generated; re-derived and compared)
└── pack.sig                     sshsig over the pack digest, namespace "adage-pack"
```

Nothing else may be present. A pack is refused when it contains:

- a symlink anywhere (including the pack directory itself), a special file, or a directory other
  than `routes/`, `types/`, `scripts/` and `attestations/` (and those hold no subdirectories);
- a file outside the allowlist above (`review.yaml`, `notes.txt`, dotfiles, `routes/x.sh`, …);
- more than 256 files, a file over 1 MiB, a script over 64 KiB, or more than 256 KiB of scripts;
- a file that is not UTF-8, contains NUL, a control character other than tab, CR and LF, or an
  invisible or direction-changing character (`adage_tracks::io::invisible`: soft hyphen,
  zero-width, bidi embeddings, overrides and isolates, BOM). This applies to vendored scripts too.

Route and type files are named by their ids (`routes/<id>.yaml`, `types/<id>.yaml`, ids are
`safe_id`: lowercase letters, digits and `-`, at most 64). Vendored scripts are named by the
SHA-256 of their bytes.

**Never in a pack:** approvals, `review.yaml`, `trust.json`, spools, session state, prompts, bound
slot values, execution outputs or their digests, absolute paths, usernames, `$ADAGE_HOME` paths,
machine keys, and per-machine route state. A route has no `state` field (it is refused as an
unknown field), and evidence has no `stale_since`.

## `pack.toml`

Every table refuses unknown keys.

```toml
format = "adage-pack/1"

[pack]
name        = "acme/work-items"       # owner/name, lowercase [a-z0-9._-]
version     = "1.4.0"                 # major.minor.patch
origin      = "reflex"                # reflex | instinct | trained | authored
description = "…"                     # people only; never rendered to an agent

[applicability]                       # exactly one form
kind = "repo"                         # repo | fingerprint | detector | any
repo_keys = ["github.com/acme/payments", "payments@1a2b3c4d5e6f"]
# kind = "fingerprint"
#   [[applicability.fingerprint]] makefile_target = "gen"
#   [[applicability.fingerprint]] package_script  = "codegen"
#   [[applicability.fingerprint]] path_exists     = "proto/**/*.proto"   # repository-relative glob
#   [[applicability.fingerprint]] lockfile        = "pnpm-lock.yaml"     # a known root lockfile
# kind = "detector"   detector = "U5", catalogue = "core@0.2.0"   (instinct packs only;
#                     detector = "*": any enabled detector of the catalogue)

[requires]
os        = ["linux", "macos"]        # "windows" only when [exec] requested = "E0"
tools     = [{ name = "python3" }, { name = "jq", version = ">=1.6" }]
harness   = ["claude-code >=2.1.200"]
models    = ["potion-base-8m@abcdef012345"]   # intent triggers; never vendored
templates = ["run@1", "read@1", "avoid@1", "advise:generated-file@1"]
command_table = ">=3"
repo_files = [{ path = "scripts/wi-lookup", sha256 = "9f2c…", deps = ["scripts/lib/*.py"] }]

[exec]
requested = "E1"                      # E0..E2: the author's ceiling request; grants nothing

[provenance]
tool = "adage 0.5.0"
origins = { organic = 412, synthetic = 12, declared = 0, instinct = 0, imported = 0 }
```

Rules:

- A tool is a program in the command table (`adage-core/data/commands.toml`): L-none programs are
  exactly those. Presence is checked by PATH lookup at bind (C1).
- `repo_files` pins the repository scripts L-ref steps run. Each entry must agree with the step's
  `script` (path, hash and deps).
- Every requirement must be used by some route (tools, templates, models, repository files):
  the manifest lists exactly the pack's closure, no more.
- Lists hold no duplicates. Repository keys are `repo_key()` forms (no absolute paths).

## Route v2 (`routes/<id>.yaml`)

```yaml
id: work-item-context
summary: Look up the work item named in the prompt    # people only (review)
shape: run                    # run | avoid | after | read | advise | obligation | sequence
trigger: { on: prompt, span: ticket }
entry: true                   # default; false: reachable only through another route's links
inputs:                       # typed parameters, bound from the stimulus and checked against their types
  item: { type: work-item-id, from: { prompt: { span: ticket } } }
steps:                        # 1..=4; E0 renders steps[0] only
  - id: lookup
    run: { argv: ["./scripts/wi-lookup", "{item}", "--json"] }
    effect: observe           # declared; re-classified locally
    script: { path: scripts/wi-lookup, closure: ref, sha256: "9f2c…" }  # or closure: vendor
    outputs:                  # typed output contract (the E1 projection)
      files: { type: repo-path-list, from: { json: files }, max: 5 }
      state: { type: wi-state, from: { json: state } }
  - id: read-linked
    read: { from: lookup.files, max: 5 }
    effect: observe
links: []                     # ["#other-route-id"]
aspect: green
exit:
  - { when: result_equals, step: lookup, field: state, value: Closed }
  - { when: intent_changes }
exec: { level: E1, timeout_ms: 1500, output: typed }
evidence: { opportunities: 10, support: 9, rate: 0.9, by_origin: { synthetic: 2 } }
```

### Triggers

| `on` | Fields | Notes |
| --- | --- | --- |
| `prompt` | `span` (`ticket`, `path`, `sha`, `url`) | The only trigger whose inputs come from the prompt. |
| `after_command` | `command` | A canonical key, `exec <plain argv tokens>`, at most 200 bytes. |
| `after_failure` | `command`, `signature` | `signature` is rendered text (`io::safe_text`). |
| `after_edit` | `paths` | A repository-relative glob (`advice::safe_glob`). |
| `session_start` | | |
| `intent` | `family`, `model`, `centroid`, `threshold` | As v1: unit centroid in 1e-4 units, 8..=1024 dimensions, threshold 0.5..=0.99; no inputs; `model` must be in `requires.models`. At most 64 intent routes per pack. |

### Inputs

Each input has a `type`, exactly one source in `from`, and for `type: enum` its `values`, and for a
list type an optional `max` (at most the type's own cap):

- `from: { prompt: { span: ticket } }`: needs a `prompt` trigger.
- `from: { result: { field: files } }`: a field of the triggering command's JSON output (`a`,
  `a.b` up to four segments, or `*paths`, the repository paths plain-text output mentions); needs
  an `after_command` or `after_failure` trigger.

Every input must be used, by an argv placeholder or a `read.from`. An argv placeholder takes a
scalar prompt input.

### Steps

Exactly one of `run`, `read`, `avoid`, `advise`, plus `effect`. Only a run step has `script` and
`outputs`.

- **`run: { argv: [...] }`**: 1..=32 tokens. A token is plain: `[A-Za-z0-9_./:=@,+{}-]`, braces
  only in whole `{input}` placeholders, and no absolute path or `..` segment in an argument or a
  flag value (`--out=/etc/x` is refused). `argv[0]` is never a placeholder. What argv runs:
  - a program in the command table (**L-none**; a tool requirement). Its effect is classified
    locally with `adage_tracks::io::classify`, and declaring less is refused;
  - a repository path (`./scripts/x`, or an interpreter's first operand that contains `/` or has a
    script extension, as `python3 manage.py`): the step must declare a `script` for exactly that
    path. **An argv naming a repository path without a `script` entry is a dangling reference and
    is refused.** Interpreters never take inline code (`-c`, `-e`, `--eval`, `-p`, `--print`).
  - For a script step the declared effect is not compared with the classifier (which names a
    script `observe` from its name alone); for approval and ceilings a script step is
    **environment** unless a person approves it lower (C2).
- **`script`**: `{ path, closure: ref | vendor, sha256, deps }`.
  - `ref` (**L-ref**): a repository-relative path, its SHA-256, optional `deps` globs; mirrored in
    `requires.repo_files`. Never approved automatically.
  - `vendor` (**L-vendor**): `path` is `scripts/sha256-<sha256>`, the bytes are in the pack, no
    `deps`, and argv runs it through an interpreter named in argv (`python3`, `node`, `sh`); the
    shebang is ignored.
- **`outputs`**: at most 16; each `{ type, from: { json: field }, max?, values? }`.
- **`read: { from, max }`**: `from` names an earlier step's output (`lookup.files`) or an input,
  of a list-of-`repo-path` type; `max` is 1..=10.
- **`avoid: { runners: [[argv]...], instead: [argv], signature }`**: 1..=8 runners, plain argv
  with no placeholders; `instead` classifies as at most regenerate.
- **`advise: { template, slots }`**: one of `adage_tracks::advice::TEMPLATES`, with slot values
  that pass their contracts.

### Shapes

| Shape | Trigger | Steps | Aspect |
| --- | --- | --- | --- |
| `run` | `prompt`, `intent`, `session_start` | run, then run or read | not red |
| `after` | `after_command`, `after_failure` | run, then run or read | not red |
| `obligation` | `after_edit` | run only | not red |
| `read` | anything but `intent` | read only | not red |
| `sequence` | any | two or more runs | not red |
| `avoid` | `session_start`, `after_failure`, `prompt` | one avoid step | red |
| `advise` | `after_edit`, `after_command`, `after_failure` | one advise step, observe, no inputs | yellow |

Each route renders with the template `<shape>@1` (and an advise step with
`advise:<template>@1`), which `requires.templates` must list.

### Effects, aspect and execution

- The route's strongest declared effect has a ceiling (`adage_tracks::ceiling`): author and
  publish are never directed; green only where the ceiling allows it.
- An instinct route (and every route of an `instinct` pack) is yellow.
- `exec`: `level` at most the pack's `[exec] requested`; `timeout_ms` in 1..=600000 (default
  1500); `output` `typed` (default) or `raw`. Avoid and advise routes stay at E0. A request that
  could never be granted is refused: E1 (prefetch) needs observe; E2 (an action a hook runs) is at
  most environment. An E2 action may declare `outputs` and `cache`, each at most 16
  repository-relative globs whose first segment is neither `.git`, hidden, nor a wildcard; a
  regenerate action must declare `outputs` (the only paths it may change). They belong to E2 only.
- `exit` predicates: `intent_changes`, `exploratory_change`, and
  `result_equals { step, field, value }`, whose value must pass the type of that output.

### Evidence

`evidence` (per route) mirrors `adage_tracks::Evidence` without per-machine fields: `opportunities`,
`support`, `days`, `successes`, `deviations_failed`, `rate`, `lower_bound` (in 0..=1),
`median_position`, `turns_saved`, `provenance`, `models`, `exposed`, `origin`, `by_origin`. It is
not part of the route exec digest. `evidence.json` holds, per route id, the counts by origin:
`{"<route>": {"organic": 9, "synthetic": 2, "declared": 0, "instinct": 0, "imported": 0}}`. Every
key must be a route of the pack.

## Attestations

`attestations/<id>.json` (at most 16, each at most 64 KiB, `id` a safe id) are bench result
records in the schema of `evals/bench/result.schema.json` (versions 1 and 2), checked strictly on
read (`adage_pack::attest`): the envelope (`schema_version`, `record`, a 16-hex `run_id`, a `tier`
of that version, `suite`, `adage_version`, `params` with `runtime` and `learn`, a SHA-256
`params_hash`, `split`; optional `git_sha`, `dataset`, `reason`, and, in version 2, `exec_level`),
each record kind's own keys and nothing else, printable bounded strings, finite numbers. The
version 2 `arms` record carries `arms` (per arm, named numbers), `deltas` (metric → number) and
`harm` (name → count).

`adage share --attest <results.jsonl>` embeds a results file's `arms`, `scorecard` and
`aggregate` records (in that order, at most 16). They are covered by the pack's signature, which
proves who wrote the numbers, not that they are true: `adage review` and `adage review
allow-exec` show each one as a single line, **self-reported by signer** `<fingerprint>`.
Attestations are informational only: nothing reads them to set a reflex's strength (aspect), an
execution level or an approval.

## Types

Built in: `repo-path` (repository-relative, no `..`, no leading `-` or `/`), `repo-path-list`
(at most 64), `git-rev` (`^[0-9a-f]{7,40}$`), `int`, `bool` (`true`/`false`) and `enum` (values
inline on the input or output, at most 32).

A user type (`types/<id>.yaml`) reduces to a base, and has only its base's fields:

```yaml
id: work-item-id
base: string            # string | int | enum | list
pattern: "^W-\\d{4,9}$" # string: anchored, strict and bounded
# range: [1, 100]       # int: inclusive bounds
# values: [Active, Closed]   # enum: 1..=32 plain tokens
# item: repo-path       # list: a scalar item type
# max: 5                # list: 1..=64
```

A string pattern must:

- be anchored `^…$` in every branch (checked on the parsed expression, so `^a$|b` is refused);
- refuse every hostile probe: the fourteen of `io::contract_is_strict` (`a b`, `a;b`, `a|b`, `$(x)`,
  `` `x` ``, `a&b`, a newline, `../x`, `-x`, `a>b`, `a<b`, quotes, the empty string) plus tab, CR,
  NUL, backslash, and zero-width, bidi-override, isolate and BOM characters;
- be bounded: the longest match is at most 128 bytes (`adage_core::vtype::MAX_LEN`), so `+`, `*`
  and `{n,}` are refused;
- compile within a 64 KiB regex size limit, and be at most 256 bytes long.

Enum values are plain tokens (`[A-Za-z0-9][A-Za-z0-9._-]*`, at most 64), so `Active` and `Closed`
are allowed. Learned value types (`adage_core::vtype::Runs`, Track G) become string types with
`TypeDef::from_runs`, whose regex uses explicit ASCII classes and is bounded by construction.
Type ids may not shadow the built-ins.

## Closure and `CLOSURE`

The closure (Track C §2.1) is computed from the pack and never trusted from it. Its nodes are
routes, types, scripts, repository files, tools, templates and models. Edges:

1. input and output types → a built-in or `types/<id>`; a list type → its item type;
2. a step's `script` → `scripts/sha256-<hex>` (vendor) or its `requires.repo_files` entry (ref);
3. a run step's program → its tool (an interpreter before a script is a tool too);
4. `links` → routes of the pack;
5. **implicit**: route B triggered `after_command C` with `pattern::meets(C, key(A.steps[0].run))`
   gives A → B (the lookup → read-linked pair);
6. an avoid step whose `instead` is another route's run command → that route;
7. a route → its shape template, an advise step → its advise template, an intent trigger → its
   model.

Checks, after a breadth-first search from every entry route:

- **no dangling references**: every edge target exists in the pack, in `requires`, or among the
  built-ins;
- **no smuggling**: every type, vendored script and requirement is reached; a non-entry route
  that nothing links to is refused;
- **bounded chains**: no cycle through `after` edges, and no `after` chain longer than four
  routes.

`CLOSURE` lists the result as sorted lines `kind id digest`, `\n`-terminated:

```text
builtin repo-path-list -
edge:after route:work-item-context->route:read-linked-files -
edge:repo-file route:work-item-context->repo-file:scripts/wi-lookup -
edge:tool route:count-items->tool:python3 -
edge:type route:work-item-context->type:work-item-id -
repo-file scripts/wi-lookup 9f2c…
route work-item-context 5d41…        (the route exec digest)
script scripts/sha256-ab12… ab12…
template run@1 -
tool python3 -
type work-item-id 77e0…              (SHA-256 of the type's canonical JSON)
```

On read it is re-derived and must match byte for byte (after line-ending normalisation);
reformatting a route does not change it, since route lines carry exec digests.

## Digests and signatures

- **Route exec digest**: SHA-256 of the canonical JSON (keys sorted, no whitespace, as
  `EngineParams::canonical_json`) of `{"route": <route minus evidence and aspect>, "types":
  {<every referenced type: user definitions inline, built-ins as {"builtin": id}>}, "scripts":
  {<path>: <sha256>}}`. Approvals are keyed by it: evidence updates and aspect moves keep it; any
  change to a trigger, argv, input, output, type definition, script hash, exit or exec request
  changes it.
- **Pack digest**: `adage_tracks::io::digest`'s algorithm over the pack's files: per-file SHA-256
  of line-ending-normalised bytes (`scripts/` verbatim), sorted by path, then the SHA-256 of the
  `"<hash>  <path>\n"` listing. `pack.sig` is excluded; `CLOSURE` is included, so the signature
  covers the declared closure. `digest_dir(dir)` is the digest of the directory as received;
  `Pack::digest()` is the digest of the canonical files `Pack::write` writes.
- **Signature**: sshsig over the digest's 32 bytes, namespace **`adage-pack`**. An
  `allowed_signers` entry with `namespaces="adage-track"` does not authorise a pack. Existing
  `adage-track` signatures stay valid for v1 tracks.

## Script closure levels

| Level | The pack holds | Runs | Auto-approvable |
| --- | --- | --- | --- |
| L-none | argv whose program is in the command table | the table's program on the machine | only if every program is `inert` (C2) |
| L-ref | the repository path, its SHA-256 and optional `deps` | the repository's file, after a hash check at execution | never |
| L-vendor | the bytes, `scripts/sha256-<hex>` | the read-only CAS copy, through an interpreter named in argv (the shebang is ignored) | never |

**L-vendor in practice.** `adage share --vendor` turns each L-ref script into L-vendor: it reads
the bytes from the repository `share` runs in, checks them against the pin, rewrites argv to
`scripts/sha256-<hex>`, and drops the `requires.repo_files` entry. Only a script run by an
interpreter named in argv (`python3`, `node`, `sh`) and without `deps` can be vendored. `adage
add` copies the bytes read-only to `$ADAGE_HOME/cas/<hex>`. A route that runs a vendored script is
**exec-only**: bound live (at probation, never a draft) with its execution ceiling, but never
directed at E0 (its argv names no file an agent could run); `adage-exec hook` brings it into play
only once it is approved at E1 or E2 on this machine (policy `allow_vendor_scripts`, full text
shown, never automatic). A vendored route whose pack requests no execution level is inert.

## Upgrading v1 tracks

`upgrade_track(dir)` / `upgrade_track_with(dir, Some(repo_root))` read a v1 track
(`track.toml` + `routes/*.yaml`, validated by `adage_tracks::io::read`) and build a pack:

| v1 trigger | v1 step | v2 shape |
| --- | --- | --- |
| `prompt`, `intent`, `session_start` | `run` | `run` |
| `after_command`, `after_failure` | `run` | `after` |
| `after_edit` | `run` | `obligation` |
| any | `read_batch` | `read` |
| any | `avoid` | `avoid` |
| any | `advise` | `advise` |

Slots become inputs: a pattern contract becomes a user type `c-<first 12 hex of its SHA-256>`
(identical contracts deduplicate), `repo_path` becomes `repo-path` (or `repo-path-list` for the
slot a `read_batch` reads). A run step naming a repository script is pinned L-ref by hashing the
file under `repo_root` (a regular file, no symlink on the way, inside the root, at most 1 MiB).
Without a root, or when the file is missing, that route is skipped and reported in
`Upgraded::skipped`: a pack never holds an unpinned script. The manifest gets applicability
`repo` (the track's scope), tools, templates and models from the routes, the track's harness
gauge, `[exec] requested = "E0"`, and `evidence.json` from each route's support and `by_origin`.
`state`, `stale_since` and `authored` are dropped.

## Using packs

### Sharing

```sh
adage share <track-or-pack> --out <dir> [--key <path>] [--ack-scripts] [--format pack|v1]
```

The source is resolved in this order:

1. A learned track by name. It is upgraded to a pack. Only its live routes and drafts travel;
   retired and demoted routes stay behind. A repository script is pinned L-ref against the
   repository `share` runs in, if that repository is the track's; otherwise that route is listed
   under "Not carried".
2. An installed pack by name.
3. A pack directory.
4. A track directory.

Before anything is written:

- **Redaction.** The redaction check runs over every byte of every pack file, vendored scripts
  included. It looks for absolute and home-directory paths, tokens, keys and high-entropy
  strings. The digests a pack holds by construction are not counted as secrets.
- **Script acknowledgement.** A vendored script is listed, and `share` refuses without
  `--ack-scripts`, when it names a URL or hostname, runs a network tool (`curl`, `wget`, `ssh`,
  `nc`, …), uses an absolute path (the shebang aside), or reads a `*_TOKEN`, `*_KEY`, `*_SECRET`
  or `*_PASSWORD` environment variable.
- **What never travels.** Approvals, `review.yaml`, proposals, `trust.json` and session state are
  never part of a pack: the file allowlist has no place for them.

The pack is written in canonical form and re-read as a recipient would read it. Its digest is then
signed into `pack.sig` in namespace `adage-pack`. `share` prints the `trusted-signers` line
recipients need, with `namespaces="adage-pack"`.

`--format v1` writes a v1 track (`track.toml`, `routes/`, and `track.sig` in namespace
`adage-track`) for recipients on an older Adage.

### Adding

```sh
adage add <pack-or-track-dir> [--trust]
```

Run it inside the repository the pack is for. It refuses to run inside an agent session.

For a pack, `add`:

1. **Reads the directory strictly.** Only the listed files are read, with no symlinks and no
   smuggled files.
2. **Checks the signature.** It computes the digest and checks `pack.sig` in namespace
   `adage-pack`.
3. **Checks trust.** The trust anchors are the same as for tracks: `$ADAGE_HOME/trusted-signers`,
   then the repository's `.adage/trusted-signers` once approved and pinned by hash, then a
   signer approved with `--trust`. An entry restricted to `namespaces="adage-track"` refuses a
   pack.
4. **Validates the pack in full.** The schema and semantics are checked, and the closure is
   re-derived and compared with `CLOSURE`.
5. **Re-classifies the steps.** The local command table classifies each step. A repository or
   vendored script counts as environment.
6. **Applies policy.** It reads `$ADAGE_HOME/policy.toml`, then the managed file (`ADAGE_POLICY`,
   else `/etc/adage/policy.toml`). `[publishers] deny = ["SHA256:…"]` refuses a signer, and
   `[exec] allow_vendor_scripts = false` refuses packs with vendored scripts.
7. **Installs the pack.** It goes under `tracks/@imported/<owner>/<name>/` exactly as verified,
   and vendored scripts are copied read-only to `$ADAGE_HOME/cas/<sha256>`.
8. **Binds the pack** to repositories (below) and recompiles the index.

It creates no execution approvals.

A **v1 track** is verified as before (namespace `adage-track`) and upgraded in place: repository
scripts are pinned against the repository `add` runs in, and routes that cannot be carried are
recorded as inert.

### Binding

`adage_pack::bind` produces a `BoundRoute` for each route of the pack: **live**, with the highest
level it may be approved at, or **inert**, with a reason. It checks, in order:

1. **Applicability.**
   - `repo`: the target's keys must include one of the pack's. A checkout answers to its
     `repo_key`, its origin form and its `<name>@<root commit>` form.
   - `fingerprint`: the predicates are evaluated over `adage_instincts::declared::extract(root)`.
   - `detector`: the compiled detector must fire.

   A pack that does not apply binds nothing. An unknown detector or catalogue makes every route
   inert.
2. **`requires.os`**, then each route's **tools**, by PATH lookup only. Nothing is run.
3. **Templates and models** must be known to this build.
4. **L-ref scripts.** The SHA-256 of `root/<path>` must equal the pin. The file must be a regular
   file inside the root, with no symlink on the way, and at most 1 MiB. `deps` globs must match
   files, whose hashes join the binding.
5. **L-vendor scripts** must be in the cas.
6. **A runtime form.** E0 delivers today's route model. Several steps, a sequence, a read of a
   step's output, or a type with no runtime contract gives the reason "shape not yet supported at
   runtime". A vendored script's route is exec-only (above).

Only signature, schema or closure failures refuse a whole pack.

`add` binds to these repositories: each repository key of a `repo` pack, with the checkout when
`add` runs in it or has bound it before; otherwise the repository `add` runs in, plus every
checkout bound before. For each, it writes a bound track under
`tracks/@imported/@bound/<owner>/<name>/<repo>/`:

- `track.toml` and `routes/`: the live routes, at probation and yellow at most, with the
  recipient's review decisions carried across re-binds;
- `exec.json`: route id → `ExecBinding` (`route_exec_digest`, resolved `scripts` with SHA-256,
  `max_level`);
- `binding.json`: every route live or inert, with the pack digest and how the pack was verified.

The index compiles bound tracks like any imported track. `IndexedRoute.exec` carries the exec
binding (`Index::compile_root`), and the E0 hot path ignores it.

`adage doctor` (the `packs` check) and `adage review` (text, and `inert` in `--json`) list inert
routes with the pack, its version and digest, the repository and the reason. To bring an inert
route back, restore what it needs (the script, the tool) and run `adage add` again. It can be run
on the installed pack's directory.

### The core instincts pack

The instinct catalogue ships as the pack `adage/core-instincts`
(`adage_pack::instincts::core_instincts(root, existing, rejections)`). Its applicability is
`kind = "detector"`, `detector = "*"`, with `catalogue` set to this build's.

The detectors are compiled Rust, so the pack names them and cannot supply them. For each
repository, the compiled detectors propose the pack's routes, which are upgraded, validated and
closed like any pack's. Binding that pack gives exactly the routes the instinct binder gives; a
test pins this.

## Library

```rust
let pack = adage_pack::Pack::load(dir)?;          // files, schema, semantics, closure == CLOSURE
pack.write(out)?;                                 // canonical files; removes a stale pack.sig
pack.digest()?;                                   // pack digest of the canonical files
pack.route_exec_digest("work-item-context");
adage_pack::route_exec_digest(&route, &pack.types);
adage_pack::upgrade_track_with(track_dir, Some(repo_root))?;
adage_pack::sign_dir(dir, agent_sock, key)?;      // writes pack.sig (namespace adage-pack)
adage_pack::verify_dir(dir, &allowed_signers, now)?;
let env = adage_pack::BindEnv::current(Some(cas_dir));
let binding = adage_pack::bind(&pack, &adage_pack::Target::checkout(root), &env);
for r in binding.inert() { println!("{}: {}", r.id, r.status.reason().unwrap()); }
```

`adage-pack inspect|verify|digest|upgrade` is a small debugging binary over the same API.
