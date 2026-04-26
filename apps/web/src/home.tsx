import type { MeResponse } from "@familiar-systems/types-app";
import { useEffect, useState } from "react";
import { client } from "./lib/api";
import { hanko } from "./lib/hanko";
import { spaRoute } from "./lib/paths";

export function Home() {
  const [me, setMe] = useState<MeResponse | null>(null);
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
        // Cast across the openapi-fetch boundary back to the ts-rs alias
        // form. openapi-fetch expands `string & { __brand }` into an
        // object-typed lookalike that has the right `__brand` property
        // but isn't assignable to a `string`-rooted intersection — the
        // primitive vs. object distinction blocks the unification.
        // Casting once here keeps every downstream consumer (`me.id`
        // passed to a function expecting `UserId`, etc.) free of casts.
        // The runtime value is the same on both sides; api.ts holds a
        // type-level guard asserting the brand property survives.
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
