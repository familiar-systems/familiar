# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Loreweaver is an AI-assisted campaign notebook for tabletop RPG game masters. It captures session content (audio, notes) and uses AI to assemble a campaign knowledge base (NPCs, locations, items, relationships) as a graph that grows from play.

**Status: Pre-implementation.** The repository contains design documents only — no application code yet. All architectural decisions are documented in `docs/`.

## Key Design Documents

- `docs/vision.md` — Product vision, core concepts (Campaign, Session, Things, Blocks, Edges, Status, Suggestions)
- `docs/plans/2026-02-14-project-structure-spa-design.md` — **Authoritative** project structure and tech stack
- `docs/plans/2026-02-14-ai-workflow-unification-design.md` — AI workflow architecture (SessionIngest, P&R, Q&A)
- `docs/plans/2026-02-20-templates-as-prototype-pages.md` — Templates are Things, not a separate entity. Categorization via `prototypeId` and tag-relationships.
- `docs/plans/2026-02-20-public-site-design.md` — Public site (Astro): landing page, blog, public campaign pages. Path-based routing.
- `docs/plans/2026-03-12-deployment-strategy.md` — Deployment strategy (k3s on Hetzner, phased migration from Coolify, libSQL files on Volume)
- `docs/plans/2026-03-14-hocuspocus-architecture.md` — Document-centric campaign architecture (Hocuspocus, campaign checkout/checkin, object storage, AI agent as collab participant, scaling model)
- `docs/discovery/2026-03-09-sqlite-over-postgres-decision.md` — libSQL over PostgreSQL decision (database-per-campaign, Turso Database upgrade path)

### Not Worth Reading On Startup

- `docs/plans/archive/2026-02-14-spa-vs-ssr-design.md` — Why SPA over SSR (decided: SPA)
- `docs/plans/archive/2026-02-14-project-structure-design.md` — **Superseded** by the SPA design.
- `docs/discovery/archive/2026-02-18-postgres-vs-turso.md` — Original PostgreSQL decision (superseded by libSQL decision)
- `docs/discovery/archive/2026-02-14-storage-overview.md` — Initial storage architecture analysis
- `docs/plans/archive/2026-02-18-deployment-strategy.md` — Previous deployment strategy (superseded by 2026-03-09 version)
- `docs/plans/archive/2026-03-09-deployment-strategy.md` — Previous deployment strategy (superseded by k3s deployment strategy)
- `docs/discovery/archive/2026-02-18-solo-dev-deployment-landscape.md` — Deployment exploration (decided: Hetzner)
- `docs/discovery/archive/2026-02-18-eu-deployment-landscape.md` — EU deployment exploration (decided: Hetzner)

Read the SPA project structure doc before making architectural decisions — it is the source of truth.

## Architecture

### Monorepo: pnpm workspaces + Turborepo

```
apps/site     — Astro static site (landing page, blog, public campaign pages)
apps/web      — Vite + React SPA (the app, behind auth, served under /app/)
apps/api      — Hono + tRPC server (CRUD, interactive AI streaming, job submission)
apps/collab   — Hono + Hocuspocus (HTTP API + WebSocket collaboration, co-located per campaign-pinning architecture)
apps/worker   — Job consumer (polling libSQL job table) (batch AI: transcription, entity extraction, journal drafting)

packages/domain  — Pure types, zero dependencies. Everything depends on this.
packages/db      — Drizzle ORM schema, migrations, query helpers (libSQL, database-per-campaign)
packages/auth    — Token verification, permissions, session management
packages/editor  — TipTap/ProseMirror schema + custom extensions (THE shared contract)
packages/ai      — LLM client, prompt templates, pipelines, agent tool definitions
packages/queue   — libSQL-backed job table, polling producer/consumer
```

### Critical Dependency Rules

- **Dependency direction flows toward `domain`.** No package imports from an app. No app imports from another app.
- **`apps/site` depends only on `domain`.** The public site has the lightest dependency footprint of any app.
- **`apps/web` depends only on `domain` and `editor`.** It structurally cannot import `db`, `auth`, `ai`, or `queue`. The client/server boundary is enforced by the dependency graph.
- **Each package's `src/index.ts` is its public API.** Import from `@loreweaver/db`, never from `@loreweaver/db/src/schema/nodes`.
- **Domain logic belongs in packages, not apps.** Apps are thin wiring that connect packages to deployment targets.

### Five Deployment Targets

Each app has a different lifecycle — deploying one must not affect the others:

1. **site** — Static HTML (CDN/nginx). Public-facing. Content changes deploy independently of the app.
2. **web** — Static files (CDN/nginx). The authenticated SPA, served under `/app/`.
3. **api** — Stateless HTTP. Fast restarts, blue/green deploys.
4. **collab** — Hono + Hocuspocus co-located in one process. Campaign-pinning routes all traffic for a campaign to the same server. See [Hocuspocus Architecture ADR](docs/plans/2026-03-14-hocuspocus-architecture.md).
5. **worker** — Long-running jobs (10+ minutes). Must survive deploys of everything else.

### AI Architecture

Two execution paths, same output primitives:

- **Interactive** (apps/api): P&R and Q&A via the agent window. Streaming, latency-sensitive.
- **Batch** (apps/worker): SessionIngest pipeline. Long-running, survives deploys.

Both produce **Suggestions** — proposed mutations to the campaign graph. AI never modifies the graph directly; every change requires GM approval. Suggestions are always durable (persisted immediately). Both use the shared `CampaignContext` interface for status-filtered graph retrieval.

The AI agent writes to documents as a Hocuspocus participant (WebSocket for active pages, HTTP/DirectConnection for inactive pages). Document-level proposals use tagged CRDT blocks; graph-level proposals use the suggestion queue.

Tool availability determines AI behavior (no mode toggles): GMs get read+write tools, players get read-only tools.

## Tech Stack

| Concern         | Choice                                                      |
| --------------- | ----------------------------------------------------------- |
| Language        | TypeScript (full stack)                                     |
| Public site     | Astro (static site generator, React islands)                |
| Frontend        | React (Vite SPA)                                            |
| Editor          | TipTap (on ProseMirror)                                     |
| Routing         | TanStack Router or React Router (not yet decided)           |
| API             | Hono + tRPC                                                 |
| Database        | libSQL (database-per-campaign), Turso Database upgrade path |
| ORM             | Drizzle                                                     |
| Collaboration   | Hocuspocus (Yjs CRDT server)                                |
| Object Storage  | Hetzner Object Storage (campaign DB source of truth)        |
| Job queue       | libSQL-backed polling table                                 |
| Validation      | Zod (at all system boundaries)                              |
| Testing         | Vitest                                                      |
| Dev runner      | tsx (server-side), Vite dev server (frontend)               |
| Linting         | oxlint (strictest config)                                   |
| Formatting      | oxfmt (alpha, Prettier fallback)                            |
| Package manager | pnpm (strict dependency resolution)                         |
| Monorepo        | Turborepo                                                   |

## Commands (planned)

```bash
# Monorepo operations
pnpm install                    # Install all dependencies
turbo build                     # Build all packages/apps (cached)
turbo dev                       # Start all dev servers (site:4321, web:5173, api:3001, collab:3002)
turbo test                      # Run all tests
turbo lint                      # Lint all packages
turbo typecheck                 # tsc --noEmit across all packages

# Single package/app
turbo test --filter=@loreweaver/domain
turbo dev --filter=apps/web
pnpm --filter @loreweaver/db test
```

## TypeScript Strictness

Maximum strictness, no exceptions:

- `strict: true`
- `noUncheckedIndexedAccess` — array indexing returns `T | undefined`
- `exactOptionalPropertyTypes` — distinguishes `undefined` from missing
- `noUnusedLocals` + `noUnusedParameters`
- Lint ban on `any`
- Zod validation at every system boundary (API inputs, DB rows, env vars)

## Core Domain Concepts

- **Status** (on nodes, blocks, relationships): `gm_only` → `known` → `retconned`. Default is `gm_only`. Status cascades down (GM-only node = all children GM-only), not up.
- **Suggestions**: Discriminated union over types (`create_thing`, `update_blocks`, `create_relationship`, `journal_draft`, `contradiction`). Always durable. Auto-reject after ~7 days.
- **AgentConversation**: Persisted record of AI interactions. Provenance for suggestions. Roles: `gm`, `player`, `system`.
- **Mentions** (block→node or block→block): Derived automatically, power backlinks and transclusion.
- **Relationships** (node→node): Authored/curated, carry semantic labels. Freeform vocabulary.
- **Prototypes (templates)**: A template is a Thing with `isTemplate: true`. No separate `Template` entity. Creating a thing from a template clones the prototype's block structure. `prototypeId?: ThingId` tracks lineage. Tags are Things connected via `tagged` relationships — no `tags: string[]` field.

## Development Notes

- Path-based routing: `apps/site` owns `/` (landing, blog), `apps/web` is served under `/app/`
- In dev, Vite proxies `/app/api/*` → localhost:3001 and `/app/collab/*` → ws://localhost:3002 (no CORS needed). Astro dev server runs independently on port 4321.
- In production, Traefik (via k3s Ingress) routes all traffic through a single domain: `/app/api/*` → api, `/app/collab/*` → collab, `/app/*` → web SPA, `/*` → site
- The `@loreweaver/editor` package is the most architecturally important — it defines the TipTap schema shared between browser (apps/web) and server (apps/worker for document manipulation)
- LLM provider is pluggable: hosted instance uses managed keys, self-hosters bring their own
- No Docker database container needed for local development. libSQL files on disk. `:memory:` databases for tests.
