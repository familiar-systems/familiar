import { Login } from "./login";
import { Home } from "./home";
import { spaRoute } from "./lib/paths";

// The SPA lives at the root of the app apex in dev/prod (base "/") and
// under a per-PR prefix in preview (base "/pr-42/"). Strip the base prefix
// before matching routes so the matcher sees e.g. "login" regardless of env.
function currentRoute(): string {
  const base = spaRoute("");
  const path = window.location.pathname;
  return path.startsWith(base) ? path.slice(base.length) : path.replace(/^\//, "");
}

export function App(): React.ReactElement {
  if (currentRoute() === "login") {
    return <Login />;
  }
  return <Home />;
}
