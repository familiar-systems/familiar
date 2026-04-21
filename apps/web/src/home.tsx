import type { MeResponse } from "@familiar-systems/types-app";
import { useEffect, useState } from "react";
import { hanko } from "./lib/hanko";
import { apiPath, spaRoute } from "./lib/paths";
import { MeResponseSchema } from "./lib/schemas";

export function Home() {
  const [me, setMe] = useState<MeResponse | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    // Why validateSession before reading the cookie/storage: a bare
    // `getSessionToken()` only tells us "the SDK has a cached token"; it
    // doesn't tell us the token is still accepted by Hanko. validateSession
    // asks the Hanko backend, so a revoked or expired session produces a
    // clean redirect to login instead of a failed /me call.
    const run = async () => {
      try {
        const { is_valid } = await hanko.validateSession();
        if (!is_valid) {
          window.location.assign(spaRoute("login"));
          return;
        }
        const token = hanko.getSessionToken();
        if (!token) {
          window.location.assign(spaRoute("login"));
          return;
        }
        const r = await fetch(apiPath("me"), {
          headers: { Authorization: `Bearer ${token}` },
        });
        if (r.status === 401) {
          window.location.assign(spaRoute("login"));
          return;
        }
        if (!r.ok) throw new Error(`HTTP ${r.status}`);
        const parsed = MeResponseSchema.parse(await r.json());
        setMe(parsed as MeResponse);
      } catch (e: unknown) {
        setError(String(e));
      }
    };
    void run();
  }, []);

  if (error) return <pre>Error: {error}</pre>;
  if (!me) return <div>Loading...</div>;
  return <pre>{JSON.stringify(me, null, 2)}</pre>;
}
