// Pathless layout route for authenticated pages. The leading underscore
// in the file name tells TanStack Router this segment is layout-only - it
// does NOT add to the URL. So src/routes/_authed/index.tsx still matches
// "/", and _authed/settings.tsx still matches "/settings".
//
// This layout owns two cross-cutting responsibilities for every authed
// page:
//
//   1. The auth gate: beforeLoad narrows context.auth from AuthState
//      (the unauthed | authed sum type) to its 'authed' variant. If
//      we're 'unauthed', it throws a redirect to /login. The redirect
//      preserves the original URL via search.redirect, so a deep link
//      like /settings/foo survives the round-trip through login. The
//      narrowed user gets returned into context, and child routes
//      consume `user: MeResponse` directly - no nullability to handle.
//
//   2. <Shell>: the persistent app chrome (sticky nav, theme toggle,
//      user menu). Because Shell renders here and not in each leaf
//      route, the DOM stays mounted across hub <-> settings navigation.
//      Only the <Outlet /> swaps. No header rebuild, no background
//      flash.
//
// /login is intentionally NOT under _authed - it sits at the top level
// so the Hanko component renders without the app chrome and without
// the auth gate.

import { Outlet, createFileRoute, redirect } from "@tanstack/react-router";
import { Shell } from "../components/Shell";

export const Route = createFileRoute("/_authed")({
  beforeLoad: ({ context, location }) => {
    if (context.auth.kind === "unauthed") {
      throw redirect({
        to: "/login",
        search: { redirect: location.href },
      });
    }
    return { user: context.auth.user };
  },
  component: AuthedLayout,
});

function AuthedLayout(): React.ReactElement {
  const { user } = Route.useRouteContext();

  // hasCampaigns is wired through Shell so the brand-link emphasis can
  // reflect "you have worlds" vs "no worlds yet". The actual signal will
  // come from a campaigns query when that endpoint lands; until then,
  // both the hub and settings render with hasCampaigns=false.
  return (
    <Shell me={user} hasCampaigns={false}>
      <Outlet />
    </Shell>
  );
}
