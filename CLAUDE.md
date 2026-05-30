# CLAUDE.md

## Project Overview

familiar.systems is an AI-assisted campaign notebook for tabletop RPG game masters. It captures session content (audio, notes) and uses AI to assemble a campaign knowledge base (NPCs, locations, items, relationships) as a graph that grows from play.

**Status: Early implementation.** Platform and campaign servers are live (auth, campaign CRUD, creation flow, idle eviction). SPA has hub listing and a 4-step campaign creation wizard. The campaign server has live CRDT collaboration - ToC + Thing room actors, the WebSocket layer, and block/ToC persistence. The client-side editor and the AI system are not yet built.

## Key Design Documents

Full catalog in `docs/plans/`. Read the **authoritative** structure doc before any architectural decision. Titles only here; each doc's own header carries the detail.

- `docs/vision.md` - product vision, core concepts
- `docs/glossary.md` - shared terminology
- `docs/plans/2026-03-26-project-structure-design.md` - **authoritative** project structure (source of truth)
- `docs/plans/2026-03-25-campaign-collaboration-architecture.md` - **authoritative** collab architecture (kameo/Loro, checkout/checkin, scaling)
- `docs/plans/2026-05-22-campaign-creation-architecture.md` - campaign creation flow + catalog (state-of-the-world)
- `docs/plans/2026-05-04-campaign-actor-domain-design.md` - actor topology, CRDT room model, suggestion model
- `docs/plans/2026-04-11-app-server-prd.md` - platform server (auth, CRUD, shard coordination, billing)
- `docs/plans/2026-04-10-entity-relationship-temporal-model.md` - relationship schema + temporal model
- `docs/plans/2026-02-20-templates-as-prototype-pages.md` - templates are Things (`prototypeId` lineage)
- `docs/plans/2026-03-30-deployment-architecture.md` - platform/campaign split, graceful restart, previews
- `docs/plans/2026-05-23-infrastructure.md` - k3s, OpenTofu, certs, CI/CD
- `docs/plans/2026-02-20-public-site-design.md` - Astro public site (has drifted; verify against `apps/site`)
- `docs/discovery/2026-03-09-sqlite-over-postgres-decision.md` - SQLite over Postgres
- AI system (designed, **not built**): `2026-02-14-ai-workflow-unification-design.md`, `2026-02-22-ai-prd.md`, `2026-03-25-ai-serialization-format-v2.md`, `2026-03-25-loro-tiptap-spike.md`

Superseded docs live in `docs/archive/` (historical only).

## Architecture

### Monorepo: pnpm workspaces + Cargo + uv (orchestrated by mise)

```
apps/site             Astro static site (landing page, blog, public campaign pages)
apps/web              Vite + React SPA (the app, behind auth)
apps/platform         Rust binary: Axum (auth, campaign CRUD, routing/shard table)
apps/campaign         Rust binary: Axum + kameo (actors, collab, AI, compiler)
workers/              Job processors, language-agnostic (Python ML in workers/whisperx/)

crates/app-shared       Rust library: IDs, auth (platform + campaign)
crates/campaign-shared  Rust library: Loro schema constants + ts-rs types (ToC/Thing schema, PM conventions), onboarding DTOs (campaign only)
crates/fs-id, fs-id-macros  Rust utility crates: type-safe ID branding (#[fs_id] macro) used by both shared crates
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
- **Two shared crates, two type packages, same split.** `app-shared` / `types-app` holds types both servers need (IDs, auth). `campaign-shared` / `types-campaign` holds campaign-only concerns (Loro schema constants and ts-rs types, ToC schema, ProseMirror conventions, onboarding DTOs); the concrete Loro doc wrappers and the `CrdtDoc` trait are app-local in `apps/campaign`. The test: "does the platform server need this type?" If yes, `app-shared`. If no, `campaign-shared`. Both crates export TypeScript types via ts-rs to their corresponding package.

### Deployment Targets

Each target has a different lifecycle. Deploying one must not affect the others:

1. **site**: Static HTML (CDN/nginx). Public-facing. Content changes deploy independently.
2. **web**: Static files (CDN/nginx). The authenticated SPA.
3. **platform**: Rust binary (Axum). Auth, campaign CRUD, routing/shard table. Talks to platform.db. Rarely changes.
4. **campaign**: Rust binary (Axum + kameo actors). Actor hierarchy, WebSocket collab, AI conversations, compiler. Campaign-pinned. Changes frequently. See [Campaign Collaboration Architecture](docs/plans/2026-03-25-campaign-collaboration-architecture.md) and [Deployment Architecture](docs/plans/2026-03-30-deployment-architecture.md).
5. **workers**: Job processors (language-agnostic). Today: a Python ML worker (WhisperX, in `workers/whisperx/`). Stateless, GPU-bound. Deployed as k8s Jobs, dispatched by the campaign server. Job state in platform.db.

### AI Architecture (not yet built)

Designed but not implemented (see the AI design docs listed above). Key constraint: AI produces **Suggestions** (proposed graph mutations) and never modifies the graph directly - every change requires GM approval.

## Tech Stack

| Concern        | Choice                                                                                                     |
| -------------- | ---------------------------------------------------------------------------------------------------------- |
| Language       | TypeScript (frontend) + Rust (servers) + Python (ML workers)                                               |
| Frontend       | React (Vite SPA), Astro (static site), TanStack Router                                                      |
| Editor         | TipTap (on ProseMirror)                                                                                    |
| Server         | Rust: Axum + kameo actors                                                                                  |
| API contract   | ts-rs (type generation) + utoipa (OpenAPI)                                                                 |
| Database       | SQLite + sqlite-vec (via `sea-orm` + `sqlx-sqlite`) for both platform and campaign. Database-per-campaign. |
| Collaboration  | Loro CRDTs + loro-dev/protocol                                                                             |
| Object Storage | Hetzner Object Storage (campaign DB source of truth)                                                       |
| ML workers     | Python: WhisperX (GPU, k8s Jobs)                                                                            |
| Testing        | Vitest (TS), cargo test (Rust), pytest (Python)                                                            |
| Orchestration  | mise (cross-language task runner + tool versions)                                                          |

## Commands

**Use mise tasks for all verification, even single-package.** No `pnpm --filter` or raw `cargo` fallbacks.

```bash
mise run setup                  # Install all dependencies (TS + Python workers + Python infra + e2e)
mise run check                  # All checks in parallel (format:check + lint + typecheck + test + web:e2e)
mise run test                   # All tests (Vitest + cargo test + pytest)
mise run typecheck              # All type-checking (tsc + cargo check + basedpyright)
mise run lint                   # All linting (oxlint + clippy + ruff + actionlint + kubeconform + content schemas)
mise run format                 # All formatting (oxfmt + cargo fmt + ruff format)
mise run format:check           # Check formatting without modifying
mise run dev                    # Start all dev servers (site:4321, web:5173, platform:3000, campaign:3001, Caddy proxy:8080)
mise run build                  # Build all targets in dependency order
mise run generate-types         # Clean + regenerate ts-rs types + OpenAPI specs
mise run e2e                    # Playwright end-to-end tests
mise run clean                  # Remove build artifacts
```

## TypeScript Strictness

Maximum strictness, no exceptions:

- `strict: true`
- `noUncheckedIndexedAccess`: array indexing returns `T | undefined`
- `exactOptionalPropertyTypes`: distinguishes `undefined` from missing
- `noUnusedLocals` + `noUnusedParameters`
- Lint ban on `any`
- Zod validation at every system boundary (API inputs, DB rows, env vars)

## Engineering Rules

- **No untyped code.** No `Any`, no `as any`, no `cast()`-as-truth, no `unwrap()` without a proof comment. Strict types are load-bearing infrastructure.
- **No silent fallback defaults.** Throw at build time or startup rather than defaulting. If a required value is missing, panic; don't invent a "reasonable" substitute.
- **Functional over imperative.** Sum types over mutable fields. Discriminated unions over boolean flags. `match` over `if/else` chains.
- **Fix all warnings on sight**, even pre-existing ones. Warnings are bugs that haven't matured yet.
- **Shared crates hold contracts and shared infrastructure.** Types, traits, constants, and middleware both apps consume identically (auth extractors, bearer validation) belong in `app-shared`/`campaign-shared`. Per-app wiring (entity definitions, ORM queries, actor topology, app-specific extractors like `PlatformUser`) stays in the consuming app. The test: "is this identical in both apps?" If yes, it belongs in the shared crate. If it diverges per-app, it stays local.
- **Prove before fixing.** Verify the bug exists before patching. Reproduce first, then fix.
- **Threat model: cooperative within a campaign, adversarial across the platform.** GM-only visibility filtering can be client-side; cross-user and platform-integrity boundaries require full server-side authorization. Internet external is always adversarial.

## Core Domain Concepts

- **Status** (on nodes, blocks, relationships): `gm_only` → `known` → `retconned`. Default is `gm_only`. Status cascades down (GM-only node = all children GM-only), not up.
- **Suggestions**: Discriminated union over types (`create_thing`, `update_blocks`, `create_relationship`, `journal_draft`, `contradiction`). Always durable. Auto-reject after ~14 days.
- **AgentConversation**: Persisted record of AI interactions. Provenance for suggestions. Roles: `gm`, `player`, `system`.
- **Mentions** (block→node or block→block): Derived automatically, power backlinks and transclusion.
- **Relationships** (node→node): Authored/curated, carry semantic labels. Freeform vocabulary.
- **Prototypes (templates)**: A template is a Thing reused as a prototype - no separate `Template` entity. Creating a thing from a template clones the prototype's block structure; `prototypeId?: ThingId` tracks lineage. (An explicit `isTemplate` flag is planned but not yet in the schema; template-ness is currently by convention via `prototype_id`.) Tags are Things connected via `tagged` relationships, not a `tags: string[]` field.

## Deployment

Three environments: **local** (`mise run dev`), **preview** (auto-deployed on push to a PR), **prod** (auto-deployed on merge to main). Preview deploys are first-class; never merge without verifying the preview.

All application secrets live in Scaleway Secrets Manager and are synced into k8s via External Secrets Operator. See `infra/CLAUDE.md` for the full credentials architecture.

### URL contract

Every environment terminates on **two apexes** - a marketing apex (Astro site) and an app apex (SPA + platform API + campaign shards) - with **path-based routing** within the app apex: `/api/*` → platform, `/campaign/*` → campaign, `/*` → SPA. Per-PR previews add a `/pr-{N}` path prefix on both apexes. Each Hanko tenant registers exactly one origin: the environment's app apex.

| Env     | Marketing apex             | App apex                       | SPA base   |
| ------- | -------------------------- | ------------------------------ | ---------- |
| local   | `localhost:8080`           | `app.localhost:8080`           | `/`        |
| preview | `preview.familiar.systems` | `app.preview.familiar.systems` | `/pr-{N}/` |
| prod    | `familiar.systems`         | `app.familiar.systems`         | `/`        |

- **Local dev** runs behind a Caddy proxy on :8080 (`Caddyfile.dev`) binding both host matchers; `*.localhost` is loopback by convention (no `/etc/hosts`). Preview uses Traefik IngressRoutes + `StripPrefix` per PR.
- **Legacy:** prod also still serves `loreweaver.no` + `preview.loreweaver.no` (old project name) from the site pod - the prod cert and `site.yaml` ingress route them. Retire-or-keep is undecided; not part of the contract above.
- **Scope:** only these apexes carry the app's services. Separate systems (Hanko's `auth.*`, future `docs.`/`status.`) own their own DNS/TLS/auth.

**Authorities:** URL structure → [`app-server-prd.md` §URL architecture](docs/plans/2026-04-11-app-server-prd.md); topology + lifecycle → [`deployment-architecture.md`](docs/plans/2026-03-30-deployment-architecture.md). SPA helpers: `apps/web/src/lib/paths.ts` (`apiPath`, `campaignPath`, `spaRoute`) - use these, never hardcode `/api/...`.

## Development Notes

- The `@familiar-systems/editor` package is the most architecturally important. It defines the TipTap schema shared between browser (apps/web via loro-prosemirror) and the campaign server (for LoroDoc reconstruction and serialization compiler).
- **Cookie scope is per apex.** Any cookie set on the app apex (`app.familiar.systems`, or its preview/dev equivalents) is visible to the SPA, platform API, and campaign shards - but not to the marketing site on `familiar.systems`, which is a separate browser origin. Hanko's session cookie (if set) lives on `auth.*`, not on either of ours, and auth tokens travel as `Authorization: Bearer` headers. When adding a cookie (analytics, preferences, feature flags, anything), pick the apex deliberately and scope it narrowly with `Path=`.
