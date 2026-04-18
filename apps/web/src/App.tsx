import { Login } from "./login";
import { Home } from "./home";

export function App(): React.ReactElement {
  if (window.location.pathname === "/login") {
    return <Login />;
  }
  return <Home />;
}
