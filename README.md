# familiar.systems

An AI-assisted campaign notebook for tabletop RPG game masters.

> **Status: Under construction.** Tooling, CI, type generation, and infrastructure scaffolding are in place. The core architectural pieces have been validated in prototypes; the work now is productionizing them.

## What is this?

Running a TTRPG campaign generates an enormous amount of information: NPCs improvised on the fly, locations described in passing, plot threads introduced and forgotten. GMs are expected to track all of it, and most existing tools treat the wiki as the primary artifact, requiring the GM to maintain a knowledge base as a separate activity from running the game.

familiar.systems flips this. The primary artifact is the **session**: what happened at the table. Capture session audio and notes, and the AI extracts the knowledge base: NPCs, locations, items, relationships, contradictions. The GM's job shifts from authoring a wiki to running their game and reviewing what the AI proposed.

The AI never modifies the campaign directly. Every change is a **suggestion** that the GM accepts, rejects, or ignores.

## Architecture

A polyglot monorepo (pnpm + Cargo + uv, orchestrated by mise) with five deployment targets and shared libraries:

```
apps/site                 Astro static site (landing page, blog, public campaign pages)
apps/web                  Vite + React SPA (the app, behind auth)
apps/platform             Rust binary: Axum (auth, CRUD, routing table, discover)
apps/campaign             Rust binary: Axum + kameo (actors, collab, AI, compiler)
workers/                  Job processors, language-agnostic (Python ML today)

crates/app-shared         Rust library: IDs, auth (used by both Rust binaries)
crates/campaign-shared    Rust library: Loro/ToC/PM conventions (campaign-only)
packages/types-app        @familiar-systems/types-app, generated from app-shared via ts-rs
packages/types-campaign   @familiar-systems/types-campaign, generated from campaign-shared via ts-rs
packages/editor           @familiar-systems/editor, TipTap/ProseMirror schema + custom extensions
```

The Rust backend splits into two binaries: the **platform** (auth, campaign CRUD, routing table, discover) and the **campaign server** (actor hierarchy, WebSocket collaboration, AI conversations, serialization compiler). Cross-binary communication goes over HTTP, with the platform owning the routing table that maps each campaign to the shard currently hosting it. TypeScript is frontend-only; domain logic lives in Rust.

The shared-library split mirrors the binary split. `app-shared` holds types and traits both servers need (IDs, auth primitives). `campaign-shared` holds campaign-only concerns (Loro CRDT wrappers, ToC/Thing schema, ProseMirror conventions). Each Rust crate exports TypeScript types via ts-rs into its sibling package, so the client/server boundary is enforced by the dependency graph.

See [project structure](docs/plans/2026-03-26-project-structure-design.md) for the full design.

## Tech stack

- **Language:** TypeScript (frontend) + Rust (server) + Python (ML workers)
- **Frontend:** React (Vite SPA), TipTap editor
- **Server:** Rust with Axum + kameo actors (platform + campaign server)
- **Collaboration:** Loro CRDTs (loro-dev/protocol)
- **Database:** libSQL (database-per-campaign), Turso Database as upgrade path
- **API contract:** ts-rs (type generation) + utoipa (OpenAPI)
- **Public site:** Astro (static, React islands)
- **ML workers:** Python with faster-whisper, pyannote (GPU, k8s Jobs)
- **Infrastructure:** k3s on Hetzner, Pulumi IaC, Traefik Ingress

## Infrastructure

libSQL files on a Hetzner Volume: one platform database plus a separate database per campaign. No database server process. PR preview environments branch via file copy.

Each environment terminates traffic on **two apexes** with **path-based routing** inside each:

- **Marketing apex** (`familiar.systems` in prod, `localhost:8080` in dev) serves the Astro site.
- **App apex** (`app.familiar.systems` in prod, `app.localhost:8080` in dev) serves the SPA at `/`, the platform API at `/api/*`, and the campaign server at `/campaign/*`.

PR previews share both apexes via a `/pr-{N}` path prefix rather than per-PR subdomains. The SPA only ever sees shard-agnostic URLs of the form `app.familiar.systems/campaign/{campaign_id}/*`; the platform's routing table maps each campaign ID to the shard currently hosting it, and that mapping never appears in user-facing URLs.

See [infrastructure](docs/plans/2026-03-30-infrastructure.md), [deployment architecture](docs/plans/2026-03-30-deployment-architecture.md), and [libSQL over PostgreSQL decision](docs/discovery/2026-03-09-sqlite-over-postgres-decision.md).

## Getting started

The repository uses [mise](https://mise.jdx.dev/) as the cross-language task runner and toolchain manager. mise is the only prerequisite; it installs the right versions of Node, Rust, Python, and other tools on first use.

```bash
# Install toolchains pinned in mise.toml
mise install

# Start all dev servers + Caddy reverse proxy on :8080
mise run dev
# Marketing site: http://localhost:8080
# App (SPA + API + campaign): http://app.localhost:8080
# (*.localhost is loopback by browser convention; no /etc/hosts edits needed.)

# Run the full check matrix (format, lint, typecheck, test) across TS/Rust/Python
mise run check

# Targeted tasks
mise run test          # All tests (Vitest + cargo test + pytest)
mise run typecheck     # tsc + cargo check + basedpyright
mise run lint          # oxlint + clippy + ruff
mise run format        # oxfmt + cargo fmt + ruff format
mise run generate-types # Regenerate ts-rs bindings from Rust
mise run build         # Build all deployable targets
```

Use `mise tasks` to list every task with its description.

`mise run dev` boots the full dev fabric (Caddy proxy, Astro site, Vite SPA, Axum platform and campaign servers). The SPA does not yet render full application functionality, but the CI matrix and type-generation pipeline are exercised end to end.

## Design documents

### Start here

- [Vision](docs/vision.md): product vision and core concepts
- [Vision for DMs](docs/vision-for-dms.md): user-facing pitch for game masters
- [Glossary](docs/glossary.md): terms and concepts used across documentation

### Architecture

- [Project structure](docs/plans/2026-03-26-project-structure-design.md): monorepo architecture (authoritative)
- [Campaign collaboration](docs/plans/2026-03-25-campaign-collaboration-architecture.md): Rust/kameo/Loro collaboration architecture (authoritative)
- [App server PRD](docs/plans/2026-04-11-app-server-prd.md): auth, campaign CRUD, routing table, shard coordination, billing
- [Deployment architecture](docs/plans/2026-03-30-deployment-architecture.md): service topology, graceful restarts, preview environments
- [Infrastructure](docs/plans/2026-03-30-infrastructure.md): k3s cluster, Hetzner Volume, Pulumi IaC, certificates

### AI and domain model

- [AI workflow](docs/plans/2026-02-14-ai-workflow-unification-design.md): interactive and batch AI design
- [AI PRD](docs/plans/2026-02-22-ai-prd.md): full AI system requirements
- [AI serialization](docs/plans/2026-03-25-ai-serialization-format-v2.md): agent serialization format, compiler pipeline, tool signatures
- [Campaign actors](docs/plans/2026-03-25-campaign-actor-domain-design.md): actor topology, trait system, WebSocket architecture
- [Entity-relationship temporal model](docs/plans/2026-04-10-entity-relationship-temporal-model.md): relationship schema, sessions as knowledge time, lifecycle

### Further reading

Additional decision records, spikes, and narrower designs (libSQL vs PostgreSQL, datalog vs SQL, the Loro+TipTap suggestion-marks spike, templates-as-prototype-pages, public site design, archived/superseded plans) live in [`docs/`](docs/).

## Contributing

The source is public because we believe in transparency, but the project is not yet open to outside contributions: there's no triage process in place, and the codebase changes too fast for external PRs to land cleanly. `CONTRIBUTING.md` will signal when that changes.

## License

[AGPL-3.0](LICENSE). Copyright (C) 2026 Grinshpon Consulting ENK.

The AGPL is a deliberate choice for a hosted product: anyone may run, modify, and redistribute the code, including hosting their own instance, provided derivative network services make their source available under the same terms.
