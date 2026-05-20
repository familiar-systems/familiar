# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Scope

Covers `apps/campaign/` only: the `familiar-systems-campaign` Axum + kameo binary. Overrides the repo-root CLAUDE.md for anything under this directory. The sibling `apps/platform/CLAUDE.md` covers the platform tier.

## Today's surface

- `GET /health`: 200 `ready` when the registry is in `Phase::Ready`, 503 `draining` once drain has begun. Wired to the k8s readiness probe.
- `GET /catalog/systems`: locale-resolved catalog of game systems and bundled template metadata. Honors `?locale=` then `Accept-Language`, falls back to `en`.
- `POST /campaign/{id}/initialize`: campaign initialization handler. **Deliberate 500 in the thin slice.** Validates payload shape, fires `init-failed` to the platform, returns a structured error body. Real init transaction lands in a later slice.
- `POST /internal/campaign/init`: bearer-protected. Asks `CampaignRegistry` to ensure a supervisor exists; idempotent on `campaign_id`. Returns 200 on success, 503 during drain, 500 on init failure.

CRDT room actors (Thing, ToC, AgentConversation), the WebSocket layer, the real wizard transaction, template instantiation, and object-storage checkin/checkout do not exist yet. See "Design docs" below for where each is specified.

## Architecture

Two-level actor topology:

```
main.rs
  └─ CampaignRegistry            // one per process
       └─ CampaignSupervisor     // one per active campaign
            └─ DatabaseActor     // owns the only sea-orm write connection
            └─ (future: ThingActor, TocActor, AgentConversationActor, ...)
```

**Single-writer invariant.** The actor system owns the only `DatabaseConnection` for a given campaign. HTTP handlers reach the database by `ask`ing the registry for a supervisor, then sending messages; no handler holds a connection directly. `DatabaseActor` currently accepts only a test `Ping`; initialization will land write commands here.

**Lifecycle.** The registry is the only path to spawn supervisors, so storage init (create dir, open pool, run migrations) is serialized through one mailbox. Spawned supervisors are `link`ed to the registry so `on_link_died` is the authoritative removal path (covers idle eviction, crash, link death). Per-supervisor idle timer self-stops the supervisor when `last_activity` exceeds `idle_timeout`; eviction drops the supervisor from RAM and leaves the `.db` on disk (no object-storage path yet).

**Shutdown.** `main` waits for SIGINT/SIGTERM, lets axum drain in-flight requests, then sends `BeginDrain` to the registry. `BeginDrain` flips the phase, snapshots the supervisor map, and runs the drain workflow on a tokio task (not in the registry mailbox) so the registry stays responsive while children stop in parallel. `DRAIN_DEADLINE` is the internal safety net; the k8s grace period is the real deadline.

**Bearer + readiness.** `/internal/*` is bearer-protected via `middleware/internal_auth.rs`; the bearer is the layer-3 backstop, Ingress and NetworkPolicy carry layers 1 and 2 (see `infra/pulumi-cloud/CLAUDE.md`). `/health` flips to 503 the moment the registry enters `Draining`.

## Module map

Read rustdoc at each site for detail; this table is a where-to-go index.

| If you are touching... | Read |
| --- | --- |
| actor topology, registry/supervisor/database lifecycle, drain workflow | `src/actors/mod.rs`, `src/actors/registry.rs`, `src/actors/supervisor.rs`, `src/actors/database.rs` |
| route registration, bearer-protected vs public split, full-path routes | `src/routes/mod.rs` |
| `/internal/campaign/init` handler and status mapping | `src/routes/internal.rs` |
| `/systems` catalog (locale resolution) | `src/routes/catalog.rs` |
| campaign initialization handler (currently deliberate 500) | `src/routes/initialize.rs` |
| bearer middleware | `src/middleware/internal_auth.rs` |
| outbound campaign → platform `/internal/platform/*` client | `src/clients/platform_internal.rs` |
| typed startup/init/ensure errors | `src/error.rs` |
| required env vars (panics on missing) | `src/config.rs` |
| `AppState` shape (what handlers see) | `src/state.rs` |
| SQLite pool, sqlite-vec registration | `src/db.rs` |
| campaign DB schema (things, blocks, vec embeddings, metadata) | `src/migrations/`, `src/entities/` |
| vector search escape hatch (vec0 MATCH/k=?) | `src/embeddings.rs` |
| catalog parser, embedded `content/` | `src/starter_content/{mod,catalog,template,localized}.rs` |
| Loro doc wrappers (Thing pages, ToC) | `src/loro/{thing,toc}.rs` |
| loro-protocol websocket wire framing | `src/wire/{assembler,fragmenter,reassembly}.rs` |
| CRDT room trait (`CrdtRoom`) | `src/domain/crdt/` |

## Commands

Prefer the workspace-wide `mise` tasks. They cover this crate's shared deps (`familiar-systems-app-shared`, `familiar-systems-campaign-shared`) and the other consumer of those crates (`apps/platform`).

```bash
mise run test
mise run lint
mise run typecheck
mise run format
mise run dev:campaign        # runs on :3001 with CAMPAIGN_DATA_DIR=data/dev-campaigns
mise run lint:content        # JSON Schema validation for content/*.yaml
```

Drop to crate-scoped `cargo` only for targeted iteration:

```bash
cargo test -p familiar-systems-campaign --test lifecycle_test
cargo test -p familiar-systems-campaign --test internal_init_test init_during_drain_returns_503
```

`cargo run -p familiar-systems-campaign` alone will panic: `Config::from_env` requires `PORT`, `CAMPAIGN_DATA_DIR`, `INTERNAL_BEARER_PRIMARY`, `PLATFORM_URL`, `CAMPAIGN_IDLE_TIMEOUT_SECS`, and `CAMPAIGN_EVICTION_CHECK_INTERVAL_SECS`. `mise run dev:campaign` injects them.

## Cross-file facts

- **Routes use full service-prefixed paths.** Public routes are registered as `/catalog/systems`, `/campaign/{id}/initialize`, etc. Reverse proxies strip only the per-environment prefix (nothing in local dev, `/pr-N` in preview) and forward the service prefix intact. `/internal/*` is pod-to-pod only and is never registered in any Ingress.
- **`register_sqlite_vec()` must run before any sea-orm pool opens.** Migrations include a `vec0` virtual table. `main.rs` calls it once at startup; tests must call it too (it's `Once`-guarded so spamming is fine).
- **CampaignId is a Nanoid** minted by the platform tier, not validated here on the wire. `<data_dir>/<campaign_id>.db` is the on-disk shape; no path-traversal concern because Nanoid is URL-safe.
- **`SetStopCause` is first-writer-wins.** A supervisor that self-tags `Idle` does not get clobbered by a later drain-side `SetStopCause(Drain)`. See the rustdoc on `SetStopCause`.
- **Supervisor `on_start` is currently `Infallible`** because storage init runs in the registry mailbox before spawn. The `FIXME` at `src/actors/supervisor.rs` documents the planned move into `on_start` when bucket-world checkout lands; until then, init failures surface as `EnsureError::Init`, not `SupervisorDied`.

## Testing

`tests/common::spawn_app()` builds a real `TestApp` with a fresh `TempDir` as `CAMPAIGN_DATA_DIR`, a `wiremock` platform standin, the real router on an ephemeral port, and a live registry handle exposed as `app.registry` for tests that need to drive lifecycle (e.g. `BeginDrain` to assert 503).

Each file under `tests/` compiles as its own integration binary. `config.rs` env-var tests use `#[serial]` from `serial_test`. `schema_drift.rs` enforces that `entities/` matches the live migration schema; treat its failures as a real bug, not test brittleness.

When writing actor tests, set `idle_timeout` to seconds (60+) so the timer doesn't fire mid-test. Eviction tests pin it to tens of milliseconds; the integration `lifecycle_test.rs` shows the full ensure → drain → reopen flow.

## Adding code

- **New route**: full service-prefixed path (`/catalog/...` or `/campaign/...`), new module under `src/routes/`, register in `routes/mod.rs`. Internal routes go through `internal_router`; public routes through `public_router`.
- **New env var**: panic-on-missing in `Config::from_env`; add a `#[serial]` test for the missing-var case. Update `mise.toml`'s `dev:campaign` env block.
- **New actor message**: bump `last_activity` if it's a real operational message (Ping is the pattern). Update the supervisor's drain ordering if the new handler does I/O that must complete before `on_stop`.
- **New `AppState` field**: cheap clone only (`Arc` or kameo `ActorRef`); `AppState` is cloned per handler invocation.
- **New migration**: new file under `src/migrations/`, register in `migrations/mod.rs`. Every test migrates from empty.
- **New shared type that crosses the platform/campaign boundary**: lives in `crates/app-shared/` (per the project structure doc); campaign-only types live in `crates/campaign-shared/`. Never put persistence/ORM in shared crates.

## Design docs for future direction

The campaign tier is being built in slices. These docs spec what comes next; read the one closest to what you're changing before extending.

- [`docs/plans/2026-05-04-campaign-actor-domain-design.md`](../../docs/plans/2026-05-04-campaign-actor-domain-design.md): canonical actor topology, CRDT room model, supervisor phase machine including the `Restoring` phase that returns when room actors land.
- [`docs/plans/2026-03-25-campaign-collaboration-architecture.md`](../../docs/plans/2026-03-25-campaign-collaboration-architecture.md): WebSocket protocol, checkout/checkin, scaling model.
- [`docs/plans/2026-05-11-new-campaign-onboarding.md`](../../docs/plans/2026-05-11-new-campaign-onboarding.md): catalog, template compiler, initialization handler, mirror callback to the platform.
- [`docs/plans/2026-04-10-entity-relationship-temporal-model.md`](../../docs/plans/2026-04-10-entity-relationship-temporal-model.md): relationship schema, sessions-as-knowledge-time, retcon/supersede lifecycle.
- [`docs/plans/2026-03-25-ai-serialization-format-v2.md`](../../docs/plans/2026-03-25-ai-serialization-format-v2.md): serialization compiler, AI tool surface.
- [`docs/plans/2026-03-30-deployment-architecture.md`](../../docs/plans/2026-03-30-deployment-architecture.md): graceful restart, preview environments, shard topology.
