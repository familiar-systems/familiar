import { RouterProvider } from "@tanstack/react-router";
import { useAuth } from "./lib/auth";
import { router } from "./router";

// Auth bootstraps once at app mount. While the fetch is in flight,
// render a minimal loading shell instead of mounting the router with a
// placeholder context - that would briefly resolve _authed beforeLoad
// against {kind:'unauthed'} and bounce the user to /login before /me
// could refute it. Once auth resolves, RouterProvider mounts with the
// live AuthState in context.
export function App(): React.ReactElement {
  const { state, error } = useAuth();

  if (error) return <pre className="p-8">Error: {error}</pre>;
  if (state === null) return <div className="p-8 text-muted-foreground">Loading...</div>;

  return <RouterProvider router={router} context={{ auth: state }} />;
}
