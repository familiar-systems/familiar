import { defineConfig, devices } from "@playwright/test";

// E2E tests run against the dev Vite server with mocked Hanko + /me. This
// keeps the SPA exercise hermetic: no Caddy proxy, no platform server, no
// real Hanko backend - just enough to assert the browser-side behavior we
// can't catch with vitest. Tests live under apps/web/e2e.
//
// Why a separate config (not vitest browser): vitest's browser mode is
// for unit tests rendered in a real browser; full-app navigation flows
// (Link click + lazy chunk + DOM-mutation assertions) want @playwright/
// test's first-class support for fixtures, traces, and webServer
// management.
//
// Browser install: `mise exec -- pnpm --filter @familiar-systems/web exec
// playwright install chromium`. CI runs this once; locally it's a
// one-time setup.
export default defineConfig({
  testDir: "./e2e",
  fullyParallel: true,
  forbidOnly: !!process.env.CI,
  retries: process.env.CI ? 1 : 0,
  reporter: "list",
  use: {
    baseURL: "http://localhost:5173",
    trace: "on-first-retry",
  },
  projects: [{ name: "chromium", use: { ...devices["Desktop Chrome"] } }],
  webServer: {
    command: "pnpm dev",
    url: "http://localhost:5173",
    reuseExistingServer: !process.env.CI,
    timeout: 30_000,
    // Vite dev needs VITE_HANKO_API_URL to instantiate Hanko; the value is
    // never actually hit (the test mocks all auth.preview.familiar.systems
    // requests via route interception), but the SDK constructs URLs with
    // it at module-load time. Pin a placeholder so dev startup doesn't
    // explode when running e2e outside `mise run dev:web`.
    env: {
      VITE_HANKO_API_URL: "https://auth.example.test",
      VITE_SITE_URL: "http://localhost:8080",
    },
  },
});
