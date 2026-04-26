import type { MeResponse } from "@familiar-systems/types-app";
import { useEffect, useState } from "react";
import { client } from "./lib/api";
import { hanko } from "./lib/hanko";
import { spaRoute } from "./lib/paths";

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
        const { data, response } = await client.GET("/me");
        if (response.status === 401) {
          window.location.assign(spaRoute("login"));
          return;
        }
        if (!response.ok || !data) throw new Error(`HTTP ${response.status}`);
        // The cast is sound: openapi-fetch infers the response shape
        // through several mapped types, which expands ts-rs branded
        // aliases (`string & { __brand }`) into a structurally-equal but
        // not-quite-identical form. Both descriptions came from the same
        // Rust struct via `Serialize`, so they describe the same wire
        // value — TypeScript just can't see the equivalence through the
        // indirection.
        setMe(data as MeResponse);
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
