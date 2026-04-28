// URL helpers for the SPA. Two flavors:
//   - same-origin paths on the app apex (apiPath, campaignPath, spaRoute)
//   - cross-origin links to the marketing apex (siteLink)
//
// Same-origin: the SPA lives on the app apex (app.familiar.systems in prod,
// app.preview.familiar.systems in preview, app.localhost:8080 in dev) and
// is served at the root of that apex. Preview environments stack a per-PR
// prefix on top:
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
// every same-origin call is CORS-preflight-free.
//
// Cross-origin: the marketing site lives on a separate apex
// (familiar.systems / preview.familiar.systems / localhost:8080). Its base
// URL is baked in at Vite build time as `VITE_SITE_URL`. In preview the
// value already includes the per-PR prefix because the marketing site is
// also path-prefixed in that environment.

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

// Static asset under apps/web/public, served from the SPA's base path.
// Use this for `<img src>`, mask-image url(), background-image url(), etc.
// A bare "/foo.svg" bypasses Vite's `base` config and 404s in preview where
// the SPA lives at /pr-${PR_NUMBER}/. Vite does NOT auto-rewrite absolute
// paths in JSX or inline styles.
export function assetPath(path: string): string {
  const cleaned = path.replace(/^\//, "");
  return base + cleaned;
}

// Marketing-site (cross-apex) link builder. The marketing site uses
// `[lang]/` path-based i18n; only `en` ships today, so it's the default.
const siteBase: string = import.meta.env.VITE_SITE_URL;

export function siteLink(path: string, lang: string = "en"): string {
  const cleaned = path.replace(/^\//, "");
  return `${siteBase}/${lang}/${cleaned}`;
}
