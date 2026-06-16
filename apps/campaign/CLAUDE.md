# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Scope

Covers `apps/campaign/` only: the `familiar-systems-campaign` Axum + kameo binary. Overrides the repo-root CLAUDE.md for anything under this directory. The sibling `apps/platform/CLAUDE.md` covers the platform tier.

## Today's surface

- `GET /health`: 200 `ready` when the registry is in `Phase::Ready`, 503 `draining` once drain has begun. Wired to the k8s readiness probe.
- `GET /catalog/systems`: locale-resolved catalog of game systems and bundled template metadata. Honors `?locale=` then `Accept-Language`, falls back to `en`.
- `GET /campaign/{id}`: Hanko-authenticated, owner-only (403 otherwise). Returns campaign metadata (name, tagline, game_system, content_locale, `wizard_completed_at`, timestamps).
- `PATCH /campaign/{id}`: Hanko-authenticated, owner-only. Partial metadata update, all fields optional. With `wizard_complete: true`, validates required fields (422 if missing), sets `wizard_completed_at`, and mirrors to platform (best-effort); 409 if the wizard was already completed. Without the flag, updates only the provided fields.
- `GET /campaign/{id}/ws`: WebSocket upgrade for CRDT collaboration. Auth is `?token=<hanko>` -> validate -> platform membership check (401 bad token, 403 non-member, 503 if the campaign can't be loaded). On upgrade, the connection joins room actors. See "CRDT rooms & collaboration".
- `POST /campaign/{id}/pages`: Hanko-authenticated, GM-only (403 otherwise; role checked on the platform tier via `check_membership`). Creates a Page. Body: `name` (required), optional `status` (default `gm_only`), optional `parent` (a `PageId` to nest under in the ToC; omitted = ToC root, unknown = 422). `from_template_id` is accepted but returns 501 (templates not built). Returns 201 + the created Page. The owning `PageActor` is spawned to persist the Page and place it in the ToC; nothing writes the rows from the handler. See "CRDT rooms & collaboration".
- `POST /campaign/{id}/sessions`: Hanko-authenticated, GM-only. Creates a session: a `kind = session` Page (prep/summary/transcript/journal sections) **and** its temporal `sessions` row (ordinal = max+1, the relationship FK target), minted together in one genesis transaction. Body: optional `name` (the GM's subtitle; blank/absent = unnamed, identified by ordinal), optional `status`, optional `parent`. Returns 201 + `{ page_id, session_id, ordinal, name }`. Driven by the supervisor's `CreateSession` workflow via the `PageActor`'s `NewSession` genesis path; nothing writes the rows from the handler. See "CRDT rooms & collaboration".
- `POST /internal/campaign`: bearer-protected. Creates a new campaign on this shard with the given owner. Idempotent on `campaign_id`.
- `PUT /internal/campaign/{id}/lease`: bearer-protected. Ensures an existing campaign is checked out on this shard. Idempotent.
- `DELETE /internal/campaign/{id}/lease`: bearer-protected. Releases a campaign from this shard (platform-initiated eviction). Idempotent; returns 200 even if the campaign is not loaded.

**Live:** ToC + Page CRDT room actors, the WebSocket collaboration path, block/ToC persistence, Page creation, session creation (page + temporal row), and a `CampaignStore` checkout/release abstraction (local + S3 backends). **Still ahead:** `AgentConversationActor`, `RelationshipGraph` (edges referencing the `sessions` temporal rows), template instantiation (cloning a template's block structure), periodic mid-session object-storage writeback, and the supervisor `SupervisorState`/`Restoring` phase. See "Design docs" below for where each is specified.

## Architecture

```
main.rs
  └─ CampaignRegistry            // one per process
       └─ CampaignSupervisor     // one per active campaign
            ├─ DatabaseWriteActor // owns the only sea-orm write connection
            ├─ TocActor           // eager singleton CRDT room (the ToC tree)
            └─ PageActor (×N)     // lazy per-Page CRDT room, keyed by PageId
            // future: AgentConversationActor, RelationshipGraph
```

**Single-writer invariant.** `DatabaseWriteActor` owns the only `DatabaseConnection` for a campaign. HTTP handlers and room actors reach the database only by sending it messages: `GetMetadata`, `PatchCampaignMetadata`, `WriteTocSnapshot`, `WritePageBlocks`, `DbCreatePage` (plus a test `Ping`). Room actors debounce CRDT edits and flush full snapshots through it; nothing writes directly.

**Lifecycle.** The registry is the only path to spawn supervisors. Storage init (create dir, open pool, run migrations, **check out from the `CampaignStore`**) runs in the supervisor's `on_start`, so a failure surfaces as a typed `InitError` -> `EnsureError::Init`. Spawned supervisors are `link`ed to the registry so `on_link_died` is the authoritative removal path (idle eviction, crash, link death). A per-supervisor idle timer self-stops the supervisor when `last_activity` exceeds `idle_timeout`; eviction drops it from RAM and leaves the `.db` on local disk (no periodic object-storage writeback yet). Room actors self-evict when their subscriber count reaches zero and flush if dirty before stopping.

**Shutdown.** `main` waits for SIGINT/SIGTERM, lets axum drain in-flight requests, then sends `BeginDrain` to the registry. `BeginDrain` flips the phase, snapshots the supervisor map, and runs the drain workflow on a tokio task (not in the registry mailbox) so the registry stays responsive while children stop in parallel. Each supervisor drains its children in order (`PageActor`s, then `TocActor`, then `DatabaseWriteActor`) so pending CRDT snapshots reach disk before the connection closes. `DRAIN_DEADLINE` is the internal safety net; the k8s grace period is the real deadline.

**Bearer + readiness.** `/internal/*` is bearer-protected via `middleware/auth.rs` (the `require_internal_bearer` fn lives in `app-shared::middleware::internal_auth`); the bearer is the layer-3 backstop, Ingress and NetworkPolicy carry layers 1 and 2 (see `infra/CLAUDE.md`). `/health` flips to 503 the moment the registry enters `Draining`.

## CRDT rooms & collaboration

**Model.** A `CrdtDoc` (`domain/crdt/doc.rs`) abstracts one CRDT document: apply updates, export/import a snapshot, report a version. `Room<D: CrdtDoc>` (`domain/crdt/room.rs`) wraps a doc with subscriber bookkeeping, snapshot-on-join, and broadcast fan-out. Actor-facing message contracts live in `domain/crdt/room_actor.rs` (`ClientJoin` / `ClientUpdate` / `ClientLeave`, and `Capability::{Read, Write}`). The concrete docs are `LoroPageDoc` and `LoroTocDoc` (`loro/{page,toc}.rs`), with block (de)serialization in `loro/block_codec.rs`. Loro schema constants and ts-rs-exported types live in `campaign-shared::loro`, not here.

**Room actors.** `TocActor` (eager singleton) restores the ToC tree from `toc_entries`; `PageActor` (lazy, one per `PageId`) restores a page's blocks from all of `blocks` for that page, grouped by `section` into the page kind's declared section containers (`preamble` + `body` for Entity/Template; see `PageKind::sections`). Both reconstruct their Loro doc on start, mark dirty on `ClientUpdate`, debounce, then flush a full snapshot to `DatabaseWriteActor` (`WriteTocSnapshot` / `WritePageBlocks`); both flush on stop if dirty.

**Page genesis & the ownership invariant.** A `PageActor` also spawns at creation (`PageInit::New`, distinct from the room-join `Restore`): `CampaignSupervisor::CreatePage` validates the requested ToC parent, spawns the actor (which builds its doc and persists its own birth row via `DbCreatePage`), then sends the `TocActor` an `AddPageNode` (which also updates the actor's `known_pages` so the new node isn't dropped by the next `snapshot_toc`). The ToC node is best-effort: if it fails, `restore_toc` re-surfaces the orphan Page at the root on the next checkout. **Every mutation to a Page flows through its `PageActor`**, never a direct DB write from a handler or "service". The actor is the single-threaded consistency boundary; writing a Page's rows around it would drift its in-memory CRDT doc from SQLite the moment the Page has live subscribers. The pure builder (`src/domain/page.rs`) composes the values to persist but performs no I/O; the actor is the only writer.

**Routing.** The supervisor's `JoinRoom` resolves a room-id string to a `RoomHandle`: `"toc"` -> the singleton; `"page:<ulid>"` -> ensure/spawn the `PageActor` for that `PageId`; anything else -> `UnknownRoom`.

**WebSocket path.** `ws/upgrade.rs` authenticates (Hanko token -> platform `check_membership` -> role mapped to `Capability`), then `ws/connection.rs` runs a read/write task pair. The read task dispatches doc updates straight to the room actor; the supervisor is consulted only at join, not on the hot path. Wire framing is loro-protocol (`wire/`): `BatchAssembler` reassembles inbound fragments (10s timeout, size/count caps against malicious clients), `BatchFragmenter` + `encode_broadcast` split and encode outbound broadcasts.

## Module map

Read rustdoc at each site for detail; this table is a where-to-go index.

| If you are touching... | Read |
| --- | --- |
| registry/supervisor lifecycle, drain workflow | `src/actors/{mod,registry,supervisor}.rs` |
| the single write connection + write commands | `src/actors/database_writer.rs` |
| CRDT room actors (ToC singleton, Page per-id) | `src/actors/{toc,page}.rs` |
| CRDT doc trait, Room orchestrator, room messages | `src/domain/crdt/{doc,room,room_actor}.rs` |
| pure Page-creation builder (functional core) | `src/domain/page.rs` |
| concrete Loro docs + block codec | `src/loro/{page,toc,block_codec}.rs` (schema constants live in `campaign-shared::loro`) |
| WebSocket upgrade + auth, connection loop | `src/ws/{upgrade,connection}.rs` |
| loro-protocol wire framing | `src/wire/{assembler,fragmenter,broadcast,reassembly}.rs` |
| campaign checkout/release store (local + S3) | `src/persistence/{store,store_local,store_s3}.rs` |
| route registration, public/internal/WS split | `src/router.rs`, `src/openapi.rs` |
| `POST /internal/campaign`, lease put/delete | `src/routes/internal.rs` |
| `/catalog/systems` (locale resolution) | `src/routes/catalog.rs` |
| `GET` and `PATCH /campaign/{id}` metadata | `src/routes/metadata.rs` |
| `POST /campaign/{id}/pages` (create a Page) | `src/routes/pages.rs` |
| `POST /campaign/{id}/sessions` (create a session) | `src/routes/sessions.rs` |
| pure session ordinal kernel | `src/domain/session.rs` |
| bearer middleware wiring | `src/middleware/auth.rs` |
| outbound campaign -> platform client | `src/clients/platform_internal.rs` |
| typed startup/init/ensure errors | `src/error.rs` |
| required env vars (panics on missing) | `src/config.rs` |
| `AppState` shape (what handlers see) | `src/state.rs` |
| SQLite pools, sqlite-vec registration | `src/db.rs` |
| schema (pages, blocks, vec embeddings, metadata, toc_entries) | `src/migrations/`, `src/entities/` |
| vector search escape hatch (vec0 MATCH/k=?) | `src/embeddings.rs` |
| catalog parser; content embedded from repo-root `content/` | `src/starter_content/{mod,catalog,template,localized}.rs` |

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

`cargo run -p familiar-systems-campaign` alone will panic: `Config::from_env` requires `CAMPAIGN_STORAGE_BACKEND` (`local`|`s3`), `HANKO_API_URL`, `PORT`, `CAMPAIGN_DATA_DIR`, `INTERNAL_BEARER_PRIMARY`, `PLATFORM_URL`, `CAMPAIGN_IDLE_TIMEOUT_SECS`, and `CAMPAIGN_EVICTION_CHECK_INTERVAL_SECS` (plus `S3_*` when the backend is `s3`). Optional: `INTERNAL_BEARER_SECONDARY` (rotation) and `CAMPAIGN_HEARTBEAT_INTERVAL_SECS` (default 30). `mise run dev:campaign` injects the dev set.

## Cross-file facts

- **Routes use full service-prefixed paths.** Public routes are registered as `/catalog/systems`, `/campaign/{id}`, etc. Reverse proxies strip only the per-environment prefix (nothing in local dev, `/pr-N` in preview) and forward the service prefix intact. `/internal/*` is pod-to-pod only and is never registered in any Ingress.
- **`register_sqlite_vec()` must run before any sea-orm pool opens.** Migrations include a `vec0` virtual table. `main.rs` calls it once at startup; tests must call it too (it's `Once`-guarded so spamming is fine).
- **CampaignId is a Nanoid** minted by the platform tier, not validated here on the wire. `<data_dir>/<campaign_id>.db` is the on-disk shape; no path-traversal concern because Nanoid is URL-safe. **PageId is a ULID** (cleaner btree inserts); WebSocket room ids are `page:<ulid>`.
- **`SetStopCause` is first-writer-wins.** A supervisor that self-tags `Idle` does not get clobbered by a later drain-side `SetStopCause(Drain)`. See the rustdoc on `SetStopCause`.
- **Supervisor `on_start` is fallible** (`Result<Self, InitError>`): storage checkout via the `CampaignStore` runs there, so init failures surface as `EnsureError::Init`. The current code still holds `db: Option<CampaignDatabase>` rather than a `SupervisorState` enum (`Starting`/`Restoring`/`Ready`/`Draining`); the `TODO` at `src/actors/supervisor.rs` documents the planned move once heartbeat phase-reporting and room-join gating need it.
- **The `CrdtDoc` trait and the concrete Loro wrappers live here**, in `src/domain/crdt/` and `src/loro/`. `campaign-shared::loro` holds only schema constants and ts-rs-exported types (Toc/Page/ProseMirror conventions), never the Rust wrappers.

## Testing

`tests/common::spawn_app()` builds a real `TestApp` with a fresh `TempDir` as `CAMPAIGN_DATA_DIR`, `wiremock` standins for both the platform and Hanko (the Hanko mock accepts any session token), the real router on an ephemeral port, and a live registry handle exposed as `app.registry` for tests that drive lifecycle (e.g. `BeginDrain` to assert 503).

Each file under `tests/` compiles as its own integration binary: `catalog_test`, `initialize_test`, `internal_init_test`, `lifecycle_test`, `metadata_test`, `schema_drift`, `spike` (pages/blocks FK round-trip + vec-search GM/player filtering), and `wire_roundtrip` (loro-protocol framing). `config.rs` env-var tests are an inline `#[serial]` module (from `serial_test`), not a `tests/` file. ToC CRDT convergence is a unit test inside `src/loro/toc.rs`. `schema_drift.rs` enforces that `entities/` matches the live migration schema; treat its failures as a real bug, not test brittleness.

When writing actor tests, set `idle_timeout` to seconds (60+) so the timer doesn't fire mid-test. Eviction tests pin it to tens of milliseconds; the integration `lifecycle_test.rs` shows the full ensure -> drain -> reopen flow.

## Adding code

- **New route**: full service-prefixed path (`/catalog/...` or `/campaign/...`), new module under `src/routes/`, register in `src/openapi.rs` (public) or `internal_router` (bearer). WebSocket routes are merged in `src/router.rs`.
- **New env var**: panic-on-missing in `Config::from_env`; add a `#[serial]` test for the missing-var case. Update `mise.toml`'s `dev:campaign` env block.
- **New room actor / `CrdtDoc` impl**: implement `CrdtDoc` for the new doc type in `src/loro/`, wrap it in `Room<D>`, add a `RoomHandle` variant and `JoinRoom` routing in the supervisor, and persist via a new `DatabaseWriteActor` command.
- **Mutating an entity (Page/ToC), including creating one**: route it through the owning room actor as a message; never write the entity's rows directly from a handler or helper. The actor is the single-threaded consistency boundary, so a write around it drifts its in-memory CRDT doc from SQLite once there are subscribers. `CreatePage` is the pattern: it spawns the `PageActor` in genesis mode rather than inserting rows from the route. Pure value-shaping can live in `src/domain/` (e.g. `domain/page.rs`); the I/O stays in the actor.
- **New actor message**: bump `last_activity` if it's a real operational supervisor message. Update the supervisor's drain ordering if the new handler does I/O that must complete before `on_stop`.
- **New `AppState` field**: cheap clone only (`Arc` or kameo `ActorRef`); `AppState` is cloned per handler invocation.
- **New migration**: new file under `src/migrations/`, register in `migrations/mod.rs`. Every test migrates from empty; `schema_drift.rs` will fail until `entities/` matches.
- **New shared type that crosses the platform/campaign boundary**: lives in `crates/app-shared/`; campaign-only types live in `crates/campaign-shared/` (schema constants, ts-rs types, onboarding DTOs). Never put persistence/ORM in shared crates.

## Design docs for future direction

The campaign tier is being built in slices. These docs spec what comes next; read the one closest to what you're changing before extending.

- [`docs/plans/2026-05-04-campaign-actor-domain-design.md`](../../docs/plans/2026-05-04-campaign-actor-domain-design.md): canonical actor topology and CRDT room model. The room actors have landed; the supervisor `SupervisorState` machine (including the `Restoring` phase for room-actor checkout) is still ahead.
- [`docs/plans/2026-03-25-campaign-collaboration-architecture.md`](../../docs/plans/2026-03-25-campaign-collaboration-architecture.md): WebSocket protocol, checkout/checkin, scaling model.
- [`docs/plans/2026-05-22-campaign-creation-architecture.md`](../../docs/plans/2026-05-22-campaign-creation-architecture.md): campaign creation flow, wizard surface, catalog system, initialization + mirror callback to the platform (template instantiation is not yet built).
- [`docs/plans/2026-04-10-entity-relationship-temporal-model.md`](../../docs/plans/2026-04-10-entity-relationship-temporal-model.md): relationship schema, sessions-as-knowledge-time, retcon/supersede lifecycle.
- [`docs/plans/2026-03-25-ai-serialization-format-v2.md`](../../docs/plans/2026-03-25-ai-serialization-format-v2.md): serialization compiler, AI tool surface.
- [`docs/plans/2026-03-30-deployment-architecture.md`](../../docs/plans/2026-03-30-deployment-architecture.md): graceful restart, preview environments, shard topology.
