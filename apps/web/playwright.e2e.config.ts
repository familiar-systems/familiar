import { defineConfig, devices } from "@playwright/test";

// Full-stack END-TO-END config (distinct from playwright.config.ts, which is
// the mocked frontend-integration tier). This config does NOT start any
// servers: the `.mise/tasks/e2e` shell harness boots the real stack (mock
// Hanko + platform + campaign + Vite + Caddy) on an isolated port block, waits
// for /health, then runs this config against the Caddy app apex. Step 9
// (graceful shutdown) and step 10 (SQLite assertion) live in the harness, not
// here, so there is no globalTeardown.
//
// baseURL is the e2e Caddy apex (port 18080), not Vite directly: the SPA calls
// same-origin /api, /campaign, /catalog and opens the Loro WebSocket through
// the proxy, so the browser must enter through Caddy.
export default defineConfig({
  testDir: "./e2e",
  testMatch: "smoke.spec.ts",
  fullyParallel: false,
  forbidOnly: !!process.env.CI,
  retries: 0,
  reporter: "list",
  // The full stack boots cold (Rust servers, WS sync); give actions room.
  timeout: 60_000,
  expect: { timeout: 15_000 },
  use: {
    baseURL: "http://app.localhost:18080",
    trace: "on-first-retry",
  },
  projects: [{ name: "chromium", use: { ...devices["Desktop Chrome"] } }],
});
