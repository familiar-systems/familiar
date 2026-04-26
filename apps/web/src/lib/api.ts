import createClient from "openapi-fetch";
import type { PlatformPaths } from "@familiar-systems/types-app";
import { apiBase } from "./paths";
import { hanko } from "./hanko";

// Typed fetch client for the platform server. Routes, methods, path/query
// parameters, request bodies, and responses are all checked against the
// OpenAPI spec generated from utoipa — calling a route that doesn't exist
// or sending a wrong-shape body fails to compile.
//
// Component shapes (e.g. MeResponse, UserId) come from ts-rs, so the
// branded ID types survive intact: a UserId returned from /me cannot be
// passed where a CampaignId is expected.
//
// The `bearerAuth` middleware attaches the current Hanko session token to
// every request. Routes that don't need auth (currently /health) ignore
// the header server-side.
export const client = createClient<PlatformPaths>({ baseUrl: apiBase });

client.use({
  async onRequest({ request }) {
    const token = hanko.getSessionToken();
    if (token) {
      request.headers.set("Authorization", `Bearer ${token}`);
    }
    return request;
  },
});
