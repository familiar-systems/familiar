# Loreweaver

An AI-assisted campaign notebook for tabletop RPG game masters.

> **Status: Pre-implementation.** The repository contains design documents and scaffolded project structure. No application code yet.

## What is this?

Running a TTRPG campaign generates an enormous amount of information: NPCs improvised on the fly, locations described in passing, plot threads introduced and forgotten. GMs are expected to track all of it, and most existing tools treat the wiki as the primary artifact, requiring the GM to maintain a knowledge base as a separate activity from running the game.

Loreweaver flips this. The primary artifact is the **session**: what happened at the table. Capture session audio and notes, and the AI extracts the knowledge base: NPCs, locations, items, relationships, contradictions. The GM's job shifts from authoring a wiki to running their game and reviewing what the AI proposed.

The AI never modifies the campaign directly. Every change is a **suggestion** that the GM accepts, rejects, or ignores.

## Architecture

A polyglot monorepo (pnpm + Cargo + uv, orchestrated by mise) with five deployment targets and shared libraries:

```
apps/site        Astro static site (landing page, blog, public campaign pages)
apps/web         Vite + React SPA (the app, behind auth)
apps/platform    Rust binary: Axum (auth, CRUD, routing table, discover)
apps/campaign    Rust binary: Axum + kameo (actors, collab, AI, compiler)
workers/         Job processors, language-agnostic (Python ML today)

crates/shared    Rust library: traits, types, auth, libSQL helpers
packages/types   @loreweaver/types, generated from Rust via ts-rs
packages/editor  @loreweaver/editor, TipTap/ProseMirror schema + custom extensions
```

The Rust backend splits into two binaries: the **platform** (auth, campaign CRUD, routing table) and the **campaign server** (actor hierarchy, WebSocket collaboration, AI conversations). They communicate over HTTP via trait-based interfaces defined in `crates/shared/`. TypeScript is frontend-only; domain logic lives in Rust.

See [project structure](docs/plans/2026-03-26-project-structure-design.md) for the full design.

## Tech stack

- **Language:** TypeScript (frontend) + Rust (server) + Python (ML workers)
- **Frontend:** React (Vite SPA), TipTap editor
- **Server:** Rust with Axum + kameo actors (platform + campaign server)
- **Collaboration:** Loro CRDTs (loro-dev/protocol)
- **Database:** libSQL (database-per-campaign), Turso Database as upgrade path
- **API contract:** ts-rs (type generation) + utoipa (OpenAPI)
- **Public site:** Astro (static, React islands)
- **ML workers:** Python with faster-whisper, pyannote (GPU, called via HTTP)
- **Infrastructure:** k3s on Hetzner, Pulumi IaC, Traefik Ingress

## Infrastructure

libSQL files on a Hetzner Volume: one platform database plus a separate database per campaign. No database server process. PR preview environments branch via file copy. Two Rust binaries (platform + campaign server) deployed on k3s with Traefik Ingress routing by subdomain (`api.loreweaver.no`, `c1.loreweaver.no`).

See [infrastructure](docs/plans/2026-03-30-infrastructure.md) and [libSQL over PostgreSQL decision](docs/discovery/2026-03-09-sqlite-over-postgres-decision.md) for the rationale.

## Design documents

- [Vision](docs/vision.md): product vision and core concepts
- [Project structure](docs/plans/2026-03-26-project-structure-design.md): monorepo architecture (authoritative)
- [Campaign collaboration](docs/plans/2026-03-25-campaign-collaboration-architecture.md): Rust/kameo/Loro collaboration architecture (authoritative)
- [Campaign actors](docs/plans/2026-03-25-campaign-actor-domain-design.md): actor topology, trait system, WebSocket architecture
- [AI serialization](docs/plans/2026-03-25-ai-serialization-format-v2.md): agent serialization format, compiler pipeline, tool signatures
- [AI workflow](docs/plans/2026-02-14-ai-workflow-unification-design.md): interactive and batch AI design
- [AI PRD](docs/plans/2026-02-22-ai-prd.md): full AI system requirements
- [Infrastructure](docs/plans/2026-03-30-infrastructure.md): k3s cluster, Hetzner Volume, Pulumi IaC, certificates
- [Deployment architecture](docs/plans/2026-03-30-deployment-architecture.md): service topology, graceful restarts, preview environments
- [Database decision](docs/discovery/2026-03-09-sqlite-over-postgres-decision.md): why libSQL over PostgreSQL
- [Suggestion marks spike](docs/plans/2026-03-25-loro-tiptap-spike.md): validates suggestion model on Loro + TipTap
- [Templates](docs/plans/2026-02-20-templates-as-prototype-pages.md): templates are Things, not a separate entity
- [Public site](docs/plans/2026-02-20-public-site-design.md): Astro static site design

## License

[AGPL-3.0](LICENSE). Copyright (C) 2026 Grinshpon Consulting ENK
