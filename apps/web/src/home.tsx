import type { MethodResponse } from "openapi-fetch";
import { useEffect, useState } from "react";
import { client } from "./lib/api";
import { hanko } from "./lib/hanko";
import { spaRoute } from "./lib/paths";

// Use openapi-fetch's `MethodResponse` rather than importing `MeResponse`
// from `@familiar-systems/types-app` directly. They describe the same
// wire value, but openapi-fetch's mapped-type machinery expands ts-rs
// branded aliases (`string & { __brand }`) into a structurally-equal
// but not-unifiable shape on the response side. Anchoring the state
// type through the same machinery sidesteps that mismatch entirely
// (no cast, no `as unknown`). The `__brand` intersection survives
// either way, so a `Me["id"]` still won't pass for a `CampaignId`.
type Me = MethodResponse<typeof client, "get", "/me">;

export function Home() {
  const [me, setMe] = useState<Me | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
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
        setMe(data);
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
