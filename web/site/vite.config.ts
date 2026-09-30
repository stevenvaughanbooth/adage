import { fileURLToPath } from "node:url";
import { defineConfig, type Plugin } from "vite";

// The site's absolute URL, for canonical and Open Graph tags: SITE_URL, or the production domain
// Vercel provides at build time.
const site = (process.env.SITE_URL ?? (process.env.VERCEL_PROJECT_PRODUCTION_URL ? `https://${process.env.VERCEL_PROJECT_PRODUCTION_URL}` : "")).replace(/\/$/, "");

const html = (): Plugin => ({
  name: "adage-site-html",
  transformIndexHtml(src) {
    let out = src.replaceAll("__SITE_URL__", site);
    // Without the film, the page must not open on its (empty) overlay.
    if (process.env.VITE_FILM === "off") out = out.replace('<div class="film" id="film"', '<div class="film" id="film" hidden');
    return out;
  },
});

// The How it works chapters are shared with the map (web/map/src/how): one source for both. They
// import three, which resolves from this package (the map's node_modules are not installed here).
const how = fileURLToPath(new URL("../map/src/how", import.meta.url));

export default defineConfig({
  plugins: [html()],
  resolve: { alias: { "@how": how }, dedupe: ["three"] },
  build: {
    outDir: "dist",
    emptyOutDir: true,
    target: "es2022",
    sourcemap: false,
    assetsInlineLimit: 0,
    chunkSizeWarningLimit: 900,
    rollupOptions: {
      input: { main: "index.html", how: "how.html", notfound: "404.html" },
      // Chunks (Rolldown groups): three.js is shared by the hero and /how; the hero's scene and the
      // walkthrough's chapters stay with their own pages, so the words paint before either arrives.
      output: {
        advancedChunks: {
          groups: [
            { name: "three", test: /[\\/]node_modules[\\/]three[\\/]/ },
            { name: "scene", test: /[\\/]src[\\/]hero[\\/]/ },
            { name: "how", test: /[\\/]map[\\/]src[\\/]how[\\/]/ },
          ],
        },
      },
    },
  },
  server: { port: 5174, fs: { allow: [".", "../map/src"] } },
});
