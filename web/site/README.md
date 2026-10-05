# web/site: the public Adage website

The landing page (the teaser film, the 3D hero, how it works, the evidence, getting started and the
early-access list); `/how`, which opens on how Adage plugs into Claude Code (a diagram that follows
the hooks as the page scrolls) and then walks the nine chapters in 3D, with the math behind every
threshold; and `/how-we-tested`, the evidence page: what we tested and how, the held-out result in
advisory and execution mode, and the limits. The chapters and their 3D stage are shared with `adage map`
(`web/map/src/how`, imported through the `@how` alias), so the two never tell different stories.
A static Vite build with one
Vercel Function (`api/waitlist.ts`). It is separate from `web/map`, which the `adage` binary embeds.

```bash
pnpm install
pnpm dev          # http://localhost:5174
pnpm typecheck
pnpm test:api     # registration availability contract; no requests leave the test
pnpm test:evidence  # the held-out converter against synthetic reports
pnpm test:e2e     # builds, serves with `vite preview`, runs Playwright and axe (both themes, phone width)
```

## How the page behaves

- **The film opens the page with sound**, in the night or day cut to match the theme. If the
  browser blocks audible autoplay, a "Play the teaser with sound" button starts it on click. The
  controls (X, sound, skip) appear when the pointer moves or a control has keyboard focus; Escape skips. When it ends or is skipped it fades into the hero, which
  then assembles. If the film cannot load, the page starts at the hero. With reduced motion or
  data saving on, it waits for Play. "Watch the teaser" replays it with sound.
- **Theme:** follows the system; the Day/Night button overrides it and is remembered.
- **The scene** loads after the words, as its own chunk (three.js, about 150 KB gzipped). Without
  WebGL the panel still describes each part.

## Evidence numbers come from data

Every result number on the pages is read from `public/evidence/*.json` at build time: a token such as
`{{ev:cold-start:cost_reduction_percent|fixed1}}` becomes that value, and `{{heldout:<kind>}}` becomes
a held-out component (`src/evidence/tokens.ts`, `src/evidence/heldout.ts`; a missing value fails the
build). Each file names the archived record and sha256 it came from.

The held-out result lives in `public/evidence/heldout.json`, converted from the report
`heldout_cohort.py report` writes. To convert a new one, then rebuild and deploy:

```bash
node scripts/heldout-evidence.mjs $H/reports/heldout-<time>.json     # $H: the held-out root
```

It reads the report two ways, as Adage ships: **advisory mode** (NOEXEC, the default) against the
agent alone, the analysis's registered report-only comparison, which leads the pages; and
**execution mode** (BEST, opt-in) against the agent alone, the study's gated test, shown as passed or
as experimental, with the report's own outcome in the detail. Unverified, protocol-failure,
incomplete or invalid reports show no verdict and no headline number. It also reads the sessions in
`$H/sessions` (read-only) for tokens and which blocks failed (`--sessions <dir>` names them,
`--no-sessions` skips them), and copies the report and its decision record beside the output
(`heldout-report.json`, `heldout-record.md`), unchanged, so the pages link the exact files. The pages
re-read `heldout.json` when they open, so a cached page shows a new result. `--pending` writes the
"running" state.

## Deploying on Vercel

1. **Import the repository** in Vercel (Add New → Project) and set **Root Directory** to
   `web/site`. `vercel.json` sets the framework (Vite), pnpm, the build and the security headers.
2. **Films.** They are not kept in git. Create a Blob store (Storage → Create → Blob) connected to
   the project, copy its `BLOB_READ_WRITE_TOKEN`, then:
   ```bash
   BLOB_READ_WRITE_TOKEN=… pnpm media path/to/film-night.mp4 path/to/film-day.mp4 --upload
   ```
   It encodes both cuts (about 14 MB each), refreshes the posters, uploads the films and prints the
   `VITE_MEDIA_BASE` to set.
3. **Environment variables** (Settings → Environment Variables), then redeploy:

   | Variable | Needed | What it does |
   | --- | --- | --- |
   | `VITE_MEDIA_BASE` | for the film | Where the films are served from, ending in `/` (printed by `pnpm media --upload`). |
   | `VITE_FILM_DARK_URL`, `VITE_FILM_DAY_URL` | or these | Each cut by its full URL, whatever name it was uploaded with (set on the project: the `adage-media` Blob store). |
   | `WAITLIST_WEBHOOK_URL` | for sign-ups | Where each sign-up is POSTed as JSON (`email`, `source`, `at`, plus `text`/`content` for Slack or Discord webhooks; Zapier or Make work too). Unset: the availability check shows registration is closed before asking for an address. Nothing is stored by the site. |
   | `VITE_REPO_URL` | when the repository is public | Shows the README and lexicon links under "Get started". |
   | `SITE_URL` | optional | The canonical URL for Open Graph tags. Defaults to the production domain Vercel provides. |
   | `VITE_FILM=off` | optional | Builds the site without the opening film. |
4. **Domain:** Settings → Domains → add your domain. Preview deployments for every branch come free.

Media URLs must stay on `*.public.blob.vercel-storage.com` (or the site itself); anything else is
blocked by the Content Security Policy in `vercel.json`.

Before opening sign-ups, add a rate limit on `/api/waitlist` (Firewall → Configure → New rule:
path equals `/api/waitlist`, rate limit, e.g. 5 requests per minute per IP). The function accepts
only plain addresses and never lets a Slack or Discord message mention anyone, but it has no limit
of its own.

The CSP allows scripts from the site only, so Vercel's preview toolbar (from vercel.live) is blocked
on preview deployments; turn it off in Settings → General → Vercel Toolbar to keep the console quiet.

## Files

| Path | What |
| --- | --- |
| `index.html` | The page: film overlay, hero, sections. |
| `src/main.ts` | Theme, hero tabs and tour, film wiring, lazy scene. |
| `src/film.ts` | The opening film. |
| `src/hero/scene.js` | The three.js scene (ported from the hero prototype; day and night palettes). |
| `src/tabs.ts` | The hero's tab copy. |
| `how.html`, `src/how-page.ts` | `/how`: the walkthrough shell around the shared chapters, and the math. |
| `src/wilson.ts` | The Wilson calculator on `/how` (the same bound as `wilson_lower` in adage-tracks). |
| `src/plug.ts` | `/how`'s opening scene: the diagram lights the hook of the step in the middle of the screen. |
| `how-we-tested.html`, `src/evidence-page.ts` | `/how-we-tested`: the evidence page. |
| `src/evidence/` | The held-out component (`heldout.ts`), the build-time number tokens (`tokens.ts`) and the loader that redraws the held-out result from its file (`load.ts`). |
| `public/evidence/` | The data every number comes from: `heldout.json` (with the report and decision record it came from), `cold-start.json`, `benchmarks.json`. |
| `scripts/heldout-evidence.mjs` | Converts a held-out report into `public/evidence/heldout.json`. |
| `e2e/fixtures/heldout/` | Synthetic held-out reports (pass, a mixed fail, a fail on savings, unverified) made with the registered analysis, for the tests; not study results. |
| `src/theme.ts` | The Day/Night choice, shared by every page. |
| `src/page.ts`, `src/signup.ts` | Section reveals, copy button, repository links, and recoverable early-access registration. |
| `api/waitlist.ts` | `GET` returns only registration availability; `POST` validates and forwards to `WAITLIST_WEBHOOK_URL`. |
| `scripts/media.mjs` | Encodes, and optionally uploads, the films. |
| `public/` | Favicon, Open Graph image, posters, `robots.txt`. |
