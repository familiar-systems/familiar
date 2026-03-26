# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Loreweaver is an AI-assisted campaign notebook for tabletop RPG game masters. It captures session content (audio, notes) and uses AI to assemble a campaign knowledge base (NPCs, locations, items, relationships) as a graph that grows from play.

**Status: Pre-implementation.** The repository contains design documents only — no application code yet. All architectural decisions are documented in `docs/`.

## Key Design Documents

- `docs/vision.md` — Product vision, core concepts (Campaign, Session, Things, Blocks, Edges, Status, Suggestions)
- `docs/plans/2026-03-26-project-structure-design.md` — **Authoritative** project structure (Rust server + TypeScript frontend + Python ML workers)
- `docs/plans/2026-02-14-ai-workflow-unification-design.md` — AI workflow architecture (SessionIngest, P&R, Q&A)
- `docs/plans/2026-02-20-templates-as-prototype-pages.md` — Templates are Things, not a separate entity. Categorization via `prototypeId` and tag-relationships.
- `docs/plans/2026-02-20-public-site-design.md` — Public site (Astro): landing page, blog, public campaign pages. Path-based routing.
- `docs/plans/2026-03-12-deployment-strategy.md` — Deployment strategy (k3s on Hetzner, libSQL files on Volume)
- `docs/plans/2026-03-25-campaign-collaboration-architecture.md` — **Authoritative** collaboration architecture (Rust/kameo/Loro, supersedes Hocuspocus ADR). Campaign checkout/checkin, actor topology, scaling model.
- `docs/plans/2026-03-25-campaign-actor-domain-design.md` — Actor topology, trait system, WebSocket architecture, suggestion model
- `docs/plans/2026-03-25-ai-serialization-format-v2.md` — Agent serialization format, progressive disclosure tiers, compiler pipeline, tool signatures
- `docs/discovery/2026-03-09-sqlite-over-postgres-decision.md` — libSQL over PostgreSQL decision (database-per-campaign, Turso Database upgrade path)

### Not Worth Reading On Startup

- `docs/archive/plans/2026-02-14-spa-vs-ssr-design.md` — Why SPA over SSR (decided: SPA)
- `docs/archive/plans/2026-02-14-project-structure-design.md` — **Superseded** by the SPA design.
- `docs/archive/discovery/2026-02-18-postgres-vs-turso.md` — Original PostgreSQL decision (superseded by libSQL decision)
- `docs/archive/discovery/2026-02-14-storage-overview.md` — Initial storage architecture analysis
- `docs/archive/plans/2026-02-18-deployment-strategy.md` — Previous deployment strategy (superseded by 2026-03-09 version)
- `docs/archive/plans/2026-03-09-deployment-strategy.md` — Previous deployment strategy (superseded by k3s deployment strategy)
- `docs/archive/discovery/2026-02-18-solo-dev-deployment-landscape.md` — Deployment exploration (decided: Hetzner)
- `docs/archive/discovery/2026-02-18-eu-deployment-landscape.md` — EU deployment exploration (decided: Hetzner)
- `docs/archive/plans/2026-03-14-hocuspocus-architecture.md` — **Superseded** by Campaign Collaboration Architecture. Hocuspocus/Yjs-era design; hypotheses validated, implementation technology replaced.

Read the project structure doc (`docs/plans/2026-03-26-project-structure-design.md`) before making architectural decisions — it is the source of truth.

## Architecture

### Monorepo: pnpm workspaces + Cargo + uv (orchestrated by mise)

```
apps/site      — Astro static site (landing page, blog, public campaign pages)
apps/web       — Vite + React SPA (the app, behind auth, served under /app/)
server/        — Rust binary: Axum + kameo (ALL backend: HTTP API, WebSocket collab, actors, AI, jobs)
workers/       — Python ML workers (audio transcription, diarization)

packages/types   — @loreweaver/types — generated from Rust via ts-rs, zero runtime deps
packages/editor  — @loreweaver/editor — TipTap/ProseMirror schema + custom extensions (THE shared contract)
```

### Critical Dependency Rules

- **Dependency direction: web -> editor -> types.** The frontend depends on two packages. The editor depends on one. Nothing else.
- **`apps/site` depends only on `types`.** The public site has the lightest dependency footprint.
- **`apps/web` depends only on `types` and `editor`.** The client/server boundary is enforced by the dependency graph -- there is no server-side TypeScript to import.
- **Each package's `src/index.ts` is its public API.** Import from `@loreweaver/types`, never from `@loreweaver/types/generated/ThingId`.
- **Domain logic is Rust.** The Rust server owns all backend logic: database access, auth, AI orchestration, job dispatch. TypeScript is frontend-only.

### Four Deployment Targets

Each target has a different lifecycle -- deploying one must not affect the others:

1. **site** — Static HTML (CDN/nginx). Public-facing. Content changes deploy independently of the app.
2. **web** — Static files (CDN/nginx). The authenticated SPA, served under `/app/`.
3. **server** — Rust binary (Axum + kameo actors). The single backend: HTTP API, WebSocket collaboration, actor lifecycle, AI conversations, job dispatch. Campaign-pinned. See [Campaign Collaboration Architecture](docs/plans/2026-03-25-campaign-collaboration-architecture.md).
4. **workers** — Python ML workers (faster-whisper, pyannote). Stateless, GPU-bound, called by the server via HTTP.

### AI Architecture

Two execution paths, same output primitives:

- **Interactive** (server -- AgentConversation actors): P&R and Q&A via the agent window. Streaming, latency-sensitive.
- **Batch** (server -- actors + Python workers): SessionIngest pipeline. Audio processing dispatched to Python ML workers, campaign-scoped work (entity extraction, journal drafting) runs through actors.

Both produce **Suggestions** -- proposed mutations to the campaign graph. AI never modifies the graph directly; every change requires GM approval. Suggestions are always durable (persisted immediately).

The AI agent writes via tool calls (`suggest_replace`, `create_page`, `propose_relationship`). The serialization compiler translates tool calls into compiled suggestions routed to ThingActors. Document-level proposals use suggestion marks on block UUID ranges; graph-level proposals use the suggestion queue. See [AI Serialization Format v2](docs/plans/2026-03-25-ai-serialization-format-v2.md) and [Campaign Actor Domain Design](docs/plans/2026-03-25-campaign-actor-domain-design.md).

Tool availability determines AI behavior (no mode toggles): GMs get read+write tools, players get read-only tools.

## Tech Stack

| Concern         | Choice                                                      |
| --------------- | ----------------------------------------------------------- |
| Language        | TypeScript (frontend) + Rust (server) + Python (ML workers) |
| Public site     | Astro (static site generator, React islands)                |
| Frontend        | React (Vite SPA)                                            |
| Editor          | TipTap (on ProseMirror)                                     |
| Routing         | TanStack Router or React Router (not yet decided)           |
| Server          | Rust: Axum + kameo actors                                   |
| API contract    | ts-rs (type generation) + utoipa (OpenAPI)                  |
| Database        | libSQL (database-per-campaign), Turso Database upgrade path |
| Collaboration   | Loro CRDTs + loro-dev/protocol                              |
| Object Storage  | Hetzner Object Storage (campaign DB source of truth)        |
| ML workers      | Python: faster-whisper, pyannote (GPU, called via HTTP)     |
| Validation      | Zod (at TypeScript system boundaries)                       |
| Testing         | Vitest (TS), cargo test (Rust), pytest (Python)             |
| Dev runner      | Vite dev server (frontend), cargo run (server)              |
| Linting         | oxlint (TS, strictest config)                               |
| Formatting      | oxfmt (alpha, Prettier fallback)                            |
| TS packages     | pnpm (strict dependency resolution)                         |
| Orchestration   | mise (cross-language task runner + tool versions)            |

## Commands (planned)

```bash
# Cross-language orchestration (mise)
mise run dev                    # Start all dev servers (site:4321, web:5173, server:3000)
mise run build                  # Build all targets in dependency order
mise run generate-types         # Run ts-rs + OpenAPI type generation pipeline
mise run test                   # Run all tests (Vitest + cargo test + pytest)

# TypeScript (pnpm)
pnpm install                    # Install all TS dependencies
pnpm --filter @loreweaver/editor test
pnpm --filter apps/web dev

# Rust (Cargo)
cargo build                     # Build the server
cargo test                      # Run server tests + emit ts-rs types
cargo run                       # Start the server (localhost:3000)

# Python (uv)
uv run pytest                   # Run ML worker tests
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
- In dev, Vite proxies `/app/api/*` and `/app/ws/*` to the Rust server at localhost:3000 (no CORS needed). Astro dev server runs independently on port 4321.
- In production, Traefik (via k3s Ingress) routes all traffic through a single domain: `/app/api/*` -> server, `/app/ws/*` -> server (WebSocket upgrade), `/app/*` -> web SPA, `/*` -> site
- The `@loreweaver/editor` package is the most architecturally important -- it defines the TipTap schema shared between browser (apps/web via loro-prosemirror) and server (Rust server for LoroDoc reconstruction and serialization compiler)
- LLM provider is pluggable: hosted instance uses managed keys, self-hosters bring their own
- No Docker database container needed for local development. libSQL files on disk. `:memory:` databases for tests.
