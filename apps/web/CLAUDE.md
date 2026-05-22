# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Scope

Covers `apps/web/` only: the Vite + React SPA. Overrides the repo-root CLAUDE.md for anything under this directory.

## API calls: use the generated typed clients, always

Every API call MUST go through the typed `openapi-fetch` clients in `src/lib/`:

- `src/lib/api.ts` exports `client` typed against `PlatformPaths` (platform server)
- `src/lib/campaigns-api.ts` exports `campaignClient` typed against `CampaignPaths` (campaign server)

These types are generated from the Rust servers' utoipa OpenAPI specs via `mise run generate-types`. The TypeScript compiler checks every path, method, parameter, and body against the spec. **A route that doesn't exist or a wrong-shape body fails to compile.**

**Never write raw `fetch()` calls or string-literal URLs to the platform or campaign servers.** The typed clients exist precisely so that backend route changes are caught by `mise run typecheck:ts` instead of surfacing as runtime 404s. If you need a new route, add it on the Rust side first, run `mise run generate-types`, then call it through the typed client.

The type pipeline: Rust struct (`#[derive(ToSchema)]`) -> utoipa OpenAPI JSON -> `tooling/openapi-codegen` -> `packages/types-campaign/src/openapi/campaign.ts` (or `types-app` for platform). Component schemas resolve to ts-rs branded types so IDs stay distinct (`CampaignId` cannot be passed where `UserId` is expected).

## Style guide

Read `docs/style-guide.md` before writing UI code. Use the `frontend-design` skill for new components.
