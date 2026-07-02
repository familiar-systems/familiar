# Campaign Creation Architecture

**Status**: Built and functional as of 2026-05-22. The full creation flow works end-to-end: hub listing, campaign creation, 4-step wizard, metadata mirroring, idle eviction. The campaign editor (post-wizard) is a placeholder. WebSocket, SSE, and template instantiation (LoroDoc compilation from authored template content) are not yet built. See [TODO](#todo) for the full list.

Supersedes [`docs/archive/plans/2026-05-11-new-campaign-onboarding.md`](../archive/plans/2026-05-11-new-campaign-onboarding.md).

## Decisions

| Decision | Why |
|---|---|
| **Platform is domain-blind.** Mirrors `name`, `tagline`, `game_system`, `content_locale` as opaque strings. Never interprets them. | Every new system, template, or locale would become a platform deploy otherwise. Platform's role is auth, routing, billing. |
| **Platform mints CampaignId.** Single source of truth for ID allocation. Nanoid (21-char URL-safe). | Routing table PK is the consistency boundary. Collision (1-in-2^126) caught by unique constraint. |
| **Catalog lives on campaign tier.** `GET /catalog/systems` answered by any shard. | Campaign binary already embeds `content/`. Avoids duplicate embedding and keeps platform domain-blind. |
| **Wizard state is client-side until Seal.** No incremental saves. Seal fires one PATCH. | No sticky-field race conditions, no partially-configured intermediate states. Abandonment leaves a blank campaign the user can re-walk. |
| **No reaper.** Un-sealed campaigns are idle campaigns. | Supervisor idle eviction handles cleanup. User deletes from hub. Orphans cost cents in object storage. |
| **Eventual consistency for metadata.** Campaign is source of truth; platform mirrors for hub listing via internal PATCH. | Hub listing is a single platform query, no per-shard fan-out. NULL mirrors mean "not configured yet." |
| **Internal bearer auth** with primary/secondary rotation. Symmetric, one token both directions. | Backstop behind Ingress exclusion and NetworkPolicy. Two-bearer rotation eliminates rolling-deploy windows. |

## Campaign creation flow

```
Hub ("Start new campaign")
  |
  POST /api/campaigns { idempotency_token }
  |                                           Platform
  |-- upsert create_attempts(token, id)
  |-- mint CampaignId
  |-- POST /internal/campaign { id, owner }   --> Campaign shard
  |-- PUT /internal/campaign/{id}/lease       --> Campaign shard
  |-- INSERT routing row (all mirrors NULL)
  `-- 200 { campaign_id }
  |
  navigate to /c/{campaignId}
  |                                           SPA
  |-- GET /campaign/{id}                      --> Campaign shard
  |-- wizard_completed_at is null
  `-- render CampaignWizard
      |
      Step 1: Name (name, tagline)
      Step 2: System (catalog pick or BYO, template selection)
      Step 3: Privacy (audio mode, evals)
      Step 4: Review + Seal
      |
      PATCH /campaign/{id}                    --> Campaign shard
        { name, game_system, content_locale,
          tagline?, template_slugs, audio,
          evals_enabled, wizard_complete: true }
      |                                       Campaign shard
      |-- validate, write campaign_metadata
      |-- set wizard_completed_at = now()
      `-- PATCH /internal/platform/campaign/{id}  --> Platform (mirror)
      |
      SPA refetches metadata, wizard dismisses
```

**Idempotency**: `create_attempts` PK on token, `INSERT OR IGNORE` on campaign_metadata and routing row. Wizard seal guarded by `wizard_completed_at IS NULL`; retry after commit returns 409 (SPA treats as success).

**Failure**: If seal fails, campaign shard fires `POST /internal/platform/campaign/{id}/init-failed { reason }`. SPA shows "seal cracked" state with retry affordance.

## Wizard surface

Four steps, all client-side state until Seal:

| Step | Collects | Advance condition |
|---|---|---|
| **Name** | Campaign name (required, max 80), tagline (optional, max 140) | Name non-empty |
| **System** | Catalog system pick or BYO (with optional custom name). Template toggles from system bundle. | System picked, at least one template selected |
| **Privacy** | Audio mode (opt-in / opt-out / text-only), AI evals (enabled / disabled) | Both set |
| **Review + Seal** | Summary of all choices. Wax seal button triggers PATCH. | All prior steps valid |

BYO is first-class (always visible below catalog, not a fallback). Catalog search uses fuzzy match on system name. Template selection only appears for catalog systems.

Hub page shows campaign cards in three states: **draft** (wizard incomplete), **init-failed** (seal error), **initialized** (wizard complete).

## Route inventory

### Campaign tier (`apps/campaign`)

| Method | Path | Auth | Purpose |
|---|---|---|---|
| GET | `/health` | none | Readiness probe. 200 while Ready, 503 while Draining. |
| GET | `/catalog/systems` | none | Locale-resolved game systems and template metadata. Honors `Accept-Language` / `?locale=`. |
| GET | `/campaign/{id}` | Hanko JWT | Campaign metadata. Owner-only (403 if not owner). |
| PATCH | `/campaign/{id}` | Hanko JWT | Partial metadata update. When `wizard_complete: true`, validates required fields, sets `wizard_completed_at`, mirrors to platform. |
| POST | `/internal/campaign` | bearer | Create campaign with owner. Idempotent on campaign_id. |
| PUT | `/internal/campaign/{id}/lease` | bearer | Ensure campaign loaded (checkout from storage if needed). Idempotent. |
| DELETE | `/internal/campaign/{id}/lease` | bearer | Release campaign (platform-initiated eviction). |

### Platform tier (`apps/platform`)

| Method | Path | Auth | Purpose |
|---|---|---|---|
| GET | `/health` | none | Readiness probe. |
| GET | `/me` | Hanko JWT | Authenticated user info (id + email). Upserts user row. |
| POST | `/api/campaigns` | Hanko JWT | Create campaign. Mints CampaignId, calls shard, writes routing row. |
| GET | `/api/campaigns` | Hanko JWT | List user's campaigns (from mirrored columns, no shard fan-out). |
| GET | `/api/campaigns/{id}` | Hanko JWT | Single campaign + implicit lease acquisition on shard. |
| PATCH | `/internal/platform/campaign/{id}` | bearer | Mirror metadata from campaign shard. |
| POST | `/internal/platform/campaign/{id}/init-failed` | bearer | Record wizard failure reason. |
| DELETE | `/internal/platform/campaign/{id}/lease` | bearer | Shard notifies idle eviction. |
| POST | `/internal/platform/heartbeat` | bearer | Shard reports loaded campaigns. Wholesale-replaces loaded_cache. |

### SPA (`apps/web`)

| Route | Purpose |
|---|---|
| `/_authed/` | Hub. Campaign list or empty state. "Start new campaign" button. |
| `/_authed/c/$campaignId` | Campaign page. Renders wizard if `wizard_completed_at` is null, placeholder otherwise. |

## Actor topology

```
CampaignRegistry (process-lifetime singleton)
  Phase machine: Ready | Draining
  HashMap<CampaignId, ActorRef<CampaignSupervisor>>
  |
  +-- CampaignSupervisor (per active campaign, linked)
  |     Owns: CampaignDatabase, last_activity tracker, idle timeout
  |     Messages: PatchCampaignMetadata, GetMetadata, IdleCheck, SetStopCause
  |     |
  |     +-- DatabaseActor (single-writer, linked)
  |           Owns: read-write DatabaseConnection
  |           Messages: PatchCampaignMetadata, GetMetadata
```

**Idle eviction**: Supervisor spawns a timer task. When `now - last_activity >= idle_timeout`, supervisor self-stops, releases storage, notifies platform via `DELETE /internal/platform/campaign/{id}/lease`.

**Graceful shutdown**: SIGTERM/SIGINT -> Axum drains in-flight requests -> Registry enters Draining phase (rejects new campaigns, health returns 503) -> stops all supervisors in parallel via JoinSet -> each supervisor releases storage -> process exits.

**Link lifecycle**: Registry tracks supervisors via kameo links. `on_link_died` removes supervisor from map when it stops for any reason.

## Persistence

**CampaignStore trait**: `checkout(id) -> PathBuf`, `writeback(id, path)`, `release(id, path)`.

- **LocalCampaignStore**: Files at `{data_dir}/{id}.db`. Checkout creates parents, writeback/release are no-ops. Used in local dev.
- **S3CampaignStore**: Remote at `campaigns/{id}/campaign.db`. Checkout downloads to local cache, writeback uploads, release uploads then deletes local. Used in hosted deployments.

**CampaignDatabase**: WAL-mode SQLite. Read pool (16 connections, read-only) for concurrent reads. Write connection owned exclusively by DatabaseActor. Migrations run on checkout. `campaign_metadata` row seeded on first open.

**Block storage and search** (designed, not built). A block's `content` blob is the lossless source of truth; the CRDT oplog is not persisted - the LoroDoc is reconstructed from these rows on checkout (see [Campaign Collaboration Architecture](2026-03-25-campaign-collaboration-architecture.md): "relational data is the data at rest"). The agent markdown and a per-block **search projection** (a derived markdown/text column, added with this work) are *derived* from that source and may be lossy - a node markdown cannot express renders a placeholder, and reconstruction is unaffected because it reads the JSON. The search projection is materialized on the debounced persist flush (`apps/campaign/src/actors/persist.rs`): a cheap, local per-block transform with no graph or ToC dependency. Grep over it is **cold scan ∪ live overlay** - bulk-scan the derived column for non-resident pages (no actor checkout), overlay a live compile for the small resident set; results are ToC-subtree-scoped and `status`-filtered (per-block RBAC). The compiler is owned by [AI Serialization & Editing Model](2026-06-30-ai-serialization-and-editing-model.md); the grep *capability* by the [AI PRD](2026-02-22-ai-prd.md).

## Catalog system

Game systems and templates live in `content/` at the repo root, embedded at build time via `include_dir!`. Parse failures fail the build.

- `content/systems.yaml`: System definitions (id, name, tagline, color, popular, bundle of template slugs). Plus `byo:` sibling with its own default bundle.
- `content/templates/{common,<system-id>}/*.yaml`: Template files with `meta` (name, description, icon as `LocalizedString`) and `body` (serde_yaml::Value, not yet compiled). The authoring format is being reworked to per-locale markdown; see [Templates](2026-06-29-templates.md).

`GET /catalog/systems` resolves `LocalizedString` fields per locale with fallback chain (requested locale -> `en` -> first available). Returns `CatalogResponse { systems, byo }` with resolved strings.

Template body compilation to blocks is not yet built, and the selected system's `bundle` is not yet consumed at creation (a new campaign is born with one empty home page). Both are owned by [Templates](2026-06-29-templates.md).

## Heartbeat reconciliation

The campaign shard periodically sends `POST /internal/platform/heartbeat { campaigns: [CampaignId] }` listing all currently loaded campaign IDs. The platform wholesale-replaces its in-memory `loaded_cache` (an `Arc<RwLock<HashSet<CampaignId>>>`). This reconciles after shard restarts, missed release notifications, or network partitions.

The `loaded` flag on `GET /api/campaigns` responses comes from this cache. It lets the SPA skip cold-checkout latency for already-loaded campaigns.

## Schema

### Campaign tier (per-campaign SQLite, `apps/campaign`)

- **campaign_metadata**: `id` (PK, check id=1), `campaign_id`, `owner_user_id`, `name`, `tagline`, `game_system`, `content_locale`, `wizard_completed_at`, `created_at`, `updated_at`
- **pages**: `id` (Nanoid PK), `name`, `status`, `template_id` (nullable self-ref), `created_at`, `updated_at`
- **blocks**: `id` (Nanoid PK), `page_id` (FK cascade), `status`, `ordering` (i64), `content` (Blob - the block's ProseMirror node tree as JSON, the lossless source of truth), `section`, `created_at`, `updated_at`
- **block_embeddings_vec**: `id` (Nanoid PK), `block_id` (FK cascade), `embedding` (sqlite-vec vector), `created_at`

### Platform tier (single SQLite, `apps/platform`)

- **users**: `id` (UUID PK, Hanko subject), `email` (unique), `created_at`, `updated_at`
- **campaigns**: `id` (Nanoid PK), `owner_user_id` (FK users), `shard_url`, `name?`, `tagline?`, `game_system?`, `content_locale?`, `last_init_error?`, `wizard_completed_at?`, `created_at`, `updated_at`. Index on `owner_user_id`.
- **create_attempts**: `idempotency_token` (PK), `campaign_id` (no FK, written before routing row), `created_at`

## TODO

- **WebSocket**: CRDT sync (Loro protocol), room multiplexing, presence. Supervisor uses connection count as activity signal for checkout/checkin lifecycle.
- **Server-sent events**: Error notifications (persistence degraded, server restarting).
- **Template instantiation**: compile a template's markdown body to blocks (the at-rest block JSON, per the block schema - a one-way parse that mints fresh block ids, *not* the stateful agent-edit round-trip), consume a system's `bundle` at campaign creation, and clone entities from templates (`from_template_id`). Owned by [Templates](2026-06-29-templates.md).
- **PageActor, TocActor, AgentConversation actors**: Per-entity actors for CRDT rooms. Currently only the supervisor and database writer exist.
- **Campaign editor UI**: Post-wizard view is a placeholder. Needs the editor surface, page navigation, ToC rendering.
- **Starter template content**: Template YAML files are partially authored. System catalog entries exist but template coverage across systems is incomplete.
