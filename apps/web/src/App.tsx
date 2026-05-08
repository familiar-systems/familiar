import { RouterProvider } from "@tanstack/react-router";
import { useAuthedMe } from "./lib/auth";
import { router } from "./router";

// Auth bootstraps once at app mount. Until that resolves we render a
// minimal loading shell rather than handing a null-me context to the
// router; otherwise requireAuth would redirect to /login on first match
// and the URL would change before /me could refute it. Once auth resolves,
// RouterProvider mounts with me in context (null = unauthed, in which case
// the router's requireAuth handles the redirect).
export function App(): React.ReactElement {
  const { me, error, loading } = useAuthedMe();

  if (loading) {
    return <div className="p-8 text-muted-foreground">Loading...</div>;
  }
  if (error) {
    return <pre className="p-8">Error: {error}</pre>;
  }

  return <RouterProvider router={router} context={{ me }} />;
}
