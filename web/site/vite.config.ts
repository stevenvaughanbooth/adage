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

export default defineConfig({
  plugins: [html()],
  build: {
    outDir: "dist",
    emptyOutDir: true,
    target: "es2022",
    sourcemap: false,
    assetsInlineLimit: 0,
    chunkSizeWarningLimit: 900,
    rollupOptions: {
      input: { main: "index.html", notfound: "404.html" },
      // three.js and the scene are one lazy chunk: the words paint before it arrives.
      output: {
        manualChunks(id) {
          if (id.includes("/node_modules/three/") || id.includes("/src/hero/")) return "scene";
        },
      },
    },
  },
  server: { port: 5174 },
});
