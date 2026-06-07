import path from "node:path";
import { fileURLToPath } from "node:url";

import { storybookTest } from "@storybook/addon-vitest/vitest-plugin";
import { playwright } from "@vitest/browser-playwright";
import { defineConfig, mergeConfig } from "vitest/config";

import viteConfig from "./vite.config";

// The component/interaction test tier: every story under src/**/*.stories.tsx
// runs as a real-browser Vitest test (Playwright Chromium). storybookTest()
// transforms stories into tests (a smoke render + the play function as an
// interaction test). We mergeConfig the app's vite.config so loro-crdt's wasm,
// Tailwind, and the React plugin all apply unchanged.
//
// Deliberately a SEPARATE config (not folded into vitest.config.ts), mirroring
// the Playwright tier split (playwright.config.ts vs playwright.e2e.config.ts):
// `mise run test` / `mise run check` stay node-only and fast; this tier runs
// via its own `mise run web:stories` task and needs `mise run setup:playwright`
// once for the chromium binary.
const dirname = path.dirname(fileURLToPath(import.meta.url));

export default mergeConfig(
  viteConfig,
  defineConfig({
    plugins: [storybookTest({ configDir: path.join(dirname, ".storybook") })],
    // loro-crdt ships a WebAssembly module; esbuild's dep pre-bundler can't
    // process the .wasm import, and discovering it mid-run triggers a
    // re-optimization that 404s already-loaded modules ("Failed to fetch
    // dynamically imported module"). Excluding it leaves the import to the
    // wasm() plugin (inherited from vite.config), which handles it correctly.
    optimizeDeps: { exclude: ["loro-crdt"] },
    test: {
      name: "storybook",
      browser: {
        enabled: true,
        provider: playwright(),
        headless: true,
        instances: [{ browser: "chromium" }],
      },
      // No setupFiles: since Storybook 10.3, @storybook/addon-vitest applies the
      // preview annotations (global decorators, the Tailwind import) to story
      // tests automatically.
    },
  }),
);
