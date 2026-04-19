import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

// VITE_BASE_PATH matches the SPA's path prefix on the deployed apex.
// Dev and prod: "/app/". Preview: "/pr-${PR_NUMBER}/app/".
// Falls back to "/app/" so local `vite dev` mirrors prod without extra env.
const basePath = process.env.VITE_BASE_PATH ?? "/app/";

// No server.proxy block: the dev-time reverse proxy is Caddy (Caddyfile.dev)
// on :8080, not Vite itself. Vite serves the SPA at localhost:5173/app/ and
// Caddy forwards /app/* to it. Backend calls (/api, /campaign) are handled
// by Caddy at the front door. See `mise run dev:proxy` + Caddyfile.dev.
export default defineConfig({
  base: basePath,
  plugins: [react()],
  server: {
    port: 5173,
  },
});
