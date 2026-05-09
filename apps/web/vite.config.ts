import { tanstackRouter } from "@tanstack/router-plugin/vite";
import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

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
export default defineConfig({
  base: basePath,
  plugins: [tanstackRouter({ target: "react", autoCodeSplitting: true }), react(), tailwindcss()],
  server: {
    port: 5173,
  },
});
