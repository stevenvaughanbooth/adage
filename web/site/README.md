# web/site: the public Adage website

The landing page: the teaser film, the 3D hero (a brain and spinal cord drawn with three.js),
how it works, the evidence, getting started and the early-access list. A static Vite build with one
Vercel Function (`api/waitlist.ts`). It is separate from `web/map`, which the `adage` binary embeds.

```bash
pnpm install
pnpm dev          # http://localhost:5174
pnpm typecheck
pnpm test:e2e     # builds, serves with `vite preview`, runs Playwright and axe (both themes, phone width)
```

## How the page behaves

- **The film opens the page**, muted (browsers allow no sound without a gesture), in the night or
  day cut to match the theme. The controls (X, sound, skip) appear when the pointer moves or a
  control has keyboard focus; Escape skips. When it ends or is skipped it fades into the hero, which
  then assembles. If the film cannot load, the page starts at the hero. With reduced motion or
  data saving on, it waits for Play. "Watch the teaser" replays it with sound.
- **Theme:** follows the system; the Day/Night button overrides it and is remembered.
- **The scene** loads after the words, as its own chunk (three.js, about 150 KB gzipped). Without
  WebGL the panel still describes each part.

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
   | `WAITLIST_WEBHOOK_URL` | for sign-ups | Where each sign-up is POSTed as JSON (`email`, `source`, `at`, plus `text`/`content` for Slack or Discord webhooks; Zapier or Make work too). Unset: the form says sign-ups aren't open yet. Nothing is stored by the site. |
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
| `src/page.ts` | Section reveals, copy button, repository links, the early-access form. |
| `api/waitlist.ts` | `POST /api/waitlist`: validates and forwards to `WAITLIST_WEBHOOK_URL`. |
| `scripts/media.mjs` | Encodes, and optionally uploads, the films. |
| `public/` | Favicon, Open Graph image, posters, `robots.txt`. |
