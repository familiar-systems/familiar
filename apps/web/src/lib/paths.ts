// Same-origin path helpers for the SPA.
//
// The SPA is served under a path prefix that varies by environment:
//   dev/prod: "/app/"
//   preview:  "/pr-42/app/"
// Vite exposes this prefix as `import.meta.env.BASE_URL` at runtime.
//
// Sibling services live under sibling prefixes on the same origin:
//   SPA       BASE_URL                    -> "/app/"        or "/pr-42/app/"
//   Platform  apiPath("me")               -> "/api/me"      or "/pr-42/api/me"
//   Campaign  campaignPath("6769/ws")     -> "/campaign/6769/ws"
//                                          or "/pr-42/campaign/6769/ws"
//
// These helpers derive sibling paths by replacing the trailing "app/"
// segment with "api/" or "campaign/". The approach works in dev with
// base "/app/" (assumes dev mirrors prod); if a future deployment serves
// the SPA at "/" directly, BASE_URL would be "/" and the replace is a
// no-op, producing "/api..." which still resolves via the Vite proxy.

const base = import.meta.env.BASE_URL;

function siblingBase(sibling: string): string {
  return base.endsWith("app/") ? base.slice(0, -"app/".length) + sibling : `/${sibling}`;
}

export const apiBase: string = siblingBase("api/");
export const campaignBase: string = siblingBase("campaign/");

export function apiPath(path: string): string {
  return apiBase + path.replace(/^\//, "");
}

export function campaignPath(path: string): string {
  return campaignBase + path.replace(/^\//, "");
}

// Relative SPA route. Use this instead of a bare "/" or "/login" when
// navigating the browser so that the path prefix is preserved.
export function spaRoute(path: string): string {
  const cleaned = path.replace(/^\//, "");
  return cleaned === "" ? base : base + cleaned;
}
