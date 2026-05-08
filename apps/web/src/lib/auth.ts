import { useEffect, useState } from "react";
import type { MeResponse } from "@familiar-systems/types-app";
import { client } from "./api";
import { hanko } from "./hanko";

// Resolved auth state, modeled as a sum type so consumers narrow the
// `user` field by checking `kind` instead of nullability. This is the
// shape that flows into the router's context: when the SPA boots, App
// fetches once via useAuth(), and the resolved AuthState is passed to
// <RouterProvider context={{ auth }} />. /_authed.tsx then narrows
// AuthState to its 'authed' variant before any protected child renders,
// so child routes get a non-nullable `user: MeResponse` directly.
export type AuthState = { kind: "unauthed" } | { kind: "authed"; user: MeResponse };

// React-side lifecycle for the /me bootstrap. While the fetch is in
// flight we return state=null; App renders a Loading shell instead of
// mounting the router. Errors return error!=null and never resolve to a
// state, so RouterProvider only mounts after we know whether the user is
// authed. This is the canonical TanStack pattern - the router itself is
// data-driven and synchronous; auth-loading lives outside it.
//
// validateSession before /me: a bare getSessionToken() only tells us
// "the SDK has a cached token"; it doesn't tell us the token is still
// accepted by Hanko. validateSession asks the Hanko backend, so a
// revoked or expired session resolves to {kind:'unauthed'} cleanly
// instead of producing a failed /me call.
export function useAuth(): { state: AuthState | null; error: string | null } {
  const [state, setState] = useState<AuthState | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    const run = async (): Promise<void> => {
      try {
        const { is_valid } = await hanko.validateSession();
        if (!is_valid) {
          setState({ kind: "unauthed" });
          return;
        }
        const { data, response } = await client.GET("/me");
        if (response.status === 401) {
          setState({ kind: "unauthed" });
          return;
        }
        if (!response.ok || !data) throw new Error(`HTTP ${response.status}`);
        // Cast across the openapi-fetch boundary back to the ts-rs alias
        // form. openapi-fetch expands `string & { __brand }` into an
        // object-typed lookalike that has the right `__brand` property
        // but isn't assignable to a `string`-rooted intersection. Casting
        // once here keeps every downstream consumer free of casts.
        setState({ kind: "authed", user: data as MeResponse });
      } catch (e: unknown) {
        setError(String(e));
      }
    };
    void run();
  }, []);

  return { state, error };
}
