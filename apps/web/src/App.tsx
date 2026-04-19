import { Login } from "./login";
import { Home } from "./home";
import { spaRoute } from "./lib/paths";

// Under path-based deployment the SPA lives at a prefix (e.g. "/app/" in
// prod, "/pr-42/app/" in preview). Strip that prefix before matching routes.
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
