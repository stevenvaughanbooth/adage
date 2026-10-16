# Adage visual redesign: one theme for setup, CLI output, watch, the terminal app and the map

> Status: approved 2026-09-28; in progress on `claude/watch-map-redesign-plan-e2n2wd`.
> Mockups: the "Adage redesign mockups" canvas. Rules: [style.md](style.md).

**Decisions since the plan was written**

- **Type is "Workshop":** Bricolage Grotesque (display), Hanken Grotesk (body), Red Hat Mono
  (mono), replacing Instrument Serif and IBM Plex everywhere, the brand book included.
- **The token generator is a Rust `xtask`, not Terrazzo.** It writes both `tokens.rs` and
  `design/build/tokens.css`, computes xterm-256 indices, and checks contrast, so no step
  needs Node. The web app imports the generated CSS.
- **Light/dark detection** uses `ADAGE_THEME` and `COLORFGBG` for now; an OSC 11 probe
  (`terminal-colorsaurus`) is deferred until it can be done without delaying start-up.
- **Pipes get no escape codes at all**; `NO_COLOR` at a terminal keeps bold and dim.

## Context

Adage shows people three things today, and they were built separately:

- **Terminal UI** (`crates/adage-cli/src/ui/`, ratatui 0.30):
  - `adage watch`, `adage ui`, and the `adage init` wizard.
  - The theme is `ui/theme.rs`: 113 lines, the 16 ANSI colours only, no brand colour.
  - Glyphs (`→ ✓ ✗ ■ ◌ · ▸`) are defined in `view.rs` and `wizard.rs` instead of the theme.
  - Snapshot tests are hand-rolled (`ui/tests.rs` `snap()`, `ADAGE_BLESS=1`).
- **Plain command output**:
  - No argument parser: `main.rs:118` matches strings and `flag()` reads flags.
  - Every command writes plain `println!`, with no colour and no NO_COLOR handling.
  - Status marks differ by command: `🟢` in status, `[green]` in graph, `pass/WARN/FAIL` in doctor, `●`/`[G]` in the TUI.
  - Headers, key–value column widths (11/22/24/26) and separators (`· — │ →`) also differ.
  - Help is two unstyled lines.
- **Web "signal box"** (`adage graph --serve`, `serve.rs`, `assets/brain.html`):
  - About 500 lines of hand-written SVG and vanilla JS, served by a standard-library server that handles one connection at a time.
  - It polls `/graph.json` every 2.5 s.
  - It loads Google Fonts and uses a green-grey palette unrelated to the brand.

The product is also being **rebranded** on `origin/claude/repo-maturity-roadmap-l95ohs`. `docs/lexicon.md` was approved 2026-09-28 and makes these changes:

- **Nouns:** traces become reflexes (learned) and instincts (shipped); holdout becomes control.
- **Strength:** fire / prime / inhibit replace green / yellow / red.
- **Lifecycle:** nascent → forming → firm → dormant → extinguished, plus ignored and misfired.
- **Verbs:** `sense` (was survey), `condition` (was learn), `map` (was `graph --serve`), `delegate`, `train`, and `review strengthen|weaken|extinguish`.
- **Rules:** no aliases; agent-facing text stays plain.

Brand book v2 (`docs/media/brand-book.html` on `claude/adage-demo-walkthrough-pi65am`) sets the look:

- Night `#050507`, one violet brand hue `#8b7cff`, and three lamps: green `#4ade80`, amber `#fbbf24`, red `#fb7185`.
- GPU orange `#ff7a59` is only for heat and cost. Light-surface variants are defined.
- Type (brand book; superseded here by Workshop): Instrument Serif, IBM Plex Sans, IBM Plex Mono.
- Instinct and reflex glyphs; motion rule "light travels; text arrives"; reduced motion honoured.

Where the brand book and the lexicon conflict (amber vs prime, "on rails", `learn`), **the lexicon wins**.

**Goal:** one design system, defined once, that drives every pixel and every terminal cell. Every surface should feel like the same product at the level of polish of Claude Code and Codex, and each step a user takes should say what happened, why, and what to do next. The web map is rebuilt as a real frontend app.

**Decisions made with you:**

1. Build the web view as a real frontend app with a build step.
2. Use a fresh identity based on the reflex/instinct rebrand, not the railway metaphor.
3. Add best-in-class dependencies freely, as long as they stay off the hook path and their cost is measured.

---

## 1. Design principles

These go into the style guide, `docs/design/style.md`, modelled on Codex's `styles.md`.

1. **Semantic, never literal.** Call sites ask for `t.fire()`, `t.muted()` or `t.accent()`. Raw `Color::` values or ANSI codes outside the theme module are banned by clippy `disallowed_methods`/`disallowed_types`, and `println!` is banned outside the output module.
2. **The terminal's colours win.**
   - Body text uses the default foreground (`Color::Reset`), and we never paint the terminal background.
   - Brand violet and the lamps use truecolor only after a contrast check against the probed background. Otherwise they fall back to precomputed ANSI-256 values, then to ANSI-16 names.
3. **Every screen ends with a next step.** Every command, wizard step, empty state and error names the exact next command, for example `next  adage review` or `→ run adage condition to form reflexes`.
4. **Evidence over enthusiasm** (Design Spec §17.3). Numbers always come with denominators, in mono: `23/33 · lb 0.53 · 8 turns saved`.
5. **Lamps are meaning, not decoration.** Fire, prime and inhibit colours appear only on reflex strength. GPU orange appears only on cost. Violet is the one accent.
6. **Degrades gracefully:**
   - with `NO_COLOR`, lamps become the words `FIRE`, `PRIME`, `INHIBIT`;
   - with no tty, output is plain text;
   - `--json` gives machine output;
   - `--plain` gives screen-reader-friendly output with no glyphs;
   - on legacy consoles, glyphs fall back to ASCII (`TERM=dumb` or a non-UTF-8 locale).
7. **Motion carries meaning.** A firing reflex pulses once; nothing loops forever. `ADAGE_REDUCED_MOTION=1` and `prefers-reduced-motion` turn it off.
8. **The same fixtures everywhere.** One set of scenario states drives the TUI snapshots, CLI golden files, Storybook stories and Playwright screenshots.

---

## 2. One token source for every surface

**New:** `design/tokens/*.tokens.json` in the W3C DTCG 2025.10 format. Its values come from brand book §07, with light and dark modes:

- **Colour:**
  - `bg.{base,panel,raised}`
  - `ink.{1,2,3}`
  - `line.{1,2}`
  - `accent`, `accent.soft`
  - `lamp.{fire,prime,inhibit}`
  - `cost` (GPU orange)
  - `state.{nascent,forming,firm,dormant,extinguished,ignored,misfired}`, mapped onto the ink and lamp scales
  - `control` (was holdout)
- **Type:** `font.{display,sans,mono}`, size and weight scale.
- **Radius, space and motion:** durations and easings, including the brand's quartic-out 0.8 s text rise.
- **Glyphs:** a tokens file with Unicode and ASCII pairs:
  - lamp `●`/`*`
  - instinct `◉`/`(o)`, a dot in a ring, per brand book §06
  - reflex `◠`/`~`
  - ok `✓`/`+`, fail `✗`/`x`
  - arrow `→`/`->`
  - rail `│`/`|`
  - spinner frames

**Generator:** `cargo xtask tokens` (a new `xtask/` crate, Rust only). It produces:

- `design/build/tokens.css`: CSS variables that the web map's Tailwind v4 `@theme` reads.
- `crates/adage-term/src/tokens.rs`: `const` values with `{ rgb, ansi256, ansi16 }` for every colour, plus the glyph table. This is a **committed file**, so `cargo build` never needs Node.
- `docs/design/tokens.html`: a living swatch page, published as the brand reference.

**CI** (`scripts/check-tokens`):

- Regenerate the outputs and fail if they differ from what is committed.
- Check WCAG 4.5:1 contrast for every text/background pair in both modes.
- Check that each `ansi256` value is the nearest match to its `rgb`.

---

## 3. New crate `crates/adage-term`: the shared terminal design system

The hook binary path never touches this crate: `main.rs:66` dispatches `adage hook` before anything else. It holds everything visual that is not a full screen.

| Module | Contents | Crates |
|---|---|---|
| `caps` | Detects colour level, hyperlink support, UTF-8 and light/dark ground **once**, from the environment (`NO_COLOR`, `CLICOLOR_FORCE`, `COLORTERM`, `TERM`, locale, `COLORFGBG`, `ADAGE_*` overrides) | none (in-house, tested) |
| `theme` | `Theme` built from `tokens.rs` + `caps`. Semantic methods return `ratatui::Style` **and** `anstyle::Style` from one definition. Also provides the lamp, state and glyph helpers used everywhere. Replaces `ui/theme.rs`. | `anstyle`, `ratatui-core` |
| `out` | The only way commands print. Pieces: `header` ("adage condition · acme-billing · gen 8dd5ec01"), `section`, `kv` (one column-width rule), `row` (lamp + id + evidence), `table` (themed comfy-table), `note`/`warn`/`fail`, `next` (next-step hint), `link` (OSC 8), `json`. Writes through `anstream`, which strips colour for pipes, NO_COLOR and CLICOLOR, and handles Windows. | `anstream`, `comfy-table` 8 |
| `progress` | Spinner, bar and multi-step "task list" (✓ done, spinner active, · pending) | `indicatif` 0.18 |
| `errors` | `miette` graphical handler themed from tokens. `main` renders `anyhow` errors as a titled block with cause chain, help and next step. | `miette` 7 |
| `md` | pulldown-cmark → a shared `Line`/`Span` model, drawn either as ANSI or as ratatui `Text` (the Codex approach) | `pulldown-cmark` |
| `dag` | Layered text layout of the reflex graph for the inline map and the TUI Map tab | `ascii-dag` 0.11 |
| `inline` | A ratatui `Viewport::Inline` runner with `insert_before` (feature `scrolling-regions`). Used for the setup wizard and for long-running commands (condition, sense, models fetch): finished lines go into real scrollback and only the live tail redraws. | `ratatui` 0.30 |

`ratatui` moves to `ratatui = "0.30"` with `crossterm` and `scrolling-regions`. Companion crates:

- `tachyonfx` (pulses and fades)
- `throbber-widgets-tui`
- `tui-widgets` (popup, scrollview, big-text)
- `ratatui-textarea` (settings editing)
- `tui-tree-widget` (reflex tree)
- `nucleo-matcher` (fuzzy search in the palette)

Every new crate is checked against the `deny.toml` licence allowlist before it is added.

---

## 4. Command line: clap, the lexicon, and uniform output

1. **Replace the hand-written dispatch** in `main.rs` with `clap` 4 (derive).
   - Themed help: `Command::styles()` built from tokens.
   - Short `-h` / long `--help`.
   - Command examples in `after_help`.
   - `clap_complete` for shell completions (`adage completions <shell>`) and `clap_mangen` for man pages, both built by `xtask`.
   - The hook command stays a pre-clap fast path.
2. **Command names follow `docs/lexicon.md`**, with no aliases:

   | Old | New |
   |---|---|
   | `survey` | `sense` |
   | `learn` | `condition` |
   | `graph --serve` | `map` |
   | `review promote` / `demote` / `reject` | `review strengthen` / `weaken` / `extinguish` |
   | `exec enable` | `delegate` |

   This is shared work with the rebrand branch: the CLI surface is renamed once, here, and the rebrand branch keeps the on-disk format migration.
3. **Every command is restyled on `adage_term::out`** so its output follows one grammar. Target for `adage status` (brand book §10, updated to the lexicon):
   ```
   adage status · acme-billing · gen 8dd5ec01 · hooks claude · auto-condition on

   instincts   1 track · 2 instincts · adage/finish-line@0.1.0
   reflexes    3 tracks · 4 reflexes from 54 traces · 0.24 s

     ● fire     lookup-work-item        ◠ reflex    23/33  lb 0.53  8 turns saved
     ● fire     read-linked-files       ◠ reflex    15/16  lb 0.72
     ● prime    finish-line/regenerate  ◉ instinct   7/7   lb 0.65
     ● inhibit  dead-end-d41b1e         ◠ reflex    11/43  "No module named pytest" → make test

   next  adage review · 2 nascent reflexes wait for you
   ```
   Commands in scope:

   | Group | Commands |
   |---|---|
   | Setup and reading | `init`, `sense`, `condition`, `status`, `report` |
   | Reviewing and sharing | `review`, `share`, `add`, `delegate` |
   | Maintenance | `doctor`, `config`, `models`, `install` |
   | Evaluation | `eval` (the `adage_eval::cli` output is moved onto `out` too) |

   `doctor` gets the same lamp/row grammar with one-line fixes, matching the TUI Health tab.
4. **`--json` on every reading command**, including `status`, with a documented, versioned schema. `report` output gets pretty-printed.
5. **Long jobs** (`condition`, `sense`, `models fetch`, `train`) use the inline task list: each phase ticks off in scrollback, and a final summary card ends with `next`.

---

## 5. Setup: `adage init` as an inline guided flow

The full-screen wizard (`ui/wizard.rs`) becomes an **inline flow in the Clack style**, on `adage_term::inline`, so setup reads like Claude Code onboarding and stays in scrollback.

**Opening line**, in mono, with the wordmark rendered by `tui-big-text` only on wide terminals:

> Adage ships with instincts and forms reflexes from your traces. Nothing is written without asking. Fail-open.

**Steps**, each with a left rail (`◇` done, `◆` active, `│` rail):

1. Detect harnesses: Claude Code, OpenCode, Codex. Detected ones are pre-selected.
2. Install hooks per harness, with a diff preview of the settings file before writing.
3. Model fetch, optional: size, SHA-256, and a progress bar.
4. Condition from existing traces: a live counter of traces read and reflexes formed.
5. Auto-condition on or off.
6. **Finish card:**
   - the first reflexes found, shown with lamps (the payoff moment);
   - three next steps: `adage watch` in a split pane, `adage map`, `adage review`.

The existing non-interactive behaviour stays: `--print`, `--yes`, piped input and agent env (`init.rs:39`). Step logic stays in pure `Kind` state types; only rendering changes. Keys: ↑/↓ and space to select, enter to confirm, esc to go back a step, q to abort (with confirmation).

---

## 6. `adage watch`: the live pane, redesigned

Stays alternate-screen; it is meant for a split terminal. Layout, all semantic tokens:

```
 adage · acme-billing                                    ● live   fire 4  prime 2  inhibit 1
 ─ now ─────────────────────────────────────────────────────────────────────────────────────
 14:02:11  f247fa claude  prompt  → ◠ lookup-work-item     ● fire     "…wi-lookup W-2045"
 14:02:14  f247fa claude  tools   ✓ followed lookup-work-item
 14:02:20  f247fa claude  tools   → ◉ finish-line/regenerate ● prime
 14:02:31  f247fa claude  stop    ✓ finish line met                       control arm: 0
 ─ conditioning ────────────────────────────────────────────────────────────────────────────
 ◆ 3 new traces · forming read-linked-files 15/16 → firm in 1 trace
 ─────────────────────────────────────────────────────────────────────────────────────────── 
 p pause · f scope · / filter · enter inspect · m map · ? keys                    q quit
```

- **Pulses:**
  - a tachyonfx one-shot glow on the lamp when a reflex fires;
  - a sweep along the row when a reflex changes state (forming → firm);
  - a dim fade for control-arm events.
- **Enter** opens an inspector popup: the exact directive text, evidence, and the trace timeline.
- **Width-adaptive rules** carry over from `view.rs:232` (≥90 / <72 columns) as token-level breakpoints.
- `--once` and `--size` stay; they become the fixture renderer (§9).

---

## 7. `adage ui`: the terminal app

**Tabs:** `Live | Reflexes | Map | Health | Settings`

- **Live:** the watch view.
- **Reflexes** (was Routes):
  - A `tui-tree-widget` tree grouped by track → reflex/instinct.
  - Filter chips for fire/prime/inhibit and the lifecycle states.
  - Fuzzy `/` search.
  - A detail pane: directive text, evidence sparkline (ratatui `Sparkline`), and treated-vs-control numbers.
  - Actions: `s` strengthen, `w` weaken, `x` extinguish, `d` delegate. Each needs confirmation and prints the equivalent CLI command so the user learns it.
- **Map (new):**
  - The `adage_term::dag` layered layout of stimulus → reflex → response, scrollable and zoomable with `+`/`-`.
  - Focus mode with `f` (N hops around the selection).
  - `o` opens the same node in the web map (`adage map --focus <id>`).
- **Health:** doctor checks with lamps and one-key fixes (existing `health::Fix`).
- **Settings:** `ratatui-textarea` inline editing and validation messages.

**App-wide:**

- A command palette on `:` or `ctrl-k`, with nucleo fuzzy matching over every action and every reflex.
- `?` shows a keys overlay generated from the same keymap table the status bar uses, so they never drift.
- Toasts for background jobs.
- Consistent modals using `tui-popup`.
- Empty states on every tab with the next command, for example "No reflexes yet. Agents' traces arrive automatically; or run `adage condition` now."

**Architecture:** keep today's good pattern (plain state, pure `draw`, `key() → Cmd`) and formalise it:

- A `Component` trait (`handle_key`, `update(Action)`, `draw`) and an `Action` enum on an `mpsc` bus.
- `app.rs` (1,081 lines) is split into `ui/tabs/*.rs`.
- `jobs.rs` and `tail.rs` stay as the data layer and are shared with the map server (§8).

---

## 8. `adage map`: the web app, rebuilt

### Frontend (`web/map/`, a new top-level pnpm package)

- **Framework and UI:** Vite 8, React 19, TypeScript (strict), Tailwind v4 (`@theme` from the generated `tokens.css`), shadcn/ui on Base UI.
- **Graph:** `@xyflow/react` 12, with **elkjs in a Web Worker** for layered left-to-right layout and `@dagrejs/dagre` for the fast first pass.
- **Interaction and charts:** `cmdk` palette, `motion`, the View Transitions API, `uPlot` for time-series charts.
- **Fonts:** Bricolage Grotesque, Hanken Grotesk and Red Hat Mono (all SIL OFL), self-hosted as subset woff2 files. The Google Fonts dependency and its CSP exception are removed.

### Layout (the Nx graph, Dagster and Langfuse patterns)

- **Top bar:**
  - wordmark, repository switcher, live pill, lamp counters, `⌘K`;
  - a theme toggle (system, light, dark), with the dark mode as brand "Night".
- **Left rail:**
  - searchable tree of tracks, packs, reflexes and instincts;
  - filters for strength, lifecycle, harness and time window.
- **Centre canvas:** stimulus → reflex → response.
  - **Aggregated/expanded toggle** (the Langfuse model).
  - **Focus mode** (N hops), minimap, and zoom-dependent detail: full cards up close, lamp-only pills far out.
  - `onlyRenderVisibleElements` for graphs with thousands of nodes.
  - **Live:** a firing reflex sends a pulse along its edge (brand motion: "light travels"). New reflexes draw in along their curve, never fade in.
- **Right inspector:**
  - the exact directive as the agent sees it;
  - evidence: support, lower bound, turns saved, a uPlot trend;
  - treated vs control;
  - lifecycle history and recent traces;
  - **"copy CLI command"** buttons (`adage review strengthen <id>`). The web view stays read-only in v1, which keeps today's security model (§8, Server).
- **Other views, in the router:**
  - **Traces:** a live timeline, and the replay scrubber that rebuilds the map over time.
  - **Evidence:** charts across reflexes, cost and turns with GPU orange on cost only.
  - **Health:** a mirror of doctor.
- **Everywhere:**
  - All state is kept in the URL, so every view is shareable.
  - `?` opens the shortcut sheet.
  - `g m` / `g t` / `g e` jump between views.
- **Empty and first-run state:** an animated walkthrough with the exact command to run. It turns into the real graph the moment the first trace lands (over SSE).

### Server (`serve.rs` → `crates/adage-cli/src/map/`)

The dependency-light standard-library design stays, with no tokio. The hook binary gains nothing on its path.

- **Connections:** a thread per connection with a small cap, replacing one connection at a time.
- **Routes:**
  - `GET /api/v1/graph`: a snapshot from `graph::build`, with a JSON Schema published for the frontend types (types generated with `json-schema-to-typescript`).
  - `GET /api/v1/events`: SSE carrying graph *deltas* and decisions, with `Last-Event-ID` resume. It is fed by the existing `ui/tail.rs` tailers (`SpoolTail`, `LineTail`).
  - `GET /api/v1/reflex/:id`, `GET /api/v1/health`.
- **Assets:**
  - Built by `xtask web` into `crates/adage-cli/assets/map/`, pre-compressed with brotli, embedded with `include_bytes!`.
  - Served with `Content-Encoding: br`, a gzip fallback, and immutable cache headers on hashed names.
  - The built bundle is **committed**, with a CI drift check, so `cargo build` and cargo-dist never need Node. Budget: ≤ 400 KB brotli, excluding elkjs, which lazy-loads in its worker.
- **Security kept:** 127.0.0.1 only, the Host check against DNS rebinding, strict CSP. The CSP can now drop `unsafe-inline` because assets are hashed files.
- **`adage map` command:**
  - opens the browser (`--no-open` suppresses it);
  - prints the URL as an OSC 8 link;
  - shows a one-line inline live status (clients connected, events/s) until Ctrl-C;
  - `--port`, `--focus <id>`.
- `demo/dashboard/` is deleted in favour of `adage map --demo`.

---

## 9. Shared fixtures and tests

**Scenario corpus:** `fixtures/ui/*.json`, generated deterministically by `xtask fixtures` from `adage-sim` (the same engine `ui/demo.rs` replays). Each scenario is a timeline of states:

- `empty`: first run
- `first-reflex`: the payoff moment
- `busy`: a live session with fire, prime and inhibit
- `dense`: 2,000 nodes, for the performance ceiling
- `degraded`: doctor failures
- `control-arm`
- `long-names`: truncation

A hidden `--fixture <name>` flag on `watch`, `ui`, `map` and each `out`-based command loads a scenario instead of `ADAGE_HOME`.

| Layer | Tool | What is checked |
|---|---|---|
| Widgets and screens | `insta` + ratatui `TestBackend` | Every tab, modal and wizard step, at widths 60/80/120, with colour off and on (styled cells serialised). The 6 hand-rolled snapshots in `ui/snapshots/` move to insta. |
| CLI output | `trycmd`/`snapbox` | One `.md` case per command × {tty-styled, piped, `NO_COLOR`, `--json`, `--plain`} |
| Styled output as docs | `term-transcript` | SVG renders of key commands, embedded in the README and docs |
| Interactive end-to-end | `expectrl` + `portable-pty` + `vt100` | `adage init` walked step by step in a real PTY (including esc/back), `watch` and `ui` keyflows |
| Demo GIFs | charmbracelet `vhs` `.tape` files | README and brand GIFs re-rendered in CI with deterministic fixtures, so they never go stale |
| Web components | Storybook 9 (Vite) | Each node type, inspector and empty state from fixture JSON, with the a11y addon |
| Web end-to-end and visual | Playwright + `@axe-core/playwright` | Screenshots of each view, light and dark, with motion disabled, in the Playwright Docker image. axe runs on every view. SSE replay from fixtures. |
| Budgets | existing `examples/hook_latency.rs` + new CI jobs | Hook p99 latency **unchanged** (hard gate). Release binary size delta ≤ +2 MB. TUI frame ≤ 8 ms at 200×60. Map first paint ≤ 1 s on `dense`. |

**Rule** (as in the Codex AGENTS.md): any user-visible change updates a snapshot, and CI rejects UI diffs without one.

---

## 10. Phasing

| Phase | Work | Exit |
|---|---|---|
| **0. Foundation** | Base the work on the rebrand branch (or after it merges to main). Write the style guide. Set up DTCG tokens and the `xtask` generator. Create `adage-term` with `caps`/`theme`/`out`, and move `ui/theme.rs` into it. Build the fixture corpus and hidden `--fixture` flag. Move snapshots to insta. Add CI budgets. | Token drift, contrast and hook latency checks run in CI. Existing snapshots re-blessed through insta. |
| **1. Command line** | clap migration with the lexicon names. `out`, `progress` and `errors` adopted by every command. `--json` everywhere. Completions and man pages. | Every command has trycmd goldens in all output modes. No `println!` outside `out` (enforced by clippy). |
| **2. Setup** | Inline `adage init` flow and the finish card. | expectrl PTY test of the full flow; vhs GIF. |
| **3. Terminal apps** | `watch` redesign; `ui` component refactor, palette, and the Reflexes and Map tabs; tachyonfx motion. | insta snapshots at 3 widths × 2 colour modes; frame budget met. |
| **4. Map** | Frontend app, v1 API + SSE server, `adage map`, asset embedding, removal of `brain.html` and `demo/dashboard`. | Playwright and axe green; bundle budget met; replay scrubber works on `busy`. |
| **5. Polish and docs** | Motion pass against brand §09. README visuals from term-transcript and vhs. `docs/design/` published. Full copy pass against the lexicon. | A cross-surface review of one fixture shown in init → status → watch → ui → map reads as one product. |

Phases 1–2 and phase 4's frontend can run in parallel once phase 0 lands.

---

## 11. Critical files

**Modified:**

- `crates/adage-cli/src/main.rs`: clap and the lexicon verbs.
- `ui/theme.rs`: moved into `adage-term`.
- `ui/app.rs`: split into `ui/tabs/`.
- `ui/view.rs`, `ui/watch.rs`, `ui/wizard.rs`, `ui/tests.rs`.
- `serve.rs`: becomes `map/`.
- `graph.rs`, `doctor.rs`, `review.rs`, `survey.rs` (→ `sense`), `init.rs`, `models.rs`, `status`/`report` in `main.rs`.
- `crates/adage-eval/src/cli.rs`.
- `Cargo.toml` and `deny.toml`.
- CI workflow.

**New:**

- `design/tokens/`
- `xtask/`
- `crates/adage-term/`
- `web/map/`
- `fixtures/ui/`
- `docs/design/style.md`

**Reused:**

- `ui/jobs.rs` (`Loader`) and `ui/tail.rs` (`SpoolTail`, `LineTail`): the data layer for the TUI and the SSE feed.
- `graph::build` for map snapshots.
- `ui/demo.rs` and `adage-sim` for fixtures and `--demo`.
- `health::Fix` for one-key fixes.
- `install::AGENT_ENV` and `ui::interactive()` for non-tty and agent fallbacks.
- `term.rs` panic-safe restore.

**Removed:** `assets/brain.html` and `demo/dashboard/`.

## 12. Verification

1. Run `cargo test --workspace`. insta, trycmd and expectrl suites pass, and `cargo insta review` shows no pending snapshots.
2. `cargo xtask tokens --check`, `cargo xtask web --check` and `cargo xtask fixtures --check` report no drift.
3. `cargo run --example hook_latency`: p99 within noise of the base branch.
4. Manual walk-through on the `busy` fixture:
   1. `adage init --fixture first-reflex`
   2. `adage status`
   3. `adage watch --fixture busy`
   4. `adage ui --fixture busy` (every tab, the palette, `?`)
   5. `adage map --demo` in a browser, light and dark
   6. Repeat 2–4 with `NO_COLOR=1`, `TERM=xterm-256color` and a light terminal theme.
5. `pnpm --dir web/map test:e2e` (Playwright and axe) and `pnpm --dir web/map build` within the bundle budget.
6. `vhs` tapes render without a diff against the committed GIFs.
7. Windows CI: `watch --once`, trycmd goldens, and `adage map` serving assets.
