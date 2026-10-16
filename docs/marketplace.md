# Packs and the marketplace

> Status: built 2026-10-01: base instincts as packs; the Adage repository as the first marketplace,
> built into the binary and synced to each machine with install records; `adage marketplace` and
> `adage packs`. Remote marketplaces are designed here and come next. The format reference is
> [packs.md](packs.md); the instincts themselves are [instincts.md](instincts.md).

Everything Adage moves between machines is a **pack**: learned reflexes you share, skills compiled
by `adage train`, and the instincts Adage ships. A **marketplace** is a repository's list of the
packs it offers, each pinned by its digest, the way a Claude Code marketplace lists plugins. The
Adage repository is the first one: its packs are Adage's **base instincts**, and every Adage binary
carries a verified copy of it.

## What a packaged reflex looks like

A pack is a directory of text files plus a signature. Tar or zip is only for moving it.

```text
acme-work-items/
├── pack.toml          what it is: name, version, origin, where it applies, what it needs
├── routes/<id>.yaml   one file per reflex: trigger → typed inputs → steps → effect → strength
├── types/<id>.yaml    input contracts, such as a work-item id matching ^W-\d{4,9}$
├── evidence.json      how much evidence each reflex has, by origin
├── CLOSURE            everything the pack depends on, re-derived and compared on install
└── pack.sig           SSH signature over the pack's digest
```

A reflex, as `routes/read-linked-files.yaml` holds it:

```yaml
id: read-linked-files
summary: Read the files the work item links, together
shape: read
trigger: { on: after_command, command: "exec python3 scripts/wi-lookup {id} --json" }
inputs:  { files: { type: repo-path-list, from: { result: { field: linked_files } } } }
steps:   [{ id: read, read: { from: files, max: 5 }, effect: observe }]
strength: prime
```

Next to a Claude Code skill:

| | Skill (`SKILL.md`) | Pack |
| --- | --- | --- |
| What it holds | Instructions in prose | Typed routes: trigger, steps, effect, strength |
| Who decides it applies | The model, from a description | Adage, on the CPU, when the trigger matches |
| What reaches the agent | The whole text, every time it loads | One line, from a fixed template, only on a match |
| Evidence | None | Per reflex: sessions, a Wilson lower bound, uplift |
| Can it run anything | Whatever the model does with it | No. It can ask for an execution level; only your machine grants one |
| Integrity | None | Content-addressed, closed, signed or pinned |

## Instincts are packs too

An instinct is a reflex with blanks. Its pack holds **templates**: routes whose trigger and step
say what the instinct may bind, with blanks where a learned reflex holds values, and the detector
that fills them in from each repository's own files:

```yaml
# packs/python-env/routes/run-tests-in-project-env.yaml
id: run-tests-in-project-env
summary: After pytest fails outside the project's environment, run it inside the environment
shape: after
detector: U11              # compiled into Adage: fills each {blank} from the repository's files
trigger: { on: after_failure, command: "{runner}", signature: "{signature}" }
steps: [{ id: run, run: { argv: ["{command}"] }, effect: verify }]
strength: prime
exit: [{ when: intent_changes }]
```

In a checkout of `pallets/click` with a `.venv`, U11 fills it in as: after `python3 -m pytest` fails
with `No module named pytest`, run `.venv/bin/python -m pytest` (and likewise for `pytest`,
`python -m pytest`, `tox` and the project's own import errors). A template is fixed bytes, digested, pinned and signed
like any route; what fills it comes from the repository and is validated as a route every time. The
pack decides what may bind: a detector no template names stays off, a route whose shape no template
admits is not bound, and an effect above the template's is refused. The rules are in
[packs.md](packs.md#templates).

## The marketplace format

A marketplace is a repository with `.adage/marketplace.toml` at its root:

```toml
format = "adage-marketplace/1"

[marketplace]
name = "adage"
repository = "github.com/stevenvaughanbooth/adage"   # its home: where it is fetched from
description = "Adage's base instincts: …"

[[pack]]
name = "adage/python-env"      # the pack's own name (its pack.toml)
source = "packs/python-env"    # its directory, relative to the repository's root
version = "0.1.0"              # its version (its pack.toml)
digest = "6ef73b4c…"           # its pack digest: exactly these bytes
default = "detected"           # detected: binds wherever it applies | off: waits to be turned on
```

Every table refuses unknown keys. A pack is the one its entry pins, or it is refused: the same name,
the same version, the same digest. Names and sources are unique; sources are repository-relative.
`repository` is the marketplace's home, a repository key (`github.com/owner/repo`): the address a
remote marketplace is fetched from and updated against.

## Adage's base instincts

The Adage repository's marketplace lists one pack per instinct that is built. Each binds only where
its detector finds what it needs in the repository's own files.

| Pack | Instinct | What it says | Applies when the repository has |
| --- | --- | --- | --- |
| `adage/python-env` | U11 | Run pytest and Python inside the project's environment | A `.venv` or `venv` with pytest, or a uv project |
| `adage/test-wrapper` | U1 | Use the test wrapper that sets what the runner needs | A Makefile target or package.json script that wraps the runner with an env var or loader |
| `adage/test-command` | U3 | Use the repository's one declared test command | Exactly one test entry point (Makefile, package.json, tox, nox, just, Taskfile) |
| `adage/pytest-wrapper` | P1 | Run pytest through its Makefile wrapper or `tox -e py` | pytest configured, and a wrapper that sets env or flags, or a plain tox base environment |
| `adage/package-manager` | N1 | Use the declared Node package manager (installs wait for review) | A `packageManager` field or a pnpm, yarn or bun lockfile, and no npm lockfile |
| `adage/regenerate` | U5 | Run the declared generator after editing its inputs | Makefile prerequisites, or the input paths a generator script names |
| `adage/generated-files` | U4 | Don't hand-edit generated files | A generated-file header, or files a declared generator writes |
| `adage/lockfiles` | U6 | Don't hand-edit lockfiles | A lockfile at the repository's root |

Instincts are prime at most, never block an agent from stopping, and give way to any reflex Adage
learns ([instincts.md](instincts.md)).

## Marketplaces on this machine

The Adage repository is the first marketplace Adage knows, and it ships inside the binary: the
build embeds its `.adage/marketplace.toml` and `packs/` and verifies every pack against its pin, so
nothing is downloaded.

A marketplace reaches a machine by a **sync** (`adage_pack::marketplace::Loaded::sync`), the same for
every marketplace:

- every pack it lists that is not installed at its pinned digest is written to
  `~/.adage/tracks/@imported/<owner>/<name>/`, staged and re-checked against its digest: the same
  code that installs a pack you `adage add`;
- each is **recorded** as from that marketplace in `~/.adage/tracks/@imported/installed.json`, with
  its version and digest (`adage add` records the signer's key; `adage train accept` records a
  review);
- a pack recorded from that marketplace that it no longer lists (a newer build dropped it) is
  removed. Until then it binds nothing: it is not a pack a person added.

Learning syncs the built-in marketplace before it binds instincts: the first `adage init --yes`,
`adage condition` or plugin session installs every base pack. Binding reads the verified copy in
the binary, so a damaged installed copy changes nothing until the next sync restores it.

```text
$ adage marketplace
adage marketplace · 1 marketplace

◆ adage · built into Adage 0.1.0
home        github.com/stevenvaughanbooth/adage
about       Adage's base instincts: defaults that bind from a repository's own files, …
packs       8 packs, every one installed as pinned
             adage/python-env       0.1.0   installed
             adage/test-wrapper     0.1.0   installed
             …
```

- `adage marketplace [--json]` lists the marketplaces, each pack they pin, and how it stands here:
  installed as pinned, not installed yet, or installed at other bytes.
- `adage marketplace show <name>` prints its manifest, as published.
- `adage marketplace update [<name>]` syncs it now and says what it installed, updated or removed.
- `adage doctor` reports it (the `marketplace` check): readable, and how many of its packs are
  installed as pinned.

## The first run

1. **Synced.** Learning syncs the built-in marketplace (above): every base pack installed and
   recorded as from it.
2. **Detected.** For each repository, the detectors the turned-on packs name run over its declared
   files (Makefile, package.json, pyproject, lockfiles, CI) and bind what they find. Nothing is
   executed to decide this, and no history is needed.
3. **Yours to change.**

In the `pytool` test fixture (pytest behind a Makefile wrapper, a uv project, a generator), after
`adage packs off adage/lockfiles`:

```text
$ adage packs
adage packs · pytool@b427488a99c0

Base instincts · the adage marketplace, built into Adage 0.1.0
  ◉ adage/python-env       on   bound here: 10 reflexes
      Use the project's own Python environment
  ◉ adage/test-wrapper     on   bound here: 1 reflex
      Use the repository's own test wrapper
  ◉ adage/test-command     on   bound here: 3 reflexes
      Use the repository's one declared test command
  ◉ adage/pytest-wrapper   on   bound here: 2 reflexes
      Run pytest the way the repository does
  ◉ adage/package-manager  on   nothing to bind here
      Use the repository's declared Node package manager
  ◉ adage/regenerate       on   bound here: 2 reflexes
      Regenerate after editing a generator's inputs
  ◉ adage/generated-files  on   bound here: 1 reflex
      Do not hand-edit generated files
  ○ adage/lockfiles        off  would apply here
      Do not hand-edit lockfiles
```

- `adage packs off <name>` stops a pack, here at once and in every other repository at its next
  session. `adage packs on <name>` starts it again (a person's decision: never inside an agent
  session).
- `adage packs show <name>` prints a pack's files, as installed.
- `adage init` lists the packs that apply to the repository before it binds anything.
- Settings: `instincts.packs` is `auto` (default built-in packs and added instinct packs), explicit names, or both.
  Optional built-ins need an explicit name, which `adage packs on <name>` adds;
  `instincts.disabled` takes pack names (`adage/lockfiles`) and instinct ids (`U3`);
  `instincts.enabled = false` turns them all off.

## Trust

A pack is trusted in one of two ways, and the host it came from is never one of them:

- **A signature** by a signer you trust: `~/.adage/trusted-signers`, a repository's
  `.adage/trusted-signers` once you approve it, or a signer you approve with `--trust`. This is
  how packs from teammates arrive (`adage share`, then `adage add`).
- **A pin** in a marketplace you trust. Today that is the one built into the binary: its packs need
  no signature, and `adage add` accepts an unsigned copy of one whose digest is its pin. Its names
  are reserved: other bytes under `adage/python-env`, signed or not, are refused.

A pack never carries permission to execute. It can request an execution level; only a person on
this machine grants one ([execution.md](execution.md)).

## Hosting packs

Because a pack is content-addressed and signed (or pinned), it can live anywhere bytes can.

| Where | How | Status |
| --- | --- | --- |
| Your repository | Commit `packs/<name>/` and `.adage/trusted-signers`; teammates run `adage add packs/<name>` | Works today |
| A shared directory or archive | `adage share <track> --out <dir>`; send the directory; `adage add <dir>` | Works today |
| A git repository as a marketplace | `.adage/marketplace.toml` and `packs/` at its root, like the Adage repository | The format, the sync and the records are built; fetching is next |
| A Claude Code plugin | A plugin that carries packs, installed from a marketplace you already use | Designed ([Design Spec §10.10](Adage%20—%20Design%20Spec.md)) |
| A registry | Search, ratings, evidence aggregated across repositories | Later |

Git comes first because it already provides review, history, access control and signing.

## Maintaining the Adage marketplace

The base packs are written by hand in `packs/` and checked like any pack:

```sh
cargo run -p adage-pack -- seal packs/python-env        # canonical form, CLOSURE derived
cargo run -p adage-pack -- marketplace . --pin          # after bumping the version: pin the new digest
cargo test -p adage-pack --test marketplace
```

The tests check that the binary's copy is exactly the repository's, that every implemented
detector ships in exactly one base pack, that binding the base packs gives exactly what the
instinct binder gives on every fixture, and that every template binds somewhere. A pack's
version names one digest: change its bytes, bump its version, re-pin. A build that ships other base
packs binds every repository's instincts again once.

## Decisions and open questions

Closed here:

- Instincts ship in the pack format, as templates a detector fills, not in a format of their own.
  The detector catalogue (`crates/adage-instincts/data/catalogue.toml`) lists the detectors a
  build has; the marketplace decides what ships.
- One pack per base instinct.
- The Adage repository is the first marketplace, named `adage`, home
  `github.com/stevenvaughanbooth/adage`. The binary embeds it and verifies it against its pins.
- A marketplace reaches a machine by one sync: its packs installed through `adage add`'s install,
  recorded as from it (`installed.json`), and removed when it no longer lists them. Every install
  path records how its pack was trusted.
- Base packs bind wherever detected unless turned off, per machine.
- An instinct's routes, from every instinct pack, bind into the learner's track for the repository
  (`<repo>/instincts`), so learned reflexes still take over and failing instincts still extinguish.
- A marketplace pin is a trust anchor beside a signature, and pinned names are reserved.

Open, for the next phase:

1. **Remote marketplaces.** `adage marketplace add <owner/repo>` fetches a repository's
   `.adage/marketplace.toml` and the packs it lists (by git or HTTPS) into a cache, a person approves
   the marketplace once (its manifest pinned by hash, as a repository's `trusted-signers` is), and
   the same sync installs and records its packs; `adage marketplace update` fetches again. Open: git
   or HTTPS, and whether manifests are also signed (an `adage-marketplace` namespace).
2. **Per-repository choices.** Packs are turned on and off per machine today; a team may want a
   committed choice (`.adage/` in the repository).
3. **Detectors as data.** Community instinct packs reuse the detectors built in. A declared-facts
   input source (`from: { declared: … }`) would let a pack bind from a repository's files without
   new Rust.
4. **One bound location.** Imported packs bind under `@imported/@bound/`, instinct packs into
   `<repo>/instincts`. They can merge once imported packs get the learner's hand-over and
   extinction rules.
5. **Release signing.** Base packs distributed outside the binary (a remote marketplace) need an
   Adage release key.
