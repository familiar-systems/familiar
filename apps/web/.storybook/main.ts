import type { StorybookConfig } from "@storybook/react-vite";

// Storybook is the component workshop AND, via @storybook/addon-vitest, the
// component-test tier: every story runs as a real-browser Vitest test (see
// vitest.stories.config.ts). The Vite builder reuses apps/web's vite.config.ts
// automatically, so the wasm() plugin (loro-crdt), Tailwind, and the React
// plugin all apply with no duplication.
const config: StorybookConfig = {
  stories: ["../src/**/*.stories.@(ts|tsx)"],
  addons: ["@storybook/addon-vitest", "@storybook/addon-themes"],
  framework: {
    name: "@storybook/react-vite",
    options: {},
  },
};

export default config;
