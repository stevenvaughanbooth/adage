# web/map: the page `adage map` serves

The React app behind `adage map` (and the map `adage ui` opens, and the Claude Code pane's
**Explore in browser**). It explains reflexes, their evidence and live traces. A map started
from the person's own terminal can review individual reflexes; an agent-launched or demo map
stays read-only and shows the appropriate terminal or `/adage` review command. Approval accepts
evidence; advice strength and automatic execution permission remain separate.

The binary embeds the built page (`crates/adage-cli/assets/map/`) and serves its graph, events,
health and traces from the same loopback origin. `GET /api/v1/review` reports whether the
startup `Person` gate enabled review and supplies a per-server random CSRF token. With
`reference` and `reflex` query parameters it returns the current directive, proposed changes,
allowed decisions and an exact revision. `POST /api/v1/review` accepts only bounded JSON with
that reference, reflex, verb and revision, the exact local Origin, and `X-Adage-CSRF` token.
The application rechecks the revision under the writer lock; a stale review writes nothing.
There is no execution-grant API. Host/port checks and duplicate-header rejection apply before
routing, and no CORS access is enabled. Read endpoints and read-only startup do not create data.

Traces separates recorded hook activity from `learning_history`, the latest retained
conditioning counts per repository. Histories and subagent histories are learner input, not
automatically recorded hook sessions; overlapping runs are never summed into a lifetime total.

## Build and check

Node 22 and pnpm 10 (`packageManager` in `package.json`).

```bash
pnpm install --frozen-lockfile
pnpm typecheck        # tsc --noEmit
pnpm test             # vitest (src/**/*.test.ts, node environment)
pnpm assets           # build, then embed into crates/adage-cli/assets/map/ and src/map/assets_list.rs
pnpm drift            # CI: rebuild and fail if the committed embedded copy differs (commit first)
pnpm test:e2e         # Playwright + axe against target/debug/adage map (it runs cargo build first)
```

After any change here, run `pnpm assets` and commit `crates/adage-cli/assets/map/` and
`crates/adage-cli/src/map/assets_list.rs` with it: the binary ships what is embedded, and `pnpm
drift` fails until they match. The embed is deterministic and the page has a 400 KB brotli budget
(`scripts/embed.mjs`). Node is never needed to build the binary.

`pnpm dev` runs Vite against a running `adage map` (`ADAGE_MAP_API`, default
`http://127.0.0.1:8765`). The demo dashboard (`demo/dashboard/serve.py`) listens on 8766, so both
run at once.

## Words by surface

The graph JSON carries `adage: { version, latest, channel }` (the serving binary's version, a newer
release when the release check knows one, and how it was installed). `src/lib/setup.ts` words every
next step for that surface: a Claude Code plugin user (`channel: "plugin"`) is told the `/adage`
verb (`/adage review`, `/adage health`, `/adage update`) and never to open a terminal for something
the plugin does; anyone else is told the `adage` command (`adage init`, `adage ui`, `adage review`,
`adage update`). A binary that sends no `adage` gets the terminal words. The Setup view, the
command buttons, the palette, the first-run view and the top bar's version text all read it.

## Layout

- `src/api/`: the JSON types the binary serves (`types.ts`), their normalisation for older
  binaries, and the store (`client.ts`: one graph, the event stream, health).
- `src/views/`: Map, Traces, Evidence, Health, How it works (three.js, loaded lazily), Setup, and
  the first-run view.
- `src/components/`, `src/lib/`: shared pieces and pure functions (tested with vitest).
- `e2e/`: Playwright specs and their fixtures (`e2e/fixtures/make.mjs` writes them; the graph's
  top-level fields are pinned by `adage-app`'s
  `the_served_graph_has_the_fields_the_web_map_is_tested_with`).
