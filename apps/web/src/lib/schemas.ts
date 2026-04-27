// Runtime validators for *external* system boundaries — anything we don't
// own the type contract for. The platform server's API is contract-checked
// at compile time via the OpenAPI-derived `PlatformPaths` and ts-rs types
// (see apps/web/src/lib/api.ts), so platform responses don't need Zod.
//
// Reserve this file for things like Hanko callbacks, third-party webhooks,
// SSO/OAuth state we read from the URL, or anything the SPA gets from a
// service we don't run. As of today, no such boundary exists; the file
// stays as a placeholder so the convention is obvious when the first one
// lands.
//
// If you find yourself adding a Zod schema for an `apps/platform` or
// `apps/campaign` response, you don't need it — pull the type from
// `@familiar-systems/types-app` (or `types-campaign`) instead and use the
// typed `client` from `./api`.

export {};
