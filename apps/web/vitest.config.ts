import { defineConfig } from "vitest/config";

// apps/web's vitest config exists for `pnpm --filter @familiar-systems/web
// test` (vitest invoked in this directory). The root vitest.config.ts
// already excludes e2e/ when tests run from the repo root; this file
// keeps the same exclusion in scope for the workspace-local invocation.
// Playwright specs under e2e/ are owned by playwright.config.ts and run
// via `mise run e2e`.
export default defineConfig({
  test: {
    exclude: ["**/node_modules/**", "**/dist/**", "**/e2e/**"],
  },
});
