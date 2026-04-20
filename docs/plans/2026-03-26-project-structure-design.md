# familiar.systems -- Project Structure Design

**Status:** Implemented
**Date:** 2026-03-26
**Supersedes:** [Project Structure Design (SPA)](../archive/plans/2026-02-14-project-structure-spa-design.md) -- same SPA decision, fundamentally different backend architecture (TypeScript full-stack to Rust server + TypeScript frontend + Python ML workers)
**Related decisions:** [Campaign Collaboration Architecture](./2026-03-25-campaign-collaboration-architecture.md), [Campaign Actor Domain Design](./2026-03-25-campaign-actor-domain-design.md), [AI Serialization Format v2](./2026-03-25-ai-serialization-format-v2.md), [Infrastructure](./2026-03-30-infrastructure.md), [Deployment Architecture](./2026-03-30-deployment-architecture.md), [Public site design](./2026-02-20-public-site-design.md), [AI workflow unification](./2026-02-14-ai-workflow-unification-design.md), [Templates as prototype pages](./2026-02-20-templates-as-prototype-pages.md), [libSQL decision](../discovery/2026-03-09-sqlite-over-postgres-decision.md)

---

## Context

familiar.systems is a web application with five workloads that have **different deployment lifecycles**:

1. **Public site** (Astro) -- static HTML for the landing page, blog, and public campaign showcase. No server process. Deploy = upload new files.
2. **Frontend** (Vite + React SPA) -- the authenticated application. Static files served from a CDN or file server.
3. **Platform** (Rust: Axum) -- authentication, campaign CRUD, routing table, discover endpoint. Talks to platform.db. Stateless HTTP, rarely changes.
4. **Campaign server** (Rust: Axum + kameo) -- actor hierarchy, WebSocket collaboration (Loro CRDTs via loro-dev/protocol), AI agent conversations, serialization compiler, job dispatch. Talks to per-campaign libSQL files. Campaign-pinned: all traffic for a given campaign routes to the same server. Changes frequently.
5. **Workers** -- job processors, language-agnostic. Today: Python ML workers (faster-whisper, pyannote) on GPU infrastructure. Deployed as k8s Jobs, dispatched by the campaign server. Job state tracked in platform.db.

### Why five targets, not one backend

The [superseded design](../archive/plans/2026-02-14-project-structure-spa-design.md) had five TypeScript processes: an API server (Hono + tRPC), a collaboration server (Hocuspocus), and a worker process alongside the static sites. The Rust rewrite collapsed the three backend TypeScript processes into one Rust binary, using actor isolation (kameo) instead of process isolation.

The platform/campaign split re-introduces a process boundary, but at a different seam. The old split was functional (API vs collaboration vs worker). The new split is by deployment lifecycle:

**The platform barely changes.** It's CRUD on a SQLite file: auth, campaign listing, routing table, discover. Once written, it goes weeks without a deploy. Restarting it is transparent to users.

**The campaign server changes constantly.** It's where all the complexity lives: the actor hierarchy, CRDT sync, the serialization compiler, AI conversations. It ships daily. Restarting it disconnects active editing sessions.

**Coupling them means the stable service restarts every time the volatile one ships.** Login breaks, campaign discovery breaks, the routing table drops. With the split, a campaign server deploy or crash produces "I can't open my campaign," not "the site is down."

**The network boundary prevents invisible coupling.** Without it, six months of development creates shortcuts: the campaign server reading platform.db directly, importing platform-internal types, sharing in-process state. The eventual split becomes an archaeological dig. With the boundary from day one, the Cargo workspace's crate boundaries enforce separation at compile time, and the HTTP interface is always exercised.

**Process isolation remains for the runtime concern.** Actor isolation handles concurrent workloads within the campaign server (WebSocket connections, AI inference, CRDT sync). Process isolation handles the deployment lifecycle concern (shipping independently, blast radius). These are orthogonal.

See [Deployment Architecture](./2026-03-30-deployment-architecture.md) for the full service topology, graceful restart protocol, and preview environment design.

### Why SPA over SSR

Unchanged from the superseded design. familiar.systems's content is entirely behind authentication (no SEO), and the centerpiece is a TipTap editor that is inherently client-rendered. SSR would produce HTML that React immediately takes over -- compute spent on an HTML shell the user never sees without JavaScript. The campaign checkout model makes SSR worse: the server would block the page render waiting for the libSQL file to download from object storage. The SPA loads instantly from CDN and handles the async checkout gracefully. See the [SPA vs SSR analysis](../archive/plans/2026-02-14-spa-vs-ssr-design.md) for the full evaluation.

### Decisions

| Decision               | Choice                                                      | Reference                                                                                  |
| ---------------------- | ----------------------------------------------------------- | ------------------------------------------------------------------------------------------ |
| Language               | TypeScript (frontend) + Rust (server) + Python (ML workers) | This document                                                                              |
| Editor                 | TipTap (MIT, on ProseMirror)                                | [tiptap.md](../discovery/stack/editor/tiptap.md)                                           |
| Frontend               | React (Vite SPA)                                            | [SPA vs SSR analysis](../archive/plans/2026-02-14-spa-vs-ssr-design.md)                    |
| Server                 | Rust: Axum + kameo actors                                   | [Campaign Collaboration Architecture](./2026-03-25-campaign-collaboration-architecture.md) |
| CRDTs                  | Loro + loro-dev/protocol                                    | [Campaign Actor Domain Design](./2026-03-25-campaign-actor-domain-design.md)               |
| ProseMirror binding    | loro-prosemirror                                            | [Campaign Actor Domain Design](./2026-03-25-campaign-actor-domain-design.md)               |
| Database               | libSQL (database-per-campaign), Turso Database upgrade path | [libSQL decision](../discovery/2026-03-09-sqlite-over-postgres-decision.md)                |
| API contract           | ts-rs (type generation) + utoipa (OpenAPI)                  | This document                                                                              |
| Public site            | Astro (static site generator)                               | [Public site design](./2026-02-20-public-site-design.md)                                   |
| Monorepo orchestration | mise                                                        | This document                                                                              |
| TS package manager     | pnpm (strict workspaces)                                    | This document                                                                              |
| Rust build             | Cargo                                                       | This document                                                                              |
| Python tooling         | uv                                                          | This document                                                                              |

---

## Repository Structure

```
familiar/
├── apps/
│   ├── site/              # Astro -- landing page, blog, public campaign pages
│   ├── web/               # Vite + React SPA (behind auth)
│   ├── platform/          # Rust binary: Axum (auth, CRUD, routing table, discover)
│   └── campaign/          # Rust binary: Axum + kameo (actors, collab, AI, compiler)
├── crates/
│   ├── app-shared/        # Rust library: IDs, auth, libSQL helpers (platform + campaign)
│   └── campaign-shared/   # Rust library: Loro wrappers, ToC/Thing schema, CrdtDoc trait, status (campaign only)
├── packages/
│   ├── types-app/         # @familiar-systems/types-app -- generated from app-shared via ts-rs
│   ├── types-campaign/    # @familiar-systems/types-campaign -- generated from campaign-shared via ts-rs
│   └── editor/            # @familiar-systems/editor -- TipTap schema + custom extensions
├── workers/               # Job processors (language-agnostic)
│   ├── pyproject.toml     # Python ML workers today (faster-whisper, pyannote)
│   └── src/
├── tooling/
│   ├── tsconfig/          # Shared TypeScript compiler configs
│   │   ├── base.json
│   │   ├── react.json
│   │   └── library.json
│   └── oxlint/            # Shared oxlint config
│       └── base.json
├── .cargo/
│   └── config.toml        # Cargo env vars (TS_RS_EXPORT_DIR, etc.)
├── docs/                  # Architecture decisions, design docs
├── mise.toml              # Tool versions + cross-language task orchestration
├── Cargo.toml             # Rust workspace root (members: apps/platform, apps/campaign, crates/*)
├── pnpm-workspace.yaml    # TypeScript workspaces: apps/site, apps/web, packages/*
└── .gitignore
```

**Why Rust binaries live in `apps/`, not a separate `server/` directory:** `apps/` means "things that deploy and run," regardless of language. The Astro site, the SPA, the platform, and the campaign server are all deployable artifacts with independent lifecycles. pnpm ignores directories without `package.json`; Cargo's workspace members are listed explicitly. There's no confusion about which build system owns what. This follows the pattern used by [Spacedrive](https://github.com/spacedriveapp/spacedrive) (Rust + TypeScript monorepo) where Rust service binaries live alongside TypeScript apps.

**Why `crates/` and `packages/` stay separate:** These are shared libraries, and the ecosystem-specific naming helps: opening `crates/app-shared/` tells you it's Cargo; opening `packages/editor/` tells you it's pnpm. Merging them into a generic `libs/` would save one directory at the cost of that instant signal.

**Why `workers/` is organized by function, not language:** Workers are defined by what they process (audio transcription, diarization), not what language they're written in. Today they're Python because the ML libraries are Python. A Rust worker binary would live in `apps/` (it's a Cargo artifact); a Python or Go worker would live in `workers/` (it has its own toolchain).

### Workspace tooling

- **mise** -- polyglot tool version manager and task runner. Pins Node.js, Rust toolchain, and Python versions in one file (`mise.toml`). Replaces `.nvmrc`. Orchestrates cross-language tasks: `mise run dev` starts all servers in parallel, `mise run build` builds all targets in dependency order, `mise run generate-types` runs the ts-rs + OpenAPI pipeline.
- **pnpm** -- TypeScript package manager with strict dependency resolution. Native workspace support via `pnpm-workspace.yaml`. Prevents phantom dependencies: a package cannot import a dependency it hasn't declared.
- **Cargo** -- Rust build system. The workspace `Cargo.toml` at the repo root lists members across `apps/` (binaries) and `crates/` (libraries). Both platform and campaign server binaries compile from the same workspace.
- **uv** -- Python project manager for the ML workers. Manages virtualenvs, dependencies, and scripts via `pyproject.toml`.
- **No Turborepo.** The TypeScript workspace has two packages and two apps. mise tasks + `pnpm --filter` handle targeted builds. Turborepo's caching value doesn't justify the tooling overhead for this scale.

---

## Packages

Three TypeScript packages survive. Everything that was in `@familiar-systems/domain`, `@familiar-systems/db`, `@familiar-systems/auth`, `@familiar-systems/ai`, and `@familiar-systems/queue` in the superseded design is now Rust code in `crates/app-shared/`, `crates/campaign-shared/`, `apps/platform/`, and `apps/campaign/`.

### Dependency graph

```mermaid
graph BT
    types-app["@familiar-systems/types-app<br/><i>generated from app-shared via ts-rs</i>"]

    types-campaign["@familiar-systems/types-campaign<br/><i>generated from campaign-shared via ts-rs</i>"] --> types-app

    editor["@familiar-systems/editor"] --> types-campaign

    site["apps/site<br/><i>Astro (static HTML)</i>"] --> types-app

    web["apps/web<br/><i>SPA (static files)</i>"] --> types-app
    web --> types-campaign
    web --> editor

    style types-app fill:#4a9,stroke:#333,color:#fff
    style types-campaign fill:#4a9,stroke:#333,color:#fff
    style editor fill:#69c,stroke:#333,color:#fff
    style site fill:#c66,stroke:#333,color:#fff
    style web fill:#c66,stroke:#333,color:#fff
```

Green = types (foundation). Blue = packages (shared logic). Red = apps (deployment targets). The Rust binaries, shared crates, and workers are outside the TypeScript dependency graph entirely.

### Generated type packages (Rust-first)

The Rust crates are the source of truth for domain types. TypeScript declarations are generated via [ts-rs](https://github.com/Aleph-Alpha/ts-rs), which derives a trait on Rust types that emits `.ts` files at test time. The types are split across two packages, mirroring the Rust crate split.

The litmus test for placement: **does the platform server need this type?** If yes, it goes in `app-shared` (and generates to `types-app`). If only the campaign server uses it, it goes in `campaign-shared` (and generates to `types-campaign`).

### `@familiar-systems/types-app` -- Platform-level types

Generated from `crates/app-shared/`. Contains types that cross the platform/campaign boundary: IDs shared by both services (CampaignId, UserId), auth primitives, and any future platform-level API types.

```
packages/types-app/
├── package.json           # @familiar-systems/types-app, zero runtime dependencies
├── tsconfig.json          # include: ["src"]
└── src/
    ├── index.ts           # Re-exports from generated/
    └── generated/         # ts-rs output -- machine-written, never hand-edited
        └── id/
            ├── CampaignId.ts
            └── UserId.ts
```

**Depends on:** nothing (generated, zero runtime dependencies)

### `@familiar-systems/types-campaign` -- Campaign-scoped types

Generated from `crates/campaign-shared/`. Contains campaign-scoped IDs (ThingId, BlockId, SessionId, JournalId, SuggestionId, ConversationId) and document schema types (TocEntry, TocEntryKind, ThingHandle). The platform server never uses these types.

```
packages/types-campaign/
├── package.json           # @familiar-systems/types-campaign, depends on types-app
├── tsconfig.json          # include: ["src"]
└── src/
    ├── index.ts           # Re-exports from generated/
    └── generated/         # ts-rs output -- machine-written, never hand-edited
        ├── id/
        │   ├── ThingId.ts
        │   ├── BlockId.ts
        │   ├── SessionId.ts
        │   └── ...
        └── document/
            ├── TocEntry.ts
            ├── TocEntryKind.ts
            └── ThingHandle.ts
```

Both packages follow the same structure: `src/index.ts` is the hand-curated public API that re-exports generated types and may add TypeScript-only utilities (type guards, narrowing helpers). The `src/generated/` directory is the output of `cargo test` and lives inside `src/` so that it falls within the tsconfig's compilation scope.

**Depends on:** `@familiar-systems/types-app`

### `@familiar-systems/editor` -- The shared contract

The TipTap/ProseMirror schema defines the document structure that both the browser (via loro-prosemirror) and the campaign server (for LoroDoc reconstruction and the serialization compiler) must agree on. The browser consumes this package directly. The campaign server defines its own parallel block type mappings as Rust enums, kept in sync by convention and integration tests.

```
packages/editor/src/
├── index.ts               # Public API
├── schema.ts              # TipTap extensions list -- THE contract
└── extensions/
    ├── mention.ts         # Entity mention (configured Mention extension)
    ├── status-block.ts    # Block with status attribute (gm_only, known, retconned)
    ├── suggestion-mark.ts # AI suggestion marks on block ranges
    ├── transcluded.ts     # Transcluded block node
    ├── stat-block.ts      # Stat block node
    └── source-link.ts     # Source reference attribute
```

The `helpers/` directory from the superseded design (doc-parser, doc-writer) is gone. Those were Yjs-specific utilities for server-side document manipulation. The Loro equivalents live in the campaign server's serialization compiler. See [AI Serialization Format v2](./2026-03-25-ai-serialization-format-v2.md).

**Depends on:** `@familiar-systems/types-campaign`, `@tiptap/core`, `loro-prosemirror`

---

## The Rust Backend

Two binaries and two shared crates, all in one Cargo workspace. The internal architecture of the campaign server is defined in:

- [Campaign Collaboration Architecture](./2026-03-25-campaign-collaboration-architecture.md) -- campaign checkout/checkin, actor topology, scaling model, WebSocket architecture, suggestion model
- [Campaign Actor Domain Design](./2026-03-25-campaign-actor-domain-design.md) -- actor traits, message patterns, persistence, eviction

The service topology, graceful restart protocol, and preview environment design are in the [Deployment Architecture](./2026-03-30-deployment-architecture.md).

### `apps/platform/` -- the platform service

Handles everything before a campaign opens:

- **Authentication** -- Hanko JWT verification. User identity, profiles, session management.
- **Campaign CRUD** -- create, list, delete, transfer ownership. Metadata lives in platform.db.
- **Routing table** -- maps campaign ID to campaign server address. Lease-based: each checkout is a lease with a heartbeat.
- **Checkout endpoint** -- `POST /api/campaigns/:id/checkout` returns `{ ws_url: "wss://{apex}/campaign/:id/ws", api_base: "https://{apex}/campaign/:id" }`. The SPA calls this to acquire access to a campaign. URLs are **shard-agnostic**: they carry `campaign_id` but never a `shard_id` — ingress-layer routing resolves the owning shard. See [app-server PRD §URL architecture](./2026-04-11-app-server-prd.md#url-architecture).
- **Campaign server health monitoring** -- receives heartbeats, tracks load, detects failed leases.

Talks to **platform.db**: users, campaigns, subscriptions, the routing table. Stateless HTTP; traffic is bursty, short-lived requests.

### `apps/campaign/` -- the campaign server

Handles everything after a campaign is checked out:

- **WebSocket collaboration** -- Loro CRDTs synced via loro-dev/protocol. Room-based multiplexing: multiple Thing pages, ToC, and agent conversation streams share one WebSocket per campaign per client.
- **Campaign-scoped REST** -- entity queries, suggestion review, conversation messages. The SPA calls the campaign server directly for these (not through the platform).
- **Campaign checkout/checkin** -- downloads libSQL files from object storage, opens them on local disk, spawns actor trees. Single-server ownership via lease-based routing.
- **Actor lifecycle** -- CampaignSupervisor, ThingActor, TocActor, RelationshipGraph, UserSession, AgentConversation. Independent async tasks with per-actor persistence and eviction.
- **AI agent conversations** -- AgentConversation actors connect to LLM inference (Nebius), run the serialization compiler, route compiled suggestions to ThingActors.
- **Job dispatch** -- dispatches audio processing to workers (k8s Jobs on GPU infrastructure), receives structured transcripts, routes them to actors for entity extraction and journal drafting.

Talks to **campaigns/\*.db**: one file per campaign. Block records, entity data, relationships, search text, embeddings, suggestion outcomes, conversation history. Campaign-as-file isolation enables trivial GDPR deletion, PR preview branching (`cp`), and horizontal scaling (add servers, route campaigns).

### `crates/app-shared/` -- the cross-service crate

Types and infrastructure that cross the platform/campaign boundary: IDs (CampaignId, UserId), trait-based interfaces (`RoutingTable`, etc.), auth (JWT validation shared between both services), and libSQL helpers. Both platform and campaign depend on this crate. The campaign server communicates with the platform exclusively through traits with a single `Remote*` implementation (HTTP calls). There is no `Local` implementation. The network boundary is always present, even in development.

The litmus test: **does the platform server need this type?** If yes, it belongs in `app-shared`. If only the campaign server uses it, it belongs in `campaign-shared`.

### `crates/campaign-shared/` -- the campaign-only crate

Campaign-scoped types and infrastructure that the platform server never touches. Contains the Loro document layer (CrdtDoc trait, typed wrappers for Thing and ToC documents, ProseMirror interop conventions), campaign-scoped IDs (ThingId, BlockId, SessionId, JournalId, SuggestionId, ConversationId), view status types (GmOnly, Known, Retconned), and WebSocket notification types. Only the campaign server depends on this crate.

### Type generation

Both shared crates and both binaries contribute to type generation:

- **ts-rs** -- Rust structs derive `#[derive(TS)]` with a per-type `#[ts(export_to = "...")]` attribute that routes each type to the correct package. Types in `crates/app-shared/` export to `packages/types-app/src/generated/`; types in `crates/campaign-shared/` export to `packages/types-campaign/src/generated/`. Service-specific request/response types in each binary's crate target whichever package is appropriate.
- **utoipa** -- Route handlers are annotated with `#[utoipa::path(...)]`. Both the platform and campaign server generate OpenAPI specs. The SPA consumes both: `lib/api.ts` for platform calls, campaign-scoped REST uses the URL from the discover endpoint.

See [libSQL decision](../discovery/2026-03-09-sqlite-over-postgres-decision.md) for the database architecture.

---

## Python ML Workers

Audio processing is compute-heavy Python work that runs on GPU infrastructure (Nebius, Finnish datacenter). It doesn't need campaign context -- it receives raw audio and returns structured output.

```
workers/
├── pyproject.toml         # Managed by uv
└── src/
    ├── transcribe.py      # faster-whisper: audio -> timestamped transcript
    └── diarize.py         # pyannote: speaker attribution on transcript
```

Workers are stateless k8s Jobs. The campaign server dispatches them with audio file references; job state is tracked in platform.db. Workers return structured transcripts with speaker attribution and timestamps. The campaign server routes results to actors for the campaign-scoped stages (entity extraction, journal drafting, suggestion creation) that require the campaign graph. See [Deployment Architecture](./2026-03-30-deployment-architecture.md) for the job dispatch model and deferred design decisions.

**Managed by:** uv (pyproject.toml, virtualenv, dependencies, scripts)

---

## Apps

### `apps/site` -- Astro (public site)

```
apps/site/
├── astro.config.ts
├── src/
│   ├── pages/
│   │   ├── index.astro              # Landing page
│   │   ├── blog/
│   │   │   ├── index.astro          # Blog listing
│   │   │   └── [...slug].astro      # Blog post (content collection)
│   │   └── campaigns/
│   │       ├── index.astro          # Campaign showcase listing
│   │       └── [id].astro           # Public campaign page
│   ├── content/
│   │   ├── config.ts                # Content collection schemas (Zod)
│   │   └── blog/                    # Markdown blog posts
│   ├── layouts/
│   └── components/
├── public/
└── tsconfig.json
```

Static HTML generated at build time. No server process. Blog content uses Astro's typed content collections. Public campaign pages are static snapshots: campaign data is fetched from the platform's HTTP API at build time and rendered as HTML.

**Depends on:** `@familiar-systems/types-app`, `astro`

### `apps/web` -- Vite + React SPA

```
apps/web/
├── index.html
├── public/
├── vite.config.ts
├── src/
│   ├── main.tsx                     # Entrypoint -- React root, providers, router
│   ├── routes/
│   │   ├── index.tsx                # Route tree definition
│   │   ├── auth/
│   │   └── campaign/
│   │       ├── layout.tsx           # Campaign shell (sidebar, nav)
│   │       ├── overview.tsx
│   │       ├── thing.$thingId.tsx   # Thing page (entity editor)
│   │       ├── graph.tsx            # Graph visualization
│   │       └── settings.tsx
│   ├── components/
│   │   ├── editor/                  # TipTap editor wrapper + toolbar
│   │   ├── graph/                   # Graph visualization
│   │   ├── agent/                   # Agent window (chat UI, streaming)
│   │   ├── review/                  # Suggestion review UI
│   │   └── ui/                      # Shared UI primitives
│   └── lib/
│       ├── api.ts                   # Typed fetch client (from OpenAPI spec)
│       └── collab.ts                # loro-prosemirror provider setup
└── tsconfig.json
```

Static files. In development, `vite dev` serves files with HMR and proxies platform API requests. In production, `vite build` outputs content-hashed chunks -- upload to CDN or serve with nginx.

`lib/api.ts` is a typed fetch client generated from the OpenAPI specs (via utoipa). This replaces the tRPC client from the superseded design. The SPA uses it for platform calls; campaign-scoped REST uses the URL returned by the discover endpoint. `lib/collab.ts` configures the loro-prosemirror binding for CRDT sync with the campaign server.

**Depends on:** `@familiar-systems/types-app`, `@familiar-systems/types-campaign`, `@familiar-systems/editor`, `react`, `loro-prosemirror`, `vite`

---

## Type Generation Pipeline

Type safety across the Rust-TypeScript boundary is maintained through two generation pipelines:

### Domain types (ts-rs)

1. Rust structs in `crates/app-shared/` and `crates/campaign-shared/` (and service crates) derive `#[derive(TS)]` via the ts-rs crate, with a per-type `#[ts(export_to = "...")]` attribute
2. `cargo test` emits `.ts` declarations to `packages/types-app/src/generated/` and `packages/types-campaign/src/generated/` respectively
3. Each package's `src/index.ts` re-exports its generated types
4. `apps/web`, `apps/site`, and `@familiar-systems/editor` import from `@familiar-systems/types-app` and/or `@familiar-systems/types-campaign`

### HTTP API (utoipa + OpenAPI)

1. Axum route handlers are annotated with utoipa macros (`#[utoipa::path(...)]`)
2. `cargo test` or a build step generates an OpenAPI JSON spec
3. A frontend fetch client is generated from the spec (or hand-maintained as a thin typed wrapper)
4. `apps/web` imports the client as `lib/api.ts`

### Orchestration

`mise run generate-types` runs both pipelines. CI verifies that generated files are up-to-date (regenerate, diff, fail if dirty).

The ts-rs base output directory is configured via `TS_RS_EXPORT_DIR` in `.cargo/config.toml`:

```toml
[env]
TS_RS_EXPORT_DIR = { value = "packages", relative = true }
```

This sets the base to `packages/`. Each Rust type's `#[ts(export_to = "...")]` attribute specifies the relative path within it (e.g., `types-campaign/src/generated/id/`), routing types from each crate to the correct TypeScript package.

---

## Deployment

### Production topology

```
                ┌──────────────────────────┐
                │      Reverse Proxy        │
                │  Traefik (k3s Ingress)    │
                └──────┬───────────────────┘
                       │
     ┌─────────────────┼──────────────────┐
     │                 │                  │
     ▼                 ▼                  ▼
┌──────────────┐ ┌──────────────┐ ┌─────────────────┐
│   familiar   │ │     app.     │ │      api.       │
│   .systems   │ │   familiar   │ │    familiar     │
│   (site)     │ │   .systems   │ │    .systems     │
│   static     │ │    (SPA)     │ │   (platform)    │
└──────────────┘ │    static    │ │    :3000        │
                 └──────────────┘ └────────┬────────┘
                                           │ discover
                                           ▼
                                  ┌─────────────────┐
                                  │      c1.        │
                                  │    familiar     │
                                  │    .systems     │
                                  │ (campaign srv)  │
                                  │    :3001        │
                                  │  HTTP + WS      │
                                  └────────┬────────┘
                                         │
                                  ┌──────┴──────┐
                                  │ libSQL files │
                                  │  (/data/)    │
                                  └─────────────┘
```

Traefik (via k3s Ingress) routes by path prefix within each of two apexes per environment — a marketing apex for the Astro site and an app apex for the SPA + platform + campaign:

- `familiar.systems/` -> apps/site static files (marketing apex)
- `app.familiar.systems/` -> apps/web static files (SPA at root; all unmatched paths serve `index.html`)
- `app.familiar.systems/api/` -> platform pod (port 3000, HTTP) via `StripPrefix` middleware
- `app.familiar.systems/campaign/{campaign_id}/` -> campaign server pod (port 3001, HTTP + WebSocket) via `StripPrefix` middleware

See [app-server PRD §URL architecture](./2026-04-11-app-server-prd.md#url-architecture) for the authoritative URL contract. The SPA talks to the platform for login, campaign listing, and checkout. The checkout endpoint returns a shard-agnostic URL. The SPA opens that URL directly; ingress routes `/campaign/{id}/*` to the owning shard. The platform is never in the CRDT hot path.

Workers run on separate GPU infrastructure (Nebius) as k8s Jobs, not as persistent services. They are not exposed to the internet. See [Deployment Architecture](./2026-03-30-deployment-architecture.md) for the job dispatch model and service topology, and [Infrastructure](./2026-03-30-infrastructure.md) for cluster configuration.

### Development

```
mise run dev
```

Launches five processes in parallel, unified behind a Caddy reverse proxy on :8080 that mirrors the prod two-apex contract. Caddy binds both host matchers; `*.localhost` is loopback by browser convention, so no `/etc/hosts` entries are needed:

- `apps/site` (Astro): `http://localhost:4321` (proxied at `http://localhost:8080/`)
- `apps/web` (Vite, `base=/`): `http://localhost:5173` (proxied at `http://app.localhost:8080/`)
- `apps/platform` (`cargo run`): `http://localhost:3000` (proxied at `http://app.localhost:8080/api/`)
- `apps/campaign` (`cargo run`): `http://localhost:3001` (proxied at `http://app.localhost:8080/campaign/`)
- Caddy reverse proxy: listens on `:8080`, two host blocks (defined in `Caddyfile.dev`)

Contributors open the marketing apex at `http://localhost:8080/` and the app apex at `http://app.localhost:8080/`. Caddy handles path-based routing and `StripPrefix` behavior within each apex so backends continue to own their own routes. SPA→API and SPA→campaign calls are same-origin on the app apex; cargo's incremental compiler + Vite HMR keep iteration sub-second because the binaries run natively (not in containers).

See [Deployment Architecture §One topology everywhere](./2026-03-30-deployment-architecture.md#one-topology-everywhere) and [app-server PRD §Deployment targets](./2026-04-11-app-server-prd.md#deployment-targets) for the full per-environment detail.

No Docker database container needed. libSQL files on disk. `:memory:` databases for tests.

---

## TypeScript Tooling

| Concern            | Tool                       | Notes                                                                                                      |
| ------------------ | -------------------------- | ---------------------------------------------------------------------------------------------------------- |
| Type checking      | **tsc** (`strict: true`)   | `strict`, `noUncheckedIndexedAccess`, `exactOptionalPropertyTypes`, `noUnusedLocals`, `noUnusedParameters` |
| Runtime validation | **Zod**                    | Validates data at system boundaries (API responses, WebSocket messages, env vars)                          |
| Testing            | **Vitest**                 | Native TypeScript, fast, Jest-compatible API. Shares Vite's transform pipeline.                            |
| Linting            | **oxlint 1.0**             | Rust-based, 520+ rules, strictest config. Ban `any`, enforce exhaustive switches.                          |
| Type-aware linting | **tsgolint** (when stable) | Uses tsgo (Microsoft's official Go port of TypeScript) for type-aware rules.                               |
| Formatting         | **oxfmt** (alpha)          | Prettier-compatible, 30x faster. Fallback to Prettier if needed.                                           |

Maximum strictness, no exceptions. TypeScript types are erased at runtime -- Zod fills the gap at system boundaries, the same role Pydantic plays in Python. The compiler is the first line of defense: if it compiles, the type-level guarantees are real.

---

## Design Principles

**All backend logic is Rust.** Two binaries (platform and campaign server) and two shared crates, all in one Cargo workspace. Actor isolation handles concurrency within the campaign server; process isolation handles deployment lifecycles between services. There is no TypeScript server code. The crate split mirrors the deployment boundary: `app-shared` holds types both servers need; `campaign-shared` holds campaign-only concerns (Loro, ToC, status). The litmus test: "does the platform server need this type?" If yes, `app-shared`. If no, `campaign-shared`.

**Three TypeScript packages, no more.** `@familiar-systems/types-app` (platform-level types, generated from `app-shared`), `@familiar-systems/types-campaign` (campaign-scoped types, generated from `campaign-shared`), and `@familiar-systems/editor` (TipTap schema). If you're writing domain logic, database queries, or AI orchestration, it's Rust in `crates/` or `apps/`. The superseded design's `@familiar-systems/db`, `@familiar-systems/auth`, `@familiar-systems/ai`, and `@familiar-systems/queue` are gone.

**Dependency direction: web -> editor -> types-campaign -> types-app.** The frontend depends on all three packages. The editor depends on `types-campaign`. `types-campaign` depends on `types-app`. `apps/site` depends only on `types-app`. The dependency graph enforces the client/server boundary: `apps/web` structurally cannot import server-side code because there is no server-side TypeScript to import.

**Type safety across the language boundary.** Rust is the source of truth for domain types. ts-rs generates TypeScript declarations. utoipa generates OpenAPI specs from Axum routes. The frontend consumes both. CI verifies generated types are fresh.

**The editor package is the bridge.** The TipTap/ProseMirror schema in `@familiar-systems/editor` defines the document structure that both the browser (via loro-prosemirror) and the campaign server (for LoroDoc reconstruction and the serialization compiler) must agree on. The browser consumes the TypeScript schema directly. The campaign server defines parallel block type mappings, kept in sync by convention and integration tests.

**Maximum TypeScript strictness.** `strict: true`, `noUncheckedIndexedAccess`, `exactOptionalPropertyTypes`, lint ban on `any`, Zod at every system boundary. pnpm's strict dependency resolution prevents phantom imports. These settings are not weakened.

**Three language ecosystems, one orchestrator.** TypeScript (pnpm), Rust (Cargo), Python (uv) each manage their own builds. mise orchestrates across them: tool versions, dev startup, type generation, CI tasks. No single build system tries to understand all three.
