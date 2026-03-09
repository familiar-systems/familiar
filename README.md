# Loreweaver

An AI-assisted campaign notebook for tabletop RPG game masters.

> **Status: Pre-implementation.** The repository contains design documents and scaffolded project structure. No application code yet.

## What is this?

Running a TTRPG campaign generates an enormous amount of information — NPCs improvised on the fly, locations described in passing, plot threads introduced and forgotten. GMs are expected to track all of it, and most existing tools treat the wiki as the primary artifact, requiring the GM to maintain a knowledge base as a separate activity from running the game.

Loreweaver flips this. The primary artifact is the **session** — what happened at the table. Capture session audio and notes, and the AI extracts the knowledge base: NPCs, locations, items, relationships, contradictions. The GM's job shifts from authoring a wiki to running their game and reviewing what the AI proposed.

The AI never modifies the campaign directly. Every change is a **suggestion** that the GM accepts, rejects, or ignores.

## Architecture

A TypeScript monorepo (pnpm + Turborepo) with five apps and six shared packages:

```
apps/
  site/       Astro static site (landing page, blog, public campaign pages)
  web/        Vite + React SPA (the app, behind auth)
  api/        Hono + tRPC server (CRUD, interactive AI streaming)
  collab/     Hocuspocus WebSocket server (real-time collaborative editing via Yjs)
  worker/     Job consumer (batch AI: transcription, entity extraction)

packages/
  domain/     Pure types, zero dependencies
  db/         Drizzle ORM schema, migrations, query helpers (libSQL)
  auth/       Token verification, permissions
  editor/     TipTap/ProseMirror schema + custom extensions
  ai/         LLM client, prompt templates, pipelines
  queue/      Job type definitions, polling producer/consumer
```

See [project structure](docs/plans/2026-02-14-project-structure-spa-design.md) for the full design.

## Tech stack

- **Language:** TypeScript (full stack)
- **Frontend:** React (Vite SPA), TipTap editor
- **API:** Hono + tRPC
- **Collaboration:** Hocuspocus (Yjs CRDT server)
- **Database:** libSQL (database-per-campaign), Turso Database as upgrade path
- **ORM:** Drizzle
- **Public site:** Astro
- **Cloud Infrastructure:** Coolify on Hetzner, Pulumi IaC
- **Self-Hosted:** TBD

## Infrastructure

libSQL files on a Hetzner Volume — one platform database plus a separate database per campaign. No database server process. PR preview environments branch via file copy. Coolify handles deployment orchestration with Traefik for routing and SSL.

See [deployment strategy](docs/plans/2026-03-09-deployment-strategy.md) and [libSQL over PostgreSQL decision](docs/discovery/2026-03-09-sqlite-over-postgres-decision.md) for the rationale.

## Design documents

- [Vision](docs/vision.md) — product vision and core concepts
- [Project structure](docs/plans/2026-02-14-project-structure-spa-design.md) — monorepo architecture (authoritative)
- [AI workflow](docs/plans/2026-02-14-ai-workflow-unification-design.md) — interactive and batch AI design
- [AI PRD](docs/plans/2026-02-22-ai-prd.md) — full AI system requirements
- [Deployment strategy](docs/plans/2026-03-09-deployment-strategy.md) — Coolify + Hetzner + libSQL
- [Database decision](docs/discovery/2026-03-09-sqlite-over-postgres-decision.md) — why libSQL over PostgreSQL
- [Templates](docs/plans/2026-02-20-templates-as-prototype-pages.md) — templates are Things, not a separate entity
- [Public site](docs/plans/2026-02-20-public-site-design.md) — Astro static site design

## License

[AGPL-3.0](LICENSE) — Copyright (C) 2026 Grinshpon Consulting ENK
