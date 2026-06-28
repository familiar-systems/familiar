import { withThemeByClassName } from "@storybook/addon-themes";
import {
  createMemoryHistory,
  createRootRoute,
  createRoute,
  createRouter,
  RouterProvider,
} from "@tanstack/react-router";
import type { Decorator, Preview } from "@storybook/react-vite";
import { useRef, useState } from "react";
import { I18nProvider } from "react-aria-components";
import { getTextDirection } from "../src/paraglide/runtime.js";

// Tailwind v4 entry (processed by @tailwindcss/vite, inherited from
// vite.config.ts). Without this stories render unstyled.
import "../src/styles/global.css";

// Many app components render TanStack Router <Link>s, which throw without a
// router in context. A global decorator wraps every story in a throwaway
// memory-history router: the story renders at the index route, and stub routes
// for the paths components link to (e.g. /c/$campaignId) let <Link> resolve an
// href in isolation. Components that don't use the router simply ignore it.
//
// The router is built once per mount (useState init) and kept stable: a fresh
// router each render would reset RouterProvider and remount the story, dropping
// any local UI state while you fiddle with controls in the workshop. Storybook
// re-binds `Story` to the current args on every render, so we read it through a
// ref (not the captured init value) to keep args live while the router stays put.
const withRouter: Decorator = (Story) => {
  const storyRef = useRef(Story);
  storyRef.current = Story;
  const [router] = useState(() => {
    const rootRoute = createRootRoute();
    const indexRoute = createRoute({
      getParentRoute: () => rootRoute,
      path: "/",
      component: () => {
        const Current = storyRef.current;
        return <Current />;
      },
    });
    const campaignRoute = createRoute({
      getParentRoute: () => rootRoute,
      path: "/c/$campaignId",
      component: () => null,
    });
    // The relationship chip links to a related page; stub the page route so its
    // <Link> resolves an href in isolation.
    const pageRoute = createRoute({
      getParentRoute: () => rootRoute,
      path: "/c/$campaignId/p/$pageId",
      component: () => null,
    });
    return createRouter({
      routeTree: rootRoute.addChildren([indexRoute, campaignRoute, pageRoute]),
      history: createMemoryHistory({ initialEntries: ["/"] }),
    });
  });
  return <RouterProvider router={router} />;
};

// Wraps every story in React Aria's locale context (locale-aware formatting +
// RTL) and sets `dir` so logical CSS and rtl: utilities react. The "Locale"
// toolbar switches it (ar mirrors to RTL); headless story tests run at en.
const withI18n: Decorator = (Story, context) => {
  const localeGlobal = context.globals.locale;
  const locale = typeof localeGlobal === "string" ? localeGlobal : "en";
  // Single-source the writing direction off Paraglide's Intl-backed resolver
  // rather than a hand-maintained RTL set, so the app and Storybook never drift.
  const dir = getTextDirection(locale);
  return (
    <I18nProvider locale={locale}>
      <div dir={dir}>
        <Story />
      </div>
    </I18nProvider>
  );
};

const preview: Preview = {
  decorators: [
    withRouter,
    withI18n,
    // Light/dark switch: toggles `.dark` on the preview <html>, the same
    // mechanism the app uses (src/lib/theme.ts). The dark tokens come from
    // packages/design via global.css.
    withThemeByClassName({
      themes: { light: "", dark: "dark" },
      defaultTheme: "light",
    }),
  ],
  globalTypes: {
    locale: {
      description: "Locale (ar mirrors to RTL)",
      defaultValue: "en",
      toolbar: {
        title: "Locale",
        icon: "globe",
        items: [
          { value: "en", title: "English" },
          { value: "ar", title: "العربية (RTL)" },
          { value: "zh-Hans", title: "中文" },
        ],
        dynamicTitle: true,
      },
    },
  },
  parameters: {
    layout: "centered",
  },
};

export default preview;
