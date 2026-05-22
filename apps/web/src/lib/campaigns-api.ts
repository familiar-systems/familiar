// Campaign-tier typed client. Same pattern as lib/api.ts (which wraps the
// platform server), but against the campaign server's OpenAPI spec.
//
// The campaign server's routes use full service-prefixed paths
// (/catalog/systems, /campaign/{id}). The proxy (Caddy in dev, Traefik in
// prod) forwards these paths intact, so the base URL is just BASE_URL
// (e.g. "/" or "/pr-42/"). The service prefix is baked into the OpenAPI
// path keys, not the base URL.

import createClient from "openapi-fetch";
import type { CampaignPaths } from "@familiar-systems/types-campaign";
import { hanko } from "./hanko";

const base = import.meta.env.BASE_URL;

export const campaignClient = createClient<CampaignPaths>({ baseUrl: base });

campaignClient.use({
  async onRequest({ request }) {
    const token = hanko.getSessionToken();
    if (token) {
      request.headers.set("Authorization", `Bearer ${token}`);
    }
    return request;
  },
});
