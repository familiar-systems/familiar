import { tanstackRouter } from "@tanstack/router-plugin/vite";
import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";
import wasm from "vite-plugin-wasm";

// VITE_BASE_PATH matches the SPA's path prefix on the deployed app apex.
// Dev and prod: "/". Preview: "/pr-${PR_NUMBER}/".
// The SPA lives at the root of the app apex (app.familiar.systems in prod,
// app.localhost:8080 in dev); preview stacks a per-PR prefix on top.
const basePath = process.env.VITE_BASE_PATH ?? "/";

// No server.proxy block: the dev-time reverse proxy is Caddy (Caddyfile.dev)
// on :8080, which exposes `app.localhost:8080/` as the SPA origin and forwards
// /api and /campaign to their respective backends. Vite itself serves at
// localhost:5173. See `mise run dev:proxy` + Caddyfile.dev.
//
// tanstackRouter must come before react(): the plugin transforms route
// files into typed exports the React plugin then compiles. Order matters,
// per TanStack's docs.
//
// wasm() lets Vite/Rollup load loro-crdt, which ships as a WebAssembly module
// (the Loro CRDT used by the editor's sync). Without it the build fails on the
// .wasm import.
export default defineConfig({
  base: basePath,
  plugins: [
    tanstackRouter({ target: "react", autoCodeSplitting: true }),
    react(),
    tailwindcss(),
    wasm(),
  ],
  server: {
    // Caddy (Caddyfile.dev) hardcodes the SPA upstream as :5173. Without
    // strictPort a busy 5173 silently drifts to 5174 and the proxy 502s. Fail
    // loudly on the contracted port instead - no silent fallback default. Host
    // stays unpinned here: this config is shared with the integration
    // webServer (playwright.config.ts), which reaches Vite over localhost; the
    // dev-only 127.0.0.1 pin lives in mise.toml's dev:web task.
    port: 5173,
    strictPort: true,
  },
});
