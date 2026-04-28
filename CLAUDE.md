# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

familiar.systems is an AI-assisted campaign notebook for tabletop RPG game masters. It captures session content (audio, notes) and uses AI to assemble a campaign knowledge base (NPCs, locations, items, relationships) as a graph that grows from play.

**Status: Pre-implementation.** The repository contains design documents only, no application code yet. All architectural decisions are documented in `docs/`.

## Key Design Documents

- `docs/vision.md`: Product vision, core concepts (Campaign, Session, Things, Blocks, Edges, Status, Suggestions)
- `docs/glossary.md`: Glossary of terms and concepts used across documentation. Intended for coding agents and developers.
- `docs/plans/2026-03-26-project-structure-design.md`: **Authoritative** project structure (Rust backend + TypeScript frontend + Python ML workers)
- `docs/plans/2026-02-14-ai-workflow-unification-design.md`: AI workflow architecture (SessionIngest, P&R, Q&A)
- `docs/plans/2026-02-22-ai-prd.md`: Full AI system requirements (SessionIngest, entity extraction, suggestion lifecycle)
- `docs/plans/2026-02-20-templates-as-prototype-pages.md`: Templates are Things, not a separate entity. Categorization via `prototypeId` and tag-relationships.
- `docs/plans/2026-02-20-public-site-design.md`: Public site (Astro) for landing page, blog, public campaign pages. Path-based routing.
- `docs/plans/2026-03-30-infrastructure.md`: Infrastructure (k3s cluster, Hetzner Volume, Pulumi project structure, certificates, CI/CD)
- `docs/plans/2026-03-30-deployment-architecture.md`: Deployment architecture (platform/campaign service split, graceful restarts, preview environments)
- `docs/plans/2026-03-25-campaign-collaboration-architecture.md`: **Authoritative** collaboration architecture (Rust/kameo/Loro, supersedes Hocuspocus ADR). Campaign checkout/checkin, actor topology, scaling model.
- `docs/plans/2026-03-25-campaign-actor-domain-design.md`: Actor topology, trait system, WebSocket architecture, suggestion model
- `docs/plans/2026-04-10-entity-relationship-temporal-model.md`: Relationship schema, temporal model (sessions as knowledge time), relationship lifecycle (superseded, retconned, deleted)
- `docs/plans/2026-04-11-app-server-prd.md`: App server PRD (auth, campaign CRUD, routing table, shard coordination, billing)
- `docs/plans/2026-03-25-ai-serialization-format-v2.md`: Agent serialization format, progressive disclosure tiers, compiler pipeline, tool signatures
- `docs/plans/2026-03-25-loro-tiptap-spike.md`: Spike plan validating suggestion marks on block UUID ranges in Loro + TipTap
- `docs/discovery/2026-03-09-sqlite-over-postgres-decision.md`: libSQL over PostgreSQL decision (database-per-campaign, Turso Database upgrade path)

### Not Worth Reading On Startup

- `docs/discovery/2026-04-11-datalog-vs-sql-query-layer.md`: Datalog vs SQL for campaign query layer. Decided: proceed with libSQL + typed tool calls; datalog is correct model but no viable Rust runtime engine exists.
- `docs/archive/plans/2026-02-14-spa-vs-ssr-design.md`: Why SPA over SSR (decided: SPA)
- `docs/archive/plans/2026-02-14-project-structure-design.md`: **Superseded** by the SPA design.
- `docs/archive/plans/2026-02-14-project-structure-spa-design.md`: **Superseded** by the 2026-03-26 project structure redesign.
- `docs/archive/discovery/2026-02-18-postgres-vs-turso.md`: Original PostgreSQL decision (superseded by libSQL decision)
- `docs/archive/discovery/2026-02-14-storage-overview.md`: Initial storage architecture analysis
- `docs/archive/plans/2026-02-18-deployment-strategy.md`: Previous deployment strategy (superseded by 2026-03-09 version)
- `docs/archive/plans/2026-03-09-deployment-strategy.md`: Previous deployment strategy (superseded by k3s deployment strategy)
- `docs/archive/plans/2026-03-12-deployment-strategy.md`: **Superseded** by Infrastructure and Deployment Architecture docs. Monolithic plan covering both concerns.
- `docs/archive/discovery/2026-02-18-solo-dev-deployment-landscape.md`: Deployment exploration (decided: Hetzner)
- `docs/archive/discovery/2026-02-18-eu-deployment-landscape.md`: EU deployment exploration (decided: Hetzner)
- `docs/archive/plans/2026-03-14-hocuspocus-architecture.md`: **Superseded** by Campaign Collaboration Architecture. Hocuspocus/Yjs-era design; hypotheses validated, implementation technology replaced.

Read the project structure doc (`docs/plans/2026-03-26-project-structure-design.md`) before making architectural decisions. It is the source of truth.

## Architecture

### Monorepo: pnpm workspaces + Cargo + uv (orchestrated by mise)

```
apps/site             Astro static site (landing page, blog, public campaign pages)
apps/web              Vite + React SPA (the app, behind auth)
apps/platform         Rust binary: Axum (auth, CRUD, routing table, discover)
apps/campaign         Rust binary: Axum + kameo (actors, collab, AI, compiler)
workers/              Job processors, language-agnostic (Python ML today)

crates/app-shared       Rust library: IDs, auth (platform + campaign)
crates/campaign-shared  Rust library: ToC/Thing Loro wrappers, PM conventions, CrdtDoc trait (campaign only)
packages/types-app      @familiar-systems/types-app, generated from app-shared via ts-rs (CampaignId, UserId)
packages/types-campaign @familiar-systems/types-campaign, generated from campaign-shared via ts-rs (ThingId, BlockId, ThingHandle, TocEntry, ...)
packages/editor         @familiar-systems/editor, TipTap/ProseMirror schema + custom extensions (THE shared contract)
```

### Critical Dependency Rules

- **Dependency direction: `web -> editor -> types-campaign -> types-app`.** The editor depends on campaign types. Campaign types depend on app types. `web` also depends on `types-app` directly (for auth, campaign listing).
- **`apps/site` depends only on `types-app`.** The public site needs platform-level types only (CampaignId, UserId).
- **`apps/web` depends on `types-app`, `types-campaign`, and `editor`.** The client/server boundary is enforced by the dependency graph. There is no server-side TypeScript to import.
- **Each package's `src/index.ts` is its public API.** Import from `@familiar-systems/types-app` or `@familiar-systems/types-campaign`, never from `@familiar-systems/types-campaign/generated/ThingId`.
- **Domain logic is Rust.** Two Rust binaries (platform + campaign server) and two shared crates own all backend logic. TypeScript is frontend-only.
- **Two shared crates, two type packages, same split.** `app-shared` / `types-app` holds types both servers need (IDs, auth). `campaign-shared` / `types-campaign` holds campaign-only concerns (Loro wrappers, ToC schema, ProseMirror conventions, CrdtDoc trait). The test: "does the platform server need this type?" If yes, `app-shared`. If no, `campaign-shared`. Both crates export TypeScript types via ts-rs to their corresponding package.

### Five Deployment Targets

Each target has a different lifecycle. Deploying one must not affect the others:

1. **site**: Static HTML (CDN/nginx). Public-facing. Content changes deploy independently.
2. **web**: Static files (CDN/nginx). The authenticated SPA.
3. **platform**: Rust binary (Axum). Auth, campaign CRUD, routing table, discover. Talks to platform.db. Rarely changes.
4. **campaign**: Rust binary (Axum + kameo actors). Actor hierarchy, WebSocket collab, AI conversations, compiler. Campaign-pinned. Changes frequently. See [Campaign Collaboration Architecture](docs/plans/2026-03-25-campaign-collaboration-architecture.md) and [Deployment Architecture](docs/plans/2026-03-30-deployment-architecture.md).
5. **workers**: Job processors (language-agnostic). Today: Python ML workers (faster-whisper, pyannote). Stateless, GPU-bound. Deployed as k8s Jobs, dispatched by the campaign server. Job state in platform.db.

### AI Architecture

Two execution paths, same output primitives:

- **Interactive** (server, AgentConversation actors): P&R and Q&A via the agent window. Streaming, latency-sensitive.
- **Batch** (server, actors + Python workers): SessionIngest pipeline. Audio processing dispatched to Python ML workers; campaign-scoped work (entity extraction, journal drafting) runs through actors.

Both produce **Suggestions**: proposed mutations to the campaign graph. AI never modifies the graph directly; every change requires GM approval. Suggestions are always durable (persisted immediately).

The AI agent writes via tool calls (`suggest_replace`, `create_page`, `propose_relationship`). The serialization compiler translates tool calls into compiled suggestions routed to ThingActors. Document-level proposals use suggestion marks on block UUID ranges; graph-level proposals use the suggestion queue. See [AI Serialization Format v2](docs/plans/2026-03-25-ai-serialization-format-v2.md) and [Campaign Actor Domain Design](docs/plans/2026-03-25-campaign-actor-domain-design.md).

Tool availability determines AI behavior (no mode toggles): GMs get read+write tools, players get read-only tools.

## Tech Stack

| Concern        | Choice                                                      |
| -------------- | ----------------------------------------------------------- |
| Language       | TypeScript (frontend) + Rust (server) + Python (ML workers) |
| Public site    | Astro (static site generator, React islands)                |
| Frontend       | React (Vite SPA)                                            |
| Editor         | TipTap (on ProseMirror)                                     |
| Routing        | TanStack Router or React Router (not yet decided)           |
| Server         | Rust: Axum + kameo actors                                   |
| API contract   | ts-rs (type generation) + utoipa (OpenAPI)                  |
| Database       | SQLite for platform (via `sea-orm` + `sqlx-sqlite`); libSQL planned for campaign server (database-per-campaign, Turso Database upgrade path) |
| Collaboration  | Loro CRDTs + loro-dev/protocol                              |
| Object Storage | Hetzner Object Storage (campaign DB source of truth)        |
| ML workers     | Python: faster-whisper, pyannote (GPU, k8s Jobs)            |
| Validation     | Zod (at TypeScript system boundaries)                       |
| Testing        | Vitest (TS), cargo test (Rust), pytest (Python)             |
| Dev runner     | Vite dev server (frontend), cargo run (server)              |
| Linting        | oxlint (TS, strictest config)                               |
| Formatting     | oxfmt (alpha, Prettier fallback)                            |
| TS packages    | pnpm (strict dependency resolution)                         |
| Orchestration  | mise (cross-language task runner + tool versions)           |

## Commands

**Use mise tasks for all verification, even single-package.** No `pnpm --filter` or raw `cargo` fallbacks.

```bash
# Use these (mise orchestrates the right tools):
mise run test                   # All tests (Vitest + cargo test + pytest)
mise run typecheck              # All type-checking (tsc + cargo check + basedpyright)
mise run lint                   # All linting (oxlint + clippy + ruff)
mise run format                 # All formatting (oxfmt + cargo fmt + ruff format)
mise run format:check           # Check formatting without modifying
mise run dev                    # Start all dev servers (site:4321, web:5173, platform:3000, campaign:3001)
mise run build                  # Build all targets in dependency order
mise run generate-types         # Clean + regenerate ts-rs types via cargo test

# Per-ecosystem commands (for targeted work on a single package):
pnpm install                    # Install all TS dependencies
pnpm --filter @familiar-systems/editor test
cargo build                     # Build Rust workspace
cargo test -p familiar-systems-campaign-shared  # Test a single crate
uv run pytest                   # Run ML worker tests
```

## TypeScript Strictness

Maximum strictness, no exceptions:

- `strict: true`
- `noUncheckedIndexedAccess`: array indexing returns `T | undefined`
- `exactOptionalPropertyTypes`: distinguishes `undefined` from missing
- `noUnusedLocals` + `noUnusedParameters`
- Lint ban on `any`
- Zod validation at every system boundary (API inputs, DB rows, env vars)

## Core Domain Concepts

- **Status** (on nodes, blocks, relationships): `gm_only` → `known` → `retconned`. Default is `gm_only`. Status cascades down (GM-only node = all children GM-only), not up.
- **Suggestions**: Discriminated union over types (`create_thing`, `update_blocks`, `create_relationship`, `journal_draft`, `contradiction`). Always durable. Auto-reject after ~14 days.
- **AgentConversation**: Persisted record of AI interactions. Provenance for suggestions. Roles: `gm`, `player`, `system`.
- **Mentions** (block→node or block→block): Derived automatically, power backlinks and transclusion.
- **Relationships** (node→node): Authored/curated, carry semantic labels. Freeform vocabulary.
- **Prototypes (templates)**: A template is a Thing with `isTemplate: true`. No separate `Template` entity. Creating a thing from a template clones the prototype's block structure. `prototypeId?: ThingId` tracks lineage. Tags are Things connected via `tagged` relationships, not a `tags: string[]` field.

## Deployment Targets

Three deployment environments, one URL contract. **Every environment terminates traffic on two apexes - a marketing apex for the Astro site and an app apex for the SPA, platform API, and campaign shards - with path-based routing applied within each.** Per-PR previews are a `/pr-{N}` path prefix applied to both apexes. Each Hanko tenant registers exactly one origin: the environment's app apex.

| Target         | Marketing host                     | App host                               | SPA base path       | Auth tenant                                        | Fabric                                                                                                                                                                                                                                                                                                                                                                                                                                       |
| -------------- | ---------------------------------- | -------------------------------------- | ------------------- | -------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **Local dev**  | `http://localhost:8080`            | `http://app.localhost:8080`            | `/`                 | preview Hanko (`HANKO_API_URL_DEV` in `mise.toml`) | `mise run dev` launches the Astro site (4321), Vite SPA (5173, `base=/`), platform (cargo, 3000), campaign (cargo, 3001), and a **Caddy reverse proxy on 8080** (`Caddyfile.dev`) that binds both host matchers. Marketing host: `/` → Astro. App host: `/api/*` → platform, `/campaign/*` → campaign, `/*` → SPA. `*.localhost` is loopback by browser convention, so no `/etc/hosts` entries are required. Data in `data/dev-platform.db`. |
| **PR preview** | `https://preview.familiar.systems` | `https://app.preview.familiar.systems` | `/pr-${PR_NUMBER}/` | preview Hanko (same tenant as local dev)           | k3s namespace `preview-pr-${PR_NUMBER}`. Traefik IngressRoutes + `StripPrefix` middlewares on both hosts per PR. PRs share the app apex origin, so browser state and auth session carry across PRs by design.                                                                                                                                                                                                                                |
| **Prod**       | `https://familiar.systems`         | `https://app.familiar.systems`         | `/`                 | prod Hanko (`HANKO_API_URL_PROD` in Pulumi)        | k3s default namespace. Marketing apex: one rule (`/` → Astro). App apex: priority-ordered Traefik path rules (`/api`, `/campaign`, catch-all `/` → SPA). Data on Hetzner Volume + object storage.                                                                                                                                                                                                                                            |

**Scope of this contract:** the application's services live on the two apexes above. Subdomains that host separate systems (Hanko's `auth.*`, and any future surfaces like `docs.`, `status.`, `blog.`) live on their own DNS and manage their own routing, TLS, and auth.

**URL-structure authority:** [`docs/plans/2026-04-11-app-server-prd.md` §URL architecture](docs/plans/2026-04-11-app-server-prd.md).
**Service topology + lifecycle:** [`docs/plans/2026-03-30-deployment-architecture.md`](docs/plans/2026-03-30-deployment-architecture.md).
**Helper paths in SPA code:** `apps/web/src/lib/paths.ts` (`apiPath`, `campaignPath`, `spaRoute`) - always use these instead of hardcoded `/api/...` or `/login`.

## Development Notes

- The `@familiar-systems/editor` package is the most architecturally important. It defines the TipTap schema shared between browser (apps/web via loro-prosemirror) and the campaign server (for LoroDoc reconstruction and serialization compiler).
- LLM provider is pluggable: hosted instance uses managed keys, self-hosters bring their own.
- No Docker database container needed for local development. SQLite files on disk (and eventually libSQL files for campaign DBs once the campaign server lands). `:memory:` databases for tests.
- **Cookie scope is per apex.** Any cookie set on the app apex (`app.familiar.systems`, or its preview/dev equivalents) is visible to the SPA, platform API, and campaign shards - but not to the marketing site on `familiar.systems`, which is a separate browser origin. Hanko's session cookie (if set) lives on `auth.*`, not on either of ours, and auth tokens travel as `Authorization: Bearer` headers. When adding a cookie (analytics, preferences, feature flags, anything), pick the apex deliberately and scope it narrowly with `Path=`.
