import { defineConfig } from "vitest/config";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";

// The build is embedded in the adage binary (crates/adage-cli/assets/map/, by scripts/embed.mjs):
// hashed names under assets/, no inline script (the server's CSP forbids it), no source maps.
export default defineConfig({
  plugins: [react(), tailwindcss()],
  base: "/",
  build: {
    outDir: "dist",
    emptyOutDir: true,
    target: "es2022",
    sourcemap: false,
    modulePreload: { polyfill: false },
    assetsInlineLimit: 0,
    cssCodeSplit: false,
    chunkSizeWarningLimit: 2048,
    // How it works (three.js) is one lazy chunk, loaded when its tab opens; the budget script
    // counts it apart from the page.
    rollupOptions: {
      output: {
        manualChunks(id) {
          if (id.includes("/node_modules/three/") || id.includes("/src/how/") || id.includes("/src/views/HowItWorks")) return "how";
        },
      },
    },
  },
  worker: { format: "es" },
  // `pnpm dev` (or `vite preview`) against a running `adage map` (ADAGE_MAP_API, default port 8765).
  server: {
    proxy: { "/api": process.env.ADAGE_MAP_API ?? "http://127.0.0.1:8765" },
    fs: { allow: ["../.."] },
  },
  preview: {
    proxy: { "/api": process.env.ADAGE_MAP_API ?? "http://127.0.0.1:8765" },
  },
  test: {
    include: ["src/**/*.test.ts"],
    environment: "node",
  },
});
