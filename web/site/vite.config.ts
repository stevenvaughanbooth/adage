import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { defineConfig, type Plugin } from "vite";
import { fillTokens } from "./src/evidence/tokens";

// The site's absolute URL, for canonical and Open Graph tags: SITE_URL, or the production domain
// Vercel provides at build time.
const site = (process.env.SITE_URL ?? (process.env.VERCEL_PROJECT_PRODUCTION_URL ? `https://${process.env.VERCEL_PROJECT_PRODUCTION_URL}` : "")).replace(/\/$/, "");

// The evidence numbers: read from public/evidence/*.json at build time (src/evidence/tokens.ts).
const evidence = (name: string): unknown =>
  JSON.parse(readFileSync(fileURLToPath(new URL(`./public/evidence/${name}.json`, import.meta.url)), "utf8"));

const html = (): Plugin => ({
  name: "adage-site-html",
  transformIndexHtml(src, ctx) {
    let out = fillTokens(src.replaceAll("__SITE_URL__", site), evidence, ctx.path);
    // Without the film, the page must not open on its (empty) overlay.
    if (process.env.VITE_FILM === "off") out = out.replace('<div class="film" id="film"', '<div class="film" id="film" hidden');
    return out;
  },
});

// The How it works chapters are shared with the map (web/map/src/how): one source for both. They
// import three, which resolves from this package (the map's node_modules are not installed here).
const how = fileURLToPath(new URL("../map/src/how", import.meta.url));
// The Packs section shows real packs, read from the repository as they are: Adage's base instincts
// (packs/) and a reflex pack shared from a study (src/packs.ts).
const repo = fileURLToPath(new URL("../..", import.meta.url));
const REFLEX_PACK = "evals/datasets/live-20260928-acme-billing-sonnet5/packs/work-items";

export default defineConfig({
  plugins: [html()],
  resolve: { alias: { "@how": how, "@repo": repo }, dedupe: ["three"] },
  build: {
    outDir: "dist",
    emptyOutDir: true,
    target: "es2022",
    sourcemap: false,
    assetsInlineLimit: 0,
    chunkSizeWarningLimit: 900,
    rollupOptions: {
      input: { main: "index.html", how: "how.html", tested: "how-we-tested.html", notfound: "404.html" },
      // Chunks (Rolldown groups): three.js is shared by the hero and /how; the hero's scene and the
      // walkthrough's chapters stay with their own pages, so the words paint before either arrives.
      output: {
        advancedChunks: {
          groups: [
            // The glossary alone, so the evidence page's dotted words do not pull in the chapters.
            { name: "glossary", test: /[\\/]map[\\/]src[\\/]how[\\/]glossary\.ts$/, priority: 2 },
            { name: "three", test: /[\\/]node_modules[\\/]three[\\/]/ },
            { name: "scene", test: /[\\/]src[\\/]hero[\\/]/ },
            { name: "how", test: /[\\/]map[\\/]src[\\/]how[\\/]/ },
          ],
        },
      },
    },
  },
  server: { port: 5174, fs: { allow: [".", "../map/src", "../../packs", `../../${REFLEX_PACK}`] } },
});
