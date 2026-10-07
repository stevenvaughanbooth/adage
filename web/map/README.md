# web/map: the page `adage map` serves

The React app behind `adage map` (and the map `adage ui` opens, and the Claude Code pane's
**Explore in browser**). It is read-only by design: it explains reflexes, their evidence and live
traces, and names the command or `/adage` verb that changes something; it never changes anything
itself. The `adage` binary embeds the built page (`crates/adage-cli/assets/map/`) and serves it
with the JSON it reads (`/api/v1/graph`, `/api/v1/events`, `/api/v1/health`, `/api/v1/trace`);
nothing is requested from anywhere but that origin.

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
