import type { NavigateOptions, ToOptions } from "@tanstack/react-router";

// Types the `href` on any React Aria link (Menu items, Tabs, ...) against the
// real route tree, so a non-existent route is a compile error. Pairs with the
// RAC RouterProvider in App.tsx, which turns those hrefs into TanStack client nav.
// NonNullable strips the `undefined` that indexing the optional `to` field
// admits; under exactOptionalPropertyTypes that undefined would otherwise fail
// the router.navigate/buildLocation call sites.
declare module "react-aria-components" {
  interface RouterConfig {
    href: NonNullable<ToOptions["to"]>;
    routerOptions: Omit<NavigateOptions, keyof ToOptions>;
  }
}
