// Root route. Owns the SPA's bootstrap concerns: typed router context,
// the 404 fallback, and a router-level error boundary.
//
// Auth lives outside the router. App.tsx fetches /me once via useAuth(),
// then mounts <RouterProvider context={{ auth }} /> with the resolved
// AuthState. Doing it that way (the pattern in TanStack's authenticated-
// routes guide) means:
//   - the router itself stays synchronous on every navigation; no
//     beforeLoad re-runs, no /me roundtrip per click, no flash;
//   - context.auth is a sum type (kind:'unauthed' | kind:'authed'), so
//     _authed.tsx narrows it to the 'authed' variant once and child
//     routes consume `user: MeResponse` directly without re-checking
//     for null.
//
// _authed.tsx wraps the protected pages and applies Shell as the shared
// layout, so navigating between protected pages doesn't remount the
// header or background. /login lives outside _authed - no chrome, no
// auth gate.

import { Link, Outlet, createRootRouteWithContext } from "@tanstack/react-router";
import type { AuthState } from "../lib/auth";

export interface RouterContext {
  auth: AuthState;
}

function NotFound(): React.ReactElement {
  return (
    <div className="flex min-h-screen flex-col items-center justify-center gap-4 p-8 text-center">
      <p className="font-display text-2xl">404: Not Found.</p>
      <p className="font-display text-2xl">The labyrinth claims another one.</p>
      <Link to="/" className="text-muted-foreground text-sm underline hover:text-foreground">
        Return to hub
      </Link>
    </div>
  );
}

function ErrorBoundary({ error }: { error: Error }): React.ReactElement {
  return <pre className="p-8">Error: {String(error)}</pre>;
}

export const Route = createRootRouteWithContext<RouterContext>()({
  component: () => <Outlet />,
  notFoundComponent: NotFound,
  errorComponent: ErrorBoundary,
});
