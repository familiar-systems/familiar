// Code-based TanStack Router setup for the SPA. Three routes today: hub
// (/), settings (/settings), and login (/login). The first two require
// auth via the shared requireAuth beforeLoad; login is open.
//
// When adding a route with a branded-ID param, brand the ID at the URL
// boundary with parseParams + a Zod schema co-located with the brand.
// Platform-shared brands (CampaignId, UserId) live in
// @familiar-systems/types-app; campaign-only brands (ThingId, BlockId)
// will live in @familiar-systems/types-campaign. Example for /campaigns/$id:
//
//   import { campaignIdSchema } from "@familiar-systems/types-app";
//   parseParams: ({ id }) => ({ id: campaignIdSchema.parse(id) }),
//
// useParams() then returns { id: CampaignId }, which flows into the
// typed API client without a cast. lib/api.ts already wires this for
// the platform server (openapi-fetch over PlatformPaths, generated from
// utoipa via tooling/openapi-codegen + ts-rs). The campaign server is
// slated to grow the same pipeline (utoipa annotations, an emit-openapi
// binary, a generated CampaignPaths interface in types-campaign, a
// sibling typed client), so the same end-to-end chain (URL string,
// branded ID, typed call) will hold across both servers once that
// lands. The router's parseParams is where brands are minted; the
// typed clients are where they're consumed. Both import the brand
// from the same source of truth, which is the project's "Zod at
// every system boundary" rule applied to the URL.

import {
  createRootRouteWithContext,
  createRoute,
  createRouter,
  Link,
  Outlet,
  redirect,
} from "@tanstack/react-router";
import type { MeResponse } from "@familiar-systems/types-app";
import { Hub } from "./hub";
import { Login } from "./login";
import { Settings } from "./settings";

export interface RouterContext {
  me: MeResponse | null;
}

const rootRoute = createRootRouteWithContext<RouterContext>()({
  component: () => <Outlet />,
});

// Refines context.me from MeResponse | null to MeResponse for child routes
// that compose this beforeLoad. Pages under requireAuth read me from route
// context (useRouteContext) and get a non-nullable type without a cast.
function requireAuth({ context }: { context: RouterContext }): { me: MeResponse } {
  if (context.me === null) {
    throw redirect({ to: "/login" });
  }
  return { me: context.me };
}

const hubRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/",
  beforeLoad: requireAuth,
  component: Hub,
});

const settingsRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/settings",
  beforeLoad: requireAuth,
  component: Settings,
});

const loginRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/login",
  component: Login,
});

const routeTree = rootRoute.addChildren([hubRoute, settingsRoute, loginRoute]);

// Renders for any URL the router can't match. Stays minimal on purpose:
// it has to work in both authed and unauthed states (unmatched URLs don't
// run beforeLoad, so context.me may be null), and the only chrome that
// requires me is Shell. Linking back to "/" funnels the user through the
// router's normal auth gate.
function NotFound(): React.ReactElement {
  return (
    <div className="min-h-screen flex flex-col items-center justify-center gap-4 p-8 text-center">
      <p className="font-display text-2xl">404: Not Found.</p>
      <p className="font-display text-2xl">The labyrinth claims another one.</p>
      <Link to="/" className="text-sm text-muted-foreground underline hover:text-foreground">
        Return to hub
      </Link>
    </div>
  );
}

// basepath honors per-PR preview prefix (/pr-42/) via Vite's BASE_URL,
// which vite.config.ts feeds from VITE_BASE_PATH. Both <Link> and
// useNavigate respect basepath, so call sites pass logical paths
// ("/settings") and the router prepends the prefix at runtime.
export const router = createRouter({
  routeTree,
  basepath: import.meta.env.BASE_URL,
  context: { me: null },
  defaultNotFoundComponent: NotFound,
});

declare module "@tanstack/react-router" {
  interface Register {
    router: typeof router;
  }
}
