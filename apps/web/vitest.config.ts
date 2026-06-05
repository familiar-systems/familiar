import { defineConfig } from "vitest/config";

// apps/web's vitest config exists for `pnpm --filter @familiar-systems/web
// test` (vitest invoked in this directory). The root vitest.config.ts
// already excludes the Playwright dirs when tests run from the repo root;
// this file keeps the same exclusion in scope for the workspace-local
// invocation. Both browser tiers are owned by Playwright, not vitest:
// integration/ (mocked backends, `playwright.config.ts`, `mise run
// web:integration`) and e2e/ (real stack, `playwright.e2e.config.ts`,
// `mise run e2e`).
export default defineConfig({
  test: {
    exclude: ["**/node_modules/**", "**/dist/**", "**/e2e/**", "**/integration/**"],
  },
});
