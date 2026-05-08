import { useEffect, useState } from "react";
import type { MeResponse } from "@familiar-systems/types-app";
import { client } from "./api";
import { hanko } from "./hanko";

// Bootstraps the SPA's auth state: validates the Hanko session, then
// fetches /me. Returns me=null for both "session invalid" and "401 from
// platform" outcomes; the caller distinguishes "still loading" from
// "definitely unauthed" via the loading flag. The router's requireAuth
// beforeLoad redirects to /login when me is null, so this hook no longer
// touches window.location.
//
// validateSession before /me: a bare getSessionToken() only tells us "the
// SDK has a cached token"; it doesn't tell us the token is still accepted
// by Hanko. validateSession asks the Hanko backend, so a revoked or expired
// session resolves to null cleanly instead of producing a failed /me call.
export function useAuthedMe(): {
  me: MeResponse | null;
  error: string | null;
  loading: boolean;
} {
  const [me, setMe] = useState<MeResponse | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    const run = async (): Promise<void> => {
      try {
        const { is_valid } = await hanko.validateSession();
        if (!is_valid) {
          setMe(null);
          return;
        }
        const { data, response } = await client.GET("/me");
        if (response.status === 401) {
          setMe(null);
          return;
        }
        if (!response.ok || !data) throw new Error(`HTTP ${response.status}`);
        // Cast across the openapi-fetch boundary back to the ts-rs alias
        // form. openapi-fetch expands `string & { __brand }` into an
        // object-typed lookalike that has the right `__brand` property
        // but isn't assignable to a `string`-rooted intersection. The
        // primitive vs. object distinction blocks unification. Casting
        // once here keeps every downstream consumer (`me.id` passed to a
        // function expecting `UserId`, etc.) free of casts. The runtime
        // value is identical on both sides; api.ts holds a type-level
        // guard asserting the brand property survives.
        setMe(data as MeResponse);
      } catch (e: unknown) {
        setError(String(e));
      } finally {
        setLoading(false);
      }
    };
    void run();
  }, []);

  return { me, error, loading };
}
