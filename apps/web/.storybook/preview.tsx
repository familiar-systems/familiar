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

const preview: Preview = {
  decorators: [
    withRouter,
    // Toggles the `.dark` class on the preview <html> — the exact mechanism the
    // app uses (src/lib/theme.ts adds `.dark` to documentElement; global.css's
    // `@custom-variant dark (.dark)` + theme.css's `.dark { --vars }` do the
    // rest). Adds a light/dark switch to the Storybook toolbar.
    withThemeByClassName({
      themes: { light: "", dark: "dark" },
      defaultTheme: "light",
    }),
  ],
  parameters: {
    layout: "centered",
  },
};

export default preview;
