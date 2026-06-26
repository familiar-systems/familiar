import { RouterProvider } from "@tanstack/react-router";
import { I18nProvider } from "react-aria-components";
import { useAuth } from "./lib/auth";
import { m } from "./paraglide/messages.js";
import { getLocale } from "./paraglide/runtime.js";
import { router } from "./router";

// Auth bootstraps once at app mount. While the fetch is in flight,
// render a minimal loading shell instead of mounting the router with a
// placeholder context - that would briefly resolve _authed beforeLoad
// against {kind:'unauthed'} and bounce the user to /login before /me
// could refute it. Once auth resolves, RouterProvider mounts with the
// live AuthState in context.
//
// I18nProvider feeds React Aria components the resolved locale (and the
// matching text direction). getLocale() reads Paraglide's strategy chain
// (localStorage -> browser preference -> baseLocale); it is stable per page
// load because switching locale reloads (no in-session switcher yet).
export function App(): React.ReactElement {
  const { state, error } = useAuth();

  if (error) return <pre className="p-8">{m.appError({ message: error })}</pre>;
  if (state === null) return <div className="p-8 text-muted-foreground">{m.appLoading()}</div>;

  return (
    <I18nProvider locale={getLocale()}>
      <RouterProvider router={router} context={{ auth: state }} />
    </I18nProvider>
  );
}
