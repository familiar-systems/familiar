# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Loreweaver is an AI-assisted campaign notebook for tabletop RPG game masters. It captures session content (audio, notes) and uses AI to assemble a campaign knowledge base (NPCs, locations, items, relationships) as a graph that grows from play.

**Status: Pre-implementation.** The repository contains design documents only — no application code yet. All architectural decisions are documented in `docs/`.

## Key Design Documents

- `docs/vision.md` — Product vision, core concepts (Campaign, Session, Things, Blocks, Edges, Status, Suggestions)
- `docs/plans/2026-02-14-project-structure-spa-design.md` — **Authoritative** project structure and tech stack
- `docs/plans/2026-02-14-ai-workflow-unification-design.md` — AI workflow architecture (SessionIngest, P&R, Q&A)
- `docs/plans/2026-02-18-deployment-strategy.md` — Deployment strategy (local dev + one VPS, provider deferred)
- `docs/plans/archive/2026-02-14-spa-vs-ssr-design.md` — Why SPA over SSR (decided: SPA)
- `docs/plans/archive/2026-02-14-project-structure-design.md` — **Superseded** by the SPA design
- `docs/discovery/archive/2026-02-18-postgres-vs-turso.md` — Why PostgreSQL over Turso/libSQL (decided: PostgreSQL)
- `docs/discovery/archive/2026-02-14-storage-overview.md` — Initial storage architecture analysis

Read the SPA project structure doc before making architectural decisions — it is the source of truth.

## Architecture

### Monorepo: pnpm workspaces + Turborepo

```
apps/web      — Vite + React SPA (static files, no server)
apps/api      — Hono + tRPC server (CRUD, interactive AI streaming, job submission)
apps/collab   — Hocuspocus WebSocket server (real-time collaborative editing via Yjs)
apps/worker   — pg-boss job consumer (batch AI: transcription, entity extraction, journal drafting)

packages/domain  — Pure types, zero dependencies. Everything depends on this.
packages/db      — Drizzle ORM schema, migrations, query helpers (PostgreSQL)
packages/auth    — Token verification, permissions, session management
packages/editor  — TipTap/ProseMirror schema + custom extensions (THE shared contract)
packages/ai      — LLM client, prompt templates, pipelines, agent tool definitions
packages/queue   — pg-boss job type definitions, producer/consumer
```

### Critical Dependency Rules

- **Dependency direction flows toward `domain`.** No package imports from an app. No app imports from another app.
- **`apps/web` depends only on `domain` and `editor`.** It structurally cannot import `db`, `auth`, `ai`, or `queue`. The client/server boundary is enforced by the dependency graph.
- **Each package's `src/index.ts` is its public API.** Import from `@loreweaver/db`, never from `@loreweaver/db/src/schema/nodes`.
- **Domain logic belongs in packages, not apps.** Apps are thin wiring that connect packages to deployment targets.

### Four Deployment Targets

Each app has a different lifecycle — deploying one must not affect the others:
1. **web** — Static files (CDN/nginx). No server process.
2. **api** — Stateless HTTP. Fast restarts, blue/green deploys.
3. **collab** — Long-lived WebSocket connections. Must not restart on web/api deploys.
4. **worker** — Long-running jobs (10+ minutes). Must survive deploys of everything else.

### AI Architecture

Two execution paths, same output primitives:
- **Interactive** (apps/api): P&R and Q&A via the agent window. Streaming, latency-sensitive.
- **Batch** (apps/worker): SessionIngest pipeline. Long-running, survives deploys.

Both produce **Suggestions** — proposed mutations to the campaign graph. AI never modifies the graph directly; every change requires GM approval. Suggestions are always durable (persisted immediately). Both use the shared `CampaignContext` interface for status-filtered graph retrieval.

Tool availability determines AI behavior (no mode toggles): GMs get read+write tools, players get read-only tools.

## Tech Stack

| Concern | Choice |
|---|---|
| Language | TypeScript (full stack) |
| Frontend | React (Vite SPA) |
| Editor | TipTap (on ProseMirror) |
| Routing | TanStack Router or React Router (not yet decided) |
| API | Hono + tRPC |
| Database | PostgreSQL |
| ORM | Drizzle |
| Collaboration | Hocuspocus (Yjs CRDT server) |
| Job queue | pg-boss (PostgreSQL-backed) |
| Validation | Zod (at all system boundaries) |
| Testing | Vitest |
| Dev runner | tsx (server-side), Vite dev server (frontend) |
| Linting | oxlint (strictest config) |
| Formatting | oxfmt (alpha, Prettier fallback) |
| Package manager | pnpm (strict dependency resolution) |
| Monorepo | Turborepo |

## Commands (planned)

```bash
# Monorepo operations
pnpm install                    # Install all dependencies
turbo build                     # Build all packages/apps (cached)
turbo dev                       # Start all dev servers (web:5173, api:3001, collab:3002)
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

## Development Notes

- In dev, Vite proxies `/api/*` → localhost:3001 and `/collab/*` → ws://localhost:3002 (no CORS needed)
- In production, a reverse proxy (nginx/Caddy) routes all traffic through a single domain
- The `@loreweaver/editor` package is the most architecturally important — it defines the TipTap schema shared between browser (apps/web) and server (apps/worker for document manipulation)
- LLM provider is pluggable: hosted instance uses managed keys, self-hosters bring their own
