// Same-origin path helpers for the SPA.
//
// The SPA lives on the app apex (app.familiar.systems in prod,
// app.preview.familiar.systems in preview, app.localhost:8080 in dev) and
// is served at the root of that apex. Preview environments stack a
// per-PR prefix on top:
//   dev/prod: "/"
//   preview:  "/pr-42/"
// Vite exposes this prefix as `import.meta.env.BASE_URL` at runtime.
//
// Sibling services live under sibling prefixes on the same origin:
//   SPA       BASE_URL                    -> "/"             or "/pr-42/"
//   Platform  apiPath("me")               -> "/api/me"       or "/pr-42/api/me"
//   Campaign  campaignPath("6769/ws")     -> "/campaign/6769/ws"
//                                          or "/pr-42/campaign/6769/ws"
//
// Because the SPA, platform, and campaign shards all share the app apex,
// every call derived here is same-origin and bypasses CORS preflight.

const base = import.meta.env.BASE_URL;

export const apiBase: string = `${base}api/`;
export const campaignBase: string = `${base}campaign/`;

export function apiPath(path: string): string {
  return apiBase + path.replace(/^\//, "");
}

export function campaignPath(path: string): string {
  return campaignBase + path.replace(/^\//, "");
}

// Relative SPA route. Use this instead of a bare "/" or "/login" when
// navigating the browser so that the per-PR prefix is preserved in preview.
export function spaRoute(path: string): string {
  const cleaned = path.replace(/^\//, "");
  return cleaned === "" ? base : base + cleaned;
}
